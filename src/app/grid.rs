//! The grid of cells: column headers, row numbers, the cells, and a last
//! row that adds one.

use eframe::egui::{self, Align, Align2, Key, Rect, Sense, Stroke, StrokeKind, Vec2, pos2, vec2};
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular as icon;

use super::*;
use crate::i18n::t;
use crate::theme::{self, palette};

const NUMBER_WIDTH: f32 = 56.0;
const HEADER_HEIGHT: f32 = 40.0;
/// Text inside cells, from their left edge.
const PADDING: f32 = 12.0;
/// Characters of a cell drawn in the grid and its tooltip.
const SHOWN_CHARS: usize = 1000;
/// Frames spent bringing the selected cell into view before giving up.
const SCROLL_TRIES: u8 = 3;

impl CsvApp {
    pub(super) fn grid(&mut self, ui: &mut egui::Ui) {
        let p = palette(ui);
        ui.painter().rect_filled(ui.max_rect(), 0.0, p.bg);
        let shown = self.shown_cols();
        let start = self.body_start();
        let rows = self.table.row_count() - start;
        let row_height = self.density.row_height();
        // Looked up by the cells to light up; computed once here.
        self.matches();
        // A selection made while the grid draws is scrolled to next frame.
        let scrolling = std::mem::take(&mut self.scroll_to_selection);
        self.scrolled = false;
        self.editor_drawn = false;

        // Cells touch; the resize handles draw the column lines.
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        ui.visuals_mut().widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.line_soft);
        // The table only scrolls up and down: sideways is this area's job.
        egui::ScrollArea::horizontal()
            .id_salt("grid-across")
            .auto_shrink(false)
            .show(ui, |ui| {
                let mut builder = TableBuilder::new(ui)
                    .id_salt("grid")
                    .striped(true)
                    .resizable(true)
                    .auto_shrink(false)
                    .cell_layout(egui::Layout::left_to_right(Align::Center))
                    .column(Column::exact(NUMBER_WIDTH));
                for &col in &shown {
                    let width = if col == 0 { 210.0 } else { 200.0 };
                    builder = builder.column(Column::initial(width).at_least(48.0).clip(true));
                }
                if scrolling && self.selected.0 >= start {
                    builder = builder.scroll_to_row(self.selected.0 - start, None);
                }
                builder
                    .header(HEADER_HEIGHT, |mut header| {
                        header.col(number_header);
                        for &col in &shown {
                            header.col(|ui| self.header_cell(ui, col, scrolling));
                        }
                    })
                    .body(|body| {
                        body.rows(row_height, rows + 1, |mut table_row| {
                            let index = table_row.index();
                            if index == rows {
                                self.new_row(&mut table_row, shown.len());
                                return;
                            }
                            let row = start + index;
                            table_row.col(|ui| self.row_number(ui, row));
                            for &col in &shown {
                                table_row.col(|ui| self.cell(ui, row, col, scrolling));
                            }
                        });
                    });
            });
        // Not reached yet (the row was off screen): try again next frame.
        if scrolling && !self.scrolled && self.scroll_tries < SCROLL_TRIES {
            self.scroll_tries += 1;
            self.scroll_to_selection = true;
        } else if !self.scroll_to_selection {
            self.scroll_tries = 0;
        }
        // Text typed in a cell scrolled out of sight is kept, not lost
        // with the keyboard focus.
        if self.editing.is_some() && !self.edit_in_inspector && !self.editor_drawn && !scrolling {
            self.commit_editing();
        }
    }

    fn row_number(&self, ui: &mut egui::Ui, row: usize) {
        if row >= self.table.row_count() {
            return;
        }
        let p = palette(ui);
        let rect = ui.max_rect();
        let selected = self.selected.0 == row;
        if selected {
            ui.painter().rect_filled(rect, 0.0, p.accent_soft);
        }
        ui.painter().line_segment(
            [rect.right_top(), rect.right_bottom()],
            Stroke::new(1.0, p.line),
        );
        bottom_line(ui, rect, p.line_soft);
        theme::paint_text(
            ui.painter(),
            pos2(rect.right() - PADDING, rect.center().y),
            Align2::RIGHT_CENTER,
            (row + 1).to_string(),
            theme::mono(12.0),
            if selected { p.accent_ink } else { p.faint },
        );
    }

    /// A column's header: its name in the chosen naming, its title (the
    /// header row) and a menu.
    fn header_cell(&mut self, ui: &mut egui::Ui, col: usize, scrolling: bool) {
        if col >= self.table.col_count() {
            return;
        }
        let p = palette(ui);
        let rect = ui.max_rect();
        let header_row = self.body_start() == 1;
        if scrolling && self.selected == (0, col) && header_row {
            ui.scroll_to_rect(rect, None);
            self.scrolled = true;
        }
        if !ui.is_rect_visible(rect) {
            return;
        }
        let active = self.selected.1 == col;
        // The header cell itself selected shows as such; a column merely
        // holding the selection leaves a match lit up.
        let hit = header_row && self.is_match(0, col);
        let fill = if active && (!hit || self.selected == (0, col)) {
            p.accent_soft
        } else if hit {
            p.hl
        } else {
            p.chrome
        };
        ui.painter().rect_filled(rect, 0.0, p.chrome);
        ui.painter().rect_filled(rect, 0.0, fill);
        bottom_line(ui, rect, p.line);
        if active {
            let line = Rect::from_min_max(pos2(rect.left(), rect.bottom() - 2.0), rect.max);
            ui.painter().rect_filled(line, 0.0, p.ring);
        }

        if header_row
            && self.selected == (0, col)
            && self.editing.is_some()
            && !self.edit_in_inspector
        {
            self.cell_editor(ui, 0, col);
            return;
        }

        let response = ui.interact(rect, ui.id().with(("header", col)), Sense::CLICK);
        let letter = ui.painter().layout_no_wrap(
            columns::label(col),
            theme::mono_semibold(11.0),
            if active { p.accent_ink } else { p.faint },
        );
        let letter_width = letter.size().x;
        theme::paint_galley(
            ui.painter(),
            pos2(
                rect.left() + PADDING,
                rect.center().y - letter.size().y / 2.0,
            ),
            letter,
            p.faint,
        );
        // The menu's chevron, at the right.
        let chevron = Rect::from_center_size(
            pos2(rect.right() - PADDING - 7.0, rect.center().y),
            vec2(22.0, 22.0),
        );
        let menu = ui.interact(chevron, ui.id().with(("header-menu", col)), Sense::CLICK);
        menu.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                t!("column-menu", column = columns::label(col)),
            )
        });
        if menu.hovered() {
            ui.painter().rect_filled(chevron, 6.0, p.raised);
        }
        theme::paint_text(
            ui.painter(),
            chevron.center(),
            Align2::CENTER_CENTER,
            icon::CARET_DOWN,
            theme::icons(14.0),
            if menu.hovered() { p.text } else { p.faint },
        );
        let mut truncated = false;
        let title = if header_row {
            flat_text(&capped(self.table.cell(0, col), SHOWN_CHARS))
        } else {
            String::new()
        };
        if header_row {
            let name = Rect::from_min_max(
                pos2(rect.left() + PADDING + letter_width + 8.0, rect.top()),
                pos2(chevron.left() - 4.0, rect.bottom()),
            );
            truncated =
                theme::truncated_text(ui, name, &title, theme::semibold(13.0), p.text, Align::Min);
        }
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                format!("{} {title}", columns::label(col)),
            )
        });
        let response = if truncated {
            response.on_hover_text(title)
        } else {
            response
        };
        if menu.clicked() || response.clicked() {
            self.commit_editing();
            let row = if header_row { 0 } else { self.selected.0 };
            self.selected = (row, col);
        }
        if response.double_clicked() && header_row {
            self.start_editing(self.table.cell(0, col).to_owned(), false);
        }
        egui::Popup::menu(&menu).show(|ui| {
            ui.set_min_width(220.0);
            self.row_and_column_items(ui);
            ui.separator();
            self.column_view_items(ui, col);
        });
        response.context_menu(|ui| {
            ui.set_min_width(220.0);
            self.column_view_items(ui, col);
        });
    }

    fn cell(&mut self, ui: &mut egui::Ui, row: usize, col: usize, scrolling: bool) {
        // A command may have shrunk the table while it is being drawn.
        if row >= self.table.row_count() || col >= self.table.col_count() {
            return;
        }
        let rect = ui.max_rect();
        let selected = self.selected == (row, col);
        if selected && scrolling {
            ui.scroll_to_rect(rect, None);
            self.scrolled = true;
        }
        if selected && self.editing.is_some() && !self.edit_in_inspector {
            self.cell_editor(ui, row, col);
            return;
        }
        // Columns scrolled out of sight cost nothing.
        if !ui.is_rect_visible(rect) {
            return;
        }
        let p = palette(ui);
        // Not focusable: the keyboard stays with the grid.
        let response = ui.allocate_rect(rect, Sense::CLICK);
        let text = self.table.cell(row, col);
        let fill = if selected {
            p.accent_soft
        } else if self.is_match(row, col) {
            p.hl
        } else if response.hovered() {
            p.raised
        } else {
            egui::Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, 0.0, fill);
        bottom_line(ui, rect, p.line_soft);
        if selected {
            ui.painter()
                .rect_stroke(rect, 0.0, Stroke::new(2.0, p.ring), StrokeKind::Inside);
        }
        let shown = flat_text(&capped(text, SHOWN_CHARS));
        let color = if col == 0 { p.muted } else { p.text };
        let inner = rect.shrink2(vec2(PADDING, 0.0));
        let truncated =
            theme::truncated_text(ui, inner, &shown, theme::regular(13.0), color, Align::Min);
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Label,
                true,
                format!("{}: {shown}", columns::reference(row, col)),
            )
        });
        let response = if truncated {
            response.on_hover_ui(|ui| {
                ui.set_max_width(480.0);
                ui.label(&shown);
            })
        } else {
            response
        };
        if response.clicked() || response.double_clicked() {
            self.commit_editing();
            self.selected = (row, col);
            if response.double_clicked() {
                self.start_editing(self.table.cell(row, col).to_owned(), false);
            }
        }
    }

    /// The text field of the cell being typed in.
    fn cell_editor(&mut self, ui: &mut egui::Ui, row: usize, col: usize) {
        self.editor_drawn = true;
        let p = palette(ui);
        let rect = ui.max_rect();
        ui.painter().rect_filled(rect, 0.0, p.surface);
        ui.painter()
            .rect_stroke(rect, 0.0, Stroke::new(2.0, p.ring), StrokeKind::Inside);
        // Tab moves to the next cell, not egui's focus to the next widget.
        let tab = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Tab));
        let back_tab = ui.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, Key::Tab));
        let Some(text) = &mut self.editing else {
            return;
        };
        let response = ui.put(
            // The text field's own margin is 4 px.
            rect.shrink2(vec2(PADDING - 4.0, 2.0)),
            egui::TextEdit::singleline(text)
                .frame(egui::Frame::NONE)
                .vertical_align(Align::Center)
                .desired_width(f32::INFINITY),
        );
        if self.focus_editor {
            response.request_focus();
            self.focus_editor = false;
        }
        if (tab || back_tab) && response.has_focus() {
            response.surrender_focus();
            self.commit_editing();
            let shown = self.shown_cols();
            let position = shown.iter().position(|&c| c == col).unwrap_or(0);
            let next = if back_tab {
                position.saturating_sub(1)
            } else {
                (position + 1).min(shown.len() - 1)
            };
            self.select(row, shown[next]);
            return;
        }
        if response.lost_focus() {
            let (enter, escape) =
                ui.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
            if escape {
                self.editing = None;
            } else {
                self.commit_editing();
                if enter {
                    self.move_down_after_enter(row, col);
                }
            }
        }
    }

    /// The last row: "+ New row".
    fn new_row(&mut self, table_row: &mut egui_extras::TableRow<'_, '_>, cols: usize) {
        let mut clicked = false;
        table_row.col(|ui| {
            clicked |= ui.allocate_rect(ui.max_rect(), Sense::CLICK).clicked();
        });
        for col in 0..cols {
            table_row.col(|ui| {
                let rect = ui.max_rect();
                if !ui.is_rect_visible(rect) {
                    return;
                }
                let p = palette(ui);
                clicked |= ui.allocate_rect(rect, Sense::CLICK).clicked();
                if col == 0 {
                    let text = format!("{}  {}", icon::PLUS, t!("new-row"));
                    let galley = ui.painter().layout_job(theme::with_icons(
                        &text,
                        theme::regular(13.0),
                        p.faint,
                    ));
                    let y = rect.center().y - galley.size().y / 2.0;
                    theme::paint_galley(
                        ui.painter(),
                        pos2(rect.left() + PADDING, y),
                        galley,
                        p.faint,
                    );
                }
            });
        }
        if clicked {
            self.commit_editing();
            self.append_row();
        }
    }
}

/// The corner above the row numbers.
fn number_header(ui: &mut egui::Ui) {
    let p = palette(ui);
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, 0.0, p.chrome);
    ui.painter().line_segment(
        [rect.right_top(), rect.right_bottom()],
        Stroke::new(1.0, p.line),
    );
    bottom_line(ui, rect, p.line);
    theme::paint_text(
        ui.painter(),
        rect.center(),
        Align2::CENTER_CENTER,
        icon::GRID_FOUR,
        theme::icons(14.0),
        p.faint,
    );
}

fn bottom_line(ui: &egui::Ui, rect: Rect, color: egui::Color32) {
    ui.painter().line_segment(
        [
            pos2(rect.left(), rect.bottom() - 0.5),
            pos2(rect.right(), rect.bottom() - 0.5),
        ],
        Stroke::new(1.0, color),
    );
}

impl CsvApp {
    /// How columns are named, and which are shown: in the column headers'
    /// menus (right click or chevron).
    pub(super) fn column_view_items(&mut self, ui: &mut egui::Ui, col: usize) {
        ui.label(
            egui::RichText::new(t!("column-labels"))
                .size(12.0)
                .color(palette(ui).faint),
        );
        super::titlebar::labels_choice(ui);
        ui.separator();
        let shown = self.shown_cols();
        if shown.len() > 1 && ui.button(t!("column-hide")).clicked() {
            self.show_columns(shown.into_iter().filter(|&c| c != col).collect());
        }
        if ui.button(t!("column-only")).clicked() {
            self.show_columns(vec![col]);
        }
        if self.shown.is_some() && ui.button(t!("columns-show-all")).clicked() {
            self.show_columns(Vec::new());
        }
    }
}
