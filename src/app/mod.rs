//! The window: title bar with the menus, toolbar, cell bar, the grid of
//! cells, the inspector and the status bar.

mod bars;
mod diagnostics;
mod find;
mod grid;
mod inspector;
mod titlebar;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Key, KeyboardShortcut, Modifiers};

use crate::columns;
use crate::i18n::t;
use crate::import::{ImportDialog, Outcome};
use crate::save::SaveError;
use crate::table::{self, Cell, Edit, Encoding, Format, LineEnding, Losses, Table, Unencodable};
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
// The spreadsheet shortcuts for inserting and deleting rows (Ctrl+= is
// the + key without Shift).
const SHORTCUT_INSERT_ROW: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Plus);
const SHORTCUT_INSERT_ROW_ALT: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND, Key::Equals);
const SHORTCUT_DELETE_ROW: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Minus);
const SHORTCUT_INSPECTOR: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::I);
// Not Ctrl+Shift+C: the system reads any Ctrl+C as Copy.
const SHORTCUT_COLUMNS: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::K);

/// What waits for an answer to "Save changes?" (or for a file to be
/// saved).
#[derive(Clone, Debug)]
enum Pending {
    New,
    /// `None`: ask for the file.
    Open(Option<PathBuf>),
    Quit,
}

/// A save being written, on a thread of its own (`save::spawn`).
struct Saving {
    path: PathBuf,
    /// The format written, and the length of `undo` then (kept up to
    /// date as `saved_depth` is): what is saved once it succeeds.
    format: Format,
    depth: Option<usize>,
    /// Saved to a new name: there is no more to warn about.
    save_as: bool,
    /// What waits for the file to be saved.
    then: Option<Pending>,
    receiver: mpsc::Receiver<Result<(), SaveError>>,
}

/// Asked for while a save is being written: done once it ends.
#[derive(Clone, Debug)]
enum Queued {
    Request(Pending),
    Save(Option<Pending>),
}

/// "Saved" shows in the status bar this long.
const SAVED_NOTICE: Duration = Duration::from_secs(3);

/// A file dialog to show, at the end of the frame (it needs the window).
#[derive(Clone, Debug)]
enum FileRequest {
    Open,
    /// Then what was waiting for the file to be saved.
    SaveAs(Option<Pending>),
    /// The diagnostics report, to a text file.
    SaveReport(String),
}

#[derive(Clone, Copy, Debug)]
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    /// In characters as seen on screen (grapheme clusters).
    longest: usize,
}

impl ColumnStats {
    fn add(&mut self, tally: Tally) {
        if tally.empty {
            self.empty += 1;
        } else {
            self.filled += 1;
        }
        self.longest = self.longest.max(tally.len);
    }

    /// Takes a cell out of the counts (the longest stays as it was).
    fn remove(&mut self, tally: Tally) {
        if tally.empty {
            self.empty -= 1;
        } else {
            self.filled -= 1;
        }
    }
}

/// What a cell counts for in `ColumnStats`.
#[derive(Clone, Copy)]
struct Tally {
    empty: bool,
    len: usize,
}

impl Tally {
    fn of(cell: &str) -> Tally {
        Tally {
            empty: cell.trim().is_empty(),
            len: grapheme_count(cell),
        }
    }
}

/// A cell at some point of the table's history: (generation, row, column).
type CellKey = (u64, usize, usize);
/// Characters, words and `<BR>` tags of a cell.
type Counts = (usize, usize, usize);

pub struct CsvApp {
    table: Table,
    format: Format,
    path: Option<PathBuf>,
    /// Undo and redo steps, with their estimated bytes (`Edit::bytes`).
    undo: Vec<(Edit, u64)>,
    redo: Vec<(Edit, u64)>,
    /// Length of `undo` when last saved; `None` once that state can't be
    /// reached again.
    saved_depth: Option<usize>,
    /// The format as last saved or opened.
    saved_format: Format,
    /// What reading the file left out: saving over it asks first.
    losses: Losses,
    /// Saving over the opened file despite `losses` was accepted.
    overwrite_accepted: bool,
    /// "Save over the original?" is asked, then this waits.
    confirm_overwrite: Option<Option<Pending>>,
    file_request: Option<FileRequest>,
    saving: Option<Saving>,
    /// The last of what was asked for during the save.
    queued: Option<Queued>,
    /// When the last save succeeded, for "Saved" in the status bar.
    saved_at: Option<Instant>,
    diagnostics: diagnostics::Diagnostics,
    /// The workbook the table was read from: it is saved as a CSV file
    /// beside it, never over it.
    origin: Option<PathBuf>,
    /// A workbook's formulas, by (row, column), shown in the inspector.
    /// Ordered, so that rows inserted or removed move only the formulas
    /// below them.
    formulas: BTreeMap<(usize, usize), Cell>,
    /// Cells that a spreadsheet would take for a formula are written with
    /// an apostrophe first (CSV injection).
    protect_formulas: bool,
    /// Commands chosen in menus, run once the frame's grid is drawn.
    deferred: Vec<Command>,
    /// The grid handled a key this frame: the keyboard stays with it.
    grid_keys: bool,
    /// Counts the table's changes, for the caches.
    generation: u64,
    /// The table was checked for characters the built-in fonts lack.
    fonts_checked: bool,
    /// The scripts of the text put in by edits since that the built-in
    /// fonts lack.
    unchecked_scripts: u16,
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
    /// The grid reached the selected cell this frame.
    scrolled: bool,
    /// Frames spent so far trying to reach it.
    scroll_tries: u8,
    /// Rows the grid shows at once: what PageUp and PageDown move by.
    page_rows: usize,
    /// The cell's editor was drawn this frame.
    editor_drawn: bool,
    /// Widths of the columns resized, by column.
    col_widths: Vec<f32>,
    find: String,
    focus_find: bool,
    matches: find::Matches,
    /// (generation, column, first data row) and the stats found then.
    column_stats: Option<(u64, usize, usize, ColumnStats)>,
    /// (generation, row, column) and the inspector's counts then.
    cell_counts: Option<(CellKey, Counts)>,
    /// The text typed in the inspector and its counts then.
    editing_counts: Option<(String, Counts)>,
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
    /// The third-party licenses are shown, from the About window.
    licenses: bool,
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
            saved_format: Format::default(),
            losses: Losses::default(),
            overwrite_accepted: false,
            confirm_overwrite: None,
            file_request: None,
            saving: None,
            queued: None,
            saved_at: None,
            diagnostics: diagnostics::Diagnostics::default(),
            origin: None,
            formulas: BTreeMap::new(),
            protect_formulas: protect_formulas_saved(),
            deferred: Vec::new(),
            grid_keys: false,
            generation: 0,
            fonts_checked: false,
            unchecked_scripts: 0,
            header: true,
            selected: (0, 0),
            editing: None,
            edit_in_inspector: false,
            focus_editor: false,
            scroll_to_selection: false,
            scrolled: false,
            scroll_tries: 0,
            page_rows: 20,
            editor_drawn: false,
            col_widths: Vec::new(),
            find: String::new(),
            focus_find: false,
            matches: find::Matches::default(),
            column_stats: None,
            cell_counts: None,
            editing_counts: None,
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
            licenses: false,
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
        self.format != self.saved_format || self.saved_depth != Some(self.undo.len())
    }

    fn reset(&mut self, table: Table, format: Format, path: Option<PathBuf>) {
        let fresh = Self {
            table,
            saved_format: format.clone(),
            format,
            path,
            header: self.header,
            find: std::mem::take(&mut self.find),
            inspector: self.inspector,
            density: self.density,
            generation: self.generation + 1,
            diagnostics: std::mem::take(&mut self.diagnostics),
            protect_formulas: self.protect_formulas,
            ..Self::new(None)
        };
        let mut old = std::mem::replace(self, fresh);
        // A large table is freed away from the interface's thread.
        if old.table.row_count().saturating_mul(old.table.col_count()) > 100_000 {
            let table = std::mem::take(&mut old.table);
            std::thread::spawn(move || drop(table));
        }
    }

    // Files

    /// Runs `action` now, or after asking to save the changes.
    fn request(&mut self, action: Pending) {
        self.commit_editing();
        if self.saving.is_some() {
            self.queued = Some(Queued::Request(action));
        } else if self.dirty() {
            self.pending = Some(action);
        } else {
            self.perform(action);
        }
    }

    fn perform(&mut self, action: Pending) {
        match action {
            Pending::New => self.reset(Table::default(), Format::default(), None),
            Pending::Open(Some(path)) => self.load(&path),
            Pending::Open(None) => self.file_request = Some(FileRequest::Open),
            Pending::Quit => self.allow_close = true,
        }
    }

    fn load(&mut self, path: &Path) {
        if crate::workbook::is_workbook(path) {
            self.import = Some(ImportDialog::workbook(path.to_owned(), self.header));
            return;
        }
        match std::fs::read(path) {
            Ok(bytes) => {
                self.import = Some(ImportDialog::new(path.to_owned(), bytes, self.header));
            }
            Err(error) => {
                let note = format!(
                    "couldn't read a {} file: {:?}",
                    file_kind(path),
                    error.kind()
                );
                self.fail(
                    t!(
                        "error-open",
                        file = file_name(path),
                        error = io_error_text(&error)
                    ),
                    &note,
                );
            }
        }
    }

    /// Saves to the file's path, then runs `then`; asks for a path first
    /// when there is none, and before writing over a file whose reading
    /// left things out.
    fn save(&mut self, then: Option<Pending>) {
        self.commit_editing();
        if self.saving.is_some() {
            self.queued = Some(Queued::Save(then));
            return;
        }
        let Some(path) = self.path.clone() else {
            self.file_request = Some(FileRequest::SaveAs(then));
            return;
        };
        if self.losses.any() && !self.overwrite_accepted {
            self.confirm_overwrite = Some(then);
            return;
        }
        self.save_to(&path, then, false);
    }

    /// The file dialogs, parented to the window so that they stay in front
    /// of it.
    fn file_dialog(&mut self, request: FileRequest, frame: &eframe::Frame) {
        let directory = self
            .path
            .as_deref()
            .or(self.origin.as_deref())
            .and_then(Path::parent)
            .filter(|dir| !dir.as_os_str().is_empty());
        let mut dialog = rfd::FileDialog::new().set_parent(frame);
        if let Some(dir) = directory {
            dialog = dialog.set_directory(dir);
        }
        match request {
            FileRequest::Open => {
                let text = ["csv", "tsv", "txt"];
                let books = crate::workbook::EXTENSIONS;
                let both: Vec<&str> = text.iter().chain(&books).copied().collect();
                dialog = dialog
                    .add_filter(t!("filter-supported"), &both)
                    .add_filter(t!("filter-csv"), &text)
                    .add_filter(t!("filter-workbooks"), &books);
                // GTK turns "*" into "*.*", which hides files without an
                // extension: without filter, it shows everything.
                if cfg!(windows) {
                    dialog = dialog.add_filter(t!("filter-all"), &["*"]);
                }
                if let Some(path) = dialog.pick_file() {
                    self.load(&path);
                }
            }
            FileRequest::SaveReport(report) => {
                let saved = dialog
                    .add_filter(t!("filter-text"), &["txt"])
                    .set_file_name("csvedit-diagnostics.txt")
                    .save_file()
                    .map(|path| (std::fs::write(&path, report), path));
                match saved {
                    Some((Ok(()), _)) => {
                        crate::diag::log(crate::diag::Level::Info, "diagnostics saved");
                        self.diagnostics.notice(t!("diagnostics-saved"));
                    }
                    Some((Err(error), path)) => self.fail(
                        t!(
                            "error-save",
                            file = file_name(&path),
                            error = io_error_text(&error)
                        ),
                        &format!("couldn't save the diagnostics: {:?}", error.kind()),
                    ),
                    None => {}
                }
            }
            FileRequest::SaveAs(then) => {
                self.commit_editing();
                // A workbook's table goes to "name.csv", beside it.
                let name = match (&self.path, &self.origin) {
                    (Some(path), _) => file_name(path),
                    (None, Some(origin)) => origin
                        .with_extension("csv")
                        .file_name()
                        .map_or_else(|| t!("untitled-file"), |n| n.to_string_lossy().into_owned()),
                    (None, None) => t!("untitled-file"),
                };
                dialog = dialog.add_filter(t!("filter-csv"), &["csv", "tsv", "txt"]);
                if let Some(path) = dialog.set_file_name(name).save_file() {
                    self.save_to(&path, then, true);
                }
            }
        }
    }

    /// Starts writing the table to `path`, on a thread; `then` runs once
    /// it is saved (`poll_saving`).
    fn save_to(&mut self, path: &Path, then: Option<Pending>, save_as: bool) {
        let mut format = self.format.clone();
        format.protect_formulas = self.protect_formulas;
        // A snapshot: the cells are shared until the next edit.
        let receiver = crate::save::spawn(self.table.clone(), format, path.to_owned());
        self.saved_at = None;
        self.saving = Some(Saving {
            path: path.to_owned(),
            format: self.format.clone(),
            depth: Some(self.undo.len()),
            save_as,
            then,
            receiver,
        });
    }

    /// Takes the result of the save being written, once there is one.
    fn poll_saving(&mut self, ctx: &egui::Context) {
        let Some(saving) = &self.saving else {
            return;
        };
        let result = match saving.receiver.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => {
                ctx.set_cursor_icon(egui::CursorIcon::Progress);
                ctx.request_repaint_after(Duration::from_millis(50));
                return;
            }
            Err(mpsc::TryRecvError::Disconnected) => Err(SaveError::Crashed),
        };
        let saving = self.saving.take().expect("a save");
        match result {
            Ok(()) => {
                crate::diag::log(
                    crate::diag::Level::Info,
                    &format!("saved a {} file", file_kind(&saving.path)),
                );
                self.path = Some(saving.path);
                self.saved_depth = saving.depth;
                self.saved_format = saving.format;
                if saving.save_as {
                    // A new file holds everything: no more to warn about.
                    self.losses = Losses::default();
                }
                self.saved_at = Some(Instant::now());
                if let Some(action) = saving.then {
                    self.perform(action);
                }
            }
            Err(error) => {
                let note = format!(
                    "couldn't save a {} file: {}",
                    file_kind(&saving.path),
                    match &error {
                        SaveError::Unencodable(_) => "a character the encoding lacks".to_owned(),
                        SaveError::ReadOnly => "read-only".to_owned(),
                        SaveError::Crashed => "the saving thread stopped".to_owned(),
                        SaveError::Io(error) => format!("{:?}", error.kind()),
                    }
                );
                let error = match error {
                    SaveError::Unencodable(Unencodable { character, cell }) => {
                        if let Some((row, col)) = cell
                            && row < self.table.row_count()
                            && col < self.table.col_count()
                        {
                            self.commit_editing();
                            self.select(row, col);
                        }
                        let place = cell
                            .map_or_else(String::new, |(row, col)| columns::reference(row, col));
                        t!(
                            "error-encoding",
                            character = character.to_string(),
                            code = format!("U+{:04X}", u32::from(character)),
                            cell = place
                        )
                    }
                    SaveError::ReadOnly => t!("error-read-only"),
                    SaveError::Crashed => t!("error-save-crashed"),
                    SaveError::Io(error) => io_error_text(&error),
                };
                self.fail(
                    t!("error-save", file = file_name(&saving.path), error = error),
                    &note,
                );
            }
        }
        if !self.allow_close
            && let Some(queued) = self.queued.take()
        {
            match queued {
                Queued::Request(action) => self.request(action),
                Queued::Save(then) => self.save(then),
            }
        }
    }

    /// Loads the system's fonts once the text shown needs them.
    fn cover_fonts(&mut self, ctx: &egui::Context) {
        if theme::system_fonts_all_requested() {
            self.unchecked_scripts = 0;
            return;
        }
        if !self.fonts_checked {
            self.fonts_checked = true;
            self.unchecked_scripts |=
                theme::scripts_needed(self.table.rows().iter().flatten().map(Cell::as_str));
        }
        let name = self.path.as_deref().map(file_name).unwrap_or_default();
        // Everything else shown that is typed or comes from the system.
        let texts = [
            self.find.as_str(),
            self.editing.as_deref().unwrap_or_default(),
            name.as_str(),
            self.error.as_deref().unwrap_or_default(),
            self.columns_filter.as_str(),
            self.columns_text.as_str(),
            self.custom_delimiter.as_str(),
        ];
        let scripts = std::mem::take(&mut self.unchecked_scripts) | theme::scripts_needed(texts);
        theme::load(ctx, scripts);
    }

    // Edits

    fn commit(&mut self, edit: Edit) {
        if !theme::system_fonts_all_requested() {
            self.unchecked_scripts |= new_text_scripts(&edit);
        }
        let inverse = self.apply(edit);
        let depth = self.undo.len();
        let saving = self.saving.as_mut().map(|saving| &mut saving.depth);
        for saved in std::iter::once(&mut self.saved_depth).chain(saving) {
            if saved.is_some_and(|saved| saved > depth) {
                *saved = None;
            }
        }
        let bytes = inverse.bytes();
        self.undo.push((inverse, bytes));
        self.redo.clear();
        let before = self.undo.len();
        forget_oldest(&mut self.undo, &mut self.saved_depth, UNDO_BUDGET);
        let forgotten = before - self.undo.len();
        if let Some(saving) = &mut self.saving {
            saving.depth = saving.depth.and_then(|depth| depth.checked_sub(forgotten));
        }
    }

    fn undo(&mut self) {
        self.commit_editing();
        if let Some((edit, _)) = self.undo.pop() {
            let inverse = self.apply(edit);
            let bytes = inverse.bytes();
            self.redo.push((inverse, bytes));
        }
    }

    fn redo(&mut self) {
        self.commit_editing();
        if let Some((edit, _)) = self.redo.pop() {
            let inverse = self.apply(edit);
            let bytes = inverse.bytes();
            self.undo.push((inverse, bytes));
        }
    }

    /// Applies `edit` (also for undo and redo), keeping what follows the
    /// table up to date; returns the edit that undoes it.
    fn apply(&mut self, edit: Edit) -> Edit {
        self.remap_shown(&edit);
        self.remap_formulas(&edit);
        let change = find::Change::of(&edit);
        let before = self.generation;
        let stats = self.stats_before(before, &change);
        let inverse = self.table.apply(edit);
        self.after_change();
        self.matches
            .follow(before, self.generation, &change, &self.table);
        self.stats_after(stats);
        inverse
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

    /// Keeps the shown columns the same ones through `edit`, about to be
    /// applied (also by undo and redo): inserted columns are shown, after
    /// the shown column before them.
    fn remap_shown(&mut self, edit: &Edit) {
        match edit {
            Edit::Batch(edits) => {
                // Applied in order, each on the result of the one before.
                for edit in edits {
                    self.remap_shown(edit);
                }
            }
            Edit::InsertCols(at, cols) => {
                let (at, n) = (*at, cols.len());
                if let Some(shown) = &mut self.shown {
                    let position = shown.iter().rposition(|&c| c < at).map_or(0, |p| p + 1);
                    for c in shown.iter_mut() {
                        if *c >= at {
                            *c += n;
                        }
                    }
                    shown.splice(position..position, at..at + n);
                }
            }
            Edit::RemoveCols(at, n) => {
                let (at, n) = (*at, *n);
                if let Some(shown) = &mut self.shown {
                    shown.retain(|&c| c < at || c >= at + n);
                    for c in shown.iter_mut() {
                        if *c >= at + n {
                            *c -= n;
                        }
                    }
                    if shown.is_empty() {
                        self.shown = None;
                    }
                }
            }
            _ => {}
        }
    }

    /// Keeps a workbook's formulas on their cells through `edit`, about to
    /// be applied; a cell given a new value loses its formula.
    fn remap_formulas(&mut self, edit: &Edit) {
        if !self.formulas.is_empty() {
            remap_formulas(&mut self.formulas, edit);
        }
    }

    /// Shows `message` in an error dialog, and notes `note` in the log:
    /// the message holds file names and maybe cell text, which the log
    /// (kept, and sent with reports) must not.
    fn fail(&mut self, message: String, note: &str) {
        crate::diag::log(crate::diag::Level::Error, note);
        self.error = Some(message);
    }

    /// Starts typing in the selected cell, in the grid or the inspector.
    fn start_editing(&mut self, text: String, in_inspector: bool) {
        self.editing = Some(text);
        self.edit_in_inspector = in_inspector;
        self.focus_editor = true;
        // The cell's editor is drawn only when the cell is.
        if !in_inspector {
            self.scroll_to_selection = true;
        }
    }

    fn commit_editing(&mut self) {
        if let Some(text) = self.editing.take() {
            let (row, col) = self.selected;
            if text != self.table.cell(row, col) {
                self.commit(Edit::SetCells(vec![(row, col, Cell::from(text))]));
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
        let blank = vec![vec![Cell::default(); self.table.col_count()]];
        let row = self.table.row_count();
        self.commit(Edit::InsertRows(row, blank));
        self.select(row, self.selected.1);
    }

    fn run(&mut self, command: Command) {
        let (row, col) = self.selected;
        // Blank rows and columns, built only by the commands that use them.
        fn blank(rows: usize, cols: usize) -> Vec<Vec<Cell>> {
            vec![vec![Cell::default(); cols]; rows]
        }
        let (row_count, col_count) = (self.table.row_count(), self.table.col_count());
        let blank_row = || blank(1, col_count);
        let blank_col = || blank(1, row_count);
        match command {
            Command::New => self.request(Pending::New),
            Command::Open => self.request(Pending::Open(None)),
            Command::Save => self.save(None),
            Command::SaveAs => self.file_request = Some(FileRequest::SaveAs(None)),
            Command::Quit => self.request(Pending::Quit),
            Command::Undo => self.undo(),
            Command::Redo => self.redo(),
            Command::Find => self.focus_find = true,
            Command::ToggleInspector => {
                // Text being typed in the inspector isn't left hidden.
                if self.edit_in_inspector {
                    self.commit_editing();
                }
                self.inspector = !self.inspector;
            }
            Command::ChooseColumns => self.open_columns = true,
            Command::InsertRowAbove => {
                self.commit_editing();
                let blank = blank_row();
                self.commit(Edit::InsertRows(row, blank));
                self.select(row, col);
            }
            Command::InsertRowBelow => {
                self.commit_editing();
                let blank = blank_row();
                self.commit(Edit::InsertRows(row + 1, blank));
                self.select(row + 1, col);
            }
            Command::DeleteRow => {
                self.editing = None;
                self.commit(Edit::RemoveRows(row, 1));
            }
            Command::InsertColLeft => {
                self.commit_editing();
                let blank = blank_col();
                self.commit(Edit::InsertCols(col, blank));
                self.select(row, col);
            }
            Command::InsertColRight => {
                self.commit_editing();
                let blank = blank_col();
                self.commit(Edit::InsertCols(col + 1, blank));
                self.select(row, col + 1);
            }
            Command::DeleteCol => {
                self.editing = None;
                let shown = self.shown_cols();
                let position = shown.iter().position(|&c| c == col).unwrap_or(0);
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
                let order =
                    crate::diag::measure("sort", || self.table.sort_order(start, col, descending));
                if order
                    .iter()
                    .enumerate()
                    .any(|(i, &from)| i != from as usize)
                {
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
    /// cell on, across the columns as shown, growing the table if needed.
    fn paste(&mut self, text: &str) {
        let lines: Vec<Vec<&str>> = text
            .trim_end_matches(['\r', '\n'])
            .split('\n')
            .map(|line| line.trim_end_matches('\r').split('\t').collect())
            .collect();
        let (row, col) = self.selected;
        let width = lines.iter().map(Vec::len).max().unwrap_or(0);
        let shown = self.shown_cols();
        let position = shown.iter().position(|&c| c == col).unwrap_or(0);
        // The columns written to: those shown from the selected one on,
        // then new ones at the end.
        let missing = (position + width).saturating_sub(shown.len());
        let count = self.table.col_count();
        let targets: Vec<usize> = shown[position..]
            .iter()
            .copied()
            .chain(count..count + missing)
            .take(width)
            .collect();
        let edits = paste_edits(&lines, row, &targets, self.table.row_count(), count);
        self.commit(Edit::Batch(edits));
    }

    // Finding

    /// The text being found, lowercased; empty when nothing is.
    fn needle(&self) -> String {
        self.find.trim().to_lowercase()
    }

    /// Cells of the shown columns containing the text being found, in the
    /// order they are shown, found through the whole table now.
    fn matches(&mut self) -> &[(u32, u32)] {
        let needle = self.needle();
        self.matches.search(
            &self.table,
            self.generation,
            &needle,
            self.shown.as_ref(),
            None,
        );
        self.matches.cells()
    }

    /// Goes on with the search for a slice of time (the grid calls it each
    /// frame), and asks for another frame while it is not done.
    fn search_slice(&mut self, ctx: &egui::Context) {
        let needle = self.needle();
        let worked = self.matches.search(
            &self.table,
            self.generation,
            &needle,
            self.shown.as_ref(),
            Some(find::SLICE),
        );
        if worked {
            // The match count, drawn earlier this frame, is redrawn.
            ctx.request_repaint();
        }
    }

    /// Matches found so far, for the counts shown.
    fn match_count(&self) -> usize {
        self.matches.cells().len()
    }

    /// Whether a cell is lit up as a match.
    fn is_match(&self, row: usize, col: usize) -> bool {
        self.matches.contains(row, col)
    }

    /// Selects the next match after the selected cell (the previous one
    /// `backwards`), wrapping around.
    fn next_match(&mut self, backwards: bool) {
        let shown = self.shown_cols();
        let order = |&(row, col): &(u32, u32)| {
            (
                row,
                shown
                    .iter()
                    .position(|&c| c == col as usize)
                    .unwrap_or(usize::MAX),
            )
        };
        let (row, col) = self.selected;
        let here = order(&(row as u32, col as u32));
        let matches = self.matches();
        // In shown order: the first one past the selection, or before it.
        let after = matches.partition_point(|m| order(m) <= here);
        let before = matches.partition_point(|m| order(m) < here);
        let found = if backwards {
            before.checked_sub(1).or(matches.len().checked_sub(1))
        } else if after < matches.len() {
            Some(after)
        } else {
            (!matches.is_empty()).then_some(0)
        };
        if let Some(i) = found {
            let (row, col) = matches[i];
            self.commit_editing();
            self.select(row as usize, col as usize);
        }
    }

    fn column_stats(&mut self, col: usize) -> ColumnStats {
        let start = self.body_start();
        if let Some((generation, c, s, stats)) = self.column_stats
            && generation == self.generation
            && c == col
            && s == start
        {
            return stats;
        }
        let mut stats = ColumnStats {
            filled: 0,
            empty: 0,
            longest: 0,
        };
        for row in &self.table.rows()[start..] {
            stats.add(Tally::of(&row[col]));
        }
        self.column_stats = Some((self.generation, col, start, stats));
        stats
    }

    /// What the cells `change` is about to set count for in the column
    /// stats, when they are up to date `before` it and it only sets cells.
    fn stats_before(&self, before: u64, change: &find::Change) -> Option<Vec<(usize, Tally)>> {
        let (generation, col, start, _) = self.column_stats?;
        let cells = change.cells_only()?;
        if generation != before {
            return None;
        }
        let mut rows: Vec<usize> = cells
            .iter()
            .filter(|&&(r, c)| c == col && r >= start)
            .map(|&(r, _)| r)
            .collect();
        rows.sort_unstable();
        rows.dedup();
        Some(
            rows.into_iter()
                .map(|r| (r, Tally::of(self.table.cell(r, col))))
                .collect(),
        )
    }

    /// Brings the column stats up to date with the edit just applied,
    /// from what its cells counted for before (`stats_before`); otherwise
    /// they are worked out again when next shown.
    fn stats_after(&mut self, old: Option<Vec<(usize, Tally)>>) {
        let (Some(old), Some((generation, col, _, stats))) = (old, &mut self.column_stats) else {
            return;
        };
        let top = stats.longest;
        // The longest cell got shorter: another may be the longest now.
        let (mut dropped, mut reached) = (false, false);
        for (row, before) in old {
            let after = Tally::of(self.table.cell(row, *col));
            dropped |= before.len == top && after.len < top;
            reached |= after.len >= top;
            stats.remove(before);
            stats.add(after);
        }
        if dropped && !reached {
            self.column_stats = None;
        } else {
            *generation = self.generation;
        }
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
            (SHORTCUT_INSERT_ROW_ALT, Command::InsertRowBelow),
            (SHORTCUT_DELETE_ROW, Command::DeleteRow),
            (SHORTCUT_INSPECTOR, Command::ToggleInspector),
            (SHORTCUT_COLUMNS, Command::ChooseColumns),
        ];
        // While typing, the text field keeps its own undo, and the table
        // isn't changed under it.
        let typing = ctx.text_edit_focused();
        for (shortcut, command) in shortcuts {
            let for_text = matches!(
                command,
                Command::Undo | Command::Redo | Command::InsertRowBelow | Command::DeleteRow
            );
            if !(typing && for_text) && ctx.input_mut(|i| i.consume_shortcut(&shortcut)) {
                self.run(command);
            }
        }
    }

    /// Moving around the grid, and typing into the selected cell, when no
    /// text field has the keyboard.
    fn handle_grid_keys(&mut self, ctx: &egui::Context) {
        if self.editing.is_some() || ctx.text_edit_focused() || ctx.any_popup_open() {
            return;
        }
        let last_row = self.table.row_count() - 1;
        let shown = self.shown_cols();
        let events = ctx.input(|i| i.events.clone());
        for event in events {
            // Keys after a typed character go to the text being typed.
            if let Some(editing) = &mut self.editing {
                match event {
                    egui::Event::Text(text) | egui::Event::Ime(egui::ImeEvent::Commit(text)) => {
                        editing.push_str(&text)
                    }
                    _ => {}
                }
                continue;
            }
            let (row, col) = self.selected;
            let position = shown.iter().position(|&c| c == col).unwrap_or(0);
            let left = shown[position.saturating_sub(1)];
            let right = shown[(position + 1).min(shown.len() - 1)];
            match event {
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } if modifiers.command && !modifiers.alt => {
                    let jump = jump(key, (row, position), last_row, shown.len() - 1);
                    if let Some((row, place)) = jump {
                        self.grid_keys = true;
                        self.select(row, shown[place]);
                    }
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } if modifiers.is_none() || modifiers.shift_only() => {
                    self.grid_keys = true;
                    let page = self.page_rows.max(1);
                    match key {
                        Key::ArrowUp => self.select(row.saturating_sub(1), col),
                        Key::ArrowDown => self.select((row + 1).min(last_row), col),
                        Key::PageUp => self.select(row.saturating_sub(page), col),
                        Key::PageDown => self.select((row + page).min(last_row), col),
                        Key::ArrowLeft => self.select(row, left),
                        Key::Tab if modifiers.shift => self.select(row, left),
                        Key::ArrowRight | Key::Tab => self.select(row, right),
                        Key::Home => self.select(row, shown[0]),
                        Key::End => self.select(row, shown[shown.len() - 1]),
                        Key::Enter | Key::F2 => {
                            self.start_editing(self.table.cell(row, col).to_owned(), false)
                        }
                        Key::Delete | Key::Backspace if !self.table.cell(row, col).is_empty() => {
                            self.commit(Edit::SetCells(vec![(row, col, Cell::default())]));
                        }
                        _ => {}
                    }
                }
                egui::Event::Text(text) | egui::Event::Ime(egui::ImeEvent::Commit(text)) => {
                    self.start_editing(text, false);
                }
                egui::Event::Copy => ctx.copy_text(self.table.cell(row, col).to_owned()),
                egui::Event::Cut => {
                    ctx.copy_text(self.table.cell(row, col).to_owned());
                    if !self.table.cell(row, col).is_empty() {
                        self.commit(Edit::SetCells(vec![(row, col, Cell::default())]));
                    }
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
        // Under another dialog, the import window waits (and takes no
        // keys).
        let covered =
            self.pending.is_some() || self.error.is_some() || self.confirm_overwrite.is_some();
        if !covered && let Some(dialog) = &mut self.import {
            match dialog.show(ctx) {
                Some(Outcome::Open(imported)) => {
                    let dialog = self.import.take().expect("an import dialog");
                    let header = dialog.header;
                    let path = dialog.path.clone();
                    let book = dialog.is_workbook();
                    crate::diag::log(
                        crate::diag::Level::Info,
                        &format!(
                            "opened a {} file ({}): {} rows x {} columns, waited {} for the reading",
                            file_kind(&path),
                            if book { "workbook" } else { "text" },
                            imported.table.row_count(),
                            imported.table.col_count(),
                            crate::diag::ms(dialog.waited())
                        ),
                    );
                    // Dropped away from the interface's thread, with the
                    // preview it holds.
                    std::thread::spawn(move || drop(dialog));
                    let imported = *imported;
                    // A workbook is never saved over: Save asks for a CSV
                    // file.
                    let saved_to = (!book).then(|| path.clone());
                    self.reset(imported.table, imported.format, saved_to);
                    self.losses = imported.losses;
                    self.header = header;
                    // The cells' characters were gathered by the reading.
                    theme::cover(ctx, [imported.rare_chars.as_str()]);
                    self.fonts_checked = true;
                    if book {
                        self.origin = Some(path);
                        self.formulas = imported
                            .formulas
                            .into_iter()
                            .map(|(r, c, f)| ((r, c), Cell::from(f)))
                            .collect();
                    }
                }
                Some(Outcome::Cancel) => self.import = None,
                None => {}
            }
        }
        // Drawn in the order they can stack in, the one on top last: egui
        // gives the keyboard and the mouse to the last dialog drawn.
        self.diagnostics_window(ctx);
        if let Some(then) = self.confirm_overwrite.clone() {
            let mut answer = None;
            theme::modal("confirm-overwrite", ctx).show(ctx, |ui| {
                ui.set_width(440.0);
                ui.heading(t!("overwrite-title", file = self.display_name()));
                ui.add_space(4.0);
                ui.label(t!("overwrite-text"));
                for (count, key) in [
                    (self.losses.blank_lines, "blank"),
                    (self.losses.trimmed, "trimmed"),
                    (self.losses.invalid_bytes, "invalid"),
                ] {
                    if count > 0 {
                        let line = match key {
                            "blank" => t!("overwrite-blank", count = count),
                            "trimmed" => t!("overwrite-trimmed", count = count),
                            _ => t!("overwrite-invalid", count = count),
                        };
                        ui.label(format!("\u{2022} {line}"));
                    }
                }
                ui.add_space(18.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if theme::primary_button(ui, &t!("overwrite-save-as"), 0.0).clicked()
                        || theme::dialog_key(ui, Key::Enter)
                    {
                        answer = Some(false);
                    }
                    if theme::secondary_button(ui, &t!("overwrite-save"), 0.0).clicked() {
                        answer = Some(true);
                    }
                    if theme::secondary_button(ui, &t!("cancel"), 0.0).clicked()
                        || theme::dialog_key(ui, Key::Escape)
                    {
                        self.confirm_overwrite = None;
                    }
                });
            });
            if let Some(overwrite) = answer {
                self.confirm_overwrite = None;
                if overwrite {
                    self.overwrite_accepted = true;
                    self.save(then);
                } else {
                    self.file_request = Some(FileRequest::SaveAs(then));
                }
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
                        || theme::dialog_key(ui, Key::Enter)
                    {
                        answer = Some(true);
                    }
                    if theme::secondary_button(ui, &t!("save-changes-discard"), 0.0).clicked() {
                        answer = Some(false);
                    }
                    if theme::secondary_button(ui, &t!("cancel"), 0.0).clicked()
                        || theme::dialog_key(ui, Key::Escape)
                    {
                        self.pending = None;
                    }
                });
            });
            if let Some(save) = answer {
                self.pending = None;
                if save {
                    self.save(Some(action));
                } else {
                    self.perform(action);
                }
            }
        }
        if self.about && !self.licenses {
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
                        || theme::dialog_key(ui, Key::Escape)
                    {
                        self.about = false;
                    }
                    if theme::secondary_button(ui, &t!("about-third-party"), 0.0).clicked() {
                        self.licenses = true;
                    }
                });
            });
        }
        if self.licenses {
            // The notices the crates' licenses ask to go with the program.
            const LICENSES: &str = include_str!("../../THIRD-PARTY-LICENSES.txt");
            let room =
                theme::dialog_room(ctx, egui::vec2(688.0, f32::INFINITY)) - egui::vec2(48.0, 48.0);
            theme::modal("licenses", ctx).show(ctx, |ui| {
                ui.set_width(room.x);
                ui.heading(t!("about-third-party"));
                ui.add_space(8.0);
                let list_height =
                    (room.y - ui.min_rect().height() - 24.0 - 36.0).clamp(60.0, 420.0);
                let lines: Vec<&str> = LICENSES.lines().collect();
                let height = ui.text_style_height(&egui::TextStyle::Monospace);
                egui::ScrollArea::vertical()
                    .max_height(list_height)
                    .auto_shrink([false, false])
                    .show_rows(ui, height, lines.len(), |ui, range| {
                        for line in &lines[range] {
                            ui.label(egui::RichText::new(*line).font(theme::mono(11.0)));
                        }
                    });
                ui.add_space(12.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if theme::primary_button(ui, &t!("ok"), 80.0).clicked()
                        || theme::dialog_key(ui, Key::Escape)
                    {
                        self.licenses = false;
                    }
                });
            });
        }
        // Last: it can come over the others (saving the diagnostics, say).
        if let Some(error) = self.error.clone() {
            theme::modal("error", ctx).show(ctx, |ui| {
                ui.set_max_width(440.0);
                ui.label(error);
                ui.add_space(18.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if theme::primary_button(ui, &t!("ok"), 80.0).clicked()
                        || (theme::dialog_key(ui, Key::Enter) || theme::dialog_key(ui, Key::Escape))
                    {
                        self.error = None;
                    }
                });
            });
        }
    }

    fn display_name(&self) -> String {
        self.path
            .as_deref()
            .or(self.origin.as_deref())
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
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let started = std::time::Instant::now();
        let ctx = ui.ctx().clone();
        self.diagnostics.frames.start_pass(
            ctx.cumulative_frame_nr(),
            frame.info().cpu_usage,
            started,
        );
        self.grid_keys = false;
        self.read_gpu(frame);
        self.cover_fonts(&ctx);
        self.poll_saving(&ctx);

        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close {
            self.commit_editing();
            if self.saving.is_some() {
                // Closed once the file is written (and asked again then
                // if there are newer changes).
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.queued = Some(Queued::Request(Pending::Quit));
            } else if self.dirty() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.pending = Some(Pending::Quit);
            }
        }
        let modal_open = self.pending.is_some()
            || self.error.is_some()
            || self.about
            || self.import.is_some()
            || self.confirm_overwrite.is_some()
            || self.diagnostics.open;
        // A file dropped on a dialog is ignored: it would open another
        // dialog over it.
        if !modal_open
            && let Some(path) =
                ctx.input(|i| i.raw.dropped_files.first().map(|f| f.path().to_owned()))
        {
            self.request(Pending::Open(Some(path)));
        }
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
        // Menu commands change the table only once the grid is drawn.
        for command in std::mem::take(&mut self.deferred) {
            self.run(command);
        }
        self.dialogs(&ctx);
        resize_grips(&ctx);
        // Waits for the save being written (a new name, or another file).
        if self.saving.is_none()
            && let Some(request) = self.file_request.take()
        {
            // The time the user takes to choose is not the frame's.
            let waiting = std::time::Instant::now();
            self.file_dialog(request, frame);
            self.diagnostics.frames.waited(waiting.elapsed());
        }
        // Tab and arrows move the grid's selection, not egui's keyboard
        // focus: a button given focus would take Enter and Space.
        if self.grid_keys
            && !ctx.text_edit_focused()
            && let Some(id) = ctx.memory(|m| m.focused())
        {
            ctx.memory_mut(|m| m.surrender_focus(id));
        }

        if self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        self.update_title(&ctx);
        self.diagnostics.frames.end_pass(started);
    }
}

/// Where Ctrl (Cmd) with `key` moves the selection, at (row, place among
/// the columns shown): Home and End to the first and last cell, the arrows
/// to the table's edge that way.
fn jump(
    key: Key,
    (row, place): (usize, usize),
    last_row: usize,
    last_place: usize,
) -> Option<(usize, usize)> {
    match key {
        Key::Home => Some((0, 0)),
        Key::End => Some((last_row, last_place)),
        Key::ArrowUp => Some((0, place)),
        Key::ArrowDown => Some((last_row, place)),
        Key::ArrowLeft => Some((row, 0)),
        Key::ArrowRight => Some((row, last_place)),
        _ => None,
    }
}

/// Most bytes the undo steps keep (`Edit::bytes`): past it, the oldest
/// steps are forgotten.
const UNDO_BUDGET: u64 = 256 << 20;

/// Forgets the oldest steps of `undo` while they keep more than `budget`
/// bytes, always keeping the latest one; `saved_depth` (a length of
/// `undo`) follows, and is `None` once the saved state is forgotten.
fn forget_oldest(undo: &mut Vec<(Edit, u64)>, saved_depth: &mut Option<usize>, budget: u64) {
    let mut total: u64 = undo.iter().map(|(_, bytes)| bytes).sum();
    let mut forgotten = 0;
    while total > budget && forgotten + 1 < undo.len() {
        total -= undo[forgotten].1;
        forgotten += 1;
    }
    if forgotten > 0 {
        undo.drain(..forgotten);
        *saved_depth = saved_depth.and_then(|depth| depth.checked_sub(forgotten));
    }
}

/// Keeps `formulas` on their cells through `edit`, about to be applied; a
/// cell given a new value loses its formula. Only the formulas that move
/// are touched.
fn remap_formulas(formulas: &mut BTreeMap<(usize, usize), Cell>, edit: &Edit) {
    // Rebuilt with the formulas moved (kept in order by `f`), when any
    // is at or past column `at`.
    fn cols(
        formulas: &mut BTreeMap<(usize, usize), Cell>,
        at: usize,
        f: impl Fn(usize) -> Option<usize>,
    ) {
        if formulas.keys().any(|&(_, c)| c >= at) {
            *formulas = std::mem::take(formulas)
                .into_iter()
                .filter_map(|((r, c), text)| f(c).map(|c| ((r, c), text)))
                .collect();
        }
    }
    match edit {
        Edit::Batch(edits) => {
            for edit in edits {
                remap_formulas(formulas, edit);
            }
        }
        Edit::SetCells(cells) => {
            for (r, c, _) in cells {
                formulas.remove(&(*r, *c));
            }
        }
        Edit::SetBlock(top, cols, values) => {
            for (i, line) in values.iter().enumerate() {
                for &c in &cols[..line.len()] {
                    formulas.remove(&(top + i, c));
                }
            }
        }
        Edit::InsertRows(at, rows) => {
            let below = formulas.split_off(&(*at, 0));
            let n = rows.len();
            formulas.extend(below.into_iter().map(|((r, c), text)| ((r + n, c), text)));
        }
        Edit::RemoveRows(at, n) => {
            let below = formulas.split_off(&(*at, 0));
            let (at, n) = (*at, *n);
            formulas.extend(
                below
                    .into_iter()
                    .filter(|&((r, _), _)| r >= at + n)
                    .map(|((r, c), text)| ((r - n, c), text)),
            );
        }
        Edit::InsertCols(at, added) => {
            let (at, n) = (*at, added.len());
            cols(formulas, at, |c| Some(if c >= at { c + n } else { c }));
        }
        Edit::RemoveCols(at, n) => {
            let (at, n) = (*at, *n);
            cols(formulas, at, |c| match c {
                c if c < at => Some(c),
                c if c < at + n => None,
                c => Some(c - n),
            });
        }
        Edit::Reorder(start, order) => {
            let start = *start;
            let below = formulas.split_off(&(start, 0));
            if below.is_empty() {
                return;
            }
            // Row start + i takes old row start + order[i].
            let new_place = crate::table::invert(order);
            formulas.extend(below.into_iter().map(|((r, c), text)| {
                let row = match new_place.get(r - start) {
                    Some(&i) => start + i as usize,
                    None => r,
                };
                ((row, c), text)
            }));
        }
    }
}

/// The edits that paste `lines` (cells by line) from row `row` on, line
/// `i` going to row `row + i` and cell `j` to column `targets[j]`, into a
/// table of `row_count` rows and `col_count` columns. The targets past the
/// last column are new columns (in order, at the end), and lines past the
/// last row new rows: these are inserted holding their text, so that only
/// the cells that were there before are kept for undo.
fn paste_edits(
    lines: &[Vec<&str>],
    row: usize,
    targets: &[usize],
    row_count: usize,
    col_count: usize,
) -> Vec<Edit> {
    let old_cols = targets.iter().take_while(|&&c| c < col_count).count();
    let new_cols = targets.len() - old_cols;
    let old_lines = lines.len().min(row_count - row);
    let mut edits = Vec::new();
    if new_cols > 0 {
        let cols = (old_cols..targets.len())
            .map(|j| {
                (0..row_count)
                    .map(|r| {
                        r.checked_sub(row)
                            .and_then(|i| lines.get(i)?.get(j))
                            .map_or_else(Cell::default, |text| Cell::new(text))
                    })
                    .collect()
            })
            .collect();
        edits.push(Edit::InsertCols(col_count, cols));
    }
    if lines.len() > old_lines {
        let width = col_count + new_cols;
        let rows = lines[old_lines..]
            .iter()
            .map(|line| {
                let mut cells = vec![Cell::default(); width];
                for (text, &c) in line.iter().zip(targets) {
                    cells[c] = Cell::new(text);
                }
                cells
            })
            .collect();
        edits.push(Edit::InsertRows(row_count, rows));
    }
    let values = lines[..old_lines]
        .iter()
        .map(|line| {
            line.iter()
                .take(old_cols)
                .map(|text| Cell::new(text))
                .collect()
        })
        .collect();
    edits.push(Edit::SetBlock(row, targets[..old_cols].to_vec(), values));
    edits
}

/// The scripts of the text an edit puts in that the built-in fonts lack.
fn new_text_scripts(edit: &Edit) -> u16 {
    match edit {
        Edit::SetCells(cells) => theme::scripts_needed(cells.iter().map(|(_, _, c)| c.as_str())),
        Edit::InsertRows(_, r) | Edit::InsertCols(_, r) | Edit::SetBlock(_, _, r) => {
            theme::scripts_needed(r.iter().flatten().map(Cell::as_str))
        }
        Edit::Batch(edits) => edits.iter().fold(0, |all, e| all | new_text_scripts(e)),
        Edit::RemoveRows(..) | Edit::RemoveCols(..) | Edit::Reorder(..) => 0,
    }
}

/// What went wrong reading or writing a file, for the user: in their
/// language when it is common, else as the system says it.
fn io_error_text(error: &std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::NotFound => t!("error-not-found"),
        std::io::ErrorKind::PermissionDenied => t!("error-permission"),
        _ => error.to_string(),
    }
}

/// "csv", "xlsx": a file's kind for the log, which keeps no file names.
fn file_kind(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .filter(|e| e.len() <= 5 && e.chars().all(|c| c.is_ascii_alphanumeric()))
        .map_or_else(|| "other".to_owned(), str::to_ascii_lowercase)
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
    if !text.bytes().any(|b| matches!(b, b'\n' | b'\r' | b'<')) {
        return text.to_owned();
    }
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
    let head = text.as_bytes();
    ["<br>", "<br/>", "<br />"]
        .into_iter()
        .find(|tag| {
            head.len() >= tag.len() && head[..tag.len()].eq_ignore_ascii_case(tag.as_bytes())
        })
        .map(str::len)
}

/// Whether `text` holds `needle` (lowercase), ignoring case, without
/// allocating when the needle is ASCII.
pub fn contains_ignoring_case(text: &str, needle: &str) -> bool {
    if needle.is_ascii() {
        let (text, needle) = (text.as_bytes(), needle.as_bytes());
        let (Some(&first), Some(last)) = (needle.first(), text.len().checked_sub(needle.len()))
        else {
            return needle.is_empty();
        };
        if !needle.iter().any(u8::is_ascii_alphabetic) {
            return memchr::memmem::find(text, needle).is_some();
        }
        // Only where the first character is, in either case.
        memchr::memchr2_iter(
            first.to_ascii_lowercase(),
            first.to_ascii_uppercase(),
            &text[..=last],
        )
        .any(|i| text[i..i + needle.len()].eq_ignore_ascii_case(needle))
    } else {
        !text.is_ascii() && text.to_lowercase().contains(needle)
    }
}

/// Characters as seen on screen: an accent typed apart or an emoji made
/// of several code points counts once.
pub fn grapheme_count(text: &str) -> usize {
    use unicode_segmentation::UnicodeSegmentation as _;
    if text.is_ascii() {
        // Each byte is one, but for a CR LF line break.
        let crlf = if text.contains('\r') {
            memchr::memmem::find_iter(text.as_bytes(), b"\r\n").count()
        } else {
            0
        };
        return text.len() - crlf;
    }
    text.graphemes(true).count()
}

/// At most `max` characters of `text`, for display (a cell can hold
/// megabytes).
pub fn capped(text: &str, max: usize) -> std::borrow::Cow<'_, str> {
    match text.char_indices().nth(max) {
        Some((end, _)) => format!("{}\u{2026}", &text[..end]).into(),
        None => text.into(),
    }
}

/// The "protect formulas" choice made last time (off at first: files are
/// written as they are).
fn protect_formulas_saved() -> bool {
    std::fs::read_to_string(crate::config::dir().join("protect-formulas"))
        .is_ok_and(|s| s.trim() == "on")
}

fn save_protect_formulas(on: bool) {
    let dir = crate::config::dir();
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join("protect-formulas"), if on { "on" } else { "off" });
}

pub(crate) fn delimiter_name(delimiter: &str) -> String {
    match delimiter {
        "," => t!("delimiter-comma"),
        ";" => t!("delimiter-semicolon"),
        "\t" => t!("delimiter-tab"),
        "|" => t!("delimiter-pipe"),
        "!#" => t!("delimiter-poliris"),
        " " => t!("delimiter-space"),
        _ => t!("delimiter-other"),
    }
}

/// The delimiter itself, as shown next to its name.
pub(crate) fn delimiter_glyph(delimiter: &str) -> String {
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
        Encoding::Utf16Le => "UTF-16 LE".to_owned(),
        Encoding::Utf16Be => "UTF-16 BE".to_owned(),
    }
}

/// "CRLF", "LF" or "CR".
pub(crate) fn line_ending_name(ending: LineEnding) -> &'static str {
    match ending {
        LineEnding::Lf => "LF",
        LineEnding::CrLf => "CRLF",
        LineEnding::Cr => "CR",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{self, Lang};

    #[test]
    fn ctrl_keys_jump_to_the_table_edges() {
        let at = (5, 2);
        assert_eq!(jump(Key::Home, at, 99, 7), Some((0, 0)));
        assert_eq!(jump(Key::End, at, 99, 7), Some((99, 7)));
        assert_eq!(jump(Key::ArrowUp, at, 99, 7), Some((0, 2)));
        assert_eq!(jump(Key::ArrowDown, at, 99, 7), Some((99, 2)));
        assert_eq!(jump(Key::ArrowLeft, at, 99, 7), Some((5, 0)));
        assert_eq!(jump(Key::ArrowRight, at, 99, 7), Some((5, 7)));
        assert_eq!(jump(Key::A, at, 99, 7), None);
    }

    #[test]
    fn formulas_follow_their_cells() {
        let mut app = CsvApp::new(None);
        app.reset(Table::empty(3, 2), Format::default(), None);
        app.formulas.insert((2, 1), "SUM(B1:B2)".into());
        app.selected = (0, 0);
        app.run(Command::InsertRowAbove);
        app.run(Command::InsertColLeft);
        assert_eq!(
            app.formulas.get(&(3, 2)).map(Cell::as_str),
            Some("SUM(B1:B2)")
        );
        app.undo();
        app.undo();
        assert!(app.formulas.contains_key(&(2, 1)));
        app.commit(Edit::SetCells(vec![(2, 1, "7".into())]));
        assert!(app.formulas.is_empty());
    }

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

    fn grid(rows: &[&[&str]]) -> Table {
        Table::from_rows(
            rows.iter()
                .map(|r| r.iter().map(|&c| Cell::from(c)).collect())
                .collect(),
        )
    }

    #[test]
    fn paste_over_ragged_lines_hidden_columns_and_new_cells_undoes_exactly() {
        let mut app = CsvApp::new(None);
        let start = grid(&[&["a", "b", "c"], &["d", "e", "f"]]);
        app.reset(start.clone(), Format::default(), None);
        // Column B hidden: the paste goes to C, A, then new columns.
        app.shown = Some(vec![2, 0]);
        app.selected = (1, 2);
        app.paste("1\t2\t3\t4\n5\n6\t7\t\t8\n");
        let pasted = grid(&[
            &["a", "b", "c", "", ""],
            &["2", "e", "1", "3", "4"],
            &["", "", "5", "", ""],
            &["7", "", "6", "", "8"],
        ]);
        assert_eq!(app.table, pasted);
        // Only the cells that were there are kept for undo.
        let (undo, _) = app.undo.last().unwrap();
        let Edit::Batch(steps) = undo else {
            panic!("{undo:?}")
        };
        assert_eq!(
            steps[0],
            Edit::SetBlock(1, vec![2, 0], vec![vec!["f".into(), "d".into()]])
        );
        app.undo();
        assert_eq!(app.table, start);
        app.redo();
        assert_eq!(app.table, pasted);
        app.undo();
        assert_eq!(app.table, start);
        assert!(!app.dirty());
    }

    #[test]
    fn paste_inside_the_table_sets_a_block() {
        let mut app = CsvApp::new(None);
        let start = grid(&[&["a", "b"], &["c", "d"], &["e", "f"]]);
        app.reset(start.clone(), Format::default(), None);
        app.selected = (1, 0);
        app.paste("x\ty\r\nz\r\n");
        assert_eq!(app.table, grid(&[&["a", "b"], &["x", "y"], &["z", "f"]]));
        app.undo();
        assert_eq!(app.table, start);
    }

    #[test]
    fn the_oldest_undo_steps_go_past_the_budget() {
        let step = |bytes| (Edit::RemoveRows(0, 1), bytes);
        let mut undo = vec![step(10), step(10), step(10), step(10)];
        let mut saved = Some(3);
        forget_oldest(&mut undo, &mut saved, 25);
        assert_eq!(undo.len(), 2);
        // Saved after the third step: one undo away still.
        assert_eq!(saved, Some(1));
        forget_oldest(&mut undo, &mut saved, 5);
        assert_eq!(undo.len(), 1, "the latest step stays");
        assert_eq!(saved, Some(0));
        undo.push(step(10));
        forget_oldest(&mut undo, &mut saved, 15);
        assert_eq!(saved, None, "the saved state can't be reached again");
        let mut undo = vec![step(10), step(10)];
        let mut saved = Some(2);
        forget_oldest(&mut undo, &mut saved, 100);
        assert_eq!((undo.len(), saved), (2, Some(2)));
    }

    #[test]
    fn undo_stays_exact_once_old_steps_are_forgotten() {
        let mut app = CsvApp::new(None);
        app.reset(grid(&[&["0"]]), Format::default(), None);
        for i in 1..=5 {
            app.commit(Edit::SetCells(vec![(0, 0, Cell::from(i.to_string()))]));
            // As if the budget held three steps (all of the same size).
            let step = app.undo[0].1;
            forget_oldest(&mut app.undo, &mut app.saved_depth, step * 3);
        }
        let kept = app.undo.len();
        assert_eq!(kept, 3);
        for _ in 0..kept {
            app.undo();
        }
        let expected = (5 - kept).to_string();
        assert_eq!(app.table.cell(0, 0), expected);
        assert!(app.dirty(), "the opened state is gone");
        for _ in 0..kept {
            app.redo();
        }
        assert_eq!(app.table.cell(0, 0), "5");
    }

    #[test]
    fn formulas_follow_rows_columns_and_sorts() {
        let mut f = BTreeMap::new();
        for key in [(0, 0), (1, 1), (2, 0), (3, 2)] {
            f.insert(key, Cell::from(format!("{key:?}")));
        }
        let keys = |f: &BTreeMap<(usize, usize), Cell>| f.keys().copied().collect::<Vec<_>>();
        let mut g = f.clone();
        remap_formulas(&mut g, &Edit::InsertRows(4, vec![vec![]]));
        assert_eq!(g, f, "a row added at the end moves nothing");
        remap_formulas(&mut g, &Edit::InsertRows(2, vec![vec![], vec![]]));
        assert_eq!(keys(&g), [(0, 0), (1, 1), (4, 0), (5, 2)]);
        remap_formulas(&mut g, &Edit::RemoveRows(1, 4));
        assert_eq!(keys(&g), [(0, 0), (1, 2)]);
        assert_eq!(g[&(1, 2)], "(3, 2)");
        let mut g = f.clone();
        remap_formulas(&mut g, &Edit::InsertCols(1, vec![vec![]]));
        assert_eq!(keys(&g), [(0, 0), (1, 2), (2, 0), (3, 3)]);
        remap_formulas(&mut g, &Edit::RemoveCols(0, 1));
        assert_eq!(keys(&g), [(1, 1), (3, 2)]);
        let mut g = f.clone();
        // Rows 1.. become old rows 3, 1, 2.
        remap_formulas(&mut g, &Edit::Reorder(1, vec![2, 0, 1]));
        assert_eq!(keys(&g), [(0, 0), (1, 2), (2, 1), (3, 0)]);
        assert_eq!(g[&(1, 2)], "(3, 2)");
        let mut g = f.clone();
        remap_formulas(
            &mut g,
            &Edit::SetBlock(
                1,
                vec![1, 0],
                vec![vec!["".into()], vec!["".into(), "".into()]],
            ),
        );
        assert_eq!(keys(&g), [(0, 0), (3, 2)]);
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

    /// Polls until the save being written ends.
    fn finish_saving(app: &mut CsvApp) {
        let ctx = egui::Context::default();
        for _ in 0..500 {
            app.poll_saving(&ctx);
            if app.saving.is_none() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the save never ended");
    }

    #[test]
    fn edits_made_while_saving_stay_unsaved() {
        let dir = std::env::temp_dir().join(format!("csvedit-app-save-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a test folder");
        let path = dir.join("a.csv");
        let mut app = CsvApp::new(None);
        app.commit(Edit::SetCells(vec![(0, 0, "saved".into())]));
        app.save_to(&path, None, true);
        // Typed while the file is written: not in it.
        app.commit(Edit::SetCells(vec![(0, 0, "later".into())]));
        // Asked for meanwhile: waits for the save.
        app.run(Command::New);
        assert!(app.pending.is_none());
        finish_saving(&mut app);
        let text = std::fs::read_to_string(&path).expect("saved");
        assert!(text.starts_with("saved,"), "{text:?}");
        assert_eq!(app.path.as_deref(), Some(path.as_path()));
        assert!(app.dirty());
        // The queued New asks about the newer change.
        assert!(matches!(app.pending, Some(Pending::New)));
        app.pending = None;
        app.undo();
        assert!(!app.dirty());
        assert_eq!(app.table.cell(0, 0), "saved");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_edit_after_undoing_past_the_save_start_stays_dirty() {
        let dir = std::env::temp_dir().join(format!("csvedit-app-undo-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a test folder");
        let path = dir.join("a.csv");
        let mut app = CsvApp::new(None);
        app.commit(Edit::SetCells(vec![(0, 0, "saved".into())]));
        app.save_to(&path, None, true);
        app.undo();
        app.commit(Edit::SetCells(vec![(0, 0, "other".into())]));
        finish_saving(&mut app);
        // The state saved can no longer be reached by undoing.
        assert_eq!(app.saved_depth, None);
        assert!(app.dirty());
        let _ = std::fs::remove_dir_all(&dir);
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
        let (row, col) = app.matches()[1];
        assert_eq!(app.selected, (row as usize, col as usize));
        app.next_match(true);
        app.next_match(true);
        let (row, col) = *app.matches().last().expect("matches");
        assert_eq!(app.selected, (row as usize, col as usize));
        assert!(count > 3);
    }

    #[test]
    fn finds_ignoring_case() {
        for (text, needle, found) in [
            ("Lyon", "ly", true),
            ("lyON", "on", true),
            ("Lyon", "yo", true),
            ("Lyon", "lyonn", false),
            ("2024-05-01", "05-0", true),
            ("2024-05-01", "06", false),
            ("aXbX", "xbx", true),
            ("", "a", false),
            ("abc", "", true),
            ("Évian", "év", true),
            ("Evian", "év", false),
        ] {
            assert_eq!(
                contains_ignoring_case(text, needle),
                found,
                "{text} {needle}"
            );
        }
    }

    #[test]
    fn counts_characters_as_seen() {
        assert_eq!(grapheme_count("abc"), 3);
        assert_eq!(grapheme_count("a\r\nb\rc\n"), 6);
        assert_eq!(grapheme_count("e\u{301}t\u{e9}"), 3);
        assert_eq!(grapheme_count(""), 0);
    }

    #[test]
    fn keeps_column_stats_up_to_date_through_edits() {
        let mut app = CsvApp::new(None);
        let (table, format) = table::parse(b"Name;City\nLyon;Paris\nNice;\nAix;Marseille\n");
        app.reset(table, format, None);
        app.header = true;
        // What working them out again finds, to compare with.
        let fresh = |app: &mut CsvApp| {
            app.column_stats = None;
            let s = app.column_stats(1);
            (s.filled, s.empty, s.longest)
        };
        let stats = |app: &mut CsvApp| {
            let s = app.column_stats(1);
            (s.filled, s.empty, s.longest)
        };
        assert_eq!(stats(&mut app), (2, 1, 9));
        let edits = [
            // Filled, longer.
            vec![(2, 1, Cell::from("Montpellier"))],
            // The longest one shorter: worked out again.
            vec![(2, 1, Cell::from("Nice"))],
            // Emptied, the same cell twice, another column, the header.
            vec![
                (1, 1, Cell::from(" ")),
                (3, 1, Cell::from("x")),
                (3, 1, Cell::from("Marseilles")),
                (1, 0, Cell::from("Lille")),
                (0, 1, Cell::from("A very long title")),
            ],
        ];
        for (i, edit) in edits.into_iter().enumerate() {
            app.commit(Edit::SetCells(edit));
            let in_place = app.column_stats.is_some_and(|(g, ..)| g == app.generation);
            assert_eq!(in_place, i != 1, "edit {i}");
            let followed = stats(&mut app);
            assert_eq!(followed, fresh(&mut app));
            app.undo();
            let followed = stats(&mut app);
            assert_eq!(followed, fresh(&mut app));
            app.redo();
            stats(&mut app);
        }
        assert_eq!(stats(&mut app), (2, 1, 10));
        app.run(Command::DeleteRow);
        assert_eq!(stats(&mut app), fresh(&mut app));
    }

    #[test]
    fn keeps_matches_up_to_date_through_edits() {
        let mut app = CsvApp::new(None);
        let (table, format) = table::parse(b"Name;City\nLyon;Paris\nNice;Lyon\n");
        app.reset(table, format, None);
        // What a full search finds, to compare with.
        let fresh = |app: &mut CsvApp| {
            app.matches.clear();
            app.matches().to_vec()
        };
        app.find = "ly".into();
        assert_eq!(app.matches(), [(1, 0), (2, 1)]);
        app.commit(Edit::SetCells(vec![
            (1, 1, Cell::from("Lyon 3e")),
            (1, 0, Cell::from("Nantes")),
        ]));
        assert_eq!(app.matches(), [(1, 1), (2, 1)]);
        assert_eq!(fresh(&mut app), [(1, 1), (2, 1)]);
        assert!(app.is_match(1, 1) && !app.is_match(1, 0));
        app.undo();
        assert_eq!(app.matches(), [(1, 0), (2, 1)]);
        app.redo();
        assert_eq!(app.matches(), [(1, 1), (2, 1)]);
        // Typing on keeps only the cells that still hold the text.
        app.find = "lyon 3".into();
        assert_eq!(app.matches(), [(1, 1)]);
        assert_eq!(fresh(&mut app), [(1, 1)]);
        // In the order the columns are shown.
        app.find = "n".into();
        app.show_columns(vec![1, 0]);
        assert_eq!(app.matches(), [(0, 0), (1, 1), (1, 0), (2, 1), (2, 0)]);
        assert!(app.is_match(1, 0) && app.is_match(2, 0));
        app.commit(Edit::SetCells(vec![(1, 0, Cell::from("Paris"))]));
        assert_eq!(app.matches(), [(0, 0), (1, 1), (2, 1), (2, 0)]);
        assert_eq!(fresh(&mut app), [(0, 0), (1, 1), (2, 1), (2, 0)]);
        // A hidden column is not searched.
        app.show_columns(vec![1]);
        assert_eq!(app.matches(), [(1, 1), (2, 1)]);
        assert!(!app.is_match(2, 0));
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
    /// (`poliris`: on a Poliris file, `xlsx`: on a workbook); with
    /// `CSVEDIT_DIAG`, the Diagnostics window; `CSVEDIT_DEMO` shows the website's
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
                app.format.line_ending = table::LineEnding::CrLf;
                app.saved_format.line_ending = table::LineEnding::Lf;
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
                if std::env::var("CSVEDIT_IMPORT").as_deref() == Ok("xlsx") {
                    let path = std::env::temp_dir().join("csvedit-capture.xlsx");
                    std::fs::write(&path, crate::workbook::tests::sample()).expect("sample");
                    app.import = Some(ImportDialog::workbook(path, true));
                } else if let Ok(kind) = std::env::var("CSVEDIT_IMPORT") {
                    let bytes = if kind == "poliris" {
                        DEMO_POLIRIS.as_bytes().to_vec()
                    } else {
                        demo.as_bytes().to_vec()
                    };
                    app.import = Some(ImportDialog::new(name.into(), bytes, true));
                }
                app.diagnostics.open = std::env::var("CSVEDIT_DIAG").is_ok();
                if std::env::var("CSVEDIT_EDIT").is_ok() {
                    app.selected = (2, 1);
                    app.start_editing("Lyon 3e".into(), false);
                }
                app
            });
        // Background readings get the time to finish.
        for _ in 0..20 {
            harness.step();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let image = harness.render().expect("render");
        image.save(&out).expect("writing the capture");
    }
}
