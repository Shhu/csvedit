//! The toolbar, the cell bar under it and the status bar.

use eframe::egui::{
    self, Align, Align2, Color32, FontId, Key, Layout, Rect, Sense, Stroke, StrokeKind, pos2,
    text::LayoutJob, vec2,
};
use egui_phosphor::regular as icon;

use super::*;
use crate::i18n::t;
use crate::theme::{self, palette};

/// The toolbar's icons that Phosphor lacks, drawn like the others.
#[derive(Clone, Copy)]
enum TableIcon {
    InsertRow,
    DeleteRow,
    InsertCol,
    DeleteCol,
}

enum Tool {
    Glyph(&'static str),
    Table(TableIcon),
}

impl CsvApp {
    pub(super) fn toolbar(&mut self, ui: &mut egui::Ui) {
        let groups: [&[(Tool, Command, String)]; 4] = [
            &[
                (
                    Tool::Glyph(icon::FILE_PLUS),
                    Command::New,
                    t!("command-new"),
                ),
                (
                    Tool::Glyph(icon::FOLDER_OPEN),
                    Command::Open,
                    t!("command-open"),
                ),
                (
                    Tool::Glyph(icon::FLOPPY_DISK),
                    Command::Save,
                    t!("command-save"),
                ),
            ],
            &[
                (
                    Tool::Glyph(icon::ARROW_U_UP_LEFT),
                    Command::Undo,
                    t!("command-undo"),
                ),
                (
                    Tool::Glyph(icon::ARROW_U_UP_RIGHT),
                    Command::Redo,
                    t!("command-redo"),
                ),
            ],
            &[
                (
                    Tool::Table(TableIcon::InsertRow),
                    Command::InsertRowBelow,
                    t!("command-insert-row-below"),
                ),
                (
                    Tool::Table(TableIcon::DeleteRow),
                    Command::DeleteRow,
                    t!("command-delete-row"),
                ),
                (
                    Tool::Table(TableIcon::InsertCol),
                    Command::InsertColRight,
                    t!("command-insert-col-right"),
                ),
                (
                    Tool::Table(TableIcon::DeleteCol),
                    Command::DeleteCol,
                    t!("command-delete-col"),
                ),
            ],
            &[
                (
                    Tool::Glyph(icon::SORT_ASCENDING),
                    Command::SortAscending,
                    t!("command-sort-ascending"),
                ),
                (
                    Tool::Glyph(icon::SORT_DESCENDING),
                    Command::SortDescending,
                    t!("command-sort-descending"),
                ),
            ],
        ];
        let shortcuts = |command| match command {
            Command::New => Some(SHORTCUT_NEW),
            Command::Open => Some(SHORTCUT_OPEN),
            Command::Save => Some(SHORTCUT_SAVE),
            Command::Undo => Some(SHORTCUT_UNDO),
            Command::Redo => Some(SHORTCUT_REDO),
            Command::InsertRowBelow => Some(SHORTCUT_INSERT_ROW),
            Command::DeleteRow => Some(SHORTCUT_DELETE_ROW),
            _ => None,
        };
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            for (i, group) in groups.iter().enumerate() {
                if i > 0 {
                    ui.add_space(8.0);
                    theme::vertical_separator(ui, 22.0);
                    ui.add_space(8.0);
                }
                ui.spacing_mut().item_spacing.x = 2.0;
                for (tool, command, tip) in group.iter() {
                    let enabled = self.can_run(*command);
                    let response = match tool {
                        Tool::Glyph(glyph) => {
                            theme::glyph_button(ui, glyph, 34.0, 18.0, enabled, false)
                        }
                        Tool::Table(kind) => {
                            let response = theme::icon_button(ui, 34.0, enabled, false);
                            let color = theme::icon_color(ui, &response, enabled, false);
                            paint_table_icon(ui, response.rect, *kind, color);
                            response
                        }
                    };
                    let tip = match shortcuts(*command) {
                        Some(s) => format!("{tip} ({})", ui.ctx().format_shortcut(&s)),
                        None => tip.clone(),
                    };
                    if response.on_hover_text(tip).clicked() {
                        self.run(*command);
                    }
                }
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let tip = format!(
                    "{} ({})",
                    t!("command-inspector"),
                    ui.ctx().format_shortcut(&SHORTCUT_INSPECTOR)
                );
                let toggle =
                    theme::glyph_button(ui, icon::SIDEBAR_SIMPLE, 34.0, 18.0, true, self.inspector);
                if toggle.on_hover_text(tip).clicked() {
                    self.inspector = !self.inspector;
                }
                self.columns_chooser(ui);
                let width = ui.available_width().min(320.0);
                if width > 120.0 {
                    self.search_field(ui, width);
                }
            });
        });
    }

    /// The search field: a magnifying glass, the text, the number of
    /// matches and the shortcut.
    fn search_field(&mut self, ui: &mut egui::Ui, width: f32) {
        let p = palette(ui);
        let (rect, _) = ui.allocate_exact_size(vec2(width, 34.0), Sense::hover());
        let id = ui.id().with("find");
        let focused = ui.memory(|m| m.has_focus(id));
        let border = if focused { p.ring } else { p.line };
        ui.painter().rect(
            rect,
            9.0,
            p.surface,
            Stroke::new(1.0, border),
            StrokeKind::Inside,
        );
        let y = rect.center().y;
        theme::paint_text(
            ui.painter(),
            pos2(rect.left() + 10.0, y),
            Align2::LEFT_CENTER,
            icon::MAGNIFYING_GLASS,
            theme::icons(16.0),
            p.faint,
        );

        // From the right: the shortcut, then the number of matches.
        let mut right = rect.right() - 6.0;
        let key = ui.painter().layout_no_wrap(
            ui.ctx().format_shortcut(&SHORTCUT_FIND).replace('+', " "),
            theme::mono(11.0),
            p.faint,
        );
        let key_rect = Rect::from_min_max(
            pos2(right - key.size().x - 12.0, y - 10.0),
            pos2(right, y + 10.0),
        );
        ui.painter()
            .rect_stroke(key_rect, 5.0, Stroke::new(1.0, p.line), StrokeKind::Inside);
        theme::paint_galley(
            ui.painter(),
            key_rect.center() - key.size() / 2.0,
            key,
            p.faint,
        );
        right = key_rect.left() - 8.0;
        if !self.needle().is_empty() {
            let count = self.matches().len();
            let chip = ui.painter().layout_no_wrap(
                t!("find-count", count = count),
                theme::semibold(12.0),
                p.hl_ink,
            );
            let chip_rect = Rect::from_min_max(
                pos2(right - chip.size().x - 16.0, y - 11.0),
                pos2(right, y + 11.0),
            );
            ui.painter().rect_filled(chip_rect, 6.0, p.hl);
            theme::paint_galley(
                ui.painter(),
                chip_rect.center() - chip.size() / 2.0,
                chip,
                p.hl_ink,
            );
            right = chip_rect.left() - 6.0;
        }

        let text_rect = Rect::from_min_max(
            pos2(rect.left() + 34.0, rect.top()),
            pos2(right, rect.bottom()),
        );
        let response = ui.put(
            text_rect,
            egui::TextEdit::singleline(&mut self.find)
                .id(id)
                .frame(egui::Frame::NONE)
                .margin(vec2(0.0, 8.0))
                .vertical_align(Align::Center)
                .hint_text(t!("find-placeholder")),
        );
        if self.focus_find {
            response.request_focus();
            self.focus_find = false;
        }
        if response.lost_focus() {
            let (enter, shift, escape) = ui.input(|i| {
                (
                    i.key_pressed(Key::Enter),
                    i.modifiers.shift,
                    i.key_pressed(Key::Escape),
                )
            });
            if escape {
                self.find.clear();
            } else if enter {
                self.next_match(shift);
                response.request_focus();
            }
        }
    }

    pub(super) fn cell_bar(&mut self, ui: &mut egui::Ui) {
        let p = palette(ui);
        let (row, col) = self.selected;
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            reference_chip(ui, &columns::reference(row, col));
            let title = self.column_title(col).to_owned();
            if !title.is_empty() {
                let galley =
                    ui.painter()
                        .layout_no_wrap(flat_text(&title), theme::regular(12.0), p.faint);
                let width = galley.size().x.min(220.0);
                let (rect, response) = ui.allocate_exact_size(vec2(width, 20.0), Sense::hover());
                if theme::truncated_text(
                    ui,
                    rect,
                    &flat_text(&title),
                    theme::regular(12.0),
                    p.faint,
                    Align::Min,
                ) {
                    response.on_hover_text(title);
                }
            }
            theme::vertical_separator(ui, 20.0);
            let rect = ui.available_rect_before_wrap();
            let rect = Rect::from_min_max(
                pos2(rect.left(), rect.center().y - 10.0),
                pos2(rect.right(), rect.center().y + 10.0),
            );
            let text = flat_text(self.table.cell(row, col));
            let response = ui.allocate_rect(rect, Sense::click());
            theme::truncated_text(ui, rect, &text, theme::regular(13.0), p.text, Align::Min);
            if response.double_clicked() {
                self.start_editing(self.table.cell(row, col).to_owned(), self.inspector);
            }
        });
    }

    pub(super) fn status_bar(&mut self, ui: &mut egui::Ui) {
        let p = palette(ui);
        let (row, col) = self.selected;
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            ui.label(
                egui::RichText::new(columns::reference(row, col))
                    .font(theme::mono_semibold(12.0))
                    .color(p.accent_ink),
            );
            let small = |text: String| egui::RichText::new(text).size(12.0).color(p.muted);
            ui.label(small(t!(
                "table-size",
                rows = self.table.row_count(),
                cols = self.table.col_count()
            )));
            theme::dot(ui);
            ui.label(small(t!("selection-one")));
            if !self.needle().is_empty() {
                let count = self.matches().len();
                theme::dot(ui);
                ui.label(small(t!(
                    "find-results",
                    count = count,
                    text = self.find.trim().to_owned()
                )));
            }
            if let Some(shown) = &self.shown {
                theme::dot(ui);
                ui.label(small(t!(
                    "columns-shown",
                    shown = shown.len(),
                    count = self.table.col_count()
                )));
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let before = self.format.clone();
                let crlf = self.format.crlf;
                selector(ui, None, if crlf { "CRLF" } else { "LF" }, None, |ui| {
                    ui.selectable_value(&mut self.format.crlf, false, "LF (Linux, macOS)");
                    ui.selectable_value(&mut self.format.crlf, true, "CRLF (Windows)");
                });
                theme::vertical_separator(ui, 14.0);
                let encoding = self.format.encoding;
                selector(ui, None, &encoding_name(encoding), None, |ui| {
                    for encoding in Encoding::ALL {
                        ui.selectable_value(
                            &mut self.format.encoding,
                            encoding,
                            encoding_name(encoding),
                        );
                    }
                });
                theme::vertical_separator(ui, 14.0);
                let custom = !table::DELIMITERS.contains(&self.format.delimiter.as_str());
                if custom {
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.custom_delimiter)
                            .font(theme::mono(12.0))
                            .desired_width(36.0)
                            .char_limit(8),
                    );
                    if response.changed() && !self.custom_delimiter.is_empty() {
                        self.format.delimiter = self.custom_delimiter.clone();
                    } else if !response.has_focus() {
                        self.custom_delimiter = self.format.delimiter.clone();
                    }
                }
                let delimiter = self.format.delimiter.clone();
                let glyph = (!custom).then(|| delimiter_glyph(&delimiter));
                selector(
                    ui,
                    Some(&t!("delimiter")),
                    &delimiter_name(&delimiter),
                    glyph.as_deref(),
                    |ui| {
                        for delimiter in table::DELIMITERS {
                            ui.selectable_value(
                                &mut self.format.delimiter,
                                delimiter.to_owned(),
                                format!(
                                    "{}  {}",
                                    delimiter_name(delimiter),
                                    delimiter_glyph(delimiter)
                                ),
                            );
                        }
                        if ui.selectable_label(custom, t!("delimiter-other")).clicked() && !custom {
                            if self.custom_delimiter.is_empty()
                                || table::DELIMITERS.contains(&self.custom_delimiter.as_str())
                            {
                                self.custom_delimiter = "#".to_owned();
                            }
                            self.format.delimiter = self.custom_delimiter.clone();
                        }
                    },
                );
                if self.format != before {
                    self.format_changed = true;
                }
            });
        });
    }
}

/// "D4" in the accent color, on its soft fill.
pub(super) fn reference_chip(ui: &mut egui::Ui, text: &str) {
    let p = palette(ui);
    let galley =
        ui.painter()
            .layout_no_wrap(text.to_owned(), theme::mono_semibold(12.0), p.accent_ink);
    let size = vec2((galley.size().x + 16.0).max(44.0), 26.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(rect, 6.0, p.accent_soft);
    theme::paint_galley(
        ui.painter(),
        rect.center() - galley.size() / 2.0,
        galley,
        p.accent_ink,
    );
}

/// A discreet status bar button opening a menu: a label, the value, maybe
/// a glyph, and a chevron.
fn selector(
    ui: &mut egui::Ui,
    label: Option<&str>,
    value: &str,
    glyph: Option<&str>,
    menu: impl FnOnce(&mut egui::Ui),
) {
    let p = palette(ui);
    let mut job = LayoutJob::default();
    let format = |font: FontId, color: Color32| egui::TextFormat {
        font_id: font,
        color,
        valign: Align::Center,
        ..Default::default()
    };
    if let Some(label) = label {
        job.append(label, 0.0, format(theme::regular(12.0), p.faint));
    }
    job.append(
        value,
        if label.is_some() { 6.0 } else { 0.0 },
        format(theme::medium(12.0), p.text),
    );
    if let Some(glyph) = glyph {
        job.append(glyph, 6.0, format(theme::mono(12.0), p.text));
    }
    job.append(icon::CARET_DOWN, 6.0, format(theme::icons(12.0), p.muted));
    let galley = ui.painter().layout_job(job);
    let size = vec2(galley.size().x + 16.0, 24.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response));
    if response.hovered() || open {
        ui.painter().rect_filled(rect, 6.0, p.raised);
    }
    theme::paint_galley(
        ui.painter(),
        rect.center() - galley.size() / 2.0,
        galley,
        p.text,
    );
    egui::Popup::menu(&response).show(|ui| {
        ui.set_min_width(180.0);
        menu(ui);
    });
}

/// Draws a toolbar icon of rows or columns, in the 24-unit grid of the
/// Phosphor-like icons, 18 px wide.
fn paint_table_icon(ui: &egui::Ui, button: Rect, kind: TableIcon, color: Color32) {
    let scale = 18.0 / 24.0;
    let origin = button.center() - vec2(9.0, 9.0);
    let at = |x: f32, y: f32| origin + vec2(x, y) * scale;
    let stroke = Stroke::new(1.5, color);
    let painter = ui.painter();
    let (rect, lines): (Rect, [[(f32, f32); 2]; 2]) = match kind {
        TableIcon::InsertRow | TableIcon::DeleteRow => (
            Rect::from_min_max(at(3.0, 4.0), at(21.0, 10.0)),
            [[(3.0, 15.0), (10.0, 15.0)], [(3.0, 19.0), (10.0, 19.0)]],
        ),
        TableIcon::InsertCol | TableIcon::DeleteCol => (
            Rect::from_min_max(at(4.0, 3.0), at(10.0, 21.0)),
            [[(14.0, 3.0), (14.0, 10.0)], [(18.0, 3.0), (18.0, 10.0)]],
        ),
    };
    painter.rect_stroke(rect, 1.5 * scale, stroke, StrokeKind::Middle);
    for [(x0, y0), (x1, y1)] in lines {
        painter.line_segment([at(x0, y0), at(x1, y1)], stroke);
    }
    painter.line_segment([at(14.0, 17.0), at(20.0, 17.0)], stroke);
    if matches!(kind, TableIcon::InsertRow | TableIcon::InsertCol) {
        painter.line_segment([at(17.0, 14.0), at(17.0, 20.0)], stroke);
    }
}

impl CsvApp {
    /// The toolbar button choosing the columns shown: by typing their
    /// numbers or letters ("3, 157"), or ticking them in a list.
    fn columns_chooser(&mut self, ui: &mut egui::Ui) {
        let p = palette(ui);
        let total = self.table.col_count();
        let count = self.shown.as_ref().map(Vec::len);
        let response = match count {
            Some(n) => {
                let text = format!("{}  {n} / {total}", icon::COLUMNS);
                let galley = ui.painter().layout_job(theme::with_icons(
                    &text,
                    theme::semibold(12.0),
                    p.accent_ink,
                ));
                let size = vec2(galley.size().x + 20.0, 34.0);
                let (rect, response) = ui.allocate_exact_size(size, Sense::click());
                ui.painter().rect_filled(rect, 8.0, p.accent_soft);
                theme::paint_galley(
                    ui.painter(),
                    rect.center() - galley.size() / 2.0,
                    galley,
                    p.accent_ink,
                );
                response
            }
            None => theme::glyph_button(ui, icon::COLUMNS, 34.0, 18.0, true, false),
        };
        let tip = format!(
            "{} ({})",
            t!("columns-choose"),
            ui.ctx().format_shortcut(&SHORTCUT_COLUMNS)
        );
        let response = response.on_hover_text(tip);
        let popup_id = egui::Popup::default_response_id(&response);
        let text_id = ui.id().with("columns-text");
        if self.open_columns {
            self.open_columns = false;
            egui::Popup::open_id(ui.ctx(), popup_id);
            ui.memory_mut(|m| m.request_focus(text_id));
        } else if response.clicked() {
            ui.memory_mut(|m| m.request_focus(text_id));
        }
        let frame = egui::Frame::popup(ui.style()).inner_margin(12);
        egui::Popup::from_toggle_button_response(&response)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .frame(frame)
            .width(340.0)
            .show(|ui| {
                // Square check boxes, not round ones.
                let widgets = &mut ui.visuals_mut().widgets;
                for state in [
                    &mut widgets.inactive,
                    &mut widgets.hovered,
                    &mut widgets.active,
                ] {
                    state.corner_radius = 4.into();
                }
                ui.spacing_mut().item_spacing.y = 6.0;
                self.columns_popup(ui, text_id);
            });
    }

    fn columns_popup(&mut self, ui: &mut egui::Ui, text_id: egui::Id) {
        let p = palette(ui);
        let total = self.table.col_count();
        ui.set_width(340.0);
        ui.label(
            egui::RichText::new(t!("columns-title"))
                .font(theme::semibold(13.0))
                .color(p.text),
        );
        ui.label(
            egui::RichText::new(t!("columns-help"))
                .size(12.0)
                .color(p.muted),
        );
        let response = ui.add(
            egui::TextEdit::singleline(&mut self.columns_text)
                .id(text_id)
                .font(theme::mono(13.0))
                .hint_text(t!("columns-example"))
                .desired_width(f32::INFINITY),
        );
        if response.changed() {
            match columns::parse_list(&self.columns_text, total) {
                Ok(cols) => {
                    self.columns_error = None;
                    self.show_columns(cols);
                }
                Err(item) => self.columns_error = Some(item),
            }
        } else if !response.has_focus() {
            self.columns_text = self
                .shown
                .as_deref()
                .map(columns::format_list)
                .unwrap_or_default();
            self.columns_error = None;
        }
        if let Some(item) = &self.columns_error {
            ui.label(
                egui::RichText::new(t!("columns-invalid", item = item.clone()))
                    .size(12.0)
                    .color(p.hl_ink),
            );
        }
        ui.separator();

        ui.add(
            egui::TextEdit::singleline(&mut self.columns_filter)
                .hint_text(format!(
                    "{}  {}",
                    icon::MAGNIFYING_GLASS,
                    t!("columns-filter")
                ))
                .desired_width(f32::INFINITY),
        );
        let filter = self.columns_filter.trim().to_lowercase();
        let header_row = self.body_start() == 1;
        let listed: Vec<usize> = (0..total)
            .filter(|&c| {
                filter.is_empty()
                    || columns::label(c).to_lowercase() == filter
                    || (header_row && self.table.cell(0, c).to_lowercase().contains(&filter))
            })
            .collect();
        let shown = self.shown_cols();
        let mut toggled = None;
        egui::ScrollArea::vertical()
            .max_height(320.0)
            .auto_shrink([false, true])
            .show_rows(ui, 24.0, listed.len(), |ui, range| {
                for &col in &listed[range] {
                    let mut on = shown.contains(&col);
                    let name = if header_row {
                        flat_text(self.table.cell(0, col))
                    } else {
                        String::new()
                    };
                    let mut job =
                        theme::with_icons(&columns::label(col), theme::mono(12.0), p.faint);
                    job.append(
                        &name,
                        10.0,
                        egui::TextFormat {
                            font_id: theme::regular(13.0),
                            color: p.text,
                            valign: Align::Center,
                            ..Default::default()
                        },
                    );
                    job.wrap = egui::text::TextWrapping::truncate_at_width(280.0);
                    if ui.checkbox(&mut on, job).changed() {
                        toggled = Some((col, on));
                    }
                }
            });
        if let Some((col, on)) = toggled {
            let mut cols = shown;
            if on {
                let at = cols.iter().position(|&c| c > col).unwrap_or(cols.len());
                cols.insert(at, col);
            } else {
                cols.retain(|&c| c != col);
            }
            // Unticking the last one shows them all again.
            self.show_columns(cols);
        }
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button(t!("columns-show-all")).clicked() {
                self.show_columns(Vec::new());
            }
            ui.label(
                egui::RichText::new(t!(
                    "columns-shown",
                    shown = self.shown_cols().len(),
                    count = total
                ))
                .size(12.0)
                .color(p.faint),
            );
        });
    }
}
