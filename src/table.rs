//! The table being edited, how it was written on disk (delimiter, encoding,
//! line endings), and the undoable edits made to it.

use std::borrow::Cow;
use std::cmp::Ordering;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};

/// A cell's text. Up to 24 bytes are kept inline, without an allocation of
/// their own: most cells of a large file fit, which halves its memory.
pub type Cell = compact_str::CompactString;

/// A rectangular grid of text cells: every row has the same number of
/// columns, and there is always at least one row and one column.
///
/// A clone shares the cells: it is a snapshot (for saving away from the
/// interface's thread), and the first edit made while one is kept copies
/// them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Table {
    rows: Arc<Vec<Vec<Cell>>>,
}

impl Default for Table {
    fn default() -> Self {
        Self::empty(1, 3)
    }
}

impl Table {
    pub fn empty(rows: usize, cols: usize) -> Self {
        Self {
            rows: Arc::new(vec![vec![Cell::default(); cols.max(1)]; rows.max(1)]),
        }
    }

    /// Ragged rows are padded with empty cells.
    pub fn from_rows(mut rows: Vec<Vec<Cell>>) -> Self {
        let cols = rows.iter().map(Vec::len).max().unwrap_or(0).max(1);
        if rows.is_empty() {
            rows.push(Vec::new());
        }
        for row in &mut rows {
            row.resize(cols, Cell::default());
        }
        Self {
            rows: Arc::new(rows),
        }
    }

    pub fn rows(&self) -> &[Vec<Cell>] {
        &self.rows
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub fn col_count(&self) -> usize {
        self.rows[0].len()
    }

    /// The distinct non-ASCII characters of the cells, in order.
    pub fn rare_chars(&self) -> String {
        let chars: std::collections::BTreeSet<char> = self
            .rows
            .iter()
            .flatten()
            .filter(|cell| !cell.is_ascii())
            .flat_map(|cell| cell.chars())
            .filter(|c| !c.is_ascii())
            .collect();
        chars.into_iter().collect()
    }

    pub fn cell(&self, row: usize, col: usize) -> &str {
        &self.rows[row][col]
    }

    /// Applies `edit` and returns the edit that undoes it.
    pub fn apply(&mut self, edit: Edit) -> Edit {
        if let Edit::Batch(edits) = edit {
            let mut inverses: Vec<_> = edits.into_iter().map(|e| self.apply(e)).collect();
            inverses.reverse();
            return Edit::Batch(inverses);
        }
        // Copies the cells only when a snapshot still shares them.
        let table = Arc::make_mut(&mut self.rows);
        match edit {
            Edit::SetCells(cells) => {
                let mut old: Vec<_> = cells
                    .into_iter()
                    .map(|(row, col, text)| {
                        let old = std::mem::replace(&mut table[row][col], text);
                        (row, col, old)
                    })
                    .collect();
                old.reverse();
                Edit::SetCells(old)
            }
            Edit::SetBlock(top, cols, values) => {
                let old = values
                    .into_iter()
                    .enumerate()
                    .map(|(i, line)| {
                        let row = &mut table[top + i];
                        line.into_iter()
                            .zip(&cols)
                            .map(|(text, &col)| std::mem::replace(&mut row[col], text))
                            .collect()
                    })
                    .collect();
                Edit::SetBlock(top, cols, old)
            }
            Edit::InsertRows(at, rows) => {
                let count = rows.len();
                table.splice(at..at, rows);
                Edit::RemoveRows(at, count)
            }
            Edit::RemoveRows(at, count) => {
                let removed: Vec<_> = table.drain(at..at + count).collect();
                if table.is_empty() {
                    table.push(vec![Cell::default(); removed[0].len()]);
                    // Undo puts back the removed rows, then drops the blank one.
                    let count = removed.len();
                    return Edit::Batch(vec![
                        Edit::InsertRows(0, removed),
                        Edit::RemoveRows(count, 1),
                    ]);
                }
                Edit::InsertRows(at, removed)
            }
            Edit::InsertCols(at, cols) => {
                let count = cols.len();
                // Each column gives its cells in order, one per row: they
                // are moved, not copied, and the rows grow by exactly
                // `count` (splice alone would double their room).
                let mut cols: Vec<_> = cols.into_iter().map(Vec::into_iter).collect();
                for row in table.iter_mut() {
                    row.reserve_exact(count);
                    row.splice(
                        at..at,
                        cols.iter_mut()
                            .map(|col| col.next().expect("a cell per row")),
                    );
                }
                Edit::RemoveCols(at, count)
            }
            Edit::RemoveCols(at, count) => {
                let mut removed = vec![Vec::with_capacity(table.len()); count];
                for row in table.iter_mut() {
                    for (i, text) in row.drain(at..at + count).enumerate() {
                        removed[i].push(text);
                    }
                }
                if table[0].is_empty() {
                    for row in table.iter_mut() {
                        row.push(Cell::default());
                    }
                    return Edit::Batch(vec![
                        Edit::InsertCols(0, removed),
                        Edit::RemoveCols(count, 1),
                    ]);
                }
                Edit::InsertCols(at, removed)
            }
            Edit::Reorder(start, order) => {
                // Moved in place, one cycle of the permutation at a time.
                let rows = &mut table[start..start + order.len()];
                let mut done = vec![false; order.len()];
                for first in 0..order.len() {
                    if done[first] {
                        continue;
                    }
                    let held = std::mem::take(&mut rows[first]);
                    let mut i = first;
                    loop {
                        done[i] = true;
                        let from = order[i] as usize;
                        if from == first {
                            rows[i] = held;
                            break;
                        }
                        rows[i] = std::mem::take(&mut rows[from]);
                        i = from;
                    }
                }
                Edit::Reorder(start, invert(&order))
            }
            Edit::Batch(_) => unreachable!("handled above"),
        }
    }

    /// The order that sorts rows `start..` on column `col` (numbers by
    /// value, text ignoring case, empty cells last), for `Edit::Reorder`.
    pub fn sort_order(&self, start: usize, col: usize, descending: bool) -> Vec<u32> {
        let rows = &self.rows[start..];
        // 03/04/2026: the column's other dates tell which is the day.
        let day_first = day_comes_first(rows.iter().map(|row| row[col].as_str()));
        // Each cell's key is worked out once, not at every comparison.
        let keys: Vec<SortKey> = rows
            .iter()
            .map(|row| SortKey::of(&row[col], day_first))
            .collect();
        let count = u32::try_from(rows.len()).expect("fewer rows than MAX_CELLS");
        let mut order: Vec<u32> = (0..count).collect();
        order.sort_by(|&a, &b| {
            let (a, b) = (a as usize, b as usize);
            let ordering = keys[a]
                .cmp(&keys[b])
                .then_with(|| rows[a][col].cmp(&rows[b][col]));
            if descending {
                ordering.reverse()
            } else {
                ordering
            }
        });
        order
    }
}

/// How a cell sorts: numbers (with a decimal point or comma) by value,
/// then dates and times in time order, then text ignoring case, then empty
/// cells.
enum SortKey {
    Number(f64),
    /// Seconds from year 0, for comparing.
    Date(i64),
    /// Lowercased: kept inline when short, like the cells.
    Text(Cell),
    Empty,
}

impl SortKey {
    fn of(cell: &str, day_first: bool) -> Self {
        let cell = cell.trim();
        if cell.is_empty() {
            return SortKey::Empty;
        }
        let number = if cell.contains(',') {
            cell.replace(',', ".").parse::<f64>().ok()
        } else {
            cell.parse::<f64>().ok()
        };
        if let Some(n) = number.filter(|n| n.is_finite()) {
            return SortKey::Number(n);
        }
        match parse_date(cell, day_first) {
            Some(date) => SortKey::Date(date),
            None if cell.is_ascii() => {
                let mut key = Cell::from(cell);
                key.as_mut_str().make_ascii_lowercase();
                SortKey::Text(key)
            }
            None => SortKey::Text(Cell::from(cell).to_lowercase()),
        }
    }

    fn rank(&self) -> u8 {
        match self {
            SortKey::Number(_) => 0,
            SortKey::Date(_) => 1,
            SortKey::Text(_) => 2,
            SortKey::Empty => 3,
        }
    }

    fn cmp(&self, other: &Self) -> Ordering {
        use SortKey::*;
        match (self, other) {
            (Number(a), Number(b)) => a.total_cmp(b),
            (Date(a), Date(b)) => a.cmp(b),
            (Text(a), Text(b)) => a.cmp(b),
            _ => self.rank().cmp(&other.rank()),
        }
    }
}

/// Whether dates written with the year last (03/04/2026) put the day first,
/// as in France, or the month, as in the United States: a first number
/// above 12 says day first, a second one month first; day first if nothing
/// tells.
fn day_comes_first<'a>(cells: impl Iterator<Item = &'a str>) -> bool {
    for cell in cells {
        let date = cell.trim().split([' ', 'T']).next().unwrap_or("");
        let parts: Vec<&str> = date.split(['/', '-', '.']).collect();
        if parts.len() != 3 || parts[0].len() > 2 {
            continue;
        }
        let (Ok(a), Ok(b)) = (parts[0].parse::<u32>(), parts[1].parse::<u32>()) else {
            continue;
        };
        if a > 12 && b <= 12 {
            return true;
        }
        if b > 12 && a <= 12 {
            return false;
        }
    }
    true
}

/// A date, a time, or a date and a time, as seconds from year 0:
/// 2026-07-01, 01/07/2026, 1.7.26, with 17:32, 17:32:30, 17:32:30.5,
/// 5:32 PM, after a space or a "T".
fn parse_date(cell: &str, day_first: bool) -> Option<i64> {
    let mut words = cell.split([' ', 'T']).filter(|w| !w.is_empty());
    let first = words.next()?;
    let (date, time) = if first.contains(':') {
        (None, Some(first))
    } else {
        (Some(first), words.next())
    };
    // "PM" after the time.
    let pm = match words.next().map(str::to_ascii_lowercase).as_deref() {
        None => None,
        Some("am") => Some(false),
        Some("pm") => Some(true),
        Some(_) => return None,
    };
    if words.next().is_some() {
        return None;
    }
    let days = match date {
        Some(date) => {
            let parts: Vec<&str> = date.split(['/', '-', '.']).collect();
            let [a, b, c] = parts.as_slice() else {
                return None;
            };
            let number = |s: &str| {
                s.parse::<u32>()
                    .ok()
                    .filter(|_| s.bytes().all(|b| b.is_ascii_digit()))
            };
            let (year, month, day) = if a.len() == 4 {
                (number(a)?, number(b)?, number(c)?)
            } else if c.len() == 4 || c.len() == 2 {
                let year = number(c)?;
                let year = if c.len() == 2 { 2000 + year } else { year };
                let (a, b) = (number(a)?, number(b)?);
                if day_first {
                    (year, b, a)
                } else {
                    (year, a, b)
                }
            } else {
                return None;
            };
            if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
                return None;
            }
            (i64::from(year) * 12 + i64::from(month)) * 31 + i64::from(day)
        }
        None => 0,
    };
    let seconds = match time {
        Some(time) => {
            let mut parts = time.split(':');
            let hours: u32 = parts.next()?.parse().ok()?;
            let minutes: u32 = parts.next()?.parse().ok()?;
            let seconds: f64 = match parts.next() {
                Some(s) => s.parse().ok()?,
                None => 0.0,
            };
            if parts.next().is_some() || minutes > 59 || !(0.0..61.0).contains(&seconds) {
                return None;
            }
            let hours = match pm {
                Some(pm) if (1..=12).contains(&hours) => hours % 12 + if pm { 12 } else { 0 },
                Some(_) => return None,
                None => hours,
            };
            if hours > 23 {
                return None;
            }
            i64::from(hours * 3600 + minutes * 60) + seconds as i64
        }
        None if pm.is_some() => return None,
        None => 0,
    };
    Some(days * 86_400 + seconds)
}

/// A change to the table. Applying one gives back its inverse (see
/// `Table::apply`), which is what the undo and redo stacks keep.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Edit {
    /// (row, column, new text)
    SetCells(Vec<(usize, usize, Cell)>),
    /// A block of cells from row `top` on, as pasted: row `top + i` takes
    /// `values[i][j]` in column `cols[j]` (a shorter line leaves the last
    /// columns as they are). No coordinates are kept per cell.
    SetBlock(usize, Vec<usize>, Vec<Vec<Cell>>),
    /// Rows inserted before row `at`.
    InsertRows(usize, Vec<Vec<Cell>>),
    RemoveRows(usize, usize),
    /// Columns (each a cell per row) inserted before column `at`.
    InsertCols(usize, Vec<Vec<Cell>>),
    RemoveCols(usize, usize),
    /// Rows from `start` on take the order given: row `start + i` becomes
    /// the old row `start + order[i]`. `u32` halves what the undo steps
    /// keep: a table holds far fewer rows (`MAX_CELLS`).
    Reorder(usize, Vec<u32>),
    Batch(Vec<Edit>),
}

/// The permutation that undoes `order`: `invert(order)[order[i]] == i`.
pub fn invert(order: &[u32]) -> Vec<u32> {
    let mut inverse = vec![0; order.len()];
    for (i, &from) in order.iter().enumerate() {
        inverse[from as usize] = i as u32;
    }
    inverse
}

/// Estimated bytes a cell takes: its own, and its text's allocation past
/// 24 bytes.
pub fn cell_bytes(cell: &Cell) -> u64 {
    (std::mem::size_of::<Cell>()
        + if cell.is_heap_allocated() {
            cell.capacity()
        } else {
            0
        }) as u64
}

/// Estimated bytes a row takes, with the room it keeps for more cells.
pub fn row_bytes(row: &Vec<Cell>) -> u64 {
    (std::mem::size_of::<Vec<Cell>>()
        + row.capacity().saturating_sub(row.len()) * std::mem::size_of::<Cell>()) as u64
        + row.iter().map(cell_bytes).sum::<u64>()
}

impl Edit {
    /// Estimated bytes the edit keeps (for the undo steps' budget).
    pub fn bytes(&self) -> u64 {
        let own = std::mem::size_of::<Edit>() as u64;
        own + match self {
            Edit::SetCells(cells) => cells.iter().map(|(_, _, s)| 16 + cell_bytes(s)).sum(),
            Edit::SetBlock(_, cols, values) => {
                cols.len() as u64 * 8 + values.iter().map(row_bytes).sum::<u64>()
            }
            Edit::InsertRows(_, rows) | Edit::InsertCols(_, rows) => {
                rows.iter().map(row_bytes).sum()
            }
            Edit::Reorder(_, order) => order.len() as u64 * 4,
            Edit::Batch(edits) => edits.iter().map(Edit::bytes).sum(),
            Edit::RemoveRows(..) | Edit::RemoveCols(..) => 0,
        }
    }
}

/// Files with more cells than this are refused: each cell takes memory of
/// its own, and a short line among long ones is padded to their width.
pub const MAX_CELLS: usize = 30_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Utf8Bom,
    /// What Excel writes in Western European locales ("CSV (separator:
    /// semicolon)" files from a French Excel).
    Windows1252,
    /// Excel's "Unicode Text", with its byte order mark.
    Utf16Le,
    Utf16Be,
}

impl Encoding {
    pub const ALL: [Encoding; 5] = [
        Encoding::Utf8,
        Encoding::Utf8Bom,
        Encoding::Windows1252,
        Encoding::Utf16Le,
        Encoding::Utf16Be,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineEnding {
    Lf,
    CrLf,
    /// Old Mac files (Excel's "CSV (Macintosh)").
    Cr,
}

impl LineEnding {
    pub fn as_str(self) -> &'static str {
        match self {
            LineEnding::Lf => "\n",
            LineEnding::CrLf => "\r\n",
            LineEnding::Cr => "\r",
        }
    }

    /// The system's: CRLF on Windows, LF elsewhere.
    pub fn native() -> Self {
        if cfg!(windows) {
            LineEnding::CrLf
        } else {
            LineEnding::Lf
        }
    }
}

/// How a file is written back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Format {
    /// Between fields; can be several characters (Poliris files use `!#`).
    pub delimiter: String,
    /// Other delimiters the file was read with: cells holding one are
    /// quoted too, so that the file reads back the same way.
    pub other_delimiters: Vec<String>,
    /// Around fields that need it; `None`: the file had no quoting, and
    /// only fields that can't be written without it get double quotes.
    pub quote: Option<char>,
    /// Every field between quotes (Poliris files), not only those that need
    /// them.
    pub quote_all: bool,
    /// Every line ends with a delimiter too (Poliris files).
    pub trailing_delimiter: bool,
    pub encoding: Encoding,
    pub line_ending: LineEnding,
    /// The lines left out at the top when the file was read (a title, a
    /// comment, Excel's "sep=;"), written back as they were.
    pub preamble: String,
    /// Cells a spreadsheet would run as a formula (=, +, -, @ first, and
    /// not a number) are written with an apostrophe first.
    pub protect_formulas: bool,
}

impl Default for Format {
    fn default() -> Self {
        Self {
            delimiter: ",".to_owned(),
            other_delimiters: Vec::new(),
            quote: Some('"'),
            quote_all: false,
            trailing_delimiter: false,
            encoding: Encoding::Utf8,
            line_ending: LineEnding::native(),
            preamble: String::new(),
            protect_formulas: false,
        }
    }
}

/// What reading a file left out or changed: saving over it would lose
/// them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Losses {
    pub blank_lines: usize,
    /// Fields whose surrounding spaces were trimmed.
    pub trimmed: usize,
    /// Byte sequences the encoding couldn't read (shown as �).
    pub invalid_bytes: usize,
}

impl Losses {
    pub fn any(&self) -> bool {
        *self != Losses::default()
    }
}

/// The usual delimiters, the ones the file is checked for when opened.
pub const DELIMITERS: [&str; 5] = [",", ";", "\t", "|", "!#"];

/// How to read a CSV file: chosen in the import window, guessed first by
/// `ImportOptions::detect`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportOptions {
    pub encoding: Encoding,
    /// Lines left out at the top of the file.
    pub skip_lines: usize,
    /// Any of them ends a field; each can be several characters.
    pub delimiters: Vec<String>,
    /// Around fields that hold delimiters, line breaks or quotes (doubled).
    pub quote: Option<char>,
    /// Consecutive delimiters count as one, and delimiters starting a line
    /// are ignored.
    pub merge_delimiters: bool,
    /// Spaces around unquoted fields are dropped.
    pub trim: bool,
}

/// Bytes looked at to guess how a file is written, and shown in the
/// import window's preview.
pub const SAMPLE_BYTES: usize = 256 * 1024;

impl ImportOptions {
    /// The encoding and delimiter the file seems to use.
    pub fn detect(bytes: &[u8]) -> Self {
        let encoding = detect_encoding(bytes);
        let sample = decode_as(prefix(bytes, SAMPLE_BYTES, encoding), encoding).text;
        // Excel's own hint, "sep=;" on the first line.
        let first_line = sample.split(['\r', '\n']).next().unwrap_or("");
        if let Some(delimiter) = first_line.strip_prefix("sep=")
            && !delimiter.is_empty()
        {
            return Self {
                encoding,
                skip_lines: 1,
                delimiters: vec![delimiter.to_owned()],
                quote: Some('"'),
                merge_delimiters: false,
                trim: false,
            };
        }
        Self {
            encoding,
            skip_lines: 0,
            delimiters: vec![sniff_delimiter(&sample).to_owned()],
            quote: Some('"'),
            merge_delimiters: false,
            trim: false,
        }
    }
}

/// The start of `bytes`, cut where the encoding allows.
fn prefix(bytes: &[u8], len: usize, encoding: Encoding) -> &[u8] {
    if bytes.len() <= len {
        return bytes;
    }
    match encoding {
        Encoding::Utf16Le | Encoding::Utf16Be => &bytes[..len & !1],
        _ => &bytes[..len],
    }
}

/// A file read with some options.
pub struct Imported {
    pub table: Table,
    pub format: Format,
    pub losses: Losses,
    /// A workbook's formulas: (row, column, formula without "=").
    pub formulas: Vec<(usize, usize, String)>,
    /// A workbook's sheets.
    pub sheets: Vec<String>,
    /// The cells' distinct non-ASCII characters (filled in by the import
    /// window's reading), for `theme::cover`.
    pub rare_chars: String,
}

/// The file has too many cells to be opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TooLarge {
    pub rows: usize,
    pub cols: usize,
}

/// Reads a CSV file's bytes, guessing its encoding, delimiter and line
/// endings (what the import window starts from).
#[cfg(test)]
pub fn parse(bytes: &[u8]) -> (Table, Format) {
    let imported = import(bytes, &ImportOptions::detect(bytes)).expect("small file");
    (imported.table, imported.format)
}

/// Reads a CSV file's bytes the way `options` say. It is written back
/// with the delimiter that split the most fields, its quotes and its line
/// endings.
#[cfg(test)]
pub fn import(bytes: &[u8], options: &ImportOptions) -> Result<Imported, TooLarge> {
    import_until(bytes, options, &AtomicBool::new(false)).map_err(|stop| match stop {
        Stop::TooLarge(too_large) => too_large,
        Stop::Cancelled => unreachable!("never cancelled"),
    })
}

/// Why `import_until` gave up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    TooLarge(TooLarge),
    /// `cancel` was set.
    Cancelled,
}

/// `import`, given up as soon as the file proves too large or `cancel`
/// is set.
pub fn import_until(
    bytes: &[u8],
    options: &ImportOptions,
    cancel: &AtomicBool,
) -> Result<Imported, Stop> {
    let decoded = decode_as(bytes, options.encoding);
    if cancel.load(AtomicOrdering::Relaxed) {
        return Err(Stop::Cancelled);
    }
    let limits = Limits {
        max_rows: usize::MAX,
        max_cells: MAX_CELLS,
        lines: false,
        cancel: Some(cancel),
    };
    let parsed = split_with(&decoded.text, options, &limits)?;
    let invalid_bytes = decoded.invalid;
    drop(decoded);
    let rows = parsed.rows.len();
    let cols = parsed.rows.iter().map(Vec::len).max().unwrap_or(0);
    if rows.saturating_mul(cols) > MAX_CELLS {
        return Err(Stop::TooLarge(TooLarge { rows, cols }));
    }
    let delimiter = parsed
        .main_delimiter
        .clone()
        .or_else(|| options.delimiters.first().cloned())
        .unwrap_or_else(|| ",".to_owned());
    let other_delimiters = options
        .delimiters
        .iter()
        .filter(|d| **d != delimiter)
        .cloned()
        .collect();
    let format = Format {
        delimiter,
        other_delimiters,
        quote: options.quote,
        quote_all: parsed.all_quoted,
        trailing_delimiter: parsed.trailing_delimiter,
        encoding: options.encoding,
        line_ending: parsed.line_ending.unwrap_or_else(LineEnding::native),
        preamble: parsed.preamble,
        protect_formulas: false,
    };
    let losses = Losses {
        blank_lines: parsed.blank_lines,
        trimmed: parsed.trimmed,
        invalid_bytes,
    };
    Ok(Imported {
        table: Table::from_rows(parsed.rows),
        format,
        losses,
        formulas: Vec::new(),
        sheets: Vec::new(),
        rare_chars: String::new(),
    })
}

/// The first rows of a file, read the way `options` say, for the import
/// window: only the start of the file is read.
pub struct Preview {
    pub rows: Vec<Vec<Cell>>,
    /// The line of the file each row starts on, counted from 1.
    pub lines: Vec<usize>,
    pub invalid_bytes: usize,
}

pub fn preview(bytes: &[u8], options: &ImportOptions, max_rows: usize) -> Preview {
    let decoded = decode_as(
        prefix(bytes, SAMPLE_BYTES, options.encoding),
        options.encoding,
    );
    let parsed = split_fields(&decoded.text, options, max_rows);
    Preview {
        rows: parsed.rows,
        lines: parsed.lines,
        invalid_bytes: decoded.invalid,
    }
}

/// A file's text, and how many byte sequences the encoding couldn't read.
pub struct Decoded<'a> {
    /// Borrowed from the bytes when they are already valid UTF-8.
    pub text: Cow<'a, str>,
    pub invalid: usize,
}

/// The text of a file in `encoding` (byte order marks are left out).
pub fn decode_as(bytes: &[u8], encoding: Encoding) -> Decoded<'_> {
    match encoding {
        Encoding::Utf8 | Encoding::Utf8Bom => {
            let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
            if let Ok(text) = std::str::from_utf8(bytes) {
                return Decoded {
                    text: Cow::Borrowed(text),
                    invalid: 0,
                };
            }
            let mut text = String::with_capacity(bytes.len());
            let mut invalid = 0;
            for chunk in bytes.utf8_chunks() {
                text.push_str(chunk.valid());
                if !chunk.invalid().is_empty() {
                    text.push('\u{FFFD}');
                    invalid += 1;
                }
            }
            Decoded {
                text: Cow::Owned(text),
                invalid,
            }
        }
        Encoding::Windows1252 => Decoded {
            text: encoding_rs::WINDOWS_1252
                .decode_without_bom_handling(bytes)
                .0,
            invalid: 0,
        },
        Encoding::Utf16Le | Encoding::Utf16Be => {
            let (codec, bom): (&'static encoding_rs::Encoding, &[u8]) =
                if encoding == Encoding::Utf16Le {
                    (encoding_rs::UTF_16LE, b"\xFF\xFE")
                } else {
                    (encoding_rs::UTF_16BE, b"\xFE\xFF")
                };
            let bytes = bytes.strip_prefix(bom).unwrap_or(bytes);
            let (text, errors) = codec.decode_without_bom_handling(bytes);
            Decoded {
                invalid: if errors {
                    text.matches('\u{FFFD}').count()
                } else {
                    0
                },
                text,
            }
        }
    }
}

/// Non-ASCII characters and invalid sequences looked at, at most, to tell
/// UTF-8 from Windows-1252.
const ENCODING_EVIDENCE: usize = 10_000;

/// The encoding a file seems to be in: its byte order mark, else UTF-16
/// by its zero bytes, else UTF-8 when most of its non-ASCII text reads as
/// UTF-8, else Windows-1252.
pub fn detect_encoding(bytes: &[u8]) -> Encoding {
    if bytes.starts_with(b"\xEF\xBB\xBF") {
        return Encoding::Utf8Bom;
    }
    if bytes.starts_with(b"\xFF\xFE") {
        return Encoding::Utf16Le;
    }
    if bytes.starts_with(b"\xFE\xFF") {
        return Encoding::Utf16Be;
    }
    // UTF-16 without a mark: ASCII text has a zero byte in every pair.
    let sample = &bytes[..bytes.len().min(4096) & !1];
    if sample.len() >= 4 {
        let pairs = sample.len() / 2;
        let zeros = |offset: usize| {
            sample
                .iter()
                .skip(offset)
                .step_by(2)
                .filter(|&&b| b == 0)
                .count()
        };
        let (even, odd) = (zeros(0), zeros(1));
        if odd * 10 > pairs * 3 && even * 20 < pairs {
            return Encoding::Utf16Le;
        }
        if even * 10 > pairs * 3 && odd * 20 < pairs {
            return Encoding::Utf16Be;
        }
    }
    if std::str::from_utf8(bytes).is_ok() {
        return Encoding::Utf8;
    }
    // A few stray bytes in a UTF-8 file shouldn't turn all of it into
    // Windows-1252. The first non-ASCII characters are enough to tell: a
    // large file isn't walked to its end.
    let (mut valid, mut invalid) = (0usize, 0usize);
    for chunk in bytes.utf8_chunks() {
        // Each non-ASCII character starts with a byte 11xxxxxx.
        valid += chunk.valid().bytes().filter(|b| *b >= 0xC0).count();
        invalid += usize::from(!chunk.invalid().is_empty());
        if valid + invalid >= ENCODING_EVIDENCE {
            break;
        }
    }
    if valid > invalid {
        Encoding::Utf8
    } else {
        Encoding::Windows1252
    }
}

/// The first `max_rows` rows of `text`. Blank lines are left out.
#[cfg(test)]
pub fn split(text: &str, options: &ImportOptions, max_rows: usize) -> Vec<Vec<Cell>> {
    split_fields(text, options, max_rows).rows
}

struct Parsed {
    rows: Vec<Vec<Cell>>,
    /// The line of the text each row starts on, counted from 1.
    lines: Vec<usize>,
    /// Every field was between quotes.
    all_quoted: bool,
    /// Every line ended with a delimiter (left out of `rows`).
    trailing_delimiter: bool,
    /// The line ending most rows end with.
    line_ending: Option<LineEnding>,
    /// The delimiter that split the most fields.
    main_delimiter: Option<String>,
    /// The skipped lines, line endings included.
    preamble: String,
    blank_lines: usize,
    trimmed: usize,
}

/// Length of the line break at the start of `text` (CRLF, LF or CR).
fn line_break_len(text: &str) -> Option<usize> {
    if text.starts_with("\r\n") {
        Some(2)
    } else if text.starts_with(['\r', '\n']) {
        Some(1)
    } else {
        None
    }
}

/// When `split_with` stops reading.
struct Limits<'a> {
    max_rows: usize,
    /// Rows × columns past which the file is refused.
    max_cells: usize,
    /// `Parsed::lines` is filled in.
    lines: bool,
    /// Looked at every `CANCEL_EVERY` rows.
    cancel: Option<&'a AtomicBool>,
}

/// Rows read between two looks at the cancel flag.
const CANCEL_EVERY: usize = 4096;

/// The first `max_rows` rows of `text`, with no other limit.
fn split_fields(text: &str, options: &ImportOptions, max_rows: usize) -> Parsed {
    let limits = Limits {
        max_rows,
        max_cells: usize::MAX,
        lines: true,
        cancel: None,
    };
    match split_with(text, options, &limits) {
        Ok(parsed) => parsed,
        Err(_) => unreachable!("nothing stops a reading without limits"),
    }
}

/// Length of the UTF-8 character starting with byte `lead`.
fn char_len(lead: u8) -> usize {
    match lead {
        0x00..0x80 => 1,
        0x80..0xE0 => 2,
        0xE0..0xF0 => 3,
        _ => 4,
    }
}

/// A field's text: what `field` holds (the parts of a quoted field already
/// copied), then `tail`, straight from the file.
fn take_field(
    field: &mut String,
    tail: &str,
    quoted: bool,
    trim: bool,
    trimmed: &mut usize,
) -> Cell {
    let mut text = if field.is_empty() {
        tail
    } else {
        field.push_str(tail);
        field.as_str()
    };
    if trim && !quoted {
        let kept = text.trim();
        if kept.len() != text.len() {
            *trimmed += 1;
            text = kept;
        }
    }
    // A short cell is copied inline, with no allocation; the buffer is
    // kept for the next quoted field.
    let cell = Cell::new(text);
    field.clear();
    cell
}

/// Splits `text` into rows of fields. Unquoted fields are copied straight
/// from `text`; the text is walked byte by byte, stopping only at bytes
/// that may start a delimiter, a quote or a line break.
fn split_with(text: &str, options: &ImportOptions, limits: &Limits) -> Result<Parsed, Stop> {
    // The skipped lines end where the parser's lines do: at CRLF, LF or CR.
    let mut start = 0;
    for _ in 0..options.skip_lines {
        match text[start..].find(['\r', '\n']) {
            Some(i) => {
                let at = start + i;
                start = at + line_break_len(&text[at..]).expect("a line break");
            }
            None => start = text.len(),
        }
    }
    let preamble = text[..start].to_owned();
    let rest = &text[start..];
    let bytes = rest.as_bytes();
    let n = bytes.len();

    // Longest first, so that `!#` wins over `!`.
    let mut delimiters: Vec<&str> = options
        .delimiters
        .iter()
        .map(String::as_str)
        .filter(|d| !d.is_empty())
        .collect();
    delimiters.sort_by_key(|d| std::cmp::Reverse(d.len()));
    let mut uses = vec![0usize; delimiters.len()];
    let delimiter_at = |i: usize| {
        delimiters
            .iter()
            .position(|d| bytes[i..].starts_with(d.as_bytes()))
    };
    let mut quote_buffer = [0u8; 4];
    let quote: Option<&str> = options.quote.map(|q| &*q.encode_utf8(&mut quote_buffer));
    // The bytes that may mean something: outside quotes, the first byte of
    // each delimiter, of the quote and of line breaks; inside, the quote's
    // and line breaks' (to count lines).
    let mut special = [false; 256];
    let mut special_quoted = [false; 256];
    for table in [&mut special, &mut special_quoted] {
        table[usize::from(b'\r')] = true;
        table[usize::from(b'\n')] = true;
        if let Some(q) = quote {
            table[usize::from(q.as_bytes()[0])] = true;
        }
    }
    for d in &delimiters {
        special[usize::from(d.as_bytes()[0])] = true;
    }

    let mut parsed = Parsed {
        rows: Vec::new(),
        lines: Vec::new(),
        all_quoted: false,
        trailing_delimiter: false,
        line_ending: None,
        main_delimiter: None,
        preamble,
        blank_lines: 0,
        trimmed: 0,
    };
    let mut row: Vec<Cell> = Vec::new();
    // The parts of a quoted field already read (doubled quotes undone).
    let mut field = String::new();
    // Where the part of the field not yet in `field` starts.
    let mut run = 0;
    let mut quoted = false;
    let mut in_quotes = false;
    // A delimiter was the last thing read on this line.
    let mut after_delimiter = false;
    let mut unquoted_fields = 0;
    let mut trailing_rows = 0;
    // The most fields a row had so far.
    let mut widest = 0;
    // Line endings seen at the end of rows: LF, CRLF, CR.
    let mut endings = [0usize; 3];
    let mut line = options.skip_lines + 1;
    let mut row_line = line;

    // Ends the field at `end`; gives up once the rows read so far, padded
    // to the widest, are sure to hold too many cells. (Each row may yet
    // lose a trailing delimiter's empty field, hence one column less.)
    macro_rules! end_field {
        ($end:expr) => {{
            if !quoted {
                unquoted_fields += 1;
            }
            let cell = take_field(
                &mut field,
                &rest[run..$end],
                quoted,
                options.trim,
                &mut parsed.trimmed,
            );
            row.push(cell);
            let rows = parsed.rows.len() + 1;
            let cols = widest.max(row.len());
            if rows.saturating_mul(cols.saturating_sub(1).max(1)) > limits.max_cells {
                return Err(Stop::TooLarge(TooLarge { rows, cols }));
            }
        }};
    }

    let mut i = 0;
    while i < n {
        if in_quotes {
            let q = quote.expect("quotes are opened with a quote");
            let Some(j) = bytes[i..]
                .iter()
                .position(|&b| special_quoted[usize::from(b)])
                .map(|j| i + j)
            else {
                break;
            };
            let b = bytes[j];
            if bytes[j..].starts_with(q.as_bytes()) {
                let after = j + q.len();
                field.push_str(&rest[run..j]);
                if bytes[after..].starts_with(q.as_bytes()) {
                    // Doubled: one quote of the text.
                    field.push_str(q);
                    i = after + q.len();
                } else {
                    in_quotes = false;
                    i = after;
                }
                run = i;
            } else if b == b'\n' || (b == b'\r' && bytes.get(j + 1) != Some(&b'\n')) {
                line += 1;
                i = j + 1;
            } else {
                i = j + char_len(b);
            }
            continue;
        }
        // Plain text up to the next byte that may mean something.
        let j = bytes[i..]
            .iter()
            .position(|&b| special[usize::from(b)])
            .map_or(n, |j| i + j);
        if j > i {
            after_delimiter = false;
            i = j;
            if i == n {
                break;
            }
        }
        let b = bytes[i];
        if let Some(q) = quote
            && !quoted
            && bytes[i..].starts_with(q.as_bytes())
            && rest[run..i].trim().is_empty()
        {
            field.clear();
            i += q.len();
            run = i;
            in_quotes = true;
            quoted = true;
            after_delimiter = false;
        } else if let Some(d) = delimiter_at(i) {
            let end = i;
            i += delimiters[d].len();
            // Merged delimiters at the start of a line (aligned text) are
            // indentation, not an empty first field.
            let leading = options.merge_delimiters && row.is_empty() && !quoted && run == end;
            if !leading {
                end_field!(end);
                quoted = false;
                uses[d] += 1;
            }
            if options.merge_delimiters {
                while let Some(d) = delimiter_at(i) {
                    i += delimiters[d].len();
                }
            }
            run = i;
            after_delimiter = !leading;
        } else if b == b'\r' || b == b'\n' {
            let end = i;
            let len = if bytes[i..].starts_with(b"\r\n") {
                2
            } else {
                1
            };
            i += len;
            line += 1;
            if row.is_empty() && !quoted && run == end {
                parsed.blank_lines += 1;
                row_line = line;
                run = i;
                continue;
            }
            endings[match (b, len) {
                (b'\n', _) => 0,
                (_, 2) => 1,
                _ => 2,
            }] += 1;
            if after_delimiter {
                trailing_rows += 1;
            }
            after_delimiter = false;
            end_field!(end);
            quoted = false;
            run = i;
            // The next row is likely as wide: room for it up front.
            let width = row.len();
            widest = widest.max(width);
            parsed
                .rows
                .push(std::mem::replace(&mut row, Vec::with_capacity(width)));
            if limits.lines {
                parsed.lines.push(row_line);
            }
            row_line = line;
            let rows = parsed.rows.len();
            if rows == limits.max_rows {
                break;
            }
            if rows.is_multiple_of(CANCEL_EVERY)
                && limits
                    .cancel
                    .is_some_and(|c| c.load(AtomicOrdering::Relaxed))
            {
                return Err(Stop::Cancelled);
            }
        } else {
            // The start of a delimiter or quote that isn't one: text.
            i += char_len(b);
            after_delimiter = false;
        }
    }
    if parsed.rows.len() < limits.max_rows && (!row.is_empty() || run < n || quoted) {
        if after_delimiter {
            trailing_rows += 1;
        }
        end_field!(n);
        parsed.rows.push(row);
        if limits.lines {
            parsed.lines.push(row_line);
        }
    }

    let rows = parsed.rows.len();
    parsed.trailing_delimiter = rows > 0 && trailing_rows == rows;
    if parsed.trailing_delimiter {
        for row in &mut parsed.rows {
            row.pop();
        }
        unquoted_fields -= rows;
    }
    parsed.all_quoted = parsed.rows.iter().any(|r| !r.is_empty()) && unquoted_fields == 0;
    let most = *endings.iter().max().expect("three counts");
    parsed.line_ending = (most > 0).then(|| {
        [LineEnding::Lf, LineEnding::CrLf, LineEnding::Cr][endings
            .iter()
            .position(|&n| n == most)
            .expect("the largest")]
    });
    // The most used delimiter; on a tie, the first one chosen.
    parsed.main_delimiter = options
        .delimiters
        .iter()
        .filter(|d| !d.is_empty())
        .max_by_key(|d| {
            let used = delimiters
                .iter()
                .position(|x| x == d)
                .map_or(0, |k| uses[k]);
            let rank = options.delimiters.iter().position(|x| x == *d).unwrap_or(0);
            (used, std::cmp::Reverse(rank))
        })
        .cloned();
    Ok(parsed)
}

/// The delimiter that splits the first lines into the same number of
/// fields, the most of them; a comma if none does.
fn sniff_delimiter(text: &str) -> &'static str {
    let sample: String = text.lines().take(50).collect::<Vec<_>>().join("\n");
    let mut best = (",", 0usize, false);
    for delimiter in DELIMITERS {
        let options = ImportOptions {
            encoding: Encoding::Utf8,
            skip_lines: 0,
            delimiters: vec![delimiter.to_owned()],
            quote: Some('"'),
            merge_delimiters: false,
            trim: false,
        };
        let counts: Vec<usize> = split_fields(&sample, &options, usize::MAX)
            .rows
            .iter()
            .map(Vec::len)
            .collect();
        let Some(&first) = counts.first() else {
            continue;
        };
        let consistent = counts.iter().all(|&c| c == first);
        // A consistent split beats an inconsistent one; then more fields win.
        if first > 1 && (consistent, first) > (best.2, best.1) {
            best = (delimiter, first, consistent);
        }
    }
    best.0
}

/// A character the file's encoding can't represent: the first one, and
/// the cell holding it (`None`: in the lines kept from the top).
#[derive(Debug, PartialEq, Eq)]
pub struct Unencodable {
    pub character: char,
    pub cell: Option<(usize, usize)>,
}

/// What decides which cells go between quotes, worked out once per save.
struct Quoting<'a> {
    quote_all: bool,
    quote: Option<char>,
    /// The delimiter written and the others the file was read with.
    delimiters: Vec<&'a [u8]>,
    /// Bytes that start a delimiter (or need quotes by themselves).
    starts: [bool; 256],
}

impl<'a> Quoting<'a> {
    fn new(delimiter: &'a str, format: &'a Format) -> Self {
        let delimiters: Vec<&[u8]> = std::iter::once(delimiter)
            .chain(format.other_delimiters.iter().map(String::as_str))
            .filter(|d| !d.is_empty())
            .map(str::as_bytes)
            .collect();
        let mut starts = [false; 256];
        starts[usize::from(b'\r')] = true;
        starts[usize::from(b'\n')] = true;
        if format.quote == Some('"') {
            starts[usize::from(b'"')] = true;
        }
        for d in &delimiters {
            starts[usize::from(d[0])] = true;
        }
        Self {
            quote_all: format.quote_all,
            quote: format.quote,
            delimiters,
            starts,
        }
    }
}

/// Whether `cell` must be written between quotes to read back the same.
fn needs_quotes(cell: &str, alone: bool, quoting: &Quoting) -> bool {
    if quoting.quote_all {
        return true;
    }
    // A line with one empty field isn't a blank line.
    if alone && cell.is_empty() {
        return true;
    }
    // One pass over the bytes: line breaks, double quotes (the reader
    // takes a quote as one only at the start of a field; double quotes
    // elsewhere are quoted too, as in files written by spreadsheets) and
    // delimiters. A delimiter's first byte never falls inside a character.
    let bytes = cell.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if quoting.starts[usize::from(b)]
            && (matches!(b, b'\r' | b'\n')
                || (b == b'"' && quoting.quote == Some('"'))
                || quoting.delimiters.iter().any(|d| bytes[i..].starts_with(d)))
        {
            return true;
        }
    }
    // The end of the cell and the delimiter after it can make up the
    // delimiter earlier ("a!" then "!!" reads "a", "!…").
    for d in &quoting.delimiters {
        if (1..d.len()).any(|k| bytes.ends_with(&d[..k]) && d[k..] == d[..d.len() - k]) {
            return true;
        }
    }
    match quoting.quote {
        Some('"') => false,
        Some(quote) => cell.trim_start().starts_with(quote),
        None => cell.trim_start().starts_with('"'),
    }
}

/// Why a table couldn't be written.
#[derive(Debug)]
pub enum WriteError {
    Unencodable(Unencodable),
    Io(std::io::Error),
}

impl From<std::io::Error> for WriteError {
    fn from(error: std::io::Error) -> Self {
        WriteError::Io(error)
    }
}

/// The table as a whole file, in memory.
#[cfg(test)]
pub fn write(table: &Table, format: &Format) -> Result<Vec<u8>, Unencodable> {
    let mut bytes = Vec::new();
    match write_to(table, format, &mut bytes) {
        Ok(()) => Ok(bytes),
        Err(WriteError::Unencodable(bad)) => Err(bad),
        Err(WriteError::Io(error)) => unreachable!("writing to memory: {error}"),
    }
}

/// Writes `text` to `out` in `encoding`, through `buf`; `false`: a
/// character can't be written in it, and nothing was.
fn encode<W: std::io::Write + ?Sized>(
    text: &str,
    encoding: Encoding,
    buf: &mut Vec<u8>,
    out: &mut W,
) -> std::io::Result<bool> {
    match encoding {
        Encoding::Utf8 | Encoding::Utf8Bom => out.write_all(text.as_bytes())?,
        Encoding::Windows1252 => {
            // ASCII text is borrowed as it is, not copied.
            let (bytes, _, unmappable) = encoding_rs::WINDOWS_1252.encode(text);
            if unmappable {
                return Ok(false);
            }
            out.write_all(&bytes)?;
        }
        Encoding::Utf16Le | Encoding::Utf16Be => {
            let little = encoding == Encoding::Utf16Le;
            buf.clear();
            for unit in text.encode_utf16() {
                buf.extend_from_slice(&if little {
                    unit.to_le_bytes()
                } else {
                    unit.to_be_bytes()
                });
            }
            out.write_all(buf)?;
        }
    }
    Ok(true)
}

/// Writes the table to `out`, one line at a time: the whole file is
/// never held in memory. On `WriteError::Unencodable`, `out` holds the
/// lines before the bad one.
pub fn write_to<W: std::io::Write + ?Sized>(
    table: &Table,
    format: &Format,
    out: &mut W,
) -> Result<(), WriteError> {
    let delimiter = if format.delimiter.is_empty() {
        ","
    } else {
        format.delimiter.as_str()
    };
    let quoting = Quoting::new(delimiter, format);
    // A file without quoting gets double quotes where they can't be done
    // without: the usual ones, which a reader expects.
    let quote = format.quote.unwrap_or('"');
    let newline = format.line_ending.as_str();
    let encoding = format.encoding;
    let mut buf = Vec::new();
    let mut put = |text: &str, out: &mut W| -> Result<(), WriteError> {
        if encode(text, encoding, &mut buf, out)? {
            Ok(())
        } else {
            Err(WriteError::Unencodable(first_unencodable(table, format)))
        }
    };
    // The byte order mark: EF BB BF in UTF-8, FEFF in UTF-16.
    if matches!(
        encoding,
        Encoding::Utf8Bom | Encoding::Utf16Le | Encoding::Utf16Be
    ) {
        put("\u{FEFF}", out)?;
    }
    put(&format.preamble, out)?;
    let mut line = String::new();
    for row in table.rows() {
        line.clear();
        for (i, cell) in row.iter().enumerate() {
            if i > 0 {
                line.push_str(delimiter);
            }
            let protected;
            let cell = if format.protect_formulas && looks_like_formula(cell) {
                protected = format!("'{cell}");
                protected.as_str()
            } else {
                cell.as_str()
            };
            if needs_quotes(cell, row.len() == 1, &quoting) {
                line.push(quote);
                for (i, piece) in cell.split(quote).enumerate() {
                    if i > 0 {
                        line.push(quote);
                        line.push(quote);
                    }
                    line.push_str(piece);
                }
                line.push(quote);
            } else {
                line.push_str(cell);
            }
        }
        if format.trailing_delimiter {
            line.push_str(delimiter);
        }
        line.push_str(newline);
        put(&line, out)?;
    }
    Ok(())
}

/// Whether a spreadsheet opening the file would take `cell` for a formula:
/// it starts with =, +, -, @, a tab or a carriage return, and isn't a
/// number ("-5" stays as it is).
fn looks_like_formula(cell: &str) -> bool {
    if !cell.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        return false;
    }
    let cell = cell.trim();
    if !cell.contains(',') {
        return cell.parse::<f64>().is_err();
    }
    // A decimal comma ("-5,25"), read as a point without an allocation
    // for the usual short numbers.
    let mut buf = [0; 64];
    let Some(buf) = buf.get_mut(..cell.len()) else {
        return cell.replace(',', ".").parse::<f64>().is_err();
    };
    for (to, &from) in buf.iter_mut().zip(cell.as_bytes()) {
        *to = if from == b',' { b'.' } else { from };
    }
    // Replacing an ASCII byte by another keeps the text valid UTF-8.
    std::str::from_utf8(buf).map_or(true, |text| text.parse::<f64>().is_err())
}

/// Where Windows-1252 first fails: in the kept lines, else in a cell.
fn first_unencodable(table: &Table, format: &Format) -> Unencodable {
    let bad = |text: &str| {
        text.chars().find(|c| {
            let mut buf = [0; 4];
            encoding_rs::WINDOWS_1252.encode(c.encode_utf8(&mut buf)).2
        })
    };
    if let Some(character) = bad(&format.preamble) {
        return Unencodable {
            character,
            cell: None,
        };
    }
    for (r, row) in table.rows().iter().enumerate() {
        for (c, cell) in row.iter().enumerate() {
            if !cell.is_ascii()
                && let Some(character) = bad(cell)
            {
                return Unencodable {
                    character,
                    cell: Some((r, c)),
                };
            }
        }
    }
    // Delimiters and quotes come from the user too.
    let character = bad(&format.delimiter)
        .or_else(|| format.quote.filter(|q| bad(&q.to_string()).is_some()))
        .unwrap_or('\u{FFFD}');
    Unencodable {
        character,
        cell: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(rows: &[&[&str]]) -> Table {
        Table::from_rows(
            rows.iter()
                .map(|r| r.iter().map(|s| Cell::from(*s)).collect())
                .collect(),
        )
    }

    fn options(delimiters: &[&str]) -> ImportOptions {
        ImportOptions {
            encoding: Encoding::Utf8,
            skip_lines: 0,
            delimiters: delimiters.iter().map(|d| d.to_string()).collect(),
            quote: Some('"'),
            merge_delimiters: false,
            trim: false,
        }
    }

    /// Reads `bytes` with `options`, writes the table back, and checks the
    /// bytes are the same.
    fn round_trip(bytes: &[u8], options: &ImportOptions) -> Table {
        let imported = import(bytes, options).expect("small file");
        let written = write(&imported.table, &imported.format).expect("encodable");
        assert_eq!(
            String::from_utf8_lossy(&written),
            String::from_utf8_lossy(bytes)
        );
        imported.table
    }

    #[test]
    fn reads_a_french_excel_file() {
        let bytes = b"Nom;Pr\xE9nom;Ville\r\nDupont;Ren\xE9;\"Paris; 5e\"\r\n";
        let (t, format) = parse(bytes);
        assert_eq!(
            t,
            table(&[
                &["Nom", "Prénom", "Ville"],
                &["Dupont", "René", "Paris; 5e"]
            ])
        );
        assert_eq!(format.delimiter, ";");
        assert_eq!(format.encoding, Encoding::Windows1252);
        assert_eq!(format.line_ending, LineEnding::CrLf);
        assert_eq!(write(&t, &format).unwrap(), bytes);
    }

    #[test]
    fn round_trips_utf8_with_bom_and_quotes() {
        let bytes = "\u{feff}a,b\n\"x, \"\"y\"\"\",\"line\nbreak\"\n".as_bytes();
        let (t, format) = parse(bytes);
        assert_eq!(format.encoding, Encoding::Utf8Bom);
        assert_eq!(format.delimiter, ",");
        assert_eq!(format.line_ending, LineEnding::Lf);
        assert_eq!(t.cell(1, 0), "x, \"y\"");
        assert_eq!(t.cell(1, 1), "line\nbreak");
        assert_eq!(write(&t, &format).unwrap(), bytes);
    }

    #[test]
    fn sniffs_tabs_and_pads_ragged_rows() {
        let (t, format) = parse(b"a\tb\tc\n1\t2\n");
        assert_eq!(format.delimiter, "\t");
        assert_eq!(t, table(&[&["a", "b", "c"], &["1", "2", ""]]));
    }

    #[test]
    fn empty_files_give_one_cell() {
        let (t, _) = parse(b"");
        assert_eq!((t.row_count(), t.col_count()), (1, 1));
    }

    #[test]
    fn refuses_characters_windows_1252_lacks_and_says_where() {
        let format = Format {
            encoding: Encoding::Windows1252,
            ..Format::default()
        };
        assert_eq!(
            write(&table(&[&["€ ok", "x"], &["a", "1\u{202F}000"]]), &format),
            Err(Unencodable {
                character: '\u{202F}',
                cell: Some((1, 1))
            })
        );
    }

    #[test]
    fn every_edit_undoes() {
        let original = table(&[&["h1", "h2"], &["b", "2"], &["a", "10"], &["c", "1,5"]]);
        let edits = [
            Edit::SetCells(vec![
                (0, 0, "x".into()),
                (1, 1, "y".into()),
                (0, 0, "z".into()),
            ]),
            Edit::InsertRows(1, vec![vec!["n".into(), "m".into()]]),
            Edit::RemoveRows(1, 2),
            Edit::RemoveRows(0, 4),
            Edit::InsertCols(
                1,
                vec![vec!["1".into(), "2".into(), "3".into(), "4".into()]],
            ),
            Edit::RemoveCols(0, 1),
            Edit::RemoveCols(0, 2),
            Edit::Reorder(1, original.sort_order(1, 1, false)),
            Edit::Reorder(0, vec![2, 0, 3, 1]),
            Edit::SetBlock(
                1,
                vec![1, 0],
                vec![vec!["p".into(), "q".into()], vec!["r".into()]],
            ),
        ];
        for edit in edits {
            let mut t = original.clone();
            let undo = t.apply(edit.clone());
            assert_ne!(t, original, "{edit:?}");
            let redo = t.apply(undo);
            assert_eq!(t, original, "{edit:?}");
            t.apply(redo);
            assert_ne!(t, original, "{edit:?}");
        }
    }

    #[test]
    fn a_block_sets_its_cells_and_gives_back_the_old_ones() {
        let mut t = table(&[&["a", "b", "c"], &["d", "e", "f"], &["g", "h", "i"]]);
        let undo = t.apply(Edit::SetBlock(
            1,
            vec![2, 0],
            vec![vec!["x".into(), "y".into()], vec!["z".into()]],
        ));
        assert_eq!(
            t,
            table(&[&["a", "b", "c"], &["y", "e", "x"], &["g", "h", "z"]])
        );
        assert_eq!(
            undo,
            Edit::SetBlock(
                1,
                vec![2, 0],
                vec![vec!["f".into(), "d".into()], vec!["i".into()]]
            )
        );
        t.apply(undo);
        assert_eq!(
            t,
            table(&[&["a", "b", "c"], &["d", "e", "f"], &["g", "h", "i"]])
        );
    }

    #[test]
    fn inserted_columns_grow_rows_by_exactly_their_count() {
        let mut t = table(&[&["a", "b"], &["c", "d"]]);
        t.apply(Edit::InsertCols(
            1,
            vec![vec!["1".into(), "2".into()], vec!["3".into(), "4".into()]],
        ));
        assert_eq!(t, table(&[&["a", "1", "3", "b"], &["c", "2", "4", "d"]]));
        let big = Table::from_rows(vec![vec![Cell::default(); 300]; 3]);
        let mut big = big;
        for row in Arc::make_mut(&mut big.rows) {
            row.shrink_to_fit();
        }
        big.apply(Edit::InsertCols(0, vec![vec![Cell::default(); 3]]));
        assert!(
            big.rows
                .iter()
                .all(|r| r.len() == 301 && r.capacity() == 301)
        );
    }

    #[test]
    fn reorders_in_place_and_inverts() {
        let mut t = table(&[&["0"], &["1"], &["2"], &["3"], &["4"], &["5"]]);
        // Two cycles and a row left in place.
        let order = vec![2, 0, 1, 3, 5, 4];
        let undo = t.apply(Edit::Reorder(0, order.clone()));
        let column: Vec<_> = t.rows().iter().map(|r| r[0].as_str()).collect();
        assert_eq!(column, ["2", "0", "1", "3", "5", "4"]);
        assert_eq!(undo, Edit::Reorder(0, invert(&order)));
        t.apply(undo);
        let column: Vec<_> = t.rows().iter().map(|r| r[0].as_str()).collect();
        assert_eq!(column, ["0", "1", "2", "3", "4", "5"]);
    }

    #[test]
    fn sorts_numbers_by_value_and_text_ignoring_case() {
        let mut t = table(&[&["h"], &["10"], &["b"], &[""], &["1,5"], &["A"], &["2"]]);
        let order = t.sort_order(1, 0, false);
        t.apply(Edit::Reorder(1, order));
        let column: Vec<_> = t.rows().iter().map(|r| r[0].as_str()).collect();
        assert_eq!(column, ["h", "1,5", "2", "10", "A", "b", ""]);
    }

    #[test]
    fn protects_cells_a_spreadsheet_would_run() {
        let format = Format {
            protect_formulas: true,
            line_ending: LineEnding::Lf,
            ..Format::default()
        };
        let t = table(&[&["=1+1", "-5", "@SUM(A1)", "-x", "a=b"]]);
        assert_eq!(
            String::from_utf8(write(&t, &format).unwrap()).unwrap(),
            "'=1+1,-5,'@SUM(A1),'-x,a=b\n"
        );
    }

    #[test]
    fn negative_numbers_with_a_decimal_comma_are_not_formulas() {
        for number in ["-5", "-5,25", "+1.5", " -3 ", "-1e3"] {
            assert!(!looks_like_formula(number), "{number:?}");
        }
        let long = format!("-{}", "1".repeat(80));
        assert!(!looks_like_formula(&long));
        assert!(!looks_like_formula(&format!("{long},5")));
        for formula in ["=1", "-x", "-5,2,1", "@A1", "+", "-,"] {
            assert!(looks_like_formula(formula), "{formula:?}");
        }
    }

    #[test]
    fn encodings_write_their_byte_order_mark_and_lines() {
        let t = table(&[&["é", "\"q\""]]);
        let format = |encoding| Format {
            encoding,
            line_ending: LineEnding::Lf,
            preamble: "sep=,\n".into(),
            ..Format::default()
        };
        let text = "sep=,\né,\"\"\"q\"\"\"\n";
        let mut bom = b"\xEF\xBB\xBF".to_vec();
        bom.extend_from_slice(text.as_bytes());
        assert_eq!(write(&t, &format(Encoding::Utf8)).unwrap(), text.as_bytes());
        assert_eq!(write(&t, &format(Encoding::Utf8Bom)).unwrap(), bom);
        let latin: Vec<u8> = text.chars().map(|c| c as u8).collect();
        assert_eq!(write(&t, &format(Encoding::Windows1252)).unwrap(), latin);
        let units: Vec<u16> = "\u{FEFF}"
            .encode_utf16()
            .chain(text.encode_utf16())
            .collect();
        let le: Vec<u8> = units.iter().flat_map(|u| u.to_le_bytes()).collect();
        let be: Vec<u8> = units.iter().flat_map(|u| u.to_be_bytes()).collect();
        assert_eq!(write(&t, &format(Encoding::Utf16Le)).unwrap(), le);
        assert_eq!(write(&t, &format(Encoding::Utf16Be)).unwrap(), be);
    }

    #[test]
    fn quotes_only_the_cells_that_need_them() {
        let format = |delimiter: &str, quote| Format {
            delimiter: delimiter.into(),
            quote,
            line_ending: LineEnding::Lf,
            ..Format::default()
        };
        let cases: &[(&str, Option<char>, &str, bool)] = &[
            (",", Some('"'), "plain", false),
            (",", Some('"'), "a,b", true),
            (",", Some('"'), "a\nb", true),
            (",", Some('"'), "a\rb", true),
            (",", Some('"'), "say \"hi\"", true),
            (",", Some('\''), "it's", false),
            (",", Some('\''), " 'x", true),
            (",", None, "5\" screen", false),
            (",", None, "\"x", true),
            ("!#", Some('"'), "a!b#c", false),
            ("!#", Some('"'), "a!#b", true),
            ("!!", None, "a!", true),
            ("!#", None, "a!", false),
            ("abab", None, "xab", true),
            ("abab", None, "xa", false),
            (";", Some('"'), "\u{e9}t\u{e9};", true),
        ];
        for &(delimiter, quote, cell, expected) in cases {
            let f = format(delimiter, quote);
            let quoting = Quoting::new(delimiter, &f);
            assert_eq!(
                needs_quotes(cell, false, &quoting),
                expected,
                "{cell:?} with {delimiter:?}"
            );
        }
        let f = format(",", Some('"'));
        assert!(needs_quotes("", true, &Quoting::new(",", &f)));
        assert!(!needs_quotes("", false, &Quoting::new(",", &f)));
    }

    #[test]
    fn an_unencodable_character_names_its_cell() {
        let format = Format {
            encoding: Encoding::Windows1252,
            ..Format::default()
        };
        let t = table(&[&["a", "b"], &["c", "\u{4E2D}"]]);
        assert_eq!(
            write(&t, &format),
            Err(Unencodable {
                character: '\u{4E2D}',
                cell: Some((1, 1))
            })
        );
    }

    #[test]
    fn a_snapshot_keeps_the_cells_as_they_were() {
        let mut t = table(&[&["a", "b"]]);
        let snapshot = t.clone();
        assert!(Arc::ptr_eq(&t.rows, &snapshot.rows));
        t.apply(Edit::SetCells(vec![(0, 0, "z".into())]));
        assert_eq!(snapshot.cell(0, 0), "a");
        assert_eq!(t.cell(0, 0), "z");
        drop(snapshot);
        // Alone again: edits change the cells in place.
        let before = Arc::as_ptr(&t.rows);
        t.apply(Edit::SetCells(vec![(0, 1, "y".into())]));
        assert_eq!(Arc::as_ptr(&t.rows), before);
    }

    #[test]
    fn sorts_dates_and_times() {
        let column = |t: &Table| -> Vec<Cell> { t.rows().iter().map(|r| r[0].clone()).collect() };
        let mut t = table(&[
            &["create_time"],
            &["01/07/2026 17:55:46"],
            &["01/07/2026 01:33:44"],
            &["13/06/2026 09:00:00"],
            &["01/07/2026 17:32:30"],
            &["02/01/2027"],
            &[""],
        ]);
        let order = t.sort_order(1, 0, false);
        t.apply(Edit::Reorder(1, order));
        assert_eq!(
            column(&t),
            [
                "create_time",
                "13/06/2026 09:00:00",
                "01/07/2026 01:33:44",
                "01/07/2026 17:32:30",
                "01/07/2026 17:55:46",
                "02/01/2027",
                ""
            ]
        );
        // Month first when a day above 12 says so; ISO dates and times.
        let mut t = table(&[&["07/13/2026"], &["07/01/2026"], &["12/31/2025 11:59 PM"]]);
        let order = t.sort_order(0, 0, false);
        t.apply(Edit::Reorder(0, order));
        assert_eq!(
            column(&t),
            ["12/31/2025 11:59 PM", "07/01/2026", "07/13/2026"]
        );
        let mut t = table(&[
            &["2026-07-01T17:32:30"],
            &["2026-07-01T08:05"],
            &["17:00"],
            &["9:30"],
        ]);
        let order = t.sort_order(0, 0, true);
        t.apply(Edit::Reorder(0, order));
        assert_eq!(
            column(&t),
            ["2026-07-01T17:32:30", "2026-07-01T08:05", "17:00", "9:30"]
        );
    }

    #[test]
    fn imports_with_the_chosen_options() {
        let text = "# exported\nRC-1  142   Viager;'a ''b'' c'\n\nRC-2 156 x;\n";
        let options = ImportOptions {
            skip_lines: 1,
            quote: Some('\''),
            merge_delimiters: true,
            trim: true,
            ..options(&[" ", ";"])
        };
        assert_eq!(
            split(text, &options, usize::MAX),
            [
                vec!["RC-1", "142", "Viager", "a 'b' c"],
                vec!["RC-2", "156", "x", ""],
            ]
        );
        assert_eq!(split(text, &options, 1).len(), 1);
        let imported = import(text.as_bytes(), &options).unwrap();
        assert_eq!(
            (imported.table.row_count(), imported.table.col_count()),
            (2, 4)
        );
        // Spaces split the most fields.
        assert_eq!(imported.format.delimiter, " ");
        assert_eq!(imported.format.preamble, "# exported\n");
        assert_eq!(imported.losses.blank_lines, 1);
    }

    #[test]
    fn round_trips_poliris_files() {
        let bytes = b"\"RC-1\"!#\"Viager\"!#\"Prix ! #1\"!#\"\"!#\r\n\"RC-2\"!#\"Maison\"!#\"2\"!#\"x\"!#\r\n";
        let (t, format) = parse(bytes);
        assert_eq!(format.delimiter, "!#");
        assert!(format.quote_all && format.trailing_delimiter);
        assert_eq!(
            t,
            table(&[
                &["RC-1", "Viager", "Prix ! #1", ""],
                &["RC-2", "Maison", "2", "x"]
            ])
        );
        assert_eq!(write(&t, &format).unwrap(), bytes);
    }

    #[test]
    fn multi_character_delimiters_need_no_quotes() {
        assert_eq!(
            split("a!b#c!#d\n", &options(&["!#"]), usize::MAX),
            [vec!["a!b#c", "d"]]
        );
    }

    #[test]
    fn delimiters_that_repeat_a_character_survive_saving() {
        for delimiter in ["!!", "::", "||"] {
            let c = delimiter.chars().next().unwrap();
            let t = table(&[&[&format!("a{c}"), "b"]]);
            let format = Format {
                delimiter: delimiter.into(),
                quote: None,
                ..Format::default()
            };
            let bytes = write(&t, &format).unwrap();
            let back = import(&bytes, &options(&[delimiter])).unwrap().table;
            assert_eq!(back, t, "{delimiter}");
        }
    }

    #[test]
    fn files_without_quoting_stay_unquoted() {
        let mut no_quotes = options(&[","]);
        no_quotes.quote = None;
        round_trip(b"id,note\n1,5\" screen\n", &no_quotes);
        let mut apostrophe = options(&[","]);
        apostrophe.quote = Some('\'');
        round_trip(b"n\nit's\n", &apostrophe);
    }

    #[test]
    fn cells_holding_another_delimiter_are_quoted() {
        let both = options(&[",", ";"]);
        let t = round_trip(b"\"x;y\",b\nc,d\n", &both);
        assert_eq!(t.cell(0, 0), "x;y");
    }

    #[test]
    fn skipped_lines_are_kept_whatever_the_line_endings() {
        let mut skip = options(&[","]);
        skip.skip_lines = 1;
        let t = round_trip(b"# c\ra,b\r1,2\r", &skip);
        assert_eq!(t, table(&[&["a", "b"], &["1", "2"]]));
        round_trip(b"Export 2026\r\na,b\r\n", &skip);
    }

    #[test]
    fn line_endings_come_from_the_rows_not_the_cells() {
        let (_, format) = parse(b"a,b\n1,\"x\r\ny\"\n2,z\n");
        assert_eq!(format.line_ending, LineEnding::Lf);
        let (_, format) = parse(b"a,b\r1,2\r");
        assert_eq!(format.line_ending, LineEnding::Cr);
    }

    #[test]
    fn excel_sep_line_names_the_delimiter() {
        let options = ImportOptions::detect(b"sep=;\na;b;c\n1;2;3\n");
        assert_eq!(options.delimiters, [";"]);
        assert_eq!(options.skip_lines, 1);
        round_trip(b"sep=;\na;b;c\n1;2;3\n", &options);
    }

    #[test]
    fn merged_delimiters_ignore_indentation() {
        let mut aligned = options(&[" "]);
        aligned.merge_delimiters = true;
        assert_eq!(
            split("name  age\n  bob   42\nalice  7\n", &aligned, usize::MAX),
            [vec!["name", "age"], vec!["bob", "42"], vec!["alice", "7"]]
        );
    }

    #[test]
    fn utf16_files_read_and_write_back() {
        let text = "a\tb\r\n€\tx\r\n";
        let mut bytes = vec![0xFF, 0xFE];
        for unit in text.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        let (t, format) = parse(&bytes);
        assert_eq!(format.encoding, Encoding::Utf16Le);
        assert_eq!(t, table(&[&["a", "b"], &["€", "x"]]));
        assert_eq!(write(&t, &format).unwrap(), bytes);
    }

    #[test]
    fn a_stray_byte_keeps_a_utf8_file_in_utf8() {
        let mut bytes = "Café;Été\n".repeat(20).into_bytes();
        bytes.extend_from_slice(b"Caf\xE9;x\n");
        assert_eq!(detect_encoding(&bytes), Encoding::Utf8);
        let imported = import(&bytes, &ImportOptions::detect(&bytes)).unwrap();
        assert_eq!(imported.table.cell(0, 0), "Café");
        assert_eq!(imported.losses.invalid_bytes, 1);
        assert_eq!(
            detect_encoding(b"Caf\xE9;\xC9t\xE9\n"),
            Encoding::Windows1252
        );
    }

    #[test]
    fn refuses_files_that_would_pad_to_too_many_cells() {
        let mut bytes = b"a\n".repeat(10_000);
        bytes.extend(std::iter::repeat_n(b',', 10_000));
        assert!(matches!(
            import(&bytes, &options(&[","])),
            Err(TooLarge { rows: 10_001, .. })
        ));
    }

    #[test]
    fn preview_reads_the_start_and_numbers_lines() {
        let bytes = b"\nname,age\n\"Ann\nB\",3\nc,d\n";
        let preview = preview(bytes, &options(&[","]), 10);
        assert_eq!(preview.rows.len(), 3);
        assert_eq!(preview.lines, [2, 3, 5]);
    }

    fn limits(max_cells: usize, cancel: Option<&AtomicBool>) -> Limits<'_> {
        Limits {
            max_rows: usize::MAX,
            max_cells,
            lines: false,
            cancel,
        }
    }

    #[test]
    fn too_many_cells_stop_the_parse_early() {
        let text = "a,b,c\n".repeat(1000);
        assert_eq!(
            split_with(&text, &options(&[","]), &limits(100, None)).err(),
            Some(Stop::TooLarge(TooLarge { rows: 51, cols: 3 }))
        );
        // A single line of delimiters is stopped too.
        let text = ",".repeat(1000);
        assert!(matches!(
            split_with(&text, &options(&[","]), &limits(100, None)),
            Err(Stop::TooLarge(TooLarge { rows: 1, .. }))
        ));
        // The empty field after a trailing delimiter isn't counted against
        // the file: 50 rows of 2 cells fit in 100.
        let text = "a,b,\n".repeat(50);
        let parsed = split_with(&text, &options(&[","]), &limits(100, None)).expect("fits");
        assert!(parsed.trailing_delimiter);
        assert_eq!(parsed.rows.len(), 50);
        assert!(parsed.lines.is_empty());
    }

    #[test]
    fn a_cancelled_import_stops() {
        let bytes = "a,b\n".repeat(10_000);
        let cancel = AtomicBool::new(true);
        assert!(matches!(
            import_until(bytes.as_bytes(), &options(&[","]), &cancel),
            Err(Stop::Cancelled)
        ));
        let text = "a,b\n".repeat(CANCEL_EVERY * 2);
        assert!(matches!(
            split_with(&text, &options(&[","]), &limits(usize::MAX, Some(&cancel))),
            Err(Stop::Cancelled)
        ));
    }

    #[test]
    fn valid_text_is_borrowed_not_copied() {
        assert!(matches!(
            decode_as(b"\xEF\xBB\xBFname", Encoding::Utf8Bom).text,
            Cow::Borrowed("name")
        ));
        assert!(matches!(
            decode_as(b"name", Encoding::Windows1252).text,
            Cow::Borrowed("name")
        ));
        assert_eq!(decode_as(b"caf\xE9", Encoding::Windows1252).text, "café");
    }

    #[test]
    fn delimiters_and_quotes_of_several_bytes() {
        let options = ImportOptions {
            quote: Some('«'),
            ..options(&["→", "!#"])
        };
        assert_eq!(
            split("«a→b«→c!#«x««y«\r\né→ü\rlast→", &options, usize::MAX),
            [vec!["a→b", "c", "x«y"], vec!["é", "ü"], vec!["last", ""]]
        );
        // A character sharing its first byte with the delimiter is text.
        assert_eq!(split("a—b→c", &options, usize::MAX), [vec!["a—b", "c"]]);
    }

    #[test]
    fn rare_chars_are_listed_once() {
        let table = table(&[&["Zoë", "abc"], &["ëé", "日本"]]);
        assert_eq!(table.rare_chars(), "éë日本");
    }
}

#[cfg(test)]
mod memory {
    /// The memory a large file takes once read: `CSVEDIT_BENCH=file.csv
    /// cargo test --release memory -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn of_a_large_file() {
        let Some(path) = std::env::var_os("CSVEDIT_BENCH") else {
            return;
        };
        let bytes = std::fs::read(path).unwrap();
        let options = super::ImportOptions::detect(&bytes);
        let before = crate::diag::process_memory().unwrap();
        let imported = super::import(&bytes, &options).unwrap();
        drop(bytes);
        let after = crate::diag::process_memory().unwrap();
        eprintln!(
            "{:?} -> {:?} for {} rows",
            before,
            after,
            imported.table.row_count()
        );
    }
}
