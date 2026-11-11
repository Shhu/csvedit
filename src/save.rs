//! Saving the table to its file, away from the interface's thread: the
//! lines are encoded straight into a new file beside it, which then takes
//! its place.

use std::fs::{File, Metadata, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Instant;

use crate::table::{self, Format, Table, Unencodable, WriteError};

/// Why a save failed.
#[derive(Debug)]
pub enum SaveError {
    /// A character the file's encoding can't hold.
    Unencodable(Unencodable),
    /// The file is read-only: it is left as it is.
    ReadOnly,
    /// The saving thread stopped without an answer (it panicked).
    Crashed,
    Io(io::Error),
}

impl From<io::Error> for SaveError {
    fn from(error: io::Error) -> Self {
        SaveError::Io(error)
    }
}

impl From<WriteError> for SaveError {
    fn from(error: WriteError) -> Self {
        match error {
            WriteError::Unencodable(bad) => SaveError::Unencodable(bad),
            WriteError::Io(error) => SaveError::Io(error),
        }
    }
}

/// Writes `table` (a snapshot: it shares the cells) to `path` on a thread
/// of its own; the result comes through the receiver.
pub fn spawn(table: Table, format: Format, path: PathBuf) -> mpsc::Receiver<Result<(), SaveError>> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let start = Instant::now();
        let result = write_safely(&path, |out| table::write_to(&table, &format, out));
        crate::diag::record("save", start.elapsed());
        // The cells are no longer shared: an edit made now copies nothing.
        drop(table);
        let _ = sender.send(result);
    });
    receiver
}

/// Writes the file with `write` to a new file beside `path`, then puts it
/// in its place: a failure halfway leaves the old file whole.
pub fn write_safely(
    path: &Path,
    write: impl Fn(&mut dyn Write) -> Result<(), WriteError>,
) -> Result<(), SaveError> {
    // A link is followed, not replaced.
    let target = match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => std::fs::canonicalize(path)?,
        _ => path.to_owned(),
    };
    let existing = std::fs::metadata(&target).ok();
    // Replacing the file would get past its being read-only (on Unix,
    // only the folder's permissions count): it is refused everywhere.
    if existing
        .as_ref()
        .is_some_and(|meta| meta.permissions().readonly())
    {
        return Err(SaveError::ReadOnly);
    }
    let in_place = |error: SaveError| match error {
        // A folder that takes no new file: write in place, as before.
        SaveError::Io(error)
            if error.kind() == io::ErrorKind::PermissionDenied && existing.is_some() =>
        {
            write_in_place(&target, &write)
        }
        other => Err(other),
    };
    let (temp, file) = match create_temp(&target, existing.as_ref()) {
        Ok(created) => created,
        Err(error) => return in_place(error.into()),
    };
    let written = (|| -> Result<(), SaveError> {
        let mut out = BufWriter::with_capacity(1 << 16, file);
        write(&mut out)?;
        let file = out.into_inner().map_err(io::IntoInnerError::into_error)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp, &target)?;
        Ok(())
    })();
    match written {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = std::fs::remove_file(&temp);
            in_place(error)
        }
    }
}

/// Writes over the file itself, once the text is known to encode (a
/// failure halfway would leave it cut short).
fn write_in_place(
    target: &Path,
    write: &impl Fn(&mut dyn Write) -> Result<(), WriteError>,
) -> Result<(), SaveError> {
    write(&mut io::sink())?;
    let mut out = BufWriter::with_capacity(1 << 16, File::create(target)?);
    write(&mut out)?;
    out.into_inner()
        .map_err(io::IntoInnerError::into_error)?
        .sync_all()?;
    Ok(())
}

/// A new file beside `target`, under a name no other file has (a file or
/// link left there is never opened), with `existing`'s permissions set
/// before anything is written to it.
fn create_temp(target: &Path, existing: Option<&Metadata>) -> io::Result<(PathBuf, File)> {
    use std::hash::BuildHasher as _;
    let dir = target.parent().filter(|d| !d.as_os_str().is_empty());
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    let random = std::collections::hash_map::RandomState::new();
    for attempt in 0u32..100 {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let tag = random.hash_one((std::process::id(), attempt, nanos)) as u32;
        let temp_name = format!(".{name}.{tag:08x}.csvedit-tmp");
        let temp = dir.map_or_else(|| PathBuf::from(&temp_name), |d| d.join(&temp_name));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        // Readable by no one else until the file's own permissions are set.
        #[cfg(unix)]
        if existing.is_some() {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        match options.open(&temp) {
            Ok(file) => {
                if let Some(meta) = existing
                    && let Err(error) = file.set_permissions(meta.permissions())
                {
                    drop(file);
                    let _ = std::fs::remove_file(&temp);
                    return Err(error);
                }
                return Ok((temp, file));
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "no free name for the temporary file",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::{Cell, Encoding};

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("csvedit-save-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a test folder");
        dir
    }

    fn table(rows: &[&[&str]]) -> Table {
        Table::from_rows(
            rows.iter()
                .map(|row| row.iter().map(|&c| Cell::from(c)).collect())
                .collect(),
        )
    }

    fn save(table: &Table, format: &Format, path: &Path) -> Result<(), SaveError> {
        write_safely(path, |out| table::write_to(table, format, out))
    }

    fn entries(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .expect("the folder")
            .map(|e| {
                e.expect("an entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        names
    }

    #[test]
    fn replaces_the_file_and_leaves_no_temporary_file() {
        let dir = dir("replace");
        let path = dir.join("a.csv");
        std::fs::write(&path, "old\n").unwrap();
        save(&table(&[&["a", "b"]]), &Format::default(), &path).expect("saved");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("a,b"), "{text:?}");
        assert_eq!(entries(&dir), ["a.csv"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unencodable_cell_leaves_the_file_whole() {
        let dir = dir("unencodable");
        let path = dir.join("a.csv");
        std::fs::write(&path, "old\n").unwrap();
        let format = Format {
            encoding: Encoding::Windows1252,
            ..Format::default()
        };
        let result = save(&table(&[&["a"], &["\u{4E2D}"]]), &format, &path);
        match result {
            Err(SaveError::Unencodable(bad)) => {
                assert_eq!(bad.character, '\u{4E2D}');
                assert_eq!(bad.cell, Some((1, 0)));
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "old\n");
        assert_eq!(entries(&dir), ["a.csv"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_read_only_file_is_refused() {
        let dir = dir("read-only");
        let path = dir.join("a.csv");
        std::fs::write(&path, "old\n").unwrap();
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&path, permissions.clone()).unwrap();
        let result = save(&table(&[&["a"]]), &Format::default(), &path);
        assert!(matches!(result, Err(SaveError::ReadOnly)), "{result:?}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "old\n");
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        std::fs::set_permissions(&path, permissions).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_left_at_a_temporary_name_is_not_touched() {
        let dir = dir("planted");
        let path = dir.join("a.csv");
        let planted = dir.join(".a.csv.csvedit-tmp");
        std::fs::write(&planted, "keep\n").unwrap();
        save(&table(&[&["a"]]), &Format::default(), &path).expect("saved");
        assert_eq!(std::fs::read_to_string(&planted).unwrap(), "keep\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn keeps_the_file_s_permissions() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = dir("mode");
        let path = dir.join("a.csv");
        std::fs::write(&path, "old\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        save(&table(&[&["a"]]), &Format::default(), &path).expect("saved");
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o640);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn saves_on_a_thread() {
        let dir = dir("thread");
        let path = dir.join("a.csv");
        let receiver = spawn(table(&[&["x", "y"]]), Format::default(), path.clone());
        receiver.recv().expect("a result").expect("saved");
        assert!(std::fs::read_to_string(&path).unwrap().starts_with("x,y"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
