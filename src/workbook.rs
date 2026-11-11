//! Spreadsheet files (.xlsx, .xlsm, .xlsb, .xls, .ods), read with calamine:
//! cell values only (formulas give the result saved with the file, and
//! their text), never macros, links or embedded objects.
//!
//! A crafted file could make calamine ask for a huge amount of memory (it
//! sets aside room for every cell between A1 and the last one) or fail: the
//! file is read by a separate process, CSV Edit started again with
//! `--read-workbook`, so that at worst that process ends and the file is
//! called unreadable. The reading is also capped: the uncompressed size of
//! zip-based files, the number of cells and formulas, the text they hold,
//! and the time it takes. The answer is checked against the same caps.

use std::io::{BufReader, BufWriter, Cursor, Read, Write};
use std::path::Path;
use std::time::Duration;

use calamine::{Data, DataRef, Reader, Sheets};

use crate::table::{Cell, MAX_CELLS};

/// The files read as workbooks, by extension.
pub const EXTENSIONS: [&str; 5] = ["xlsx", "xlsm", "xlsb", "xls", "ods"];

/// Largest size of a zip-based workbook's parts once uncompressed: a few
/// kilobytes can unzip to gigabytes.
const MAX_UNZIPPED: u64 = 1 << 30;

/// Most text, in bytes, a sheet's cells, formulas and sheet names hold in
/// all: one long shared string used by every cell would otherwise be
/// copied into each of them.
const MAX_TEXT: usize = 1 << 30;

/// Most sheets a workbook's answer names.
const MAX_SHEETS: usize = 1 << 16;

/// Longest a reading takes before its process is killed.
pub const READ_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// The argument that starts CSV Edit as the workbook reader.
pub const READER_ARG: &str = "--read-workbook";

pub fn is_workbook(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// A sheet read: its sheets' names, and the chosen sheet's cells from A1
/// (so that columns keep their letters) and formulas.
#[derive(Debug, Default, PartialEq)]
pub struct Workbook {
    pub sheets: Vec<String>,
    pub rows: Vec<Vec<Cell>>,
    /// Inside `rows`.
    pub formulas: Vec<Formula>,
}

/// (row, column, formula without its "=").
pub type Formula = (usize, usize, String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkbookError {
    /// Not a workbook calamine can read, or damaged.
    Unreadable(String),
    /// More cells or text than CSV Edit opens, or a zip that unzips too
    /// large.
    TooLarge,
    /// The reading process ended without an answer (out of memory).
    Crashed,
    /// The reading took longer than [`READ_TIMEOUT`].
    TimedOut,
}

/// Reads sheet `index` of the workbook at `path`, in a separate process;
/// `None` once `cancel` stops it (the process is killed).
pub fn read(
    path: &Path,
    index: usize,
    cancel: &crate::cancel::Cancel,
) -> Option<Result<Workbook, WorkbookError>> {
    if cfg!(test) {
        // The test binary can't be started as the reader.
        let read = std::fs::read(path)
            .map_err(|e| WorkbookError::Unreadable(e.to_string()))
            .and_then(|bytes| read_bytes(&bytes, index));
        return (!cancel.is_cancelled()).then_some(read);
    }
    let unreadable = |e: std::io::Error| WorkbookError::Unreadable(e.to_string());
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(error) => return Some(Err(unreadable(error))),
    };
    let mut command = std::process::Command::new(exe);
    command
        .arg(READER_ARG)
        .arg(path)
        .arg(index.to_string())
        // Held open while the reader runs: it ends once it closes, when
        // CSV Edit ends without killing it.
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        // No console window for the reader.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => return Some(Err(unreadable(error))),
    };
    let stdin = child.stdin.take();
    // Both pipes are emptied while the process runs, so that it never
    // waits on a full one. The answer is decoded as it comes, within the
    // caps; a wrong one closes the pipe, and the reader then ends.
    let stdout = child.stdout.take().map(|pipe| {
        std::thread::spawn(move || {
            let mut input = BufReader::new(pipe);
            decode(&mut input).filter(|_| input.read(&mut [0]).is_ok_and(|n| n == 0))
        })
    });
    let stderr = child.stderr.take().map(|pipe| {
        std::thread::spawn(move || {
            // A message, at most a few kilobytes of it.
            let mut bytes = Vec::new();
            let mut pipe = pipe;
            let _ = pipe.by_ref().take(64 << 10).read_to_end(&mut bytes);
            let _ = std::io::copy(&mut pipe, &mut std::io::sink());
            bytes
        })
    });
    let status = match cancel.wait(child, READ_TIMEOUT) {
        Ok(Some(status)) => status,
        // Killed: its pipes close, and the threads above end.
        Ok(None) => return None,
        Err(error) if error.kind() == std::io::ErrorKind::TimedOut => {
            return Some(Err(WorkbookError::TimedOut));
        }
        Err(error) => return Some(Err(unreadable(error))),
    };
    drop(stdin);
    let stdout = stdout.and_then(|t| t.join().ok()).flatten();
    let stderr = stderr.and_then(|t| t.join().ok()).unwrap_or_default();
    Some(match status.code() {
        Some(0) => stdout.ok_or(WorkbookError::Crashed),
        Some(3) => Err(WorkbookError::TooLarge),
        Some(2) => Err(WorkbookError::Unreadable(
            String::from_utf8_lossy(&stderr).trim().to_owned(),
        )),
        _ => Err(WorkbookError::Crashed),
    })
}

/// The reader process: reads the file named in `args` and writes the
/// result to the standard output. Its exit code says how it went.
pub fn reader_main(args: &[std::ffi::OsString]) -> i32 {
    // Ends once CSV Edit, which holds the other end, is gone.
    std::thread::spawn(|| {
        let mut stdin = std::io::stdin().lock();
        let mut byte = [0];
        while stdin.read(&mut byte).is_ok_and(|n| n > 0) {}
        std::process::exit(5);
    });
    let (Some(path), Some(index)) = (args.first(), args.get(1)) else {
        return 2;
    };
    let index = index.to_string_lossy().parse().unwrap_or(0);
    let result = std::fs::read(path)
        .map_err(|e| WorkbookError::Unreadable(e.to_string()))
        .and_then(|bytes| read_bytes(&bytes, index));
    match result {
        Ok(workbook) => {
            let mut out = BufWriter::new(std::io::stdout().lock());
            if encode(&workbook, &mut out)
                .and_then(|()| out.flush())
                .is_err()
            {
                return 4;
            }
            0
        }
        Err(WorkbookError::TooLarge) => 3,
        Err(WorkbookError::Unreadable(message)) => {
            eprintln!("{message}");
            2
        }
        Err(WorkbookError::Crashed | WorkbookError::TimedOut) => 4,
    }
}

/// Reads sheet `index` of a workbook's bytes (in the reader process).
pub fn read_bytes(bytes: &[u8], index: usize) -> Result<Workbook, WorkbookError> {
    read_capped(bytes, index, MAX_TEXT)
}

/// [`read_bytes`], with at most `max_text` bytes of text.
fn read_capped(bytes: &[u8], index: usize, max_text: usize) -> Result<Workbook, WorkbookError> {
    check_zip(bytes, MAX_UNZIPPED)?;
    let unreadable = |e: calamine::Error| WorkbookError::Unreadable(e.to_string());
    let mut workbook =
        calamine::open_workbook_auto_from_rs(Cursor::new(bytes)).map_err(unreadable)?;
    let sheets = workbook.sheet_names();
    let mut cells = Cells::new(max_text);
    if sheets.len() > MAX_SHEETS {
        return Err(WorkbookError::TooLarge);
    }
    for name in &sheets {
        cells.spend(name.len())?;
    }
    let Some(name) = sheets.get(index).or(sheets.first()).cloned() else {
        return Ok(Workbook::default());
    };
    match &mut workbook {
        // Read cell by cell: nothing is set aside for empty cells.
        Sheets::Xlsx(xlsx) => {
            let error = |e: calamine::XlsxError| WorkbookError::Unreadable(e.to_string());
            let mut reader = xlsx.worksheet_cells_reader(&name).map_err(error)?;
            // Values and formulas in one pass.
            while let Some(cell) = reader.next_cell_with_formula().map_err(error)? {
                cells.add(cell.pos, cell_of_ref(&cell.value))?;
                if let Some(formula) = &cell.formula {
                    cells.add_formula(cell.pos, formula)?;
                }
            }
        }
        // No single pass for both here.
        Sheets::Xlsb(xlsb) => {
            let error = |e: calamine::XlsbError| WorkbookError::Unreadable(e.to_string());
            let mut reader = xlsb.worksheet_cells_reader(&name).map_err(error)?;
            while let Some(cell) = reader.next_cell().map_err(error)? {
                cells.add(cell.get_position(), cell_of_ref(cell.get_value()))?;
            }
            drop(reader);
            let mut reader = xlsb.worksheet_cells_reader(&name).map_err(error)?;
            while let Some(cell) = reader.next_formula().map_err(error)? {
                cells.add_formula(cell.get_position(), cell.get_value())?;
            }
        }
        // Older formats, smaller by design (.xls has at most 65,536 rows).
        _ => {
            let range = workbook.worksheet_range(&name).map_err(unreadable)?;
            let (top, left) = range.start().unwrap_or((0, 0));
            for (row, col, value) in range.used_cells() {
                cells.add(
                    (top + row as u32, left + col as u32),
                    Cell::from(text_of(value)),
                )?;
            }
            if let Ok(range) = workbook.worksheet_formula(&name) {
                let (top, left) = range.start().unwrap_or((0, 0));
                for (row, col, formula) in range.used_cells() {
                    cells.add_formula((top + row as u32, left + col as u32), formula)?;
                }
            }
        }
    }
    let (rows, formulas) = cells.finish();
    Ok(Workbook {
        sheets,
        rows,
        formulas,
    })
}

/// Refuses a zip whose parts would unzip larger than `limit` in all. Any
/// file the zip reader opens is checked, whatever comes before the zip.
fn check_zip(bytes: &[u8], limit: u64) -> Result<(), WorkbookError> {
    // Not a zip (an .xls, say): calamine won't open it as one either.
    let Ok(mut archive) = zip::ZipArchive::new(Cursor::new(bytes)) else {
        return Ok(());
    };
    let mut total: u64 = 0;
    for i in 0..archive.len() {
        let file = archive
            .by_index_raw(i)
            .map_err(|e| WorkbookError::Unreadable(e.to_string()))?;
        total = total.saturating_add(file.size());
        if total > limit {
            return Err(WorkbookError::TooLarge);
        }
    }
    Ok(())
}

/// The cells and formulas found, kept apart until the sheet's size is
/// known, within the caps.
struct Cells {
    cells: Vec<((u32, u32), Cell)>,
    formulas: Vec<(u32, u32, String)>,
    rows: usize,
    cols: usize,
    /// Text bytes still allowed.
    text_left: usize,
    /// Most cells, and most formulas.
    max_cells: usize,
}

impl Cells {
    fn new(max_text: usize) -> Self {
        Self {
            cells: Vec::new(),
            formulas: Vec::new(),
            rows: 0,
            cols: 0,
            text_left: max_text,
            max_cells: MAX_CELLS,
        }
    }

    fn spend(&mut self, bytes: usize) -> Result<(), WorkbookError> {
        self.text_left = self
            .text_left
            .checked_sub(bytes)
            .ok_or(WorkbookError::TooLarge)?;
        Ok(())
    }

    fn add(&mut self, (row, col): (u32, u32), text: Cell) -> Result<(), WorkbookError> {
        if text.is_empty() {
            return Ok(());
        }
        self.rows = self.rows.max(row as usize + 1);
        self.cols = self.cols.max(col as usize + 1);
        // The table pads every row to the widest: the cells it will hold.
        // A cell given again and again counts each time.
        if self.rows.saturating_mul(self.cols) > self.max_cells
            || self.cells.len() >= self.max_cells
        {
            return Err(WorkbookError::TooLarge);
        }
        self.spend(text.len())?;
        self.cells.push(((row, col), text));
        Ok(())
    }

    fn add_formula(&mut self, (row, col): (u32, u32), text: &str) -> Result<(), WorkbookError> {
        if text.is_empty() {
            return Ok(());
        }
        if self.formulas.len() >= self.max_cells {
            return Err(WorkbookError::TooLarge);
        }
        self.spend(text.len())?;
        self.formulas.push((row, col, text.to_owned()));
        Ok(())
    }

    /// The rows, and the formulas inside them.
    fn finish(self) -> (Vec<Vec<Cell>>, Vec<Formula>) {
        let (rows, cols) = (self.rows, self.cols);
        let mut grid = vec![vec![Cell::default(); cols]; rows];
        for ((row, col), text) in self.cells {
            grid[row as usize][col as usize] = text;
        }
        let formulas = self
            .formulas
            .into_iter()
            .map(|(row, col, text)| (row as usize, col as usize, text))
            .filter(|&(row, col, _)| row < rows && col < cols)
            .collect();
        (grid, formulas)
    }
}

/// A cell's value as text: numbers as written by Rust (shortest exact
/// form), dates and times as ISO 8601, errors as Excel shows them.
fn text_of(value: &Data) -> String {
    match value {
        Data::Int(n) => n.to_string(),
        Data::Float(f) => float_text(*f),
        Data::String(s) | Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
        Data::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_owned(),
        Data::DateTime(date) => date_text(date),
        Data::Error(e) => e.to_string(),
        Data::Empty => String::new(),
    }
}

fn cell_of_ref(value: &DataRef<'_>) -> Cell {
    match value {
        DataRef::SharedString(s) => Cell::new(s),
        other => Cell::from(text_of(&Data::from(other.clone()))),
    }
}

/// 3 rather than 3.0; 0.1 rather than 0.1000000000000000055.
fn float_text(f: f64) -> String {
    if f.fract() == 0.0 && f.abs() < 1e15 {
        format!("{}", f as i64)
    } else {
        format!("{f}")
    }
}

/// 2026-07-01, 2026-07-01 17:32:30, 17:32:30 (a time alone), 36:15:00 (a
/// duration).
fn date_text(date: &calamine::ExcelDateTime) -> String {
    if date.is_duration() {
        let seconds = (date.as_f64() * 86_400.0).round() as i64;
        let sign = if seconds < 0 { "-" } else { "" };
        let seconds = seconds.abs();
        return format!(
            "{sign}{}:{:02}:{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        );
    }
    let (year, month, day, hour, minute, second, milli) = date.to_ymd_hms_milli();
    let time = if milli > 0 {
        format!("{hour:02}:{minute:02}:{second:02}.{milli:03}")
    } else {
        format!("{hour:02}:{minute:02}:{second:02}")
    };
    let value = date.as_f64();
    if value < 1.0 {
        time
    } else if value.fract() == 0.0 {
        format!("{year:04}-{month:02}-{day:02}")
    } else {
        format!("{year:04}-{month:02}-{day:02} {time}")
    }
}

// The reader's answer: lengths in 4 bytes (little-endian), then the text.

/// Writes `workbook` as the reader's answer, as it goes.
fn encode(workbook: &Workbook, out: &mut impl Write) -> std::io::Result<()> {
    fn number(out: &mut impl Write, n: usize) -> std::io::Result<()> {
        out.write_all(&(n as u32).to_le_bytes())
    }
    fn text(out: &mut impl Write, s: &str) -> std::io::Result<()> {
        number(out, s.len())?;
        out.write_all(s.as_bytes())
    }
    number(out, workbook.sheets.len())?;
    for name in &workbook.sheets {
        text(out, name)?;
    }
    number(out, workbook.rows.len())?;
    number(out, workbook.rows.first().map_or(0, Vec::len))?;
    for row in &workbook.rows {
        for cell in row {
            text(out, cell)?;
        }
    }
    number(out, workbook.formulas.len())?;
    for (row, col, formula) in &workbook.formulas {
        number(out, *row)?;
        number(out, *col)?;
        text(out, formula)?;
    }
    Ok(())
}

/// Reads the reader's answer, within the reader's caps: `None` when it is
/// wrong, without reading more than those caps allow.
fn decode(input: &mut impl Read) -> Option<Workbook> {
    decode_capped(input, MAX_TEXT)
}

fn decode_capped(input: &mut impl Read, max_text: usize) -> Option<Workbook> {
    let mut text_left = max_text;
    let number = |input: &mut dyn Read| -> Option<usize> {
        let mut four = [0; 4];
        input.read_exact(&mut four).ok()?;
        Some(u32::from_le_bytes(four) as usize)
    };
    // Checked against the text left before anything is set aside.
    let mut bytes = |input: &mut dyn Read| -> Option<Vec<u8>> {
        let len = number(input)?;
        text_left = text_left.checked_sub(len)?;
        let mut bytes = Vec::with_capacity(len.min(1 << 16));
        input.take(len as u64).read_to_end(&mut bytes).ok()?;
        (bytes.len() == len).then_some(bytes)
    };
    let mut workbook = Workbook::default();
    let sheets = number(input)?;
    if sheets > MAX_SHEETS {
        return None;
    }
    for _ in 0..sheets {
        workbook.sheets.push(String::from_utf8(bytes(input)?).ok()?);
    }
    let (rows, cols) = (number(input)?, number(input)?);
    // Rows without columns are not an answer the reader gives.
    if rows.saturating_mul(cols) > MAX_CELLS || (rows > 0 && cols == 0) {
        return None;
    }
    for _ in 0..rows {
        let row = (0..cols)
            .map(|_| {
                let text = bytes(input)?;
                Some(Cell::new(std::str::from_utf8(&text).ok()?))
            })
            .collect::<Option<Vec<_>>>()?;
        workbook.rows.push(row);
    }
    let formulas = number(input)?;
    if formulas > rows * cols {
        return None;
    }
    for _ in 0..formulas {
        let (row, col) = (number(input)?, number(input)?);
        if row >= rows || col >= cols {
            return None;
        }
        let text = String::from_utf8(bytes(input)?).ok()?;
        workbook.formulas.push((row, col, text));
    }
    Some(workbook)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use rust_xlsxwriter::{ExcelDateTime, Format, Workbook as Writer};

    pub(crate) fn sample() -> Vec<u8> {
        let mut book = Writer::new();
        let sheet = book.add_worksheet().set_name("Data").unwrap();
        sheet.write_string(0, 0, "Name").unwrap();
        sheet.write_string(0, 1, "Price").unwrap();
        sheet.write_string(0, 2, "Date").unwrap();
        sheet.write_string(1, 0, "Café").unwrap();
        sheet.write_number(1, 1, 2.5).unwrap();
        let date = ExcelDateTime::parse_from_str("2026-07-01 17:32:30").unwrap();
        let format = Format::new().set_num_format("dd/mm/yyyy hh:mm:ss");
        sheet
            .write_datetime_with_format(1, 2, &date, &format)
            .unwrap();
        sheet.write_number(2, 1, 4.0).unwrap();
        sheet
            .write_formula(
                3,
                1,
                rust_xlsxwriter::Formula::new("=SUM(B2+B3)").set_result("6.5"),
            )
            .unwrap();
        book.add_worksheet()
            .set_name("Other")
            .unwrap()
            .write_string(4, 3, "x")
            .unwrap();
        book.save_to_buffer().unwrap()
    }

    #[test]
    fn reads_values_dates_and_formulas() {
        let workbook = read_bytes(&sample(), 0).unwrap();
        assert_eq!(workbook.sheets, ["Data", "Other"]);
        assert_eq!(workbook.rows[0], ["Name", "Price", "Date"]);
        assert_eq!(workbook.rows[1], ["Café", "2.5", "2026-07-01 17:32:30"]);
        assert_eq!(workbook.rows[3][1], "6.5");
        assert_eq!(workbook.formulas, [(3, 1, "SUM(B2+B3)".to_owned())]);
        // Cells stay where Excel has them: D5 is D5.
        let other = read_bytes(&sample(), 1).unwrap();
        assert_eq!((other.rows.len(), other.rows[4][3].as_str()), (5, "x"));
    }

    /// Writes the sample workbook to the file named by `CSVEDIT_SAMPLE`,
    /// to try the reader process by hand: `cargo test sample -- --ignored`.
    #[test]
    #[ignore = "writes a file"]
    fn sample_workbook() {
        let path = std::env::var("CSVEDIT_SAMPLE").expect("CSVEDIT_SAMPLE");
        std::fs::write(path, sample()).unwrap();
    }

    #[test]
    fn answers_survive_the_trip_between_processes() {
        let workbook = read_bytes(&sample(), 0).unwrap();
        let mut answer = Vec::new();
        encode(&workbook, &mut answer).unwrap();
        assert_eq!(decode(&mut answer.as_slice()), Some(workbook));
        assert_eq!(decode(&mut &b"\x05\x00"[..]), None);
    }

    fn answer(numbers: &[u32]) -> Vec<u8> {
        numbers.iter().flat_map(|n| n.to_le_bytes()).collect()
    }

    #[test]
    fn wrong_answers_are_refused_before_anything_is_set_aside() {
        // No sheets, 4 billion rows of no columns.
        assert_eq!(decode(&mut answer(&[0, u32::MAX, 0]).as_slice()), None);
        // More sheets than any workbook.
        assert_eq!(decode(&mut answer(&[u32::MAX]).as_slice()), None);
        // A cell claiming 4 GB of text.
        assert_eq!(decode(&mut answer(&[0, 1, 1, u32::MAX]).as_slice()), None);
        // A formula outside the cells.
        let mut far = answer(&[0, 1, 1, 1]);
        far.push(b'a');
        far.extend(answer(&[1, 0, 5, 1]));
        far.push(b'x');
        assert_eq!(decode(&mut far.as_slice()), None);
        // Text past the cap, in all.
        let mut two = answer(&[0, 1, 2, 3]);
        two.extend(b"abc");
        two.extend(answer(&[3]));
        two.extend(b"def");
        two.extend(answer(&[0]));
        assert!(decode_capped(&mut two.as_slice(), 6).is_some());
        assert_eq!(decode_capped(&mut two.as_slice(), 5), None);
    }

    #[test]
    fn text_shared_by_many_cells_counts_for_each() {
        let mut book = Writer::new();
        let sheet = book.add_worksheet();
        let long = "x".repeat(1000);
        for row in 0..100 {
            sheet.write_string(row, 0, &long).unwrap();
        }
        let bytes = book.save_to_buffer().unwrap();
        // "Sheet1", then 100 copies.
        assert!(read_capped(&bytes, 0, 6 + 100 * 1000).is_ok());
        assert_eq!(
            read_capped(&bytes, 0, 6 + 100 * 1000 - 1),
            Err(WorkbookError::TooLarge)
        );
    }

    #[test]
    fn formulas_count_toward_the_cap_and_stay_inside_the_cells() {
        let mut book = Writer::new();
        let sheet = book.add_worksheet();
        let formula = format!("LEN(\"{}\")", "y".repeat(500));
        sheet
            .write_formula(
                0,
                0,
                rust_xlsxwriter::Formula::new(&formula).set_result("1"),
            )
            .unwrap();
        let bytes = book.save_to_buffer().unwrap();
        let fits = 6 + 1 + formula.len();
        assert_eq!(read_capped(&bytes, 0, fits).unwrap().formulas.len(), 1);
        assert_eq!(
            read_capped(&bytes, 0, fits - 1),
            Err(WorkbookError::TooLarge)
        );

        let mut cells = Cells::new(MAX_TEXT);
        cells.add((0, 0), Cell::new("a")).unwrap();
        cells.add_formula((0, 0), "1+1").unwrap();
        // A formula whose cell shows nothing, far away.
        cells.add_formula((1_000_000, 16_000), "\"\"").unwrap();
        let (rows, formulas) = cells.finish();
        assert_eq!(rows, [[Cell::new("a")]]);
        assert_eq!(formulas, [(0, 0, "1+1".to_owned())]);
    }

    #[test]
    fn a_cell_given_again_and_again_counts_each_time() {
        let mut cells = Cells::new(MAX_TEXT);
        cells.max_cells = 2;
        cells.add((0, 0), Cell::new("a")).unwrap();
        cells.add((0, 0), Cell::new("a")).unwrap();
        assert_eq!(
            cells.add((0, 0), Cell::new("a")),
            Err(WorkbookError::TooLarge)
        );
        cells.add_formula((0, 0), "1").unwrap();
        cells.add_formula((0, 0), "1").unwrap();
        assert_eq!(cells.add_formula((0, 0), "1"), Err(WorkbookError::TooLarge));
    }

    #[test]
    fn refuses_far_cells_and_large_zips() {
        let mut book = Writer::new();
        let sheet = book.add_worksheet();
        sheet.write_string(0, 0, "a").unwrap();
        sheet.write_string(1_048_575, 16_383, "far").unwrap();
        let bytes = book.save_to_buffer().unwrap();
        assert_eq!(read_bytes(&bytes, 0), Err(WorkbookError::TooLarge));
        assert_eq!(check_zip(&sample(), 1000), Err(WorkbookError::TooLarge));
        // Calamine still opens a zip with something before it.
        let mut prefixed = b"JUNK".to_vec();
        prefixed.extend(sample());
        assert_eq!(read_bytes(&prefixed, 0).unwrap().rows[0][0], "Name");
        assert_eq!(check_zip(&prefixed, 1000), Err(WorkbookError::TooLarge));
        assert!(matches!(
            read_bytes(b"PK\x03\x04 not a zip", 0),
            Err(WorkbookError::Unreadable(_))
        ));
    }
}
