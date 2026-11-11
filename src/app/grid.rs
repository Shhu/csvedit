//! The grid of cells: column headers, row numbers, the cells, and a last
//! row that adds one.
//!
//! Only what is on screen is laid out: rows and columns are placed by
//! arithmetic, so a wide or long table costs what a small one does.

use std::ops::Range;

use eframe::egui::{
    self, Align, Align2, Key, Rangef, Rect, Sense, Stroke, StrokeKind, UiBuilder, Vec2, pos2, vec2,
};
use egui_phosphor::regular as icon;

use super::*;
use crate::i18n::t;
use crate::theme::{self, palette};

const NUMBER_WIDTH: f32 = 56.0;
const HEADER_HEIGHT: f32 = 40.0;
/// Text inside cells, from their left edge.
const PADDING: f32 = 12.0;
/// Characters of a cell in its tooltip and its accessibility label.
const SHOWN_CHARS: usize = 1000;
/// Frames spent bringing the selected cell into view before giving up.
const SCROLL_TRIES: u8 = 3;
/// Columns are never resized narrower.
const MIN_WIDTH: f32 = 48.0;

impl CsvApp {
    pub(super) fn grid(&mut self, ui: &mut egui::Ui) {
        let p = palette(ui);
        ui.painter().rect_filled(ui.max_rect(), 0.0, p.bg);
        let shown = self.shown_cols();
        let start = self.body_start();
        let rows = self.table.row_count() - start;
        let row_height = self.density.row_height();
        // Looked up by the cells to light up; searched a slice at a time.
        self.search_slice(ui.ctx());
        // A selection made while the grid draws is scrolled to next frame.
        let scrolling = std::mem::take(&mut self.scroll_to_selection);
        self.scrolled = false;
        self.editor_drawn = false;

        // Cells touch; the column lines are drawn last, over them.
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        ui.visuals_mut().widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.line_soft);
        let edges = self.edges(&shown);
        let width = edges[shown.len()];
        let selected_place = shown.iter().position(|&c| c == self.selected.1);
        let (header, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), HEADER_HEIGHT), Sense::hover());
        // The body scrolls both ways; the header follows it sideways.
        // Not animated: an animation lays the content out a frame late,
        // and asking again then would overshoot.
        let body = egui::ScrollArea::both()
            .id_salt("grid")
            .auto_shrink(false)
            .animated(false)
            .show_viewport(ui, |ui, viewport| {
                let origin = ui.max_rect().min;
                ui.set_min_size(vec2(width, (rows + 1) as f32 * row_height));
                let places = columns_between(&edges, viewport.min.x, viewport.max.x);
                let lines = rows_between(viewport.min.y, viewport.max.y, row_height, rows + 1);
                self.page_rows = ((viewport.height() / row_height).floor() as usize).max(1);
                if scrolling && let Some(place) = selected_place {
                    let x = Rangef::new(origin.x + edges[place], origin.x + edges[place + 1]);
                    let y = match self.selected.0.checked_sub(start) {
                        Some(index) => {
                            let top = origin.y + index as f32 * row_height;
                            Rangef::new(top, top + row_height)
                        }
                        // The header row: only sideways.
                        None => ui.clip_rect().y_range(),
                    };
                    ui.scroll_to_rect(Rect::from_x_y_ranges(x, y), None);
                    self.scrolled = self
                        .selected
                        .0
                        .checked_sub(start)
                        .is_none_or(|index| lines.contains(&index));
                }
                for index in lines {
                    let top = origin.y + index as f32 * row_height;
                    let rect = Rect::from_min_size(pos2(origin.x, top), vec2(width, row_height));
                    if index % 2 == 0 {
                        ui.painter().rect_filled(rect, 0.0, p.zebra);
                    }
                    if index == rows {
                        self.new_row(ui, rect, &edges);
                    } else {
                        self.body_row(ui, start + index, rect, &shown, &edges, places.clone());
                    }
                }
                (origin.x, places)
            });
        let (left, places) = body.inner;
        let area = body.inner_rect;

        // The header, over the columns on screen.
        let header = Rect::from_x_y_ranges(area.x_range(), header.y_range());
        let column = |place: usize| {
            Rect::from_x_y_ranges(
                Rangef::new(left + edges[place], left + edges[place + 1]),
                header.y_range(),
            )
        };
        let corner =
            Rect::from_x_y_ranges(Rangef::new(left, left + NUMBER_WIDTH), header.y_range());
        if corner.right() > header.left() {
            let mut ui = clipped_child(ui, corner, header, "corner");
            number_header(&mut ui);
        }
        for place in places.clone() {
            let col = shown[place];
            let mut ui = clipped_child(ui, column(place), header, ("header", col));
            self.header_cell(&mut ui, col);
        }

        // Column lines, from the top of the header to the bottom; the
        // columns' ones resize them.
        let top_to_bottom = |x: f32| [pos2(x, header.top()), pos2(x, area.bottom())];
        let painter = ui.painter().with_clip_rect(header.union(area));
        painter.line_segment(
            top_to_bottom(left + NUMBER_WIDTH),
            Stroke::new(1.0, p.line_soft),
        );
        let grab = ui.style().interaction.resize_grab_radius_side;
        for place in places {
            let col = shown[place];
            let column_left = left + edges[place];
            let mut x = left + edges[place + 1];
            let line = Rect::from_two_pos(pos2(x, header.top()), pos2(x, area.bottom()));
            let response = ui.interact(
                line.expand(grab).intersect(header.union(area)),
                ui.id().with(("resize", col)),
                Sense::drag(),
            );
            if response.dragged()
                && let Some(pointer) = ui.ctx().pointer_latest_pos()
            {
                let new_width = (pointer.x - column_left).max(MIN_WIDTH);
                self.set_col_width(col, new_width);
                x = column_left + new_width;
            }
            let dragging_else = ui.input(|i| i.pointer.any_down() || i.pointer.any_pressed());
            let hover = response.hovered() && !dragging_else;
            if hover || response.dragged() {
                ui.set_cursor_icon(egui::CursorIcon::ResizeColumn);
            }
            let stroke = if response.dragged() {
                ui.visuals().widgets.active.bg_stroke
            } else if hover {
                ui.visuals().widgets.hovered.bg_stroke
            } else {
                Stroke::new(1.0, p.line_soft)
            };
            painter.line_segment(top_to_bottom(x), stroke);
        }

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

    /// The width of column `col`.
    fn col_width(&self, col: usize) -> f32 {
        self.col_widths
            .get(col)
            .copied()
            .unwrap_or(if col == 0 { 210.0 } else { 200.0 })
    }

    fn set_col_width(&mut self, col: usize, width: f32) {
        while self.col_widths.len() <= col {
            self.col_widths.push(self.col_width(self.col_widths.len()));
        }
        self.col_widths[col] = width;
    }

    /// The left edges of the columns `shown`, from the left of the row
    /// numbers, then the right edge of the last one.
    fn edges(&self, shown: &[usize]) -> Vec<f32> {
        let mut edges = Vec::with_capacity(shown.len() + 1);
        let mut x = NUMBER_WIDTH;
        for &col in shown {
            edges.push(x);
            x += self.col_width(col);
        }
        edges.push(x);
        edges
    }

    /// A row of the table: its number, then its cells in the columns on
    /// screen (`places` among those `shown`).
    fn body_row(
        &mut self,
        ui: &mut egui::Ui,
        row: usize,
        rect: Rect,
        shown: &[usize],
        edges: &[f32],
        places: Range<usize>,
    ) {
        // A command may have shrunk the table while it is being drawn.
        if row >= self.table.row_count() {
            return;
        }
        let p = palette(ui);
        // One line under the whole row; the cells' fills stop above it.
        bottom_line(ui.painter(), rect, p.line_soft);
        let number = Rect::from_x_y_ranges(
            Rangef::new(rect.left(), rect.left() + NUMBER_WIDTH),
            rect.y_range(),
        );
        if ui.is_rect_visible(number) {
            self.row_number(ui, number, row);
        }
        let cell_rect = |place: usize| {
            Rect::from_x_y_ranges(
                Rangef::new(rect.left() + edges[place], rect.left() + edges[place + 1]),
                rect.y_range(),
            )
        };
        // The editor stays while its row is on screen, even scrolled
        // sideways out of sight.
        let editor = (self.selected.0 == row && self.editing.is_some() && !self.edit_in_inspector)
            .then(|| shown.iter().position(|&c| c == self.selected.1))
            .flatten();
        for place in places {
            if Some(place) != editor {
                self.cell(ui, cell_rect(place), row, shown[place]);
            }
        }
        if let Some(place) = editor
            && self.selected.1 < self.table.col_count()
        {
            let col = shown[place];
            let mut ui = clipped_child(ui, cell_rect(place), ui.clip_rect(), ("editor", row, col));
            self.cell_editor(&mut ui, row, col);
        }
    }

    fn row_number(&self, ui: &egui::Ui, rect: Rect, row: usize) {
        let p = palette(ui);
        let selected = self.selected.0 == row;
        if selected {
            ui.painter()
                .rect_filled(above_line(rect), 0.0, p.accent_soft);
        }
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
    fn header_cell(&mut self, ui: &mut egui::Ui, col: usize) {
        if col >= self.table.col_count() {
            return;
        }
        let p = palette(ui);
        let rect = ui.max_rect();
        let header_row = self.body_start() == 1;
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
        bottom_line(ui.painter(), rect, p.line);
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
            self.table.cell(0, col)
        } else {
            ""
        };
        if header_row {
            let name = Rect::from_min_max(
                pos2(rect.left() + PADDING + letter_width + 8.0, rect.top()),
                pos2(chevron.left() - 4.0, rect.bottom()),
            );
            truncated = fitted_text(ui, name, title, theme::semibold(13.0), p.text);
        }
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                format!("{} {}", columns::label(col), shown_text(title)),
            )
        });
        let response = if truncated {
            response.on_hover_ui(|ui| {
                ui.label(shown_text(title));
            })
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

    fn cell(&mut self, ui: &egui::Ui, rect: Rect, row: usize, col: usize) {
        // A command may have shrunk the table while it is being drawn.
        if row >= self.table.row_count() || col >= self.table.col_count() {
            return;
        }
        let p = palette(ui);
        let selected = self.selected == (row, col);
        // Not focusable: the keyboard stays with the grid.
        let response = ui.interact(rect, ui.id().with(("cell", row, col)), Sense::CLICK);
        let fill = if selected {
            Some(p.accent_soft)
        } else if self.is_match(row, col) {
            Some(p.hl)
        } else if response.hovered() {
            Some(p.raised)
        } else {
            None
        };
        if let Some(fill) = fill {
            ui.painter().rect_filled(above_line(rect), 0.0, fill);
        }
        if selected {
            ui.painter()
                .rect_stroke(rect, 0.0, Stroke::new(2.0, p.ring), StrokeKind::Inside);
        }
        let text = self.table.cell(row, col);
        let color = if col == 0 { p.muted } else { p.text };
        let inner = rect.shrink2(vec2(PADDING, 0.0));
        let truncated = fitted_text(ui, inner, text, theme::regular(13.0), color);
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Label,
                true,
                format!("{}: {}", columns::reference(row, col), shown_text(text)),
            )
        });
        let response = if truncated {
            response.on_hover_ui(|ui| {
                ui.set_max_width(480.0);
                ui.label(shown_text(text));
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
    fn new_row(&mut self, ui: &egui::Ui, rect: Rect, edges: &[f32]) {
        let p = palette(ui);
        let clicked = ui
            .interact(rect, ui.id().with("new-row"), Sense::CLICK)
            .clicked();
        if let [left, right, ..] = *edges {
            let column = Rect::from_x_y_ranges(
                Rangef::new(rect.left() + left, rect.left() + right),
                rect.y_range(),
            );
            if ui.is_rect_visible(column) {
                let text = format!("{}  {}", icon::PLUS, t!("new-row"));
                let galley = ui.painter().layout_job(theme::with_icons(
                    &text,
                    theme::regular(13.0),
                    p.faint,
                ));
                let y = column.center().y - galley.size().y / 2.0;
                theme::paint_galley(
                    &ui.painter().with_clip_rect(column),
                    pos2(column.left() + PADDING, y),
                    galley,
                    p.faint,
                );
            }
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
    bottom_line(ui.painter(), rect, p.line);
    theme::paint_text(
        ui.painter(),
        rect.center(),
        Align2::CENTER_CENTER,
        icon::GRID_FOUR,
        theme::icons(14.0),
        p.faint,
    );
}

fn bottom_line(painter: &egui::Painter, rect: Rect, color: egui::Color32) {
    painter.line_segment(
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

/// A child of `ui` filling `rect`, drawn only inside `clip`.
fn clipped_child(
    ui: &mut egui::Ui,
    rect: Rect,
    clip: Rect,
    salt: impl std::hash::Hash + std::fmt::Debug,
) -> egui::Ui {
    let mut child = ui.new_child(UiBuilder::new().id_salt(salt).max_rect(rect));
    child.set_clip_rect(rect.intersect(clip));
    child
}

/// `rect` without its last line of pixels, where the row's line is.
fn above_line(rect: Rect) -> Rect {
    rect.with_max_y(rect.bottom() - 1.0)
}

/// Paints a cell's text on one line in `rect`, cut with "…" when it
/// doesn't fit; returns whether it was cut. Only the characters that can
/// fit are laid out: a cell can hold megabytes.
fn fitted_text(
    ui: &egui::Ui,
    rect: Rect,
    text: &str,
    font: egui::FontId,
    color: egui::Color32,
) -> bool {
    let capped = capped(text, chars_fitting(rect.width()));
    let cut = matches!(capped, std::borrow::Cow::Owned(_));
    theme::truncated_text(ui, rect, flat_text(&capped), font, color, Align::Min) || cut
}

/// A cell's text as its tooltip and accessibility label show it.
fn shown_text(text: &str) -> String {
    flat_text(&capped(text, SHOWN_CHARS))
}

/// Characters of a cell worth laying out in `width` points: more than fit
/// even when all are narrow.
fn chars_fitting(width: f32) -> usize {
    ((width.max(0.0) / 2.0) as usize + 1).max(64)
}

/// Places of the columns at least partly between `min` and `max`, given
/// their left `edges` and then the right edge of the last one.
fn columns_between(edges: &[f32], min: f32, max: f32) -> Range<usize> {
    let count = edges.len().saturating_sub(1);
    let first = edges[1..].partition_point(|&right| right <= min);
    let end = edges[..count].partition_point(|&left| left < max);
    first..end.max(first)
}

/// Rows at least partly between `min` and `max`, among `count` rows of
/// `height` from 0.
fn rows_between(min: f32, max: f32, height: f32, count: usize) -> Range<usize> {
    let first = ((min / height).floor().max(0.0) as usize).min(count);
    let end = ((max / height).ceil().max(0.0) as usize).min(count);
    first..end.max(first)
}

#[cfg(test)]
mod tests {
    use egui_kittest::kittest::Queryable as _;

    use super::*;

    #[test]
    fn finds_the_columns_on_screen() {
        // Row numbers 0..56, then columns of 100.
        let edges = [56.0, 156.0, 256.0, 356.0];
        assert_eq!(columns_between(&edges, 0.0, 50.0), 0..0);
        assert_eq!(columns_between(&edges, 0.0, 57.0), 0..1);
        assert_eq!(columns_between(&edges, 156.0, 256.0), 1..2);
        assert_eq!(columns_between(&edges, 155.0, 257.0), 0..3);
        assert_eq!(columns_between(&edges, 400.0, 900.0), 3..3);
        assert_eq!(columns_between(&[56.0], 0.0, 900.0), 0..0);
    }

    #[test]
    fn finds_the_rows_on_screen() {
        assert_eq!(rows_between(0.0, 100.0, 36.0, 1000), 0..3);
        assert_eq!(rows_between(36.0, 72.0, 36.0, 1000), 1..2);
        assert_eq!(rows_between(-10.0, 30.0, 36.0, 1000), 0..1);
        assert_eq!(rows_between(0.0, 1e6, 36.0, 5), 0..5);
        assert_eq!(rows_between(500.0, 900.0, 36.0, 5), 5..5);
    }

    #[test]
    fn lays_out_only_what_can_fit() {
        assert_eq!(chars_fitting(0.0), 64);
        assert_eq!(chars_fitting(-5.0), 64);
        assert_eq!(chars_fitting(1000.0), 501);
    }

    /// Cells of a table of `rows` × `cols` read "r{row}c{col}".
    fn harness(rows: usize, cols: usize) -> egui_kittest::Harness<'static, CsvApp> {
        let table = Table::from_rows(
            (0..rows)
                .map(|r| (0..cols).map(|c| Cell::from(format!("r{r}c{c}"))).collect())
                .collect(),
        );
        let mut app = CsvApp::new(None);
        app.reset(table, Format::default(), None);
        let mut fonts = false;
        egui_kittest::Harness::builder()
            .with_size([1200.0, 800.0])
            .build_ui_state(
                move |ui, app: &mut CsvApp| {
                    // The fonts serve from the next frame on.
                    if !std::mem::replace(&mut fonts, true) {
                        theme::install(ui.ctx());
                        return;
                    }
                    app.grid(ui);
                },
                app,
            )
    }

    #[test]
    fn draws_only_the_cells_on_screen_and_scrolls_to_others() {
        let mut harness = harness(1000, 50);
        harness.run();
        assert!(harness.query_by_label_contains(": r1c0").is_some());
        assert!(harness.query_by_label_contains(": r900c40").is_none());
        assert!(harness.query_by_label_contains(": r1c40").is_none());
        assert!(harness.query_by_label_contains(" r0c40").is_none());
        harness.state_mut().select(900, 40);
        harness.run();
        assert!(harness.query_by_label_contains(": r1c0").is_none());
        assert!(harness.query_by_label_contains(": r900c40").is_some());
        // The header follows sideways.
        assert!(harness.query_by_label_contains(" r0c40").is_some());
        assert!(harness.query_by_label_contains(" r0c0").is_none());
        let far = harness.get_by_label_contains(": r899c39");
        far.click();
        harness.run();
        assert_eq!(harness.state().selected, (899, 39));
        assert!(harness.state().editing.is_none());
        harness.state_mut().start_editing("typed".into(), false);
        harness.run();
        assert_eq!(harness.state().editing.as_deref(), Some("typed"));
        assert!(harness.state().editor_drawn);
    }

    #[test]
    fn resizes_a_column_by_its_line() {
        let mut harness = harness(10, 5);
        harness.run();
        let header = harness.get_by_label_contains(" r0c0").rect();
        let (right, y) = (header.right(), header.center().y);
        let press = |harness: &mut egui_kittest::Harness<'_, CsvApp>, x: f32, pressed: bool| {
            harness.event(egui::Event::PointerButton {
                pos: pos2(x, y),
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
            harness.step();
        };
        harness.hover_at(pos2(right, y));
        harness.step();
        press(&mut harness, right, true);
        for dx in [-20.0, -50.0, -70.0] {
            harness.hover_at(pos2(right + dx, y));
            harness.step();
        }
        press(&mut harness, right - 70.0, false);
        harness.run();
        assert_eq!(harness.state().col_width(0), 140.0);
        assert_eq!(harness.state().col_width(1), 200.0);
        // The header's cells take clicks between the lines.
        harness.get_by_label_contains(" r0c2").click();
        harness.run();
        assert_eq!(harness.state().selected, (0, 2));
    }

    #[test]
    fn remembers_resized_columns() {
        let mut app = CsvApp::new(None);
        assert_eq!(app.col_width(0), 210.0);
        assert_eq!(app.col_width(3), 200.0);
        app.set_col_width(2, 90.0);
        assert_eq!([app.col_width(0), app.col_width(1)], [210.0, 200.0]);
        assert_eq!(app.col_width(2), 90.0);
        assert_eq!(app.edges(&[0, 2]), [56.0, 266.0, 356.0]);
    }
}
