//! The table being edited, how it was written on disk (delimiter, encoding,
//! line endings), and the undoable edits made to it.

use std::cmp::Ordering;

/// A rectangular grid of text cells: every row has the same number of
/// columns, and there is always at least one row and one column.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Table {
    rows: Vec<Vec<String>>,
}

impl Default for Table {
    fn default() -> Self {
        Self::empty(1, 3)
    }
}

impl Table {
    pub fn empty(rows: usize, cols: usize) -> Self {
        Self {
            rows: vec![vec![String::new(); cols.max(1)]; rows.max(1)],
        }
    }

    /// Ragged rows are padded with empty cells.
    pub fn from_rows(mut rows: Vec<Vec<String>>) -> Self {
        let cols = rows.iter().map(Vec::len).max().unwrap_or(0).max(1);
        if rows.is_empty() {
            rows.push(Vec::new());
        }
        for row in &mut rows {
            row.resize(cols, String::new());
        }
        Self { rows }
    }

    pub fn rows(&self) -> &[Vec<String>] {
        &self.rows
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub fn col_count(&self) -> usize {
        self.rows[0].len()
    }

    pub fn cell(&self, row: usize, col: usize) -> &str {
        &self.rows[row][col]
    }

    /// Applies `edit` and returns the edit that undoes it.
    pub fn apply(&mut self, edit: Edit) -> Edit {
        match edit {
            Edit::SetCells(cells) => {
                let mut old: Vec<_> = cells
                    .into_iter()
                    .map(|(row, col, text)| {
                        let old = std::mem::replace(&mut self.rows[row][col], text);
                        (row, col, old)
                    })
                    .collect();
                old.reverse();
                Edit::SetCells(old)
            }
            Edit::InsertRows(at, rows) => {
                let count = rows.len();
                self.rows.splice(at..at, rows);
                Edit::RemoveRows(at, count)
            }
            Edit::RemoveRows(at, count) => {
                let removed: Vec<_> = self.rows.drain(at..at + count).collect();
                if self.rows.is_empty() {
                    self.rows.push(vec![String::new(); removed[0].len()]);
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
                for (r, row) in self.rows.iter_mut().enumerate() {
                    row.splice(at..at, cols.iter().map(|col| col[r].clone()));
                }
                Edit::RemoveCols(at, count)
            }
            Edit::RemoveCols(at, count) => {
                let mut removed = vec![Vec::with_capacity(self.rows.len()); count];
                for row in &mut self.rows {
                    for (i, text) in row.drain(at..at + count).enumerate() {
                        removed[i].push(text);
                    }
                }
                if self.col_count() == 0 {
                    for row in &mut self.rows {
                        row.push(String::new());
                    }
                    return Edit::Batch(vec![
                        Edit::InsertCols(0, removed),
                        Edit::RemoveCols(count, 1),
                    ]);
                }
                Edit::InsertCols(at, removed)
            }
            Edit::Reorder(start, order) => {
                let mut taken: Vec<Option<Vec<String>>> = self.rows[start..start + order.len()]
                    .iter_mut()
                    .map(|row| Some(std::mem::take(row)))
                    .collect();
                for (i, &from) in order.iter().enumerate() {
                    self.rows[start + i] = taken[from].take().expect("a permutation");
                }
                let mut inverse = vec![0; order.len()];
                for (i, &from) in order.iter().enumerate() {
                    inverse[from] = i;
                }
                Edit::Reorder(start, inverse)
            }
            Edit::Batch(edits) => {
                let mut inverses: Vec<_> = edits.into_iter().map(|e| self.apply(e)).collect();
                inverses.reverse();
                Edit::Batch(inverses)
            }
        }
    }

    /// The order that sorts rows `start..` on column `col` (numbers by
    /// value, text ignoring case, empty cells last), for `Edit::Reorder`.
    pub fn sort_order(&self, start: usize, col: usize, descending: bool) -> Vec<usize> {
        let rows = &self.rows[start..];
        // 03/04/2026: the column's other dates tell which is the day.
        let day_first = day_comes_first(rows.iter().map(|row| row[col].as_str()));
        // Each cell's key is worked out once, not at every comparison.
        let keys: Vec<SortKey> = rows
            .iter()
            .map(|row| SortKey::of(&row[col], day_first))
            .collect();
        let mut order: Vec<usize> = (0..rows.len()).collect();
        order.sort_by(|&a, &b| {
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
    Text(String),
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
            None => SortKey::Text(cell.to_lowercase()),
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
    SetCells(Vec<(usize, usize, String)>),
    /// Rows inserted before row `at`.
    InsertRows(usize, Vec<Vec<String>>),
    RemoveRows(usize, usize),
    /// Columns (each a cell per row) inserted before column `at`.
    InsertCols(usize, Vec<Vec<String>>),
    RemoveCols(usize, usize),
    /// Rows from `start` on take the order given: row `start + i` becomes
    /// the old row `start + order[i]`.
    Reorder(usize, Vec<usize>),
    Batch(Vec<Edit>),
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
pub fn import(bytes: &[u8], options: &ImportOptions) -> Result<Imported, TooLarge> {
    let decoded = decode_as(bytes, options.encoding);
    let parsed = split_fields(&decoded.text, options, usize::MAX);
    let rows = parsed.rows.len();
    let cols = parsed.rows.iter().map(Vec::len).max().unwrap_or(0);
    if rows.saturating_mul(cols) > MAX_CELLS {
        return Err(TooLarge { rows, cols });
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
        invalid_bytes: decoded.invalid,
    };
    Ok(Imported {
        table: Table::from_rows(parsed.rows),
        format,
        losses,
        formulas: Vec::new(),
        sheets: Vec::new(),
    })
}

/// The first rows of a file, read the way `options` say, for the import
/// window: only the start of the file is read.
pub struct Preview {
    pub rows: Vec<Vec<String>>,
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
pub struct Decoded {
    pub text: String,
    pub invalid: usize,
}

/// The text of a file in `encoding` (byte order marks are left out).
pub fn decode_as(bytes: &[u8], encoding: Encoding) -> Decoded {
    match encoding {
        Encoding::Utf8 | Encoding::Utf8Bom => {
            let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
            if let Ok(text) = std::str::from_utf8(bytes) {
                return Decoded {
                    text: text.to_owned(),
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
            Decoded { text, invalid }
        }
        Encoding::Windows1252 => Decoded {
            text: encoding_rs::WINDOWS_1252
                .decode_without_bom_handling(bytes)
                .0
                .into_owned(),
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
                text: text.into_owned(),
            }
        }
    }
}

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
    // Windows-1252.
    let (mut valid, mut invalid) = (0usize, 0usize);
    for chunk in bytes.utf8_chunks() {
        valid += chunk.valid().chars().filter(|c| !c.is_ascii()).count();
        invalid += usize::from(!chunk.invalid().is_empty());
    }
    if valid > invalid {
        Encoding::Utf8
    } else {
        Encoding::Windows1252
    }
}

/// The first `max_rows` rows of `text`. Blank lines are left out.
#[cfg(test)]
pub fn split(text: &str, options: &ImportOptions, max_rows: usize) -> Vec<Vec<String>> {
    split_fields(text, options, max_rows).rows
}

struct Parsed {
    rows: Vec<Vec<String>>,
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

fn split_fields(text: &str, options: &ImportOptions, max_rows: usize) -> Parsed {
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

    // Longest first, so that `!#` wins over `!`.
    let mut delimiters: Vec<&str> = options
        .delimiters
        .iter()
        .map(String::as_str)
        .filter(|d| !d.is_empty())
        .collect();
    delimiters.sort_by_key(|d| std::cmp::Reverse(d.len()));
    let mut uses = vec![0usize; delimiters.len()];
    let delimiter_at = |i: usize| delimiters.iter().position(|d| rest[i..].starts_with(*d));

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
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut in_quotes = false;
    // A delimiter was the last thing read on this line.
    let mut after_delimiter = false;
    let mut unquoted_fields = 0;
    let mut trailing_rows = 0;
    // Line endings seen at the end of rows: LF, CRLF, CR.
    let mut endings = [0usize; 3];
    let mut line = options.skip_lines + 1;
    let mut row_line = line;
    let mut end_field = |row: &mut Vec<String>, field: &mut String, quoted: &mut bool| {
        let text = std::mem::take(field);
        if !*quoted {
            unquoted_fields += 1;
        }
        if options.trim && !*quoted {
            let trimmed = text.trim();
            if trimmed.len() != text.len() {
                parsed.trimmed += 1;
                row.push(trimmed.to_owned());
                *quoted = false;
                return;
            }
        }
        row.push(text);
        *quoted = false;
    };
    let mut i = 0;
    while let Some(c) = rest[i..].chars().next() {
        let next = i + c.len_utf8();
        if in_quotes {
            if Some(c) == options.quote {
                if rest[next..].starts_with(c) {
                    field.push(c);
                    i = next + c.len_utf8();
                    continue;
                }
                in_quotes = false;
            } else {
                if c == '\n' || (c == '\r' && !rest[next..].starts_with('\n')) {
                    line += 1;
                }
                field.push(c);
            }
            i = next;
        } else if Some(c) == options.quote && !quoted && field.trim().is_empty() {
            field.clear();
            in_quotes = true;
            quoted = true;
            after_delimiter = false;
            i = next;
        } else if let Some(d) = delimiter_at(i) {
            i += delimiters[d].len();
            // Merged delimiters at the start of a line (aligned text) are
            // indentation, not an empty first field.
            let leading = options.merge_delimiters && row.is_empty() && field.is_empty() && !quoted;
            if !leading {
                end_field(&mut row, &mut field, &mut quoted);
                uses[d] += 1;
            }
            if options.merge_delimiters {
                while let Some(d) = delimiter_at(i) {
                    i += delimiters[d].len();
                }
            }
            after_delimiter = !leading;
        } else if c == '\r' || c == '\n' {
            let len = line_break_len(&rest[i..]).expect("a line break");
            i += len;
            line += 1;
            if row.is_empty() && field.is_empty() && !quoted {
                parsed.blank_lines += 1;
                row_line = line;
                continue;
            }
            endings[match (c, len) {
                ('\n', _) => 0,
                (_, 2) => 1,
                _ => 2,
            }] += 1;
            if after_delimiter {
                trailing_rows += 1;
            }
            after_delimiter = false;
            end_field(&mut row, &mut field, &mut quoted);
            // The next row is likely as wide: room for it up front.
            let width = row.len();
            parsed
                .rows
                .push(std::mem::replace(&mut row, Vec::with_capacity(width)));
            parsed.lines.push(row_line);
            row_line = line;
            if parsed.rows.len() == max_rows {
                break;
            }
        } else {
            field.push(c);
            after_delimiter = false;
            i = next;
        }
    }
    if parsed.rows.len() < max_rows && (!row.is_empty() || !field.is_empty() || quoted) {
        if after_delimiter {
            trailing_rows += 1;
        }
        end_field(&mut row, &mut field, &mut quoted);
        parsed.rows.push(row);
        parsed.lines.push(row_line);
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
    parsed
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

/// Whether `cell` must be written between quotes to read back the same.
fn needs_quotes(cell: &str, alone: bool, format: &Format) -> bool {
    if format.quote_all || cell.contains(['\r', '\n']) {
        return true;
    }
    // A line with one empty field isn't a blank line.
    if alone && cell.is_empty() {
        return true;
    }
    let delimiters = std::iter::once(&format.delimiter).chain(&format.other_delimiters);
    for delimiter in delimiters.filter(|d| !d.is_empty()) {
        if cell.contains(delimiter.as_str()) {
            return true;
        }
        // The end of the cell and the delimiter after it can make up the
        // delimiter earlier ("a!" then "!!" reads "a", "!…").
        if delimiter.chars().any(|c| cell.ends_with(c))
            && format!("{cell}{delimiter}").find(delimiter.as_str()) != Some(cell.len())
        {
            return true;
        }
    }
    // The reader takes a quote as one only at the start of a field; double
    // quotes elsewhere are quoted too, as in files written by spreadsheets.
    match format.quote {
        Some('"') => cell.contains('"'),
        Some(quote) => cell.trim_start().starts_with(quote),
        None => cell.trim_start().starts_with('"'),
    }
}

pub fn write(table: &Table, format: &Format) -> Result<Vec<u8>, Unencodable> {
    let delimiter = if format.delimiter.is_empty() {
        ","
    } else {
        format.delimiter.as_str()
    };
    // A file without quoting gets double quotes where they can't be done
    // without: the usual ones, which a reader expects.
    let quote = format.quote.unwrap_or('"');
    let doubled = format!("{quote}{quote}");
    let newline = format.line_ending.as_str();
    let size: usize = table
        .rows()
        .iter()
        .map(|row| row.iter().map(|c| c.len() + delimiter.len()).sum::<usize>() + 2)
        .sum();
    let mut text = String::with_capacity(format.preamble.len() + size + size / 8);
    text.push_str(&format.preamble);
    for row in table.rows() {
        for (i, cell) in row.iter().enumerate() {
            if i > 0 {
                text.push_str(delimiter);
            }
            let protected;
            let cell = if format.protect_formulas && looks_like_formula(cell) {
                protected = format!("'{cell}");
                &protected
            } else {
                cell
            };
            if needs_quotes(cell, row.len() == 1, format) {
                text.push(quote);
                text.push_str(&cell.replace(quote, &doubled));
                text.push(quote);
            } else {
                text.push_str(cell);
            }
        }
        if format.trailing_delimiter {
            text.push_str(delimiter);
        }
        text.push_str(newline);
    }
    Ok(match format.encoding {
        Encoding::Utf8 => text.into_bytes(),
        Encoding::Utf8Bom => {
            let mut bytes = Vec::with_capacity(text.len() + 3);
            bytes.extend_from_slice(b"\xEF\xBB\xBF");
            bytes.extend_from_slice(text.as_bytes());
            bytes
        }
        Encoding::Windows1252 => {
            let (bytes, _, unmappable) = encoding_rs::WINDOWS_1252.encode(&text);
            if unmappable {
                return Err(first_unencodable(table, format));
            }
            bytes.into_owned()
        }
        Encoding::Utf16Le | Encoding::Utf16Be => {
            let little = format.encoding == Encoding::Utf16Le;
            let mut bytes = Vec::with_capacity(text.len() * 2 + 2);
            for unit in std::iter::once(0xFEFF).chain(text.encode_utf16()) {
                bytes.extend_from_slice(&if little {
                    unit.to_le_bytes()
                } else {
                    unit.to_be_bytes()
                });
            }
            bytes
        }
    })
}

/// Whether a spreadsheet opening the file would take `cell` for a formula:
/// it starts with =, +, -, @, a tab or a carriage return, and isn't a
/// number ("-5" stays as it is).
fn looks_like_formula(cell: &str) -> bool {
    cell.starts_with(['=', '+', '-', '@', '\t', '\r'])
        && cell.trim().replace(',', ".").parse::<f64>().is_err()
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
                .map(|r| r.iter().map(|s| s.to_string()).collect())
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
    fn sorts_dates_and_times() {
        let column = |t: &Table| -> Vec<String> { t.rows().iter().map(|r| r[0].clone()).collect() };
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
}
