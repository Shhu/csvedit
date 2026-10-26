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
    /// value, text ignoring case), for `Edit::Reorder`.
    pub fn sort_order(&self, start: usize, col: usize, descending: bool) -> Vec<usize> {
        let rows = &self.rows[start..];
        let mut order: Vec<usize> = (0..rows.len()).collect();
        order.sort_by(|&a, &b| {
            let ordering = compare_cells(&rows[a][col], &rows[b][col]);
            if descending {
                ordering.reverse()
            } else {
                ordering
            }
        });
        order
    }
}

/// Numbers before text, numbers by value (with a decimal point or comma),
/// text ignoring case; empty cells last.
fn compare_cells(a: &str, b: &str) -> Ordering {
    let number = |s: &str| {
        s.trim()
            .replace(',', ".")
            .parse::<f64>()
            .ok()
            .filter(|n| n.is_finite())
    };
    match (a.trim().is_empty(), b.trim().is_empty()) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Greater,
        (false, true) => return Ordering::Less,
        _ => {}
    }
    match (number(a), number(b)) {
        (Some(x), Some(y)) => x.total_cmp(&y),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => a
            .to_lowercase()
            .cmp(&b.to_lowercase())
            .then_with(|| a.cmp(b)),
    }
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Utf8Bom,
    /// What Excel writes in Western European locales ("CSV (separator:
    /// semicolon)" files from a French Excel).
    Windows1252,
}

impl Encoding {
    pub const ALL: [Encoding; 3] = [Encoding::Utf8, Encoding::Utf8Bom, Encoding::Windows1252];
}

/// How a file is written back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Format {
    /// Between fields; can be several characters (Poliris files use `!#`).
    pub delimiter: String,
    pub quote: char,
    /// Every field between quotes (Poliris files), not only those that need
    /// them.
    pub quote_all: bool,
    /// Every line ends with a delimiter too (Poliris files).
    pub trailing_delimiter: bool,
    pub encoding: Encoding,
    pub crlf: bool,
}

impl Default for Format {
    fn default() -> Self {
        Self {
            delimiter: ",".to_owned(),
            quote: '"',
            quote_all: false,
            trailing_delimiter: false,
            encoding: Encoding::Utf8,
            crlf: cfg!(windows),
        }
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
    /// Consecutive delimiters count as one.
    pub merge_delimiters: bool,
    /// Spaces around unquoted fields are dropped.
    pub trim: bool,
}

impl ImportOptions {
    /// The encoding and delimiter the file seems to use.
    pub fn detect(bytes: &[u8]) -> Self {
        let (text, encoding) = decode(bytes);
        Self {
            encoding,
            skip_lines: 0,
            delimiters: vec![sniff_delimiter(&text).to_owned()],
            quote: Some('"'),
            merge_delimiters: false,
            trim: false,
        }
    }
}

/// Reads a CSV file's bytes, guessing its encoding, delimiter and line
/// endings (what the import window starts from).
#[cfg(test)]
pub fn parse(bytes: &[u8]) -> (Table, Format) {
    import(bytes, &ImportOptions::detect(bytes))
}

/// Reads a CSV file's bytes the way `options` say. The file is saved back
/// with the first delimiter, its quotes and its line endings.
pub fn import(bytes: &[u8], options: &ImportOptions) -> (Table, Format) {
    let text = decode_as(bytes, options.encoding);
    let parsed = split_fields(&text, options, usize::MAX);
    let format = Format {
        delimiter: options
            .delimiters
            .first()
            .cloned()
            .unwrap_or_else(|| ",".to_owned()),
        quote: options.quote.unwrap_or('"'),
        quote_all: parsed.all_quoted,
        trailing_delimiter: parsed.trailing_delimiter,
        encoding: options.encoding,
        crlf: text.contains("\r\n"),
    };
    (Table::from_rows(parsed.rows), format)
}

/// The text of a file in `encoding` (a UTF-8 BOM is always left out).
pub fn decode_as(bytes: &[u8], encoding: Encoding) -> String {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    match encoding {
        Encoding::Utf8 | Encoding::Utf8Bom => String::from_utf8_lossy(bytes).into_owned(),
        Encoding::Windows1252 => encoding_rs::WINDOWS_1252
            .decode_without_bom_handling(bytes)
            .0
            .into_owned(),
    }
}

/// The first `max_rows` rows of `text`. Blank lines are left out.
pub fn split(text: &str, options: &ImportOptions, max_rows: usize) -> Vec<Vec<String>> {
    split_fields(text, options, max_rows).rows
}

struct Parsed {
    rows: Vec<Vec<String>>,
    /// Every field was between quotes.
    all_quoted: bool,
    /// Every line ended with a delimiter (left out of `rows`).
    trailing_delimiter: bool,
}

fn split_fields(text: &str, options: &ImportOptions, max_rows: usize) -> Parsed {
    let mut rest = text;
    for _ in 0..options.skip_lines {
        rest = rest.find('\n').map_or("", |i| &rest[i + 1..]);
    }
    // Longest first, so that `!#` wins over `!`.
    let mut delimiters: Vec<&str> = options
        .delimiters
        .iter()
        .map(String::as_str)
        .filter(|d| !d.is_empty())
        .collect();
    delimiters.sort_by_key(|d| std::cmp::Reverse(d.len()));
    let delimiter_at = |i: usize| {
        delimiters
            .iter()
            .find(|d| rest[i..].starts_with(**d))
            .map(|d| d.len())
    };

    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut in_quotes = false;
    // A delimiter was the last thing read on this line.
    let mut after_delimiter = false;
    let mut unquoted_fields = 0;
    let mut trailing_rows = 0;
    let mut end_field = |row: &mut Vec<String>, field: &mut String, quoted: &mut bool| {
        let text = std::mem::take(field);
        if !*quoted {
            unquoted_fields += 1;
        }
        row.push(if options.trim && !*quoted {
            text.trim().to_owned()
        } else {
            text
        });
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
                field.push(c);
            }
            i = next;
        } else if Some(c) == options.quote && !quoted && field.trim().is_empty() {
            field.clear();
            in_quotes = true;
            quoted = true;
            after_delimiter = false;
            i = next;
        } else if let Some(len) = delimiter_at(i) {
            end_field(&mut row, &mut field, &mut quoted);
            i += len;
            if options.merge_delimiters {
                while let Some(len) = delimiter_at(i) {
                    i += len;
                }
            }
            after_delimiter = true;
        } else if c == '\r' || c == '\n' {
            i = next;
            if c == '\r' && rest[i..].starts_with('\n') {
                i += 1;
            }
            if row.is_empty() && field.is_empty() && !quoted {
                continue;
            }
            if after_delimiter {
                trailing_rows += 1;
            }
            after_delimiter = false;
            end_field(&mut row, &mut field, &mut quoted);
            rows.push(std::mem::take(&mut row));
            if rows.len() == max_rows {
                break;
            }
        } else {
            field.push(c);
            after_delimiter = false;
            i = next;
        }
    }
    if rows.len() < max_rows && (!row.is_empty() || !field.is_empty() || quoted) {
        if after_delimiter {
            trailing_rows += 1;
        }
        end_field(&mut row, &mut field, &mut quoted);
        rows.push(row);
    }

    let trailing_delimiter = !rows.is_empty() && trailing_rows == rows.len();
    if trailing_delimiter {
        for row in &mut rows {
            row.pop();
        }
        unquoted_fields -= rows.len();
    }
    let all_quoted = rows.iter().any(|r| !r.is_empty()) && unquoted_fields == 0;
    Parsed {
        rows,
        all_quoted,
        trailing_delimiter,
    }
}

fn decode(bytes: &[u8]) -> (String, Encoding) {
    if bytes.starts_with(b"\xEF\xBB\xBF") {
        return (decode_as(bytes, Encoding::Utf8Bom), Encoding::Utf8Bom);
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => (text.to_owned(), Encoding::Utf8),
        Err(_) => (
            decode_as(bytes, Encoding::Windows1252),
            Encoding::Windows1252,
        ),
    }
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
        let counts: Vec<usize> = split(&sample, &options, usize::MAX)
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

/// A character the file's encoding can't represent (the first one).
#[derive(Debug)]
pub struct Unencodable(pub char);

pub fn write(table: &Table, format: &Format) -> Result<Vec<u8>, Unencodable> {
    let delimiter = if format.delimiter.is_empty() {
        ","
    } else {
        format.delimiter.as_str()
    };
    let quote = format.quote;
    let doubled = format!("{quote}{quote}");
    let mut text = String::new();
    for row in table.rows() {
        for (i, cell) in row.iter().enumerate() {
            if i > 0 {
                text.push_str(delimiter);
            }
            let needs_quotes = format.quote_all
                // A line with one empty field isn't a blank line.
                || (row.len() == 1 && cell.is_empty())
                || cell.contains(delimiter)
                || cell.contains(quote)
                || cell.contains(['\r', '\n']);
            if needs_quotes {
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
        text.push_str(if format.crlf { "\r\n" } else { "\n" });
    }
    Ok(match format.encoding {
        Encoding::Utf8 => text.into_bytes(),
        Encoding::Utf8Bom => [b"\xEF\xBB\xBF".as_slice(), text.as_bytes()].concat(),
        Encoding::Windows1252 => {
            let (bytes, _, unmappable) = encoding_rs::WINDOWS_1252.encode(&text);
            if unmappable {
                let bad = text
                    .chars()
                    .find(|c| encoding_rs::WINDOWS_1252.encode(&c.to_string()).2)
                    .expect("an unmappable character");
                return Err(Unencodable(bad));
            }
            bytes.into_owned()
        }
    })
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
        assert_eq!(
            format,
            Format {
                delimiter: ";".into(),
                encoding: Encoding::Windows1252,
                crlf: true,
                ..Format::default()
            }
        );
        assert_eq!(write(&t, &format).unwrap(), bytes);
    }

    #[test]
    fn round_trips_utf8_with_bom_and_quotes() {
        let bytes = "\u{feff}a,b\n\"x, \"\"y\"\"\",\"line\nbreak\"\n".as_bytes();
        let (t, format) = parse(bytes);
        assert_eq!(format.encoding, Encoding::Utf8Bom);
        assert_eq!(format.delimiter, ",");
        assert!(!format.crlf);
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
    fn refuses_characters_windows_1252_lacks() {
        let format = Format {
            encoding: Encoding::Windows1252,
            ..Format::default()
        };
        assert!(matches!(
            write(&table(&[&["€ ok", "日本"]]), &format),
            Err(Unencodable('日'))
        ));
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
    fn imports_with_the_chosen_options() {
        let text = "# exported\nRC-1  142   Viager;'a ''b'' c'\n\nRC-2 156 x;\n";
        let options = ImportOptions {
            encoding: Encoding::Utf8,
            skip_lines: 1,
            delimiters: vec![" ".into(), ";".into()],
            quote: Some('\''),
            merge_delimiters: true,
            trim: true,
        };
        assert_eq!(
            split(text, &options, usize::MAX),
            [
                vec!["RC-1", "142", "Viager", "a 'b' c"],
                vec!["RC-2", "156", "x", ""],
            ]
        );
        assert_eq!(split(text, &options, 1).len(), 1);
        let (t, format) = import(text.as_bytes(), &options);
        assert_eq!((t.row_count(), t.col_count()), (2, 4));
        assert_eq!(format.delimiter, " ");
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
        let options = ImportOptions {
            delimiters: vec!["!#".into()],
            ..ImportOptions::detect(b"")
        };
        assert_eq!(
            split("a!b#c!#d\n", &options, usize::MAX),
            [vec!["a!b#c", "d"]]
        );
    }
}
