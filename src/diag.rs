//! Diagnostics: cheap measures of how the program behaves (frame times,
//! freezes, how long the operations take) and the warnings, errors and
//! crashes it meets, kept in memory and in a log file. Help › Diagnostics
//! shows them as a report to copy (see `app::diagnostics`).
//!
//! Measuring costs a lock and two clock reads per frame or operation:
//! nothing is computed until the window asks for it.

use std::collections::{HashMap, VecDeque};
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// A frame longer than this is a freeze.
pub const FREEZE: Duration = Duration::from_millis(100);
/// Frame times are kept per second, for this many seconds.
const SECONDS: usize = 300;
const MAX_FREEZES: usize = 100;
const LOG_CAPACITY: usize = 200;
/// The log file is set aside (one older file kept) beyond this size.
const LOG_FILE_MAX: u64 = 1 << 20;

static START: LazyLock<Instant> = LazyLock::new(Instant::now);
static MEASURES: LazyLock<Mutex<Measures>> = LazyLock::new(|| Mutex::new(Measures::default()));
static LOG: LazyLock<Mutex<VecDeque<Entry>>> = LazyLock::new(|| Mutex::new(VecDeque::new()));

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Starts the session's clock.
pub fn start() {
    LazyLock::force(&START);
}

/// Since the program started.
pub fn uptime() -> Duration {
    START.elapsed()
}

/// One operation's statistics.
#[derive(Clone, Copy, Debug, Default)]
pub struct OpStats {
    pub count: u32,
    pub total: Duration,
    pub max: Duration,
    pub last: Duration,
}

/// A frame longer than [`FREEZE`], and the operations it ran.
#[derive(Clone, Debug)]
pub struct Freeze {
    pub at: Duration,
    pub took: Duration,
    pub ran: Vec<(&'static str, Duration)>,
}

/// Frame times over a period.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameStats {
    pub frames: u32,
    pub average: Duration,
    pub max: Duration,
}

#[derive(Default)]
struct Measures {
    /// (second since the start, frames, total, max), the last ones.
    seconds: VecDeque<(u64, u32, Duration, Duration)>,
    freezes: VecDeque<Freeze>,
    operations: HashMap<&'static str, OpStats>,
    /// The operations run on the window's thread during this frame.
    this_frame: Vec<(&'static str, Duration)>,
    frame_thread: Option<std::thread::ThreadId>,
}

/// Times `f` as the operation `name`.
pub fn measure<T>(name: &'static str, f: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let result = f();
    record(name, start.elapsed());
    result
}

pub fn record(name: &'static str, took: Duration) {
    let mut m = lock(&MEASURES);
    let stats = m.operations.entry(name).or_default();
    stats.count += 1;
    stats.total += took;
    stats.max = stats.max.max(took);
    stats.last = took;
    if m.frame_thread == Some(std::thread::current().id()) {
        m.this_frame.push((name, took));
    }
}

/// The end of a frame that began at `began` and took `took` (all of it:
/// the interface built, laid out and painted), with the operations
/// recorded since the last frame ended.
pub fn end_frame(began: Instant, took: Duration) {
    let at = began.saturating_duration_since(*START);
    let mut m = lock(&MEASURES);
    m.frame_thread = Some(std::thread::current().id());
    let second = at.as_secs();
    match m.seconds.back_mut() {
        Some(s) if s.0 == second => {
            s.1 += 1;
            s.2 += took;
            s.3 = s.3.max(took);
        }
        _ => {
            m.seconds.push_back((second, 1, took, took));
            if m.seconds.len() > SECONDS {
                m.seconds.pop_front();
            }
        }
    }
    let ran = std::mem::take(&mut m.this_frame);
    if took >= FREEZE {
        m.freezes.push_back(Freeze { at, took, ran });
        if m.freezes.len() > MAX_FREEZES {
            m.freezes.pop_front();
        }
    }
}

/// Frames over the last `seconds` seconds.
pub fn frames(seconds: u64) -> FrameStats {
    let m = lock(&MEASURES);
    let now = START.elapsed().as_secs();
    let mut stats = FrameStats::default();
    let mut total = Duration::ZERO;
    for &(second, frames, sum, max) in &m.seconds {
        if now.saturating_sub(second) < seconds {
            stats.frames += frames;
            total += sum;
            stats.max = stats.max.max(max);
        }
    }
    if stats.frames > 0 {
        stats.average = total / stats.frames;
    }
    stats
}

pub fn freezes() -> Vec<Freeze> {
    lock(&MEASURES).freezes.iter().cloned().collect()
}

/// The operations, the slowest first.
pub fn operations() -> Vec<(&'static str, OpStats)> {
    let mut ops: Vec<_> = lock(&MEASURES)
        .operations
        .iter()
        .map(|(n, s)| (*n, *s))
        .collect();
    ops.sort_by_key(|op| std::cmp::Reverse(op.1.max));
    ops
}

// Log

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Warning,
    Error,
    Panic,
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub at: Duration,
    pub level: Level,
    pub message: String,
}

/// Notes `message` in the session's log and the log file, without
/// personal paths. It must not hold file names or cell text: the log file
/// is kept across sessions and shown in the reports sent.
pub fn log(level: Level, message: &str) {
    let message = scrub(message);
    let entry = Entry {
        at: START.elapsed(),
        level,
        message,
    };
    append_to_file(&entry);
    let mut log = lock(&LOG);
    log.push_back(entry);
    if log.len() > LOG_CAPACITY {
        log.pop_front();
    }
}

pub fn entries() -> Vec<Entry> {
    lock(&LOG).iter().cloned().collect()
}

fn log_file() -> PathBuf {
    crate::config::dir().join("diagnostics.log")
}

/// The log file set aside once too large.
fn old_log_file() -> PathBuf {
    log_file().with_extension("old.log")
}

fn append_to_file(entry: &Entry) {
    let file = log_file();
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::metadata(&file).is_ok_and(|m| m.len() > LOG_FILE_MAX) {
        let _ = std::fs::rename(&file, old_log_file());
    }
    // One line, one write: lines of two running copies don't mix.
    let line = format!(
        "{} {} v{} {:?}: {}\n",
        humantime(std::time::SystemTime::now()),
        std::process::id(),
        env!("CARGO_PKG_VERSION"),
        entry.level,
        entry.message.replace(['\n', '\r'], " ")
    );
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    // Read by the user only.
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    if let Ok(mut f) = options.open(&file) {
        let _ = f.write_all(line.as_bytes());
    }
}

/// The bytes read from the end of a log file for its last lines.
const TAIL_BYTES: u64 = 16 << 10;

/// The end of the log file: what earlier sessions met (a crash shows
/// there, since the session that crashed couldn't show it), from the
/// older file too when the log was just set aside.
pub fn log_file_tail(lines: usize) -> Vec<String> {
    let mut tail = file_tail(&log_file(), lines);
    if tail.len() < lines {
        let mut older = file_tail(&old_log_file(), lines - tail.len());
        older.append(&mut tail);
        tail = older;
    }
    tail
}

/// The last `lines` lines of `path`, read from its last bytes only; bytes
/// that aren't UTF-8 (a line cut by a crash) are replaced.
fn file_tail(path: &std::path::Path, lines: usize) -> Vec<String> {
    use std::io::{Read as _, Seek as _, SeekFrom};
    let Ok(mut file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let len = file.metadata().map_or(0, |m| m.len());
    let start = len.saturating_sub(TAIL_BYTES);
    let mut bytes = Vec::new();
    if file.seek(SeekFrom::Start(start)).is_err() || file.read_to_end(&mut bytes).is_err() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&bytes);
    let mut all: Vec<&str> = text.lines().collect();
    // The first line, cut by where the reading started.
    if start > 0 && !all.is_empty() {
        all.remove(0);
    }
    all[all.len().saturating_sub(lines)..]
        .iter()
        .map(|l| (*l).to_owned())
        .collect()
}

/// "2026-11-11 14:03:07 UTC".
pub fn humantime(time: std::time::SystemTime) -> String {
    let secs = time
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (days, rest) = ((secs / 86_400) as i64, secs % 86_400);
    // Days to a civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC",
        rest / 3600,
        rest / 60 % 60,
        rest % 60
    )
}

/// Logs every panic (any thread) with where it happened, then lets the
/// default hook print it.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info.payload();
        let message = payload
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "(no message)".to_owned());
        let place = info
            .location()
            .map(|l| format!(" at {}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_default();
        let thread = std::thread::current();
        let thread = thread.name().unwrap_or("unnamed").to_owned();
        let message = redact_panic(&message);
        log(Level::Panic, &format!("{message}{place} (thread {thread})"));
        previous(info);
    }));
}

// Privacy

/// Most characters of a panic's message kept.
const PANIC_MESSAGE_MAX: usize = 160;

/// A panic's message without what it quotes (std quotes the text it was
/// given, a cell's maybe, between backticks or single quotes), shortened.
fn redact_panic(message: &str) -> String {
    let mut out = String::new();
    let mut quote = None;
    for c in message.chars() {
        match quote {
            Some(q) if c == q => {
                out.push('…');
                out.push(c);
                quote = None;
            }
            Some(_) => {}
            None => {
                out.push(c);
                if matches!(c, '`' | '\'') {
                    quote = Some(c);
                }
            }
        }
    }
    if quote.is_some() {
        out.push('…');
    }
    if out.chars().count() > PANIC_MESSAGE_MAX {
        out = out.chars().take(PANIC_MESSAGE_MAX).collect();
        out.push('…');
    }
    out
}

/// The user's home folders, as written in paths: on Windows its profile
/// folder and, when set (by Git or MSYS), HOME too.
fn homes() -> Vec<String> {
    let vars: &[&str] = if cfg!(windows) {
        &["USERPROFILE", "HOME"]
    } else {
        &["HOME"]
    };
    vars.iter()
        .filter_map(std::env::var_os)
        .map(|home| {
            home.to_string_lossy()
                .trim_end_matches(['/', '\\'])
                .to_owned()
        })
        // A folder below the root ("/home/al", "C:\\Users\\Al"), not a
        // drive ("H:") nor the root itself.
        .filter(|home| home.split(['/', '\\']).filter(|p| !p.is_empty()).count() >= 2)
        .collect()
}

/// `text` with the home folder written `~`, so that no user name shows.
pub fn scrub(text: &str) -> String {
    scrub_homes(text, &homes(), cfg!(windows))
}

/// `text` with each of `homes` (written with either separator) replaced
/// by `~` where it is a whole path, not part of a longer name; ignoring
/// ASCII case when `fold`, as Windows paths do.
fn scrub_homes(text: &str, homes: &[String], fold: bool) -> String {
    let mut text = text.to_owned();
    for home in homes {
        for home in [home.replace('\\', "/"), home.replace('/', "\\")] {
            text = replace_path(&text, &home, fold);
        }
    }
    text
}

fn replace_path(text: &str, path: &str, fold: bool) -> String {
    if path.is_empty() {
        return text.to_owned();
    }
    // ASCII folding keeps every byte where it is.
    let (haystack, needle) = if fold {
        (text.to_ascii_lowercase(), path.to_ascii_lowercase())
    } else {
        (text.to_owned(), path.to_owned())
    };
    let in_name = |c: char| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | '\\');
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut from = 0;
    while let Some(found) = haystack[from..].find(&needle) {
        let start = from + found;
        let end = start + needle.len();
        let before = text[..start].chars().next_back();
        let after = text[end..].chars().next();
        let whole = !before.is_some_and(in_name)
            && after.is_none_or(|c| matches!(c, '/' | '\\') || !in_name(c));
        if whole {
            out.push_str(&text[copied..start]);
            out.push('~');
            copied = end;
            from = end;
        } else {
            from = start + text[start..].chars().next().map_or(1, char::len_utf8);
        }
    }
    out.push_str(&text[copied..]);
    out
}

// Memory

/// The process's memory, in bytes.
#[derive(Clone, Copy, Debug)]
pub struct Memory {
    /// In RAM now (Linux RSS, Windows working set).
    pub resident: u64,
    /// The process's own (heap, stacks), when the system says.
    pub private: Option<u64>,
}

pub fn process_memory() -> Option<Memory> {
    platform::memory()
}

#[cfg(target_os = "linux")]
mod platform {
    use super::Memory;

    /// A "Name:   1234 kB" line of /proc/self/status, in bytes.
    fn status_field(status: &str, field: &str) -> Option<u64> {
        let line = status.lines().find(|l| l.starts_with(field))?;
        let mut words = line[field.len()..].split_whitespace();
        let value: u64 = words.next()?.parse().ok()?;
        Some(match words.next() {
            Some("kB") => value * 1024,
            _ => value,
        })
    }

    pub fn memory() -> Option<Memory> {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        Some(Memory {
            resident: status_field(&status, "VmRSS:")?,
            private: status_field(&status, "RssAnon:"),
        })
    }
}

#[cfg(windows)]
mod platform {
    use super::Memory;

    /// PROCESS_MEMORY_COUNTERS_EX.
    #[repr(C)]
    #[derive(Default)]
    struct Counters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
        private_usage: usize,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut core::ffi::c_void;
        fn K32GetProcessMemoryInfo(
            process: *mut core::ffi::c_void,
            counters: *mut Counters,
            cb: u32,
        ) -> i32;
    }

    pub fn memory() -> Option<Memory> {
        let mut c = Counters {
            cb: size_of::<Counters>() as u32,
            ..Default::default()
        };
        // SAFETY: the pseudo-handle of the current process needs no
        // closing; the call fills the structure whose size it is given.
        let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
        (ok != 0).then_some(Memory {
            resident: c.working_set_size as u64,
            private: Some(c.private_usage as u64),
        })
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
mod platform {
    use super::Memory;

    pub fn memory() -> Option<Memory> {
        None
    }
}

/// 1.5 MB, 312 kB…
pub fn bytes(n: u64) -> String {
    match n {
        n if n >= 1 << 30 => format!("{:.2} GB", n as f64 / (1u64 << 30) as f64),
        n if n >= 1 << 20 => format!("{:.1} MB", n as f64 / (1u64 << 20) as f64),
        n if n >= 1 << 10 => format!("{:.0} kB", n as f64 / 1024.0),
        n => format!("{n} B"),
    }
}

/// 3 ms, 1.24 s.
pub fn ms(d: Duration) -> String {
    if d >= Duration::from_secs(1) {
        format!("{:.2} s", d.as_secs_f64())
    } else {
        format!("{:.1} ms", d.as_secs_f64() * 1000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures_frames_operations_and_freezes() {
        measure("test.op", || std::thread::sleep(Duration::from_millis(2)));
        end_frame(Instant::now(), Duration::from_millis(1));
        record("test.op", Duration::from_millis(5));
        end_frame(Instant::now(), Duration::from_millis(150));
        let ops = operations();
        let op = ops
            .iter()
            .find(|(n, _)| *n == "test.op")
            .expect("measured")
            .1;
        assert_eq!(op.count, 2);
        assert!(op.max >= Duration::from_millis(5));
        assert!(frames(10).frames >= 2);
        assert!(freezes().iter().any(|f| f.took >= FREEZE));
    }

    #[test]
    fn dates_and_sizes_read_well() {
        let t = std::time::UNIX_EPOCH + Duration::from_secs(1_789_000_000);
        assert_eq!(humantime(t), "2026-09-10 00:26:40 UTC");
        assert_eq!(bytes(1536), "2 kB");
        assert_eq!(bytes(3 << 20), "3.0 MB");
        assert_eq!(ms(Duration::from_micros(2500)), "2.5 ms");
    }

    #[test]
    fn reports_hide_the_home_folder() {
        if let Some(home) = homes().first() {
            assert_eq!(scrub(&format!("{home}/data/x.csv")), "~/data/x.csv");
        }
        let homes = ["C:\\Users\\Al".to_owned()];
        let scrub = |text: &str, fold| scrub_homes(text, &homes, fold);
        assert_eq!(scrub("in C:\\Users\\Al\\x.csv", false), "in ~\\x.csv");
        assert_eq!(scrub("C:/Users/Al/x.csv", false), "~/x.csv");
        assert_eq!(scrub("c:\\users\\al\\x.csv", true), "~\\x.csv");
        assert_eq!(scrub("c:\\users\\al\\x.csv", false), "c:\\users\\al\\x.csv");
        // Another user's folder that starts the same is left alone.
        assert_eq!(
            scrub("C:\\Users\\Alice\\x.csv", true),
            "C:\\Users\\Alice\\x.csv"
        );
        assert_eq!(scrub("(C:\\Users\\Al)", true), "(~)");
        let unix = ["/home/al".to_owned()];
        assert_eq!(
            scrub_homes("/home/al/a /home/alice/b /mnt/home/al/c", &unix, false),
            "~/a /home/alice/b /mnt/home/al/c"
        );
    }

    #[test]
    fn panics_keep_no_quoted_text() {
        let message =
            "byte index 3 is not a char boundary; it is inside 'é' (bytes 2..4) of `Zoé secret`";
        assert_eq!(
            redact_panic(message),
            "byte index 3 is not a char boundary; it is inside '…' (bytes 2..4) of `…`"
        );
        assert_eq!(redact_panic("cut `abc"), "cut `…");
        assert_eq!(
            redact_panic(&"x".repeat(500)).chars().count(),
            PANIC_MESSAGE_MAX + 1
        );
    }

    #[test]
    fn log_tails_read_the_last_lines_only() {
        let dir = std::env::temp_dir().join(format!("csvedit-tail-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("log");
        let mut text = String::new();
        for i in 0..5000 {
            text.push_str(&format!("line {i}\n"));
        }
        let mut bytes = text.into_bytes();
        // A cut line from a crash: not UTF-8.
        bytes.extend_from_slice(b"bad \xFF byte\nlast\n");
        std::fs::write(&path, bytes).unwrap();
        let tail = file_tail(&path, 3);
        assert_eq!(tail, ["line 4999", "bad \u{FFFD} byte", "last"]);
        assert_eq!(file_tail(&dir.join("none"), 3), Vec::<String>::new());
        let _ = std::fs::remove_dir_all(dir);
    }
}
