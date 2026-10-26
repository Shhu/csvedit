//! The window: title bar with the menus, toolbar, cell bar, the grid of
//! cells, the inspector and the status bar.

mod bars;
mod grid;
mod inspector;
mod titlebar;

use std::path::{Path, PathBuf};

use eframe::egui::{self, Key, KeyboardShortcut, Modifiers};

use crate::columns;
use crate::i18n::t;
use crate::import::{ImportDialog, Outcome};
use crate::table::{self, Edit, Encoding, Format, Table, Unencodable};
use crate::theme;

pub use titlebar::resize_grips;

pub const APP_NAME: &str = "CSV Edit";

const SHORTCUT_NEW: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::N);
const SHORTCUT_OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
const SHORTCUT_SAVE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
const SHORTCUT_SAVE_AS: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::S);
const SHORTCUT_QUIT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Q);
const SHORTCUT_UNDO: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Z);
const SHORTCUT_REDO: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::Z);
const SHORTCUT_REDO_ALT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Y);
const SHORTCUT_FIND: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::F);
// The spreadsheet shortcuts for inserting and deleting rows.
const SHORTCUT_INSERT_ROW: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Plus);
const SHORTCUT_DELETE_ROW: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Minus);
const SHORTCUT_INSPECTOR: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::I);
const SHORTCUT_COLUMNS: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::C);

/// What waits for an answer to "Save changes?".
#[derive(Clone, Debug)]
enum Pending {
    New,
    /// `None`: ask for the file.
    Open(Option<PathBuf>),
    Quit,
}

#[derive(Clone, Copy)]
enum Command {
    New,
    Open,
    Save,
    SaveAs,
    Quit,
    Undo,
    Redo,
    Find,
    InsertRowAbove,
    InsertRowBelow,
    DeleteRow,
    InsertColLeft,
    InsertColRight,
    DeleteCol,
    SortAscending,
    SortDescending,
    ToggleInspector,
    ChooseColumns,
}

/// Height of the grid's rows.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Density {
    Compact,
    Normal,
    Comfortable,
}

impl Density {
    fn row_height(self) -> f32 {
        match self {
            Density::Compact => 30.0,
            Density::Normal => 36.0,
            Density::Comfortable => 44.0,
        }
    }
}

/// Facts about the selected column, for the inspector.
#[derive(Clone, Copy)]
struct ColumnStats {
    filled: usize,
    empty: usize,
    /// In characters.
    longest: usize,
}

pub struct CsvApp {
    table: Table,
    format: Format,
    path: Option<PathBuf>,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    /// Length of `undo` when last saved; `None` once that state can't be
    /// reached again.
    saved_depth: Option<usize>,
    /// The delimiter, encoding or line endings changed since the last save.
    format_changed: bool,
    /// Counts the table's changes, for the caches.
    generation: u64,
    /// The first row holds the column names: it stays at the top and is
    /// left out of sorting.
    header: bool,
    /// (row, column) of the selected cell.
    selected: (usize, usize),
    /// Text being typed in the selected cell.
    editing: Option<String>,
    /// It is typed in the inspector rather than in the grid.
    edit_in_inspector: bool,
    focus_editor: bool,
    scroll_to_selection: bool,
    find: String,
    focus_find: bool,
    /// Cells containing the text being found, in reading order.
    matches: Vec<(usize, usize)>,
    /// (`generation`, needle) `matches` was computed for.
    matches_key: Option<(u64, String)>,
    column_stats: Option<(u64, usize, ColumnStats)>,
    inspector: bool,
    /// The columns shown, in this order; `None`: all of them.
    shown: Option<Vec<usize>>,
    /// Typed in the column chooser ("3, 157").
    columns_text: String,
    /// The chooser's text names no column there.
    columns_error: Option<String>,
    /// Filters the chooser's list by name.
    columns_filter: String,
    /// The column chooser opens at the next frame.
    open_columns: bool,
    density: Density,
    pending: Option<Pending>,
    error: Option<String>,
    about: bool,
    /// Typed in the status bar, for a delimiter of one's own.
    custom_delimiter: String,
    /// The file being opened, waiting for its import options.
    import: Option<ImportDialog>,
    allow_close: bool,
    title: String,
}

impl CsvApp {
    pub fn new(path: Option<PathBuf>) -> Self {
        let mut app = Self {
            table: Table::default(),
            format: Format::default(),
            path: None,
            undo: Vec::new(),
            redo: Vec::new(),
            saved_depth: Some(0),
            format_changed: false,
            generation: 0,
            header: true,
            selected: (0, 0),
            editing: None,
            edit_in_inspector: false,
            focus_editor: false,
            scroll_to_selection: false,
            find: String::new(),
            focus_find: false,
            matches: Vec::new(),
            matches_key: None,
            column_stats: None,
            inspector: true,
            shown: None,
            columns_text: String::new(),
            columns_error: None,
            columns_filter: String::new(),
            open_columns: false,
            density: Density::Normal,
            pending: None,
            error: None,
            about: false,
            custom_delimiter: String::new(),
            import: None,
            allow_close: false,
            title: String::new(),
        };
        if let Some(path) = path {
            app.load(&path);
        }
        app
    }

    fn dirty(&self) -> bool {
        self.format_changed || self.saved_depth != Some(self.undo.len())
    }

    fn reset(&mut self, table: Table, format: Format, path: Option<PathBuf>) {
        let fresh = Self {
            table,
            format,
            path,
            header: self.header,
            find: std::mem::take(&mut self.find),
            inspector: self.inspector,
            density: self.density,
            generation: self.generation + 1,
            ..Self::new(None)
        };
        *self = fresh;
    }

    // Files

    /// Runs `action` now, or after asking to save the changes.
    fn request(&mut self, action: Pending) {
        self.commit_editing();
        if self.dirty() {
            self.pending = Some(action);
        } else {
            self.perform(action);
        }
    }

    fn perform(&mut self, action: Pending) {
        match action {
            Pending::New => self.reset(Table::default(), Format::default(), None),
            Pending::Open(Some(path)) => self.load(&path),
            Pending::Open(None) => {
                let dialog = rfd::FileDialog::new()
                    .add_filter(t!("filter-csv"), &["csv", "tsv", "txt"])
                    .add_filter(t!("filter-all"), &["*"]);
                let dialog = match self.path.as_deref().and_then(Path::parent) {
                    Some(dir) => dialog.set_directory(dir),
                    None => dialog,
                };
                if let Some(path) = dialog.pick_file() {
                    self.load(&path);
                }
            }
            Pending::Quit => self.allow_close = true,
        }
    }

    fn load(&mut self, path: &Path) {
        match std::fs::read(path) {
            Ok(bytes) => {
                self.import = Some(ImportDialog::new(path.to_owned(), bytes, self.header));
            }
            Err(error) => {
                self.error = Some(t!(
                    "error-open",
                    file = file_name(path),
                    error = error.to_string()
                ));
            }
        }
    }

    /// Whether the file was saved.
    fn save(&mut self) -> bool {
        self.commit_editing();
        match self.path.clone() {
            Some(path) => self.save_to(&path),
            None => self.save_as(),
        }
    }

    fn save_as(&mut self) -> bool {
        self.commit_editing();
        let name = self
            .path
            .as_deref()
            .map_or_else(|| t!("untitled-file"), file_name);
        let dialog = rfd::FileDialog::new()
            .add_filter(t!("filter-csv"), &["csv", "tsv", "txt"])
            .set_file_name(name);
        let dialog = match self.path.as_deref().and_then(Path::parent) {
            Some(dir) => dialog.set_directory(dir),
            None => dialog,
        };
        match dialog.save_file() {
            Some(path) => self.save_to(&path),
            None => false,
        }
    }

    fn save_to(&mut self, path: &Path) -> bool {
        let result = match table::write(&self.table, &self.format) {
            Ok(bytes) => std::fs::write(path, bytes).map_err(|e| e.to_string()),
            Err(Unencodable(c)) => Err(t!("error-encoding", character = c.to_string())),
        };
        match result {
            Ok(()) => {
                self.path = Some(path.to_owned());
                self.saved_depth = Some(self.undo.len());
                self.format_changed = false;
                true
            }
            Err(error) => {
                self.error = Some(t!("error-save", file = file_name(path), error = error));
                false
            }
        }
    }

    // Edits

    fn commit(&mut self, edit: Edit) {
        let inverse = self.table.apply(edit);
        if self
            .saved_depth
            .is_some_and(|depth| depth > self.undo.len())
        {
            self.saved_depth = None;
        }
        self.undo.push(inverse);
        self.redo.clear();
        self.after_change();
    }

    fn undo(&mut self) {
        self.commit_editing();
        if let Some(edit) = self.undo.pop() {
            self.redo.push(self.table.apply(edit));
            self.after_change();
        }
    }

    fn redo(&mut self) {
        self.commit_editing();
        if let Some(edit) = self.redo.pop() {
            self.undo.push(self.table.apply(edit));
            self.after_change();
        }
    }

    fn after_change(&mut self) {
        self.generation += 1;
        if let Some(shown) = &mut self.shown {
            let count = self.table.col_count();
            shown.retain(|&c| c < count);
            if shown.is_empty() {
                self.shown = None;
            }
        }
        let (row, col) = self.selected;
        self.selected = (
            row.min(self.table.row_count() - 1),
            col.min(self.table.col_count() - 1),
        );
    }

    // Columns shown

    /// The columns shown, in order.
    fn shown_cols(&self) -> Vec<usize> {
        match &self.shown {
            Some(shown) => shown.clone(),
            None => (0..self.table.col_count()).collect(),
        }
    }

    /// Shows `cols`, in this order (all columns when empty), keeping a
    /// shown cell selected.
    fn show_columns(&mut self, cols: Vec<usize>) {
        let all =
            cols.len() == self.table.col_count() && cols.iter().enumerate().all(|(i, &c)| i == c);
        self.shown = (!cols.is_empty() && !all).then_some(cols);
        let shown = self.shown_cols();
        if !shown.contains(&self.selected.1) {
            self.commit_editing();
            self.select(self.selected.0, shown[0]);
        }
    }

    /// A column inserted at `at` is shown, next to the selected one.
    fn shown_after_insert(&mut self, at: usize) {
        let selected = self.selected.1;
        if let Some(shown) = &mut self.shown {
            let position = shown.iter().position(|&c| c == selected);
            for c in shown.iter_mut() {
                if *c >= at {
                    *c += 1;
                }
            }
            let position = match position {
                Some(p) if at <= selected => p,
                Some(p) => p + 1,
                None => shown.len(),
            };
            shown.insert(position, at);
        }
    }

    fn shown_after_delete(&mut self, at: usize) {
        if let Some(shown) = &mut self.shown {
            shown.retain(|&c| c != at);
            for c in shown.iter_mut() {
                if *c > at {
                    *c -= 1;
                }
            }
            if shown.is_empty() {
                self.shown = None;
            }
        }
    }

    /// Starts typing in the selected cell, in the grid or the inspector.
    fn start_editing(&mut self, text: String, in_inspector: bool) {
        self.editing = Some(text);
        self.edit_in_inspector = in_inspector;
        self.focus_editor = true;
    }

    fn commit_editing(&mut self) {
        if let Some(text) = self.editing.take() {
            let (row, col) = self.selected;
            if text != self.table.cell(row, col) {
                self.commit(Edit::SetCells(vec![(row, col, text)]));
            }
        }
    }

    fn select(&mut self, row: usize, col: usize) {
        self.selected = (row, col);
        self.scroll_to_selection = true;
    }

    /// First row of data: sorting leaves out the header row.
    fn body_start(&self) -> usize {
        usize::from(self.header && self.table.row_count() > 1)
    }

    /// The name of column `col`, from the header row.
    fn column_title(&self, col: usize) -> &str {
        if self.body_start() == 1 {
            self.table.cell(0, col)
        } else {
            ""
        }
    }

    fn append_row(&mut self) {
        let blank = vec![vec![String::new(); self.table.col_count()]];
        let row = self.table.row_count();
        self.commit(Edit::InsertRows(row, blank));
        self.select(row, self.selected.1);
    }

    fn run(&mut self, command: Command) {
        let (row, col) = self.selected;
        let blank_row = vec![vec![String::new(); self.table.col_count()]];
        let blank_col = vec![vec![String::new(); self.table.row_count()]];
        match command {
            Command::New => self.request(Pending::New),
            Command::Open => self.request(Pending::Open(None)),
            Command::Save => {
                self.save();
            }
            Command::SaveAs => {
                self.save_as();
            }
            Command::Quit => self.request(Pending::Quit),
            Command::Undo => self.undo(),
            Command::Redo => self.redo(),
            Command::Find => self.focus_find = true,
            Command::ToggleInspector => self.inspector = !self.inspector,
            Command::ChooseColumns => self.open_columns = true,
            Command::InsertRowAbove => {
                self.commit_editing();
                self.commit(Edit::InsertRows(row, blank_row));
                self.select(row, col);
            }
            Command::InsertRowBelow => {
                self.commit_editing();
                self.commit(Edit::InsertRows(row + 1, blank_row));
                self.select(row + 1, col);
            }
            Command::DeleteRow => {
                self.editing = None;
                self.commit(Edit::RemoveRows(row, 1));
            }
            Command::InsertColLeft => {
                self.commit_editing();
                self.shown_after_insert(col);
                self.commit(Edit::InsertCols(col, blank_col));
                self.select(row, col);
            }
            Command::InsertColRight => {
                self.commit_editing();
                self.shown_after_insert(col + 1);
                self.commit(Edit::InsertCols(col + 1, blank_col));
                self.select(row, col + 1);
            }
            Command::DeleteCol => {
                self.editing = None;
                let shown = self.shown_cols();
                let position = shown.iter().position(|&c| c == col).unwrap_or(0);
                self.shown_after_delete(col);
                self.commit(Edit::RemoveCols(col, 1));
                // The next column shown, or the previous one at the end.
                let shown = self.shown_cols();
                let next = shown[position.min(shown.len() - 1)];
                self.select(row, next);
            }
            Command::SortAscending | Command::SortDescending => {
                self.commit_editing();
                let start = self.body_start();
                let descending = matches!(command, Command::SortDescending);
                let order = self.table.sort_order(start, col, descending);
                if order.iter().enumerate().any(|(i, &from)| i != from) {
                    self.commit(Edit::Reorder(start, order));
                }
            }
        }
    }

    fn can_run(&self, command: Command) -> bool {
        match command {
            Command::Undo => !self.undo.is_empty(),
            Command::Redo => !self.redo.is_empty(),
            _ => true,
        }
    }

    /// Pastes tab-separated text (what spreadsheets copy) from the selected
    /// cell on, growing the table if needed.
    fn paste(&mut self, text: &str) {
        let lines: Vec<Vec<&str>> = text
            .trim_end_matches(['\r', '\n'])
            .split('\n')
            .map(|line| line.trim_end_matches('\r').split('\t').collect())
            .collect();
        let (row, col) = self.selected;
        let height = lines.len();
        let width = lines.iter().map(Vec::len).max().unwrap_or(0);
        let mut edits = Vec::new();
        let extra_cols = (col + width).saturating_sub(self.table.col_count());
        if extra_cols > 0 {
            let cols = vec![vec![String::new(); self.table.row_count()]; extra_cols];
            edits.push(Edit::InsertCols(self.table.col_count(), cols));
        }
        let extra_rows = (row + height).saturating_sub(self.table.row_count());
        if extra_rows > 0 {
            let rows = vec![vec![String::new(); self.table.col_count() + extra_cols]; extra_rows];
            edits.push(Edit::InsertRows(self.table.row_count(), rows));
        }
        let cells = lines
            .iter()
            .enumerate()
            .flat_map(|(r, line)| {
                line.iter()
                    .enumerate()
                    .map(move |(c, text)| (row + r, col + c, (*text).to_owned()))
            })
            .collect();
        edits.push(Edit::SetCells(cells));
        self.commit(Edit::Batch(edits));
    }

    // Finding

    /// The text being found, lowercased; empty when nothing is.
    fn needle(&self) -> String {
        self.find.trim().to_lowercase()
    }

    /// Cells containing the text being found.
    fn matches(&mut self) -> &[(usize, usize)] {
        let needle = self.needle();
        let key = (self.generation, needle.clone());
        if self.matches_key.as_ref() != Some(&key) {
            self.matches = if needle.is_empty() {
                Vec::new()
            } else {
                self.table
                    .rows()
                    .iter()
                    .enumerate()
                    .flat_map(|(r, row)| {
                        let needle = &needle;
                        row.iter()
                            .enumerate()
                            .filter(move |(_, cell)| cell.to_lowercase().contains(needle))
                            .map(move |(c, _)| (r, c))
                    })
                    .collect()
            };
            self.matches_key = Some(key);
        }
        &self.matches
    }

    /// Selects the next match after the selected cell (the previous one
    /// `backwards`), wrapping around.
    fn next_match(&mut self, backwards: bool) {
        let selected = self.selected;
        let shown = self.shown.clone();
        let matches: Vec<_> = self
            .matches()
            .iter()
            .copied()
            .filter(|(_, c)| shown.as_ref().is_none_or(|s| s.contains(c)))
            .collect();
        let found = if backwards {
            matches
                .iter()
                .rev()
                .find(|&&m| m < selected)
                .or(matches.last())
        } else {
            matches.iter().find(|&&m| m > selected).or(matches.first())
        };
        if let Some(&(row, col)) = found {
            self.commit_editing();
            self.select(row, col);
        }
    }

    fn column_stats(&mut self, col: usize) -> ColumnStats {
        if let Some((generation, c, stats)) = self.column_stats
            && generation == self.generation
            && c == col
        {
            return stats;
        }
        let mut stats = ColumnStats {
            filled: 0,
            empty: 0,
            longest: 0,
        };
        for row in &self.table.rows()[self.body_start()..] {
            let cell = &row[col];
            if cell.trim().is_empty() {
                stats.empty += 1;
            } else {
                stats.filled += 1;
            }
            stats.longest = stats.longest.max(cell.chars().count());
        }
        self.column_stats = Some((self.generation, col, stats));
        stats
    }

    // Keyboard

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let shortcuts = [
            (SHORTCUT_SAVE_AS, Command::SaveAs),
            (SHORTCUT_NEW, Command::New),
            (SHORTCUT_OPEN, Command::Open),
            (SHORTCUT_SAVE, Command::Save),
            (SHORTCUT_QUIT, Command::Quit),
            (SHORTCUT_REDO, Command::Redo),
            (SHORTCUT_REDO_ALT, Command::Redo),
            (SHORTCUT_UNDO, Command::Undo),
            (SHORTCUT_FIND, Command::Find),
            (SHORTCUT_INSERT_ROW, Command::InsertRowBelow),
            (SHORTCUT_DELETE_ROW, Command::DeleteRow),
            (SHORTCUT_INSPECTOR, Command::ToggleInspector),
            (SHORTCUT_COLUMNS, Command::ChooseColumns),
        ];
        // While typing, the text field keeps its own undo.
        let typing = ctx.egui_wants_keyboard_input();
        for (shortcut, command) in shortcuts {
            let text_command = matches!(command, Command::Undo | Command::Redo);
            if !(typing && text_command) && ctx.input_mut(|i| i.consume_shortcut(&shortcut)) {
                self.run(command);
            }
        }
    }

    /// Moving around the grid, and typing into the selected cell, when no
    /// text field has the keyboard.
    fn handle_grid_keys(&mut self, ctx: &egui::Context) {
        if self.editing.is_some() || ctx.egui_wants_keyboard_input() || ctx.any_popup_open() {
            return;
        }
        let (row, col) = self.selected;
        let last_row = self.table.row_count() - 1;
        let shown = self.shown_cols();
        let position = shown.iter().position(|&c| c == col).unwrap_or(0);
        let left = shown[position.saturating_sub(1)];
        let right = shown[(position + 1).min(shown.len() - 1)];
        let (first_col, last_col) = (shown[0], shown[shown.len() - 1]);
        let events = ctx.input(|i| i.events.clone());
        for event in events {
            match event {
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } if modifiers.is_none() || modifiers.shift_only() => match key {
                    Key::ArrowUp => self.select(row.saturating_sub(1), col),
                    Key::ArrowDown => self.select((row + 1).min(last_row), col),
                    Key::PageUp => self.select(row.saturating_sub(20), col),
                    Key::PageDown => self.select((row + 20).min(last_row), col),
                    Key::ArrowLeft => self.select(row, left),
                    Key::ArrowRight | Key::Tab => self.select(row, right),
                    Key::Home => self.select(row, first_col),
                    Key::End => self.select(row, last_col),
                    Key::Enter | Key::F2 => {
                        self.start_editing(self.table.cell(row, col).to_owned(), false)
                    }
                    Key::Delete | Key::Backspace if !self.table.cell(row, col).is_empty() => {
                        self.commit(Edit::SetCells(vec![(row, col, String::new())]));
                    }
                    _ => {}
                },
                egui::Event::Text(text) => self.start_editing(text, false),
                egui::Event::Copy => ctx.copy_text(self.table.cell(row, col).to_owned()),
                egui::Event::Cut => {
                    ctx.copy_text(self.table.cell(row, col).to_owned());
                    self.commit(Edit::SetCells(vec![(row, col, String::new())]));
                }
                egui::Event::Paste(text) => self.paste(&text),
                _ => {}
            }
        }
    }

    /// Enter on the last row adds a row, the way spreadsheets grow.
    fn move_down_after_enter(&mut self, row: usize, col: usize) {
        if row + 1 < self.table.row_count() {
            self.select(row + 1, col);
        } else {
            self.append_row();
        }
    }

    // Dialogs

    fn dialogs(&mut self, ctx: &egui::Context) {
        if let Some(dialog) = &mut self.import {
            match dialog.show(ctx) {
                Some(Outcome::Open) => {
                    let dialog = self.import.take().expect("an import dialog");
                    let header = dialog.header;
                    let path = dialog.path.clone();
                    let (table, format) = dialog.into_table();
                    self.reset(table, format, Some(path));
                    self.header = header;
                }
                Some(Outcome::Cancel) => self.import = None,
                None => {}
            }
        }
        if let Some(action) = self.pending.clone() {
            let mut answer = None;
            theme::modal("save-changes", ctx).show(ctx, |ui| {
                ui.set_width(400.0);
                ui.heading(t!("save-changes-title"));
                ui.add_space(4.0);
                ui.label(t!("save-changes-text", file = self.display_name()));
                ui.add_space(18.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if theme::primary_button(ui, &t!("save-changes-save"), 0.0).clicked()
                        || ui.input(|i| i.key_pressed(Key::Enter))
                    {
                        answer = Some(true);
                    }
                    if theme::secondary_button(ui, &t!("save-changes-discard"), 0.0).clicked() {
                        answer = Some(false);
                    }
                    if theme::secondary_button(ui, &t!("cancel"), 0.0).clicked()
                        || ui.input(|i| i.key_pressed(Key::Escape))
                    {
                        self.pending = None;
                    }
                });
            });
            if let Some(save) = answer {
                self.pending = None;
                if !save || self.save() {
                    self.perform(action);
                }
            }
        }
        if let Some(error) = self.error.clone() {
            theme::modal("error", ctx).show(ctx, |ui| {
                ui.set_max_width(440.0);
                ui.label(error);
                ui.add_space(18.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if theme::primary_button(ui, &t!("ok"), 80.0).clicked()
                        || ui.input(|i| i.key_pressed(Key::Enter) || i.key_pressed(Key::Escape))
                    {
                        self.error = None;
                    }
                });
            });
        }
        if self.about {
            theme::modal("about", ctx).show(ctx, |ui| {
                ui.set_width(360.0);
                ui.heading(format!("{APP_NAME} {}", env!("CARGO_PKG_VERSION")));
                ui.add_space(4.0);
                ui.label(t!("about-text"));
                ui.label(t!("about-license"));
                ui.hyperlink(env!("CARGO_PKG_REPOSITORY"));
                ui.add_space(18.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if theme::primary_button(ui, &t!("ok"), 80.0).clicked()
                        || ui.input(|i| i.key_pressed(Key::Escape))
                    {
                        self.about = false;
                    }
                });
            });
        }
    }

    fn display_name(&self) -> String {
        self.path
            .as_deref()
            .map_or_else(|| t!("untitled"), file_name)
    }

    /// The window's title, for the taskbar (the title bar draws its own).
    fn update_title(&mut self, ctx: &egui::Context) {
        let title = format!(
            "{}{} \u{2014} {APP_NAME}",
            if self.dirty() { "\u{2022} " } else { "" },
            self.display_name()
        );
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }
}

impl eframe::App for CsvApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close {
            self.commit_editing();
            if self.dirty() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.pending = Some(Pending::Quit);
            }
        }
        if let Some(path) = ctx.input(|i| i.raw.dropped_files.first().map(|f| f.path().to_owned()))
        {
            self.request(Pending::Open(Some(path)));
        }
        let modal_open =
            self.pending.is_some() || self.error.is_some() || self.about || self.import.is_some();
        if !modal_open {
            self.handle_shortcuts(&ctx);
            self.handle_grid_keys(&ctx);
        }

        let p = theme::palette(ui);
        let bar = |fill| {
            egui::Frame::new()
                .fill(fill)
                .inner_margin(egui::Margin::symmetric(12, 0))
        };
        egui::Panel::top("title")
            .exact_size(38.0)
            .show_separator_line(false)
            .frame(
                egui::Frame::new()
                    .fill(p.chrome)
                    .inner_margin(egui::Margin {
                        left: 12,
                        ..Default::default()
                    }),
            )
            .show(ui, |ui| self.title_bar(ui));
        egui::Panel::top("toolbar")
            .exact_size(52.0)
            .frame(bar(p.chrome))
            .show(ui, |ui| self.toolbar(ui));
        egui::Panel::top("cell-bar")
            .exact_size(44.0)
            .frame(bar(p.bg))
            .show(ui, |ui| self.cell_bar(ui));
        egui::Panel::bottom("status")
            .exact_size(30.0)
            .frame(
                egui::Frame::new()
                    .fill(p.chrome)
                    .inner_margin(egui::Margin {
                        left: 12,
                        right: 8,
                        ..Default::default()
                    }),
            )
            .show(ui, |ui| self.status_bar(ui));
        if self.inspector {
            egui::Panel::right("inspector")
                .exact_size(340.0)
                .frame(egui::Frame::new().fill(p.chrome))
                .show(ui, |ui| self.inspector(ui));
        }
        egui::CentralPanel::no_frame().show(ui, |ui| self.grid(ui));
        self.dialogs(&ctx);
        resize_grips(&ctx);

        if self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        self.update_title(&ctx);
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// A cell's text on one line: line breaks, real ones and HTML `<BR>` tags
/// (Poliris files), shown as "↵".
pub fn flat_text(text: &str) -> String {
    let mut flat = String::with_capacity(text.len());
    for (i, part) in split_breaks(text).into_iter().enumerate() {
        if i > 0 {
            flat.push_str(" \u{21b5} ");
        }
        flat.push_str(part.0);
    }
    flat
}

/// A cell's text cut at its line breaks: each piece, and whether the break
/// after it is a `<BR>` tag (rather than a real line break).
pub fn split_breaks(text: &str) -> Vec<(&str, bool)> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut i = 0;
    let bytes = text.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'\n' || bytes[i] == b'\r' {
            let end = i;
            i += if text[i..].starts_with("\r\n") { 2 } else { 1 };
            parts.push((&text[start..end], false));
            start = i;
        } else if bytes[i] == b'<'
            && let Some(len) = br_tag_len(&text[i..])
        {
            parts.push((&text[start..i], true));
            i += len;
            start = i;
        } else {
            i += 1;
        }
    }
    parts.push((&text[start..], false));
    // The last piece has no break after it.
    parts
}

/// Length of the `<br>`, `<BR/>` or `<br />` tag at the start of `text`.
fn br_tag_len(text: &str) -> Option<usize> {
    let head = text.get(..6).unwrap_or(text).to_ascii_lowercase();
    ["<br>", "<br/>", "<br />"]
        .into_iter()
        .find(|tag| head.starts_with(tag))
        .map(str::len)
}

fn delimiter_name(delimiter: &str) -> String {
    match delimiter {
        "," => t!("delimiter-comma"),
        ";" => t!("delimiter-semicolon"),
        "\t" => t!("delimiter-tab"),
        "|" => t!("delimiter-pipe"),
        "!#" => "Poliris".to_owned(),
        _ => t!("delimiter-other"),
    }
}

/// The delimiter itself, as shown next to its name.
fn delimiter_glyph(delimiter: &str) -> String {
    match delimiter {
        "\t" => "\u{21e5}".to_owned(),
        " " => "\u{2423}".to_owned(),
        other => other.to_owned(),
    }
}

pub fn encoding_name(encoding: Encoding) -> String {
    match encoding {
        Encoding::Utf8 => "UTF-8".to_owned(),
        Encoding::Utf8Bom => t!("encoding-utf8-bom"),
        Encoding::Windows1252 => "Windows-1252".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{self, Lang};

    #[test]
    fn line_breaks_and_br_tags() {
        assert_eq!(
            split_breaks("a<BR><br />b\r\nc"),
            [("a", true), ("", true), ("b", false), ("c", false)]
        );
        assert_eq!(flat_text("x<br>y"), "x \u{21b5} y");
        assert_eq!(flat_text("<b>bold</b>"), "<b>bold</b>");
    }

    #[test]
    fn paste_grows_the_table() {
        let mut app = CsvApp::new(None);
        app.selected = (0, 2);
        app.paste("a\tb\r\nc\td\r\n");
        assert_eq!((app.table.row_count(), app.table.col_count()), (2, 4));
        assert_eq!(app.table.cell(1, 3), "d");
        app.undo();
        assert_eq!(app.table, Table::default());
    }

    #[test]
    fn undoing_back_to_the_saved_state_is_clean() {
        let mut app = CsvApp::new(None);
        app.run(Command::InsertRowBelow);
        assert!(app.dirty());
        app.undo();
        assert!(!app.dirty());
        app.redo();
        app.saved_depth = Some(app.undo.len());
        app.undo();
        app.run(Command::InsertColLeft);
        assert!(app.dirty());
        assert_eq!(app.saved_depth, None);
    }

    #[test]
    fn finds_and_cycles_through_matches() {
        let mut app = CsvApp::new(None);
        let (table, format) = table::parse(DEMO_CSV.as_bytes());
        app.reset(table, format, None);
        app.find = "LY".into();
        assert_eq!(app.matches(), [(2, 1)]);
        app.find = "a".into();
        let count = app.matches().len();
        app.selected = (0, 0);
        app.next_match(false);
        assert_eq!(app.matches()[0], (0, 0));
        let second = app.matches()[1];
        assert_eq!(app.selected, second);
        app.next_match(true);
        app.next_match(true);
        let last = *app.matches().last().expect("matches");
        assert_eq!(app.selected, last);
        assert!(count > 3);
    }

    const DEMO_CSV: &str = "Name;City;Age;Notes\nDupont;\"Paris; 5e\";42;A rather long note that does not fit in its column<BR>with a second line\nMartin;Lyon;7;\nBérard;Nantes;19;\"Two\nlines\"\n";

    const DEMO_POLIRIS: &str = "\"RC-330099\"!#\"142\"!#\"Viager\"!#\"Appartement\"!#\"25120\"!#\r\n\"RC-330099\"!#\"163/164\"!#\"Viager\"!#\"Maison/villa\"!#\"33990\"!#\r\n";

    /// A made-up listing of homes, for the website's screenshots.
    const DEMO_SITE: &str = "Ref;Type;City;Postcode;Rooms;Area;Price;Description\r\n\
AN-1042;Apartment;Bordeaux;33000;3;81;245000;Bright apartment in a well-kept building.<BR><BR>Balcony facing south, parquet floors.\r\n\
AN-1043;House;Biarritz;64200;5;140;620000;Family house near the beach.<BR>Garden of 880 m², garage.\r\n\
AN-1044;House;Lanton;33138;4;110;315000;Single-storey house in a quiet street.\r\n\
AN-1045;Apartment;Nantes;44000;2;48;189000;Close to shops and the tram.\r\n\
AN-1046;House;Arès;33740;6;165;540000;Former farmhouse with outbuildings.<BR><BR>Workshop of 30 m².\r\n\
AN-1047;Apartment;Lyon;69003;3;72;298000;Top floor, open view, lift.\r\n\
AN-1048;House;Bègles;33130;4;95;342000;Renovated in 2019, terrace and garden.\r\n\
AN-1049;Apartment;Paris;75011;2;41;415000;Quiet courtyard side, cellar.\r\n\
AN-1050;House;Censeau;39250;5;150;198000;Stone house with a large barn.\r\n\
AN-1051;Apartment;Lille;59000;1;28;132000;Studio near the station.\r\n\
AN-1052;House;Tresses;33370;5;128;468000;Contemporary house, pool, three bedrooms.\r\n";

    /// Capture of the interface (offscreen render): `cargo test capture -- --ignored`.
    /// The image is written to the file given by `CSVEDIT_CAPTURE`, in the
    /// language given by `CSVEDIT_LANG` (English by default) and the theme
    /// given by `CSVEDIT_THEME` (`light`, dark by default); with
    /// `CSVEDIT_EDIT`, a cell is being typed in; with `CSVEDIT_FIND`, that
    /// text is found; `CSVEDIT_COLUMNS` lists the columns shown,
    /// `CSVEDIT_LABELS` names them (`one`, `zero`), `CSVEDIT_CHOOSER` opens
    /// the column chooser; with `CSVEDIT_IMPORT`, the import window is open
    /// (`poliris`: on a Poliris file); `CSVEDIT_DEMO` shows the website's
    /// made-up listing of homes.
    #[test]
    #[ignore = "needs a graphics renderer"]
    fn capture() {
        let out = std::env::var("CSVEDIT_CAPTURE").unwrap_or_else(|_| "capture.png".into());
        if let Some(lang) = Lang::ALL
            .into_iter()
            .find(|l| std::env::var("CSVEDIT_LANG").is_ok_and(|v| v == l.code()))
        {
            i18n::choose(lang);
        }
        let theme = if std::env::var("CSVEDIT_THEME").as_deref() == Ok("light") {
            egui::Theme::Light
        } else {
            egui::Theme::Dark
        };
        let mut harness = egui_kittest::Harness::builder()
            .with_size([1280.0, 760.0])
            .with_theme(theme)
            .wgpu()
            .build_eframe(|cc| {
                theme::install(&cc.egui_ctx);
                cc.egui_ctx.set_theme(theme);
                let mut app = CsvApp::new(None);
                let site = std::env::var("CSVEDIT_DEMO").is_ok();
                let demo = if site { DEMO_SITE } else { DEMO_CSV };
                let name = if site { "listings.csv" } else { "people.csv" };
                let (table, format) = table::parse(demo.as_bytes());
                app.reset(table, format, Some(name.into()));
                app.selected = if site { (2, 7) } else { (1, 3) };
                app.format_changed = true;
                match std::env::var("CSVEDIT_LABELS").as_deref() {
                    Ok("one") => columns::choose(columns::Labels::FromOne),
                    Ok("zero") => columns::choose(columns::Labels::FromZero),
                    _ => columns::choose(columns::Labels::Letters),
                }
                if let Ok(list) = std::env::var("CSVEDIT_COLUMNS") {
                    let cols = columns::parse_list(&list, app.table.col_count()).expect("columns");
                    app.show_columns(cols);
                }
                app.open_columns = std::env::var("CSVEDIT_CHOOSER").is_ok();
                if let Ok(find) = std::env::var("CSVEDIT_FIND") {
                    app.find = find;
                }
                if let Ok(kind) = std::env::var("CSVEDIT_IMPORT") {
                    let bytes = if kind == "poliris" {
                        DEMO_POLIRIS.as_bytes().to_vec()
                    } else {
                        demo.as_bytes().to_vec()
                    };
                    app.import = Some(ImportDialog::new(name.into(), bytes, true));
                }
                if std::env::var("CSVEDIT_EDIT").is_ok() {
                    app.selected = (2, 1);
                    app.start_editing("Lyon 3e".into(), false);
                }
                app
            });
        harness.run_steps(4);
        let image = harness.render().expect("render");
        image.save(&out).expect("writing the capture");
    }
}
