//! The window shown when opening a file: how to read it (encoding,
//! delimiters, quotes…), with a preview of the result.

use std::path::PathBuf;

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, Key, Layout, Margin, Rect, Sense, Stroke,
    StrokeKind, pos2, vec2,
};
use egui_phosphor::regular as icon;

use crate::app::{encoding_name, flat_text};
use crate::columns;
use crate::i18n::t;
use crate::table::{self, Encoding, Format, ImportOptions, Table};
use crate::theme::{self, Palette, palette};

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

pub enum Outcome {
    Open,
    Cancel,
}

pub struct ImportDialog {
    pub path: PathBuf,
    bytes: Vec<u8>,
    options: ImportOptions,
    /// The first row holds the column names.
    pub header: bool,
    tab: bool,
    comma: bool,
    semicolon: bool,
    space: bool,
    /// A delimiter of one's own, one or more characters (`!#`).
    other_text: String,
    /// What was guessed when the file was opened, for the footer.
    detected: Option<String>,
    /// The file read with `options`; `None` when they changed since.
    parsed: Option<(Table, Format)>,
}

impl ImportDialog {
    pub fn new(path: PathBuf, bytes: Vec<u8>, header: bool) -> Self {
        let options = ImportOptions::detect(&bytes);
        let has = |d: &str| options.delimiters.iter().any(|o| o == d);
        let other_text = options
            .delimiters
            .iter()
            .find(|d| !["\t", ",", ";", " "].contains(&d.as_str()))
            .cloned()
            .unwrap_or_default();
        // Poliris files have no header row.
        let header = header && !has("!#");
        let parsed = table::import(&bytes, &options);
        let format = &parsed.1;
        let detected = (!bytes.is_empty()).then(|| {
            let delimiter = match format.delimiter.as_str() {
                "," => t!("delimiter-comma"),
                ";" => t!("delimiter-semicolon"),
                "\t" => t!("delimiter-tab"),
                "|" => t!("delimiter-pipe"),
                other => other.to_owned(),
            };
            t!(
                "import-detected",
                delimiter = delimiter.to_lowercase(),
                encoding = encoding_name(format.encoding),
                endings = if format.crlf { "CRLF" } else { "LF" }
            )
        });
        Self {
            path,
            header,
            tab: has("\t"),
            comma: has(","),
            semicolon: has(";"),
            space: has(" "),
            other_text,
            detected,
            options,
            bytes,
            parsed: Some(parsed),
        }
    }

    /// The table read with the chosen options.
    pub fn into_table(mut self) -> (Table, Format) {
        self.parse();
        self.parsed.expect("parsed")
    }

    fn parse(&mut self) -> &(Table, Format) {
        self.parsed
            .get_or_insert_with(|| table::import(&self.bytes, &self.options))
    }

    fn update_delimiters(&mut self) {
        let mut delimiters = Vec::new();
        for (on, d) in [
            (self.tab, "\t"),
            (self.comma, ","),
            (self.semicolon, ";"),
            (self.space, " "),
        ] {
            if on {
                delimiters.push(d.to_owned());
            }
        }
        if !self.other_text.is_empty() && !delimiters.contains(&self.other_text) {
            delimiters.push(self.other_text.clone());
        }
        self.options.delimiters = delimiters;
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Option<Outcome> {
        let mut outcome = None;
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
                let before = self.options.clone();
                if self.title(ui) {
                    outcome = Some(Outcome::Cancel);
                }
                egui::Frame::new()
                    .inner_margin(Margin::symmetric(SIDE as i8, 20))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 20.0;
                        self.fields(ui);
                        self.separators(ui);
                        self.switches(ui);
                        self.preview(ui);
                    });
                if self.options != before {
                    self.parsed = None;
                }
                if let Some(o) = self.footer(ui) {
                    outcome = Some(o);
                }
            });
        if modal.should_close() && outcome.is_none() {
            outcome = Some(Outcome::Cancel);
        }
        if outcome.is_none()
            && !ctx.egui_wants_keyboard_input()
            && ctx.input(|i| i.key_pressed(Key::Enter))
        {
            outcome = Some(Outcome::Open);
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
        let skip = self.options.skip_lines;
        let (table, _) = self.parse();
        let rows = table.row_count() - usize::from(header && table.row_count() > 1);
        let cols = table.col_count();
        let mut summary = format!(
            "{} \u{b7} {}",
            t!("import-data-rows", count = rows),
            t!("import-columns", count = cols)
        );
        if header {
            summary.push_str(&format!(
                " \u{b7} {}",
                t!("import-header-line", line = skip + 1)
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
                        close = theme::glyph_button(ui, icon::X, 32.0, 16.0, true, false)
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

    /// The delimiters, as chips that toggle, and one of one's own.
    fn separators(&mut self, ui: &mut egui::Ui) {
        let p = palette(ui);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            section_label(ui, &t!("import-separators"));
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let mut changed = false;
                for (on, label, glyph) in [
                    (&mut self.tab, t!("delimiter-tab"), "\u{21e5}"),
                    (&mut self.comma, t!("delimiter-comma"), ","),
                    (&mut self.semicolon, t!("delimiter-semicolon"), ";"),
                    (&mut self.space, t!("delimiter-space"), "\u{2423}"),
                ] {
                    if toggle_chip(ui, *on, &label, glyph).clicked() {
                        *on = !*on;
                        changed = true;
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
                let response = response.on_hover_text(t!("import-other-hint"));
                if response.changed() || changed {
                    self.update_delimiters();
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
        let skip = self.options.skip_lines;
        let (table, _) = self.parse();
        let cols = table.col_count();
        let rows = &table.rows()[..table.row_count().min(PREVIEW_ROWS)];
        let inner = WIDTH - 2.0 * SIDE;
        let fitting = ((inner - 2.0 - PREVIEW_NUMBER) / PREVIEW_COL).floor() as usize;
        let note = if no_delimiter {
            t!("import-no-separator")
        } else if cols > fitting {
            t!(
                "import-columns-shown",
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
                        .show(ui, |ui| preview_table(ui, rows, cols, header, skip, p));
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
                    if let Some(detected) = &self.detected {
                        ui.label(
                            egui::RichText::new(icon::INFO)
                                .font(theme::icons(15.0))
                                .color(p.faint),
                        );
                        ui.label(egui::RichText::new(detected).size(12.0).color(p.muted));
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let import = format!("{}  \u{21b5}", t!("import-open"));
                        if theme::primary_button(ui, &import, 0.0).clicked() {
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
    let off = p.faint.gamma_multiply(0.45);
    ui.painter()
        .rect_filled(track, 9.0, if on { p.accent } else { off });
    let knob_x = if on {
        track.right() - 9.0
    } else {
        track.left() + 9.0
    };
    ui.painter()
        .circle_filled(pos2(knob_x, track.center().y), 7.0, Color32::WHITE);
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
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The preview's grid: column letters, line numbers, the first rows. Only
/// what is scrolled into view is drawn.
fn preview_table(
    ui: &mut egui::Ui,
    rows: &[Vec<String>],
    cols: usize,
    header: bool,
    skip: usize,
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
            (skip + r + 1).to_string(),
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
