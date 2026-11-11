//! Spreadsheet files (.xlsx, .xlsm, .xlsb, .xls, .ods), read with calamine:
//! cell values only (formulas give the result saved with the file, and
//! their text), never macros, links or embedded objects.
//!
//! A crafted file could make calamine ask for a huge amount of memory (it
//! sets aside room for every cell between A1 and the last one) or fail: the
//! file is read by a separate process, CSV Edit started again with
//! `--read-workbook`, so that at worst that process ends and the file is
//! called unreadable. The reading is also capped: the uncompressed size of
//! zip-based files, and the number of cells.

use std::io::{Cursor, Read, Write};
use std::path::Path;

use calamine::{Data, DataRef, Reader, Sheets};

use crate::table::MAX_CELLS;

/// The files read as workbooks, by extension.
pub const EXTENSIONS: [&str; 5] = ["xlsx", "xlsm", "xlsb", "xls", "ods"];

/// Largest size of a zip-based workbook's parts once uncompressed: a few
/// kilobytes can unzip to gigabytes.
const MAX_UNZIPPED: u64 = 1 << 30;

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
    pub rows: Vec<Vec<String>>,
    /// (row, column, formula without its "=").
    pub formulas: Vec<(usize, usize, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkbookError {
    /// Not a workbook calamine can read, or damaged.
    Unreadable(String),
    /// More cells than CSV Edit opens, or a zip that unzips too large.
    TooLarge,
    /// The reading process ended without an answer (out of memory).
    Crashed,
}

/// Reads sheet `index` of the workbook at `path`, in a separate process.
pub fn read(path: &Path, index: usize) -> Result<Workbook, WorkbookError> {
    if cfg!(test) {
        // The test binary can't be started as the reader.
        let bytes = std::fs::read(path).map_err(|e| WorkbookError::Unreadable(e.to_string()))?;
        return read_bytes(&bytes, index);
    }
    let exe = std::env::current_exe().map_err(|e| WorkbookError::Unreadable(e.to_string()))?;
    let mut command = std::process::Command::new(exe);
    command
        .arg(READER_ARG)
        .arg(path)
        .arg(index.to_string())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        // No console window for the reader.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let output = command
        .output()
        .map_err(|e| WorkbookError::Unreadable(e.to_string()))?;
    match output.status.code() {
        Some(0) => decode(&output.stdout).ok_or(WorkbookError::Crashed),
        Some(3) => Err(WorkbookError::TooLarge),
        Some(2) => Err(WorkbookError::Unreadable(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        )),
        _ => Err(WorkbookError::Crashed),
    }
}

/// The reader process: reads the file named in `args` and writes the
/// result to the standard output. Its exit code says how it went.
pub fn reader_main(args: &[std::ffi::OsString]) -> i32 {
    let (Some(path), Some(index)) = (args.first(), args.get(1)) else {
        return 2;
    };
    let index = index.to_string_lossy().parse().unwrap_or(0);
    let result = std::fs::read(path)
        .map_err(|e| WorkbookError::Unreadable(e.to_string()))
        .and_then(|bytes| read_bytes(&bytes, index));
    match result {
        Ok(workbook) => {
            let mut out = std::io::BufWriter::new(std::io::stdout().lock());
            if out
                .write_all(&encode(&workbook))
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
        Err(WorkbookError::Crashed) => 4,
    }
}

/// Reads sheet `index` of a workbook's bytes (in the reader process).
pub fn read_bytes(bytes: &[u8], index: usize) -> Result<Workbook, WorkbookError> {
    check_zip(bytes, MAX_UNZIPPED)?;
    let unreadable = |e: calamine::Error| WorkbookError::Unreadable(e.to_string());
    let mut workbook =
        calamine::open_workbook_auto_from_rs(Cursor::new(bytes)).map_err(unreadable)?;
    let sheets = workbook.sheet_names();
    let Some(name) = sheets.get(index).or(sheets.first()).cloned() else {
        return Ok(Workbook::default());
    };
    let mut cells = Cells::default();
    let mut formulas = Vec::new();
    match &mut workbook {
        // Read cell by cell: nothing is set aside for empty cells.
        Sheets::Xlsx(xlsx) => {
            let error = |e: calamine::XlsxError| WorkbookError::Unreadable(e.to_string());
            let mut reader = xlsx.worksheet_cells_reader(&name).map_err(error)?;
            while let Some(cell) = reader.next_cell().map_err(error)? {
                cells.add(cell.get_position(), text_of_ref(cell.get_value()))?;
            }
            drop(reader);
            let mut reader = xlsx.worksheet_cells_reader(&name).map_err(error)?;
            while let Some(cell) = reader.next_formula().map_err(error)? {
                push_formula(&mut formulas, cell.get_position(), cell.get_value());
            }
        }
        Sheets::Xlsb(xlsb) => {
            let error = |e: calamine::XlsbError| WorkbookError::Unreadable(e.to_string());
            let mut reader = xlsb.worksheet_cells_reader(&name).map_err(error)?;
            while let Some(cell) = reader.next_cell().map_err(error)? {
                cells.add(cell.get_position(), text_of_ref(cell.get_value()))?;
            }
            drop(reader);
            let mut reader = xlsb.worksheet_cells_reader(&name).map_err(error)?;
            while let Some(cell) = reader.next_formula().map_err(error)? {
                push_formula(&mut formulas, cell.get_position(), cell.get_value());
            }
        }
        // Older formats, smaller by design (.xls has at most 65,536 rows).
        _ => {
            let range = workbook.worksheet_range(&name).map_err(unreadable)?;
            for (row, col, value) in range.used_cells() {
                let (top, left) = range.start().unwrap_or((0, 0));
                cells.add((top + row as u32, left + col as u32), text_of(value))?;
            }
            if let Ok(range) = workbook.worksheet_formula(&name) {
                let (top, left) = range.start().unwrap_or((0, 0));
                for (row, col, formula) in range.used_cells() {
                    push_formula(
                        &mut formulas,
                        (top + row as u32, left + col as u32),
                        formula,
                    );
                }
            }
        }
    }
    Ok(Workbook {
        sheets,
        rows: cells.into_rows(),
        formulas,
    })
}

/// Refuses a zip whose parts would unzip larger than `limit` in all.
fn check_zip(bytes: &[u8], limit: u64) -> Result<(), WorkbookError> {
    if !bytes.starts_with(b"PK") {
        return Ok(());
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| WorkbookError::Unreadable(e.to_string()))?;
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

/// The cells found, kept apart until the sheet's size is known.
#[derive(Default)]
struct Cells {
    cells: Vec<((u32, u32), String)>,
    rows: usize,
    cols: usize,
}

impl Cells {
    fn add(&mut self, (row, col): (u32, u32), text: String) -> Result<(), WorkbookError> {
        if text.is_empty() {
            return Ok(());
        }
        self.rows = self.rows.max(row as usize + 1);
        self.cols = self.cols.max(col as usize + 1);
        // The table pads every row to the widest: the cells it will hold.
        if self.rows.saturating_mul(self.cols) > MAX_CELLS {
            return Err(WorkbookError::TooLarge);
        }
        self.cells.push(((row, col), text));
        Ok(())
    }

    fn into_rows(self) -> Vec<Vec<String>> {
        let mut rows = vec![vec![String::new(); self.cols]; self.rows];
        for ((row, col), text) in self.cells {
            rows[row as usize][col as usize] = text;
        }
        rows
    }
}

fn push_formula(formulas: &mut Vec<(usize, usize, String)>, (row, col): (u32, u32), text: &str) {
    if !text.is_empty() {
        formulas.push((row as usize, col as usize, text.to_owned()));
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

fn text_of_ref(value: &DataRef<'_>) -> String {
    match value {
        DataRef::SharedString(s) => (*s).to_owned(),
        other => text_of(&Data::from(other.clone())),
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

fn encode(workbook: &Workbook) -> Vec<u8> {
    fn number(out: &mut Vec<u8>, n: usize) {
        out.extend_from_slice(&(n as u32).to_le_bytes());
    }
    fn text(out: &mut Vec<u8>, s: &str) {
        number(out, s.len());
        out.extend_from_slice(s.as_bytes());
    }
    let mut out = Vec::new();
    number(&mut out, workbook.sheets.len());
    for name in &workbook.sheets {
        text(&mut out, name);
    }
    number(&mut out, workbook.rows.len());
    number(&mut out, workbook.rows.first().map_or(0, Vec::len));
    for row in &workbook.rows {
        for cell in row {
            text(&mut out, cell);
        }
    }
    number(&mut out, workbook.formulas.len());
    for (row, col, formula) in &workbook.formulas {
        number(&mut out, *row);
        number(&mut out, *col);
        text(&mut out, formula);
    }
    out
}

fn decode(bytes: &[u8]) -> Option<Workbook> {
    let mut input = bytes;
    let number = |input: &mut &[u8]| -> Option<usize> {
        let mut four = [0; 4];
        input.read_exact(&mut four).ok()?;
        Some(u32::from_le_bytes(four) as usize)
    };
    let text = |input: &mut &[u8]| -> Option<String> {
        let len = number(input)?;
        let head = input.get(..len)?;
        let text = String::from_utf8(head.to_vec()).ok()?;
        *input = &input[len..];
        Some(text)
    };
    let mut workbook = Workbook::default();
    for _ in 0..number(&mut input)? {
        workbook.sheets.push(text(&mut input)?);
    }
    let (rows, cols) = (number(&mut input)?, number(&mut input)?);
    if rows.saturating_mul(cols) > MAX_CELLS {
        return None;
    }
    for _ in 0..rows {
        let row = (0..cols)
            .map(|_| text(&mut input))
            .collect::<Option<Vec<_>>>()?;
        workbook.rows.push(row);
    }
    for _ in 0..number(&mut input)? {
        let (row, col) = (number(&mut input)?, number(&mut input)?);
        workbook.formulas.push((row, col, text(&mut input)?));
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
        assert_eq!(decode(&encode(&workbook)), Some(workbook));
        assert_eq!(decode(b"\x05\x00"), None);
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
        assert!(matches!(
            read_bytes(b"PK\x03\x04 not a zip", 0),
            Err(WorkbookError::Unreadable(_))
        ));
    }
}
