//! The window shown when opening a file: how to read it (encoding,
//! delimiters, quotes…), with a preview of the result.

use std::path::PathBuf;
use std::sync::{Arc, mpsc};

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, Key, Layout, Margin, Modifiers, Rect, Sense,
    Stroke, StrokeKind, pos2, vec2,
};
use egui_phosphor::regular as icon;

use crate::app::{delimiter_glyph, delimiter_name, encoding_name, flat_text, line_ending_name};
use crate::columns;
use crate::i18n::t;
use crate::table::{self, Encoding, Format, ImportOptions, Imported, Preview, Table, TooLarge};
use crate::theme::{self, Palette, palette};
use crate::workbook::{self, WorkbookError};

/// Width of the window.
const WIDTH: f32 = 820.0;
/// Room on each side of its content.
const SIDE: f32 = 24.0;
/// Preview: rows shown at most, row height, column width, rows visible
/// without scrolling.
const PREVIEW_ROWS: usize = 100;
const PREVIEW_ROW: f32 = 30.0;
const PREVIEW_COL: f32 = 180.0;
const PREVIEW_NUMBER: f32 = 44.0;
const PREVIEW_VISIBLE: f32 = 8.0;
/// Seconds the options must stay unchanged before the whole file is read
/// again with them.
const SETTLE: f64 = 0.3;

pub enum Outcome {
    Open,
    Cancel,
}

pub struct ImportDialog {
    pub path: PathBuf,
    bytes: Arc<Vec<u8>>,
    options: ImportOptions,
    /// The first row holds the column names.
    pub header: bool,
    /// A delimiter of one's own, one or more characters (`!#`).
    other_text: String,
    /// What was guessed when the file was opened, for the footer.
    detected: Option<String>,
    /// The start of the file read with `options`.
    preview: Option<Preview>,
    /// The whole file read with some options, in the background.
    reading: Option<Reading>,
    /// When `options` last changed (egui time).
    changed_at: f64,
    /// The "Other" field had the keyboard at the end of the last frame.
    other_focused: bool,
    /// A workbook: the sheets found so far, and the one chosen.
    workbook: Option<(Vec<String>, usize)>,
}

/// Why a file can't be opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenError {
    TooLarge(TooLarge),
    Workbook(WorkbookError),
}

/// What is read: a text file with some options, or a workbook's sheet.
#[derive(Clone, Debug, PartialEq)]
enum ReadKey {
    Text(ImportOptions),
    Sheet(usize),
}

/// The whole file being read in the background.
struct Reading {
    key: ReadKey,
    receiver: mpsc::Receiver<Result<Imported, OpenError>>,
    result: Option<Result<Imported, OpenError>>,
}

impl ImportDialog {
    pub fn new(path: PathBuf, bytes: Vec<u8>, header: bool) -> Self {
        let options = ImportOptions::detect(&bytes);
        let other_text = options
            .delimiters
            .iter()
            .find(|d| !["\t", ",", ";", " "].contains(&d.as_str()))
            .cloned()
            .unwrap_or_default();
        // Poliris files have no header row.
        let header = header && !options.delimiters.iter().any(|d| d == "!#");
        let detected = (!bytes.is_empty()).then(|| {
            let sample = table::preview(&bytes, &options, 50);
            let delimiter = &options.delimiters[0];
            let mut name = delimiter_name(delimiter);
            if !matches!(delimiter.as_str(), "\t" | " ") {
                name = format!("{name} {}", delimiter_glyph(delimiter));
            }
            // The line endings of the first rows (all of them, at most 50).
            let text = table::decode_as(
                &bytes[..bytes.len().min(table::SAMPLE_BYTES)],
                options.encoding,
            )
            .text;
            let endings = if text.contains("\r\n") {
                table::LineEnding::CrLf
            } else if text.contains('\n') || sample.rows.len() < 2 {
                table::LineEnding::Lf
            } else {
                table::LineEnding::Cr
            };
            t!(
                "import-detected",
                delimiter = name,
                encoding = encoding_name(options.encoding),
                endings = line_ending_name(endings)
            )
        });
        Self {
            path,
            header,
            other_text,
            detected,
            options,
            bytes: Arc::new(bytes),
            preview: None,
            reading: None,
            changed_at: f64::NEG_INFINITY,
            other_focused: false,
            workbook: None,
        }
    }

    /// For a workbook (.xlsx, .xls, .ods…): its sheets are read in the
    /// background, apart (see `workbook`).
    pub fn workbook(path: PathBuf, header: bool) -> Self {
        Self {
            detected: Some(t!("import-workbook")),
            workbook: Some((Vec::new(), 0)),
            ..Self::new(path, Vec::new(), header)
        }
    }

    pub fn is_workbook(&self) -> bool {
        self.workbook.is_some()
    }

    fn key(&self) -> ReadKey {
        match self.workbook {
            Some((_, sheet)) => ReadKey::Sheet(sheet),
            None => ReadKey::Text(self.options.clone()),
        }
    }

    /// Reads now what `key` says.
    fn read(path: &std::path::Path, bytes: &[u8], key: &ReadKey) -> Result<Imported, OpenError> {
        match key {
            ReadKey::Text(options) => table::import(bytes, options).map_err(OpenError::TooLarge),
            ReadKey::Sheet(sheet) => {
                let book = workbook::read(path, *sheet).map_err(OpenError::Workbook)?;
                let table = if book.rows.is_empty() {
                    Table::default()
                } else {
                    Table::from_rows(book.rows)
                };
                // Saved as a CSV Excel reads well: with a BOM, and the
                // delimiter of the language's Excel.
                let format = Format {
                    delimiter: if crate::i18n::lang() == crate::i18n::Lang::Fr {
                        ";".to_owned()
                    } else {
                        ",".to_owned()
                    },
                    encoding: Encoding::Utf8Bom,
                    ..Format::default()
                };
                Ok(Imported {
                    table,
                    format,
                    losses: table::Losses::default(),
                    formulas: book.formulas,
                    sheets: book.sheets,
                })
            }
        }
    }

    /// The file read with the chosen options: the background reading if it
    /// is done or under way with them, else read now.
    pub fn into_imported(mut self) -> Result<Imported, OpenError> {
        let key = self.key();
        match self.reading.take() {
            Some(Reading {
                key: k,
                result: Some(result),
                ..
            }) if k == key => result,
            Some(Reading {
                key: k, receiver, ..
            }) if k == key => receiver
                .recv()
                .unwrap_or_else(|_| Self::read(&self.path, &self.bytes, &key)),
            _ => Self::read(&self.path, &self.bytes, &key),
        }
    }

    /// The background reading's result, when it is for what is shown.
    fn result(&mut self) -> Option<&Result<Imported, OpenError>> {
        let key = self.key();
        let reading = self.reading.as_mut()?;
        if reading.result.is_none() {
            reading.result = reading.receiver.try_recv().ok();
            // A workbook's sheets and preview come with its reading.
            if let (Some(Ok(imported)), Some((sheets, _))) = (&reading.result, &mut self.workbook) {
                *sheets = imported.sheets.clone();
                let rows: Vec<Vec<String>> = imported
                    .table
                    .rows()
                    .iter()
                    .take(PREVIEW_ROWS)
                    .cloned()
                    .collect();
                self.preview = Some(Preview {
                    lines: (1..=rows.len()).collect(),
                    rows,
                    invalid_bytes: 0,
                });
            }
        }
        (reading.key == key)
            .then_some(reading.result.as_ref())
            .flatten()
    }

    /// Reads the whole file again once the options have settled.
    fn keep_reading(&mut self, ctx: &egui::Context) {
        let key = self.key();
        if self.reading.as_ref().is_some_and(|r| r.key == key) {
            return;
        }
        let now = ctx.input(|i| i.time);
        if now - self.changed_at < SETTLE {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(SETTLE));
            return;
        }
        let (sender, receiver) = mpsc::channel();
        let bytes = Arc::clone(&self.bytes);
        let path = self.path.clone();
        let ctx = ctx.clone();
        let job = key.clone();
        std::thread::spawn(move || {
            let _ = sender.send(Self::read(&path, &bytes, &job));
            ctx.request_repaint();
        });
        self.reading = Some(Reading {
            key,
            receiver,
            result: None,
        });
    }

    fn preview_rows(&mut self) -> &Preview {
        if self.workbook.is_some() {
            // Filled in when the sheet has been read.
            return self.preview.get_or_insert_with(|| Preview {
                rows: Vec::new(),
                lines: Vec::new(),
                invalid_bytes: 0,
            });
        }
        self.preview
            .get_or_insert_with(|| table::preview(&self.bytes, &self.options, PREVIEW_ROWS))
    }

    /// Ticks or unticks a delimiter, keeping the order they were chosen in.
    fn toggle_delimiter(&mut self, delimiter: &str) {
        let list = &mut self.options.delimiters;
        if let Some(i) = list.iter().position(|d| d == delimiter) {
            list.remove(i);
        } else {
            list.push(delimiter.to_owned());
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Option<Outcome> {
        let mut outcome = None;
        // Read before the fields: a single-line field drops the keyboard
        // in the frame Enter is pressed.
        let typing = ctx.memory(|m| m.focused().is_some());
        // Escape leaves the "Other" field, not the window.
        if self.other_focused {
            ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
        }
        self.keep_reading(ctx);
        let dark = ctx.global_style().visuals.dark_mode;
        let p = if dark { &theme::DARK } else { &theme::LIGHT };
        let frame = egui::Frame::new()
            .fill(p.chrome)
            .stroke(Stroke::new(1.0, p.line))
            .corner_radius(14)
            .shadow(ctx.global_style().visuals.window_shadow);
        let modal = egui::Modal::new(egui::Id::new("import"))
            .backdrop_color(p.scrim)
            .frame(frame)
            .show(ctx, |ui| {
                ui.set_width(WIDTH);
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                let before = self.key();
                if self.title(ui) {
                    outcome = Some(Outcome::Cancel);
                }
                egui::Frame::new()
                    .inner_margin(Margin::symmetric(SIDE as i8, 20))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 20.0;
                        if self.workbook.is_some() {
                            self.sheet_field(ui);
                        } else {
                            self.fields(ui);
                            self.separators(ui);
                        }
                        self.switches(ui);
                        self.preview(ui);
                    });
                if self.key() != before {
                    self.preview = None;
                    // A sheet is read at once; options, once they settle.
                    if self.workbook.is_none() {
                        self.changed_at = ui.input(|i| i.time);
                    }
                }
                if let Some(o) = self.footer(ui) {
                    outcome = Some(o);
                }
            });
        if modal.should_close() && outcome.is_none() {
            outcome = Some(Outcome::Cancel);
        }
        if outcome.is_none() && !typing && ctx.input(|i| i.key_pressed(Key::Enter)) {
            outcome = Some(Outcome::Open);
        }
        // A file too large to open can't be imported.
        if matches!(outcome, Some(Outcome::Open)) && matches!(self.result(), Some(Err(_))) {
            outcome = None;
        }
        outcome
    }

    /// Icon, "Import file.csv", a summary of the result, and a close
    /// button; whether it was clicked.
    fn title(&mut self, ui: &mut egui::Ui) -> bool {
        let p = palette(ui);
        let file = self
            .path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let header = self.header;
        let first_line = self.preview_rows().lines.first().copied();
        let mut summary = match self.result() {
            Some(Ok(imported)) => {
                let rows = imported.table.row_count();
                let rows = rows - usize::from(header && rows > 1);
                format!(
                    "{} \u{b7} {}",
                    t!("import-data-rows", count = rows),
                    t!("import-columns", count = imported.table.col_count())
                )
            }
            Some(Err(error)) => open_error_text(error),
            None => t!("import-reading"),
        };
        if header && let Some(line) = first_line {
            summary.push_str(&format!(
                " \u{b7} {}",
                t!("import-header-line", line = line)
            ));
        }
        let mut close = false;
        let response = egui::Frame::new()
            .inner_margin(Margin {
                left: SIDE as i8,
                right: 16,
                top: 20,
                bottom: 18,
            })
            .show(ui, |ui| {
                ui.set_width(WIDTH - SIDE - 16.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 14.0;
                    let (rect, _) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::hover());
                    ui.painter().rect_filled(rect, 10.0, p.accent_soft);
                    theme::paint_text(
                        ui.painter(),
                        rect.center(),
                        Align2::CENTER_CENTER,
                        icon::FILE_ARROW_DOWN,
                        theme::icons(20.0),
                        p.accent_ink,
                    );
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 3.0;
                        ui.label(
                            egui::RichText::new(t!("import-title", file = file))
                                .font(theme::semibold(17.0))
                                .color(p.text),
                        );
                        ui.label(egui::RichText::new(summary).size(13.0).color(p.muted));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let label = t!("cancel");
                        close = theme::glyph_button(ui, icon::X, 32.0, 16.0, true, false, &label)
                            .on_hover_text(t!("cancel"))
                            .clicked();
                    });
                });
            })
            .response;
        let rect = response.rect;
        ui.painter().line_segment(
            [rect.left_bottom(), rect.right_bottom()],
            Stroke::new(1.0, p.line),
        );
        close
    }

    /// Character set, starting line and quote character.
    fn fields(&mut self, ui: &mut egui::Ui) {
        let p = palette(ui);
        let width = (WIDTH - 2.0 * SIDE - 32.0) / 3.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 16.0;
            field(ui, width, &t!("import-encoding"), |ui| {
                let encoding = self.options.encoding;
                dropdown(ui, width, &encoding_name(encoding), None, |ui| {
                    for encoding in Encoding::ALL {
                        ui.selectable_value(
                            &mut self.options.encoding,
                            encoding,
                            encoding_name(encoding),
                        );
                    }
                });
            });
            field(ui, width, &t!("import-start-line"), |ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(width, 36.0), Sense::hover());
                ui.painter().rect(
                    rect,
                    8.0,
                    p.surface,
                    Stroke::new(1.0, p.line),
                    StrokeKind::Inside,
                );
                let minus = Rect::from_min_size(rect.min, vec2(36.0, 36.0));
                let plus =
                    Rect::from_min_size(pos2(rect.right() - 36.0, rect.top()), vec2(36.0, 36.0));
                for (button, glyph, delta) in [(minus, icon::MINUS, -1isize), (plus, icon::PLUS, 1)]
                {
                    let response = ui.interact(button, ui.id().with(glyph), Sense::click());
                    response.widget_info(|| {
                        let name = format!(
                            "{} {}",
                            t!("import-start-line"),
                            if delta < 0 { "−" } else { "+" }
                        );
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name)
                    });
                    if response.hovered() {
                        ui.painter().rect_filled(button.shrink(1.0), 7.0, p.raised);
                    }
                    theme::paint_text(
                        ui.painter(),
                        button.center(),
                        Align2::CENTER_CENTER,
                        glyph,
                        theme::icons(14.0),
                        if response.hovered() { p.text } else { p.muted },
                    );
                    if response.clicked() {
                        self.options.skip_lines =
                            self.options.skip_lines.saturating_add_signed(delta);
                    }
                }
                theme::paint_text(
                    ui.painter(),
                    rect.center(),
                    Align2::CENTER_CENTER,
                    (self.options.skip_lines + 1).to_string(),
                    theme::mono(13.0),
                    p.text,
                );
            });
            field(ui, width, &t!("import-quote"), |ui| {
                let quote_name = |q: Option<char>| match q {
                    Some('"') => t!("import-quote-double"),
                    Some('\'') => t!("import-quote-single"),
                    _ => t!("import-quote-none"),
                };
                let glyph = self.options.quote.map(String::from);
                dropdown(
                    ui,
                    width,
                    &quote_name(self.options.quote),
                    glyph.as_deref(),
                    |ui| {
                        for quote in [Some('"'), Some('\''), None] {
                            ui.selectable_value(&mut self.options.quote, quote, quote_name(quote));
                        }
                    },
                );
            });
        });
    }

    /// A workbook's sheet to open.
    fn sheet_field(&mut self, ui: &mut egui::Ui) {
        let width = (WIDTH - 2.0 * SIDE - 32.0) / 3.0;
        let Some((sheets, sheet)) = &mut self.workbook else {
            return;
        };
        let name = sheets
            .get(*sheet)
            .cloned()
            .unwrap_or_else(|| "\u{2026}".to_owned());
        field(ui, width * 2.0 + 16.0, &t!("import-sheet"), |ui| {
            dropdown(ui, width * 2.0 + 16.0, &name, None, |ui| {
                for (i, name) in sheets.iter().enumerate() {
                    ui.selectable_value(sheet, i, name);
                }
            });
        });
    }

    /// The delimiters, as chips that toggle, and one of one's own.
    fn separators(&mut self, ui: &mut egui::Ui) {
        let p = palette(ui);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            section_label(ui, &t!("import-separators"));
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                for delimiter in ["\t", ",", ";", " "] {
                    let on = self.options.delimiters.iter().any(|d| d == delimiter);
                    let label = delimiter_name(delimiter);
                    if toggle_chip(ui, on, &label, &delimiter_glyph(delimiter)).clicked() {
                        self.toggle_delimiter(delimiter);
                    }
                }
                // "Other", with its field.
                let label =
                    ui.painter()
                        .layout_no_wrap(t!("import-other"), theme::medium(13.0), p.muted);
                let size = vec2(12.0 + label.size().x + 8.0 + 40.0 + 4.0, 34.0);
                let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                let on = !self.other_text.is_empty();
                let (fill, ring) = if on {
                    (p.accent_soft, Stroke::new(1.5, p.ring))
                } else {
                    (p.surface, Stroke::new(1.0, p.line))
                };
                ui.painter().rect(rect, 8.0, fill, ring, StrokeKind::Inside);
                let text_color = if on { p.text } else { p.muted };
                theme::paint_galley(
                    ui.painter(),
                    pos2(rect.left() + 12.0, rect.center().y - label.size().y / 2.0),
                    label,
                    text_color,
                );
                let field_rect = Rect::from_min_size(
                    pos2(rect.right() - 44.0, rect.center().y - 13.0),
                    vec2(40.0, 26.0),
                );
                ui.painter().rect(
                    field_rect,
                    6.0,
                    p.bg,
                    Stroke::new(1.0, p.line),
                    StrokeKind::Inside,
                );
                let before = self.other_text.clone();
                let response = ui.put(
                    field_rect.shrink2(vec2(4.0, 0.0)),
                    egui::TextEdit::singleline(&mut self.other_text)
                        .frame(egui::Frame::NONE)
                        .font(theme::mono(13.0))
                        .horizontal_align(Align::Center)
                        .vertical_align(Align::Center)
                        .char_limit(8)
                        .hint_text("!#"),
                );
                self.other_focused = response.has_focus();
                let response = response.on_hover_text(t!("import-other-hint"));
                if response.changed() {
                    // The new text takes the old one's place.
                    let list = &mut self.options.delimiters;
                    let at = list.iter().position(|d| *d == before);
                    if let Some(at) = at {
                        list.remove(at);
                    }
                    let fresh = !self.other_text.is_empty() && !list.contains(&self.other_text);
                    if fresh {
                        list.insert(at.unwrap_or(list.len()), self.other_text.clone());
                    }
                }
            });
        });
    }

    /// Header row, merged delimiters and trimmed spaces: cards with a
    /// switch.
    fn switches(&mut self, ui: &mut egui::Ui) {
        let width = (WIDTH - 2.0 * SIDE - 16.0) / 3.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let header = self.header;
            if switch_card(
                ui,
                width,
                header,
                &t!("header-row"),
                &t!("import-header-hint"),
            )
            .clicked()
            {
                self.header = !header;
            }
            let merge = self.options.merge_delimiters;
            if self.workbook.is_some() {
                return;
            }
            if switch_card(
                ui,
                width,
                merge,
                &t!("import-merge"),
                &t!("import-merge-hint"),
            )
            .clicked()
            {
                self.options.merge_delimiters = !merge;
            }
            let trim = self.options.trim;
            if switch_card(ui, width, trim, &t!("import-trim"), &t!("import-trim-hint")).clicked() {
                self.options.trim = !trim;
            }
        });
    }

    fn preview(&mut self, ui: &mut egui::Ui) {
        let p = palette(ui);
        let header = self.header;
        let no_delimiter = self.options.delimiters.is_empty();
        let total = match self.result() {
            Some(Ok(imported)) => Some(imported.table.col_count()),
            _ => None,
        };
        let preview = self.preview_rows();
        let rows = &preview.rows;
        let lines = &preview.lines;
        let cols = rows
            .iter()
            .map(Vec::len)
            .max()
            .unwrap_or(1)
            .max(total.unwrap_or(0));
        let inner = WIDTH - 2.0 * SIDE;
        let fitting = ((inner - 2.0 - PREVIEW_NUMBER) / PREVIEW_COL).floor() as usize;
        let note = if no_delimiter {
            t!("import-no-separator")
        } else if cols > fitting {
            t!(
                "import-columns-shown",
                first = columns::label(0),
                last = columns::label(fitting - 1),
                count = cols
            )
        } else {
            String::new()
        };
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            ui.horizontal(|ui| {
                ui.set_width(inner);
                section_label(ui, &t!("import-preview"));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(egui::RichText::new(note).size(12.0).color(p.faint));
                });
            });
            egui::Frame::new()
                .fill(p.bg)
                .stroke(Stroke::new(1.0, p.line))
                .corner_radius(10)
                .show(ui, |ui| {
                    ui.set_width(inner - 2.0);
                    let height = PREVIEW_ROW * (PREVIEW_VISIBLE + 1.0);
                    egui::ScrollArea::both()
                        .max_height(height)
                        .min_scrolled_height(height)
                        .auto_shrink([false, false])
                        .show(ui, |ui| preview_table(ui, rows, lines, cols, header, p));
                });
        });
    }

    fn footer(&mut self, ui: &mut egui::Ui) -> Option<Outcome> {
        let p = palette(ui);
        let mut outcome = None;
        let response = egui::Frame::new()
            .fill(p.bg)
            .corner_radius(CornerRadius {
                sw: 14,
                se: 14,
                ..Default::default()
            })
            .inner_margin(Margin {
                left: SIDE as i8,
                right: 16,
                top: 14,
                bottom: 14,
            })
            .show(ui, |ui| {
                ui.set_width(WIDTH - SIDE - 16.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    // Bytes the encoding can't read come first: they would be
                    // lost.
                    let invalid = match self.result() {
                        Some(Ok(imported)) => imported.losses.invalid_bytes,
                        _ => self.preview_rows().invalid_bytes,
                    };
                    let too_large = matches!(self.result(), Some(Err(_)));
                    if invalid > 0 {
                        ui.label(
                            egui::RichText::new(icon::WARNING)
                                .font(theme::icons(15.0))
                                .color(p.hl_ink),
                        );
                        ui.label(
                            egui::RichText::new(t!("import-invalid", count = invalid))
                                .size(12.0)
                                .color(p.hl_ink),
                        );
                    } else if let Some(detected) = &self.detected {
                        ui.label(
                            egui::RichText::new(icon::INFO)
                                .font(theme::icons(15.0))
                                .color(p.faint),
                        );
                        ui.label(egui::RichText::new(detected).size(12.0).color(p.muted));
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let import = format!("{}  \u{21b5}", t!("import-open"));
                        let open = ui
                            .add_enabled_ui(!too_large, |ui| {
                                theme::primary_button(ui, &import, 0.0)
                            })
                            .inner;
                        if open.clicked() {
                            outcome = Some(Outcome::Open);
                        }
                        if theme::secondary_button(ui, &t!("cancel"), 0.0).clicked() {
                            outcome = Some(Outcome::Cancel);
                        }
                    });
                });
            })
            .response;
        let rect = response.rect;
        ui.painter().line_segment(
            [rect.left_top(), rect.right_top()],
            Stroke::new(1.0, p.line),
        );
        outcome
    }
}

/// A label above a control.
fn field(ui: &mut egui::Ui, width: f32, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.vertical(|ui| {
        ui.set_width(width);
        ui.spacing_mut().item_spacing.y = 6.0;
        section_label(ui, label);
        add(ui);
    });
}

fn section_label(ui: &mut egui::Ui, text: &str) {
    let p = palette(ui);
    ui.label(
        egui::RichText::new(text)
            .font(theme::medium(12.0))
            .color(p.muted),
    );
}

/// A bordered button showing a value, opening a menu.
fn dropdown(
    ui: &mut egui::Ui,
    width: f32,
    value: &str,
    glyph: Option<&str>,
    menu: impl FnOnce(&mut egui::Ui),
) {
    let p = palette(ui);
    let (rect, response) = ui.allocate_exact_size(vec2(width, 36.0), Sense::click());
    let fill = if response.hovered() {
        p.raised
    } else {
        p.surface
    };
    ui.painter().rect(
        rect,
        8.0,
        fill,
        Stroke::new(1.0, p.line),
        StrokeKind::Inside,
    );
    let galley = ui
        .painter()
        .layout_no_wrap(value.to_owned(), theme::regular(13.0), p.text);
    let x = rect.left() + 12.0;
    let value_width = galley.size().x;
    theme::paint_galley(
        ui.painter(),
        pos2(x, rect.center().y - galley.size().y / 2.0),
        galley,
        p.text,
    );
    if let Some(glyph) = glyph {
        theme::paint_text(
            ui.painter(),
            pos2(x + value_width + 6.0, rect.center().y),
            Align2::LEFT_CENTER,
            glyph,
            theme::mono(13.0),
            p.muted,
        );
    }
    theme::paint_text(
        ui.painter(),
        pos2(rect.right() - 12.0, rect.center().y),
        Align2::RIGHT_CENTER,
        icon::CARET_DOWN,
        theme::icons(14.0),
        p.muted,
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, value));
    theme::focus_ring(ui, &response, 8.0);
    egui::Popup::menu(&response).show(|ui| {
        ui.set_min_width(width - 12.0);
        menu(ui);
    });
}

/// A delimiter that can be on or off: a check box, its name and itself.
fn toggle_chip(ui: &mut egui::Ui, on: bool, label: &str, glyph: &str) -> egui::Response {
    let p = palette(ui);
    let text = ui
        .painter()
        .layout_no_wrap(label.to_owned(), theme::medium(13.0), p.text);
    let mark = ui
        .painter()
        .layout_no_wrap(glyph.to_owned(), theme::mono(12.0), p.faint);
    let size = vec2(
        10.0 + 16.0 + 8.0 + text.size().x + 8.0 + mark.size().x + 12.0,
        34.0,
    );
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let (fill, ring) = if on {
        (p.accent_soft, Stroke::new(1.5, p.ring))
    } else if response.hovered() {
        (p.raised, Stroke::new(1.0, p.line))
    } else {
        (p.surface, Stroke::new(1.0, p.line))
    };
    ui.painter().rect(rect, 8.0, fill, ring, StrokeKind::Inside);
    let check = Rect::from_min_size(
        pos2(rect.left() + 10.0, rect.center().y - 8.0),
        vec2(16.0, 16.0),
    );
    if on {
        ui.painter().rect_filled(check, 4.0, p.accent);
        theme::paint_text(
            ui.painter(),
            check.center(),
            Align2::CENTER_CENTER,
            icon::CHECK,
            theme::icons(12.0),
            p.on_accent,
        );
    } else {
        ui.painter()
            .rect_stroke(check, 4.0, Stroke::new(1.5, p.faint), StrokeKind::Inside);
    }
    let x = check.right() + 8.0;
    let text_width = text.size().x;
    theme::paint_galley(
        ui.painter(),
        pos2(x, rect.center().y - text.size().y / 2.0),
        text,
        p.text,
    );
    theme::paint_galley(
        ui.painter(),
        pos2(x + text_width + 8.0, rect.center().y - mark.size().y / 2.0),
        mark,
        p.faint,
    );
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, on, label));
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A card with a switch, a name and what it does.
fn switch_card(ui: &mut egui::Ui, width: f32, on: bool, label: &str, hint: &str) -> egui::Response {
    let p = palette(ui);
    let (rect, response) = ui.allocate_exact_size(vec2(width, 54.0), Sense::click());
    let fill = if response.hovered() {
        p.raised
    } else {
        p.surface
    };
    ui.painter().rect(
        rect,
        10.0,
        fill,
        Stroke::new(1.0, p.line),
        StrokeKind::Inside,
    );
    let track = Rect::from_min_size(
        pos2(rect.left() + 12.0, rect.center().y - 9.0),
        vec2(32.0, 18.0),
    );
    // Opaque tracks, so that the white knob stands out on both.
    let off = p.faint.lerp_to_gamma(p.surface, 0.45);
    let on_fill = if p.dark { p.accent } else { p.ring };
    ui.painter()
        .rect_filled(track, 9.0, if on { on_fill } else { off });
    let knob_x = if on {
        track.right() - 9.0
    } else {
        track.left() + 9.0
    };
    ui.painter().circle(
        pos2(knob_x, track.center().y),
        7.0,
        Color32::WHITE,
        Stroke::new(1.0, Color32::from_black_alpha(40)),
    );
    let x = track.right() + 10.0;
    let text_width = rect.right() - x - 8.0;
    let name = Rect::from_min_size(pos2(x, rect.top() + 9.0), vec2(text_width, 18.0));
    theme::truncated_text(ui, name, label, theme::medium(13.0), p.text, Align::Min);
    let explanation = Rect::from_min_size(pos2(x, rect.top() + 28.0), vec2(text_width, 16.0));
    theme::truncated_text(
        ui,
        explanation,
        hint,
        theme::regular(11.0),
        p.faint,
        Align::Min,
    );
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, on, label));
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The preview's grid: column letters, line numbers, the first rows. Only
/// what is scrolled into view is drawn.
fn preview_table(
    ui: &mut egui::Ui,
    rows: &[Vec<String>],
    lines: &[usize],
    cols: usize,
    header: bool,
    p: &Palette,
) {
    let size = vec2(
        PREVIEW_NUMBER + cols as f32 * PREVIEW_COL,
        PREVIEW_ROW * (rows.len() as f32 + 1.0),
    );
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let clip = ui.clip_rect();
    let line = |a, b, color| {
        ui.painter().line_segment([a, b], Stroke::new(1.0, color));
    };
    let col_x = |c: usize| rect.left() + PREVIEW_NUMBER + c as f32 * PREVIEW_COL;
    let visible_cols = {
        let from = ((clip.left() - col_x(0)) / PREVIEW_COL).floor().max(0.0) as usize;
        let to = ((clip.right() - col_x(0)) / PREVIEW_COL).ceil().max(0.0) as usize;
        from.min(cols)..to.min(cols)
    };

    for (r, row) in rows.iter().enumerate() {
        let top = rect.top() + PREVIEW_ROW * (r as f32 + 1.0);
        let row_rect = Rect::from_min_size(pos2(rect.left(), top), vec2(size.x, PREVIEW_ROW));
        if !clip.intersects(row_rect) {
            continue;
        }
        let head = header && r == 0;
        if head {
            ui.painter().rect_filled(row_rect, 0.0, p.accent_soft);
        } else if r % 2 == 1 {
            ui.painter().rect_filled(row_rect, 0.0, p.zebra);
        }
        line(
            row_rect.left_bottom(),
            row_rect.right_bottom(),
            if head { p.line } else { p.line_soft },
        );
        theme::paint_text(
            ui.painter(),
            pos2(rect.left() + PREVIEW_NUMBER - 10.0, row_rect.center().y),
            Align2::RIGHT_CENTER,
            lines.get(r).copied().unwrap_or(r + 1).to_string(),
            theme::mono(11.0),
            if head { p.accent_ink } else { p.faint },
        );
        for c in visible_cols.clone() {
            let Some(cell) = row.get(c) else { break };
            let cell_rect = Rect::from_min_size(
                pos2(col_x(c) + 10.0, top),
                vec2(PREVIEW_COL - 20.0, PREVIEW_ROW),
            );
            let font = if head {
                theme::semibold(12.0)
            } else {
                theme::regular(12.0)
            };
            let color = if c == 0 && !head { p.muted } else { p.text };
            theme::truncated_text(ui, cell_rect, &flat_text(cell), font, color, Align::Min);
        }
    }
    for c in visible_cols.clone() {
        line(
            pos2(col_x(c + 1), rect.top()),
            pos2(col_x(c + 1), rect.bottom()),
            p.line_soft,
        );
    }
    // Column letters, staying at the top while scrolling.
    let top = clip.top().max(rect.top());
    let head = Rect::from_min_size(pos2(rect.left(), top), vec2(size.x, PREVIEW_ROW));
    ui.painter().rect_filled(head, 0.0, p.chrome);
    line(head.left_bottom(), head.right_bottom(), p.line);
    for c in visible_cols {
        theme::paint_text(
            ui.painter(),
            pos2(col_x(c) + 10.0, head.center().y),
            Align2::LEFT_CENTER,
            columns::label(c),
            theme::mono_semibold(11.0),
            p.faint,
        );
        line(
            pos2(col_x(c + 1), head.top()),
            pos2(col_x(c + 1), head.bottom()),
            p.line_soft,
        );
    }
    line(
        pos2(rect.left() + PREVIEW_NUMBER, rect.top()),
        pos2(rect.left() + PREVIEW_NUMBER, rect.bottom()),
        p.line,
    );
}

/// Why a file can't be opened, for the user.
pub fn open_error_text(error: &OpenError) -> String {
    match error {
        OpenError::TooLarge(too_large) => t!(
            "import-too-large",
            rows = too_large.rows,
            cols = too_large.cols
        ),
        OpenError::Workbook(WorkbookError::TooLarge) => t!("import-workbook-too-large"),
        OpenError::Workbook(WorkbookError::Unreadable(why)) => {
            t!("import-workbook-unreadable", error = why.clone())
        }
        OpenError::Workbook(WorkbookError::Crashed) => t!("import-workbook-crashed"),
    }
}
