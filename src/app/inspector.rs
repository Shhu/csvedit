//! The inspector: the selected cell's whole text, with its line breaks,
//! a few counts, and facts about its column.

use eframe::egui::{
    self, Align, Key, Layout, Margin, Sense, Stroke, StrokeKind, text::LayoutJob, vec2,
};
use egui_phosphor::regular as icon;

use super::*;
use crate::i18n::t;
use crate::theme::{self, palette};

/// Room on each side of the inspector's content.
const SIDE: f32 = 20.0;
/// Characters of a cell shown (all of them are edited).
const SHOWN_CHARS: usize = 20_000;

impl CsvApp {
    pub(super) fn inspector(&mut self, ui: &mut egui::Ui) {
        let p = palette(ui);
        let rect = ui.max_rect();
        ui.painter().line_segment(
            [rect.left_top(), rect.left_bottom()],
            Stroke::new(1.0, p.line),
        );
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                egui::Frame::new()
                    .inner_margin(Margin {
                        left: SIDE as i8,
                        right: 12,
                        top: 10,
                        bottom: 16,
                    })
                    .show(ui, |ui| self.inspector_contents(ui));
            });
    }

    fn inspector_contents(&mut self, ui: &mut egui::Ui) {
        let p = palette(ui);
        let (row, col) = self.selected;
        let width = ui.available_width() - (SIDE - 12.0);
        ui.set_max_width(width + (SIDE - 12.0));
        let caption = |text: String| {
            egui::RichText::new(text.to_uppercase())
                .font(theme::semibold(12.0))
                .color(p.faint)
        };

        ui.horizontal(|ui| {
            ui.set_width(width + (SIDE - 12.0));
            ui.label(caption(t!("inspector-cell")));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let label = t!("inspector-close");
                let close = theme::glyph_button(ui, icon::X, 30.0, 15.0, true, false, &label);
                if close.on_hover_text(t!("inspector-close")).clicked() {
                    self.inspector = false;
                }
            });
        });
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.label(
            egui::RichText::new(columns::reference(row, col))
                .font(theme::mono_semibold(22.0))
                .color(p.text),
        );
        let title = flat_text(self.column_title(col));
        let column = if title.is_empty() {
            t!("inspector-column", letter = columns::label(col))
        } else {
            format!(
                "{} \u{b7} {title}",
                t!("inspector-column", letter = columns::label(col))
            )
        };
        ui.add(egui::Label::new(egui::RichText::new(column).size(13.0).color(p.muted)).truncate());
        // A workbook's formula: the cell shows the result saved with it.
        if let Some(formula) = self.formulas.get(&(row, col)) {
            ui.add_space(4.0);
            ui.add(
                egui::Label::new(
                    egui::RichText::new(t!(
                        "inspector-formula",
                        formula = capped(formula, 500).into_owned()
                    ))
                    .font(theme::mono(12.0))
                    .color(p.accent_ink),
                )
                .wrap(),
            )
            .on_hover_text(t!("inspector-formula-hint"));
        }
        ui.add_space(12.0);

        // The text, in a card.
        let editing_here = self.editing.is_some() && self.edit_in_inspector;
        egui::Frame::new()
            .fill(p.surface)
            .stroke(Stroke::new(1.0, if editing_here { p.ring } else { p.line }))
            .corner_radius(10)
            .inner_margin(Margin::symmetric(16, 14))
            .show(ui, |ui| {
                ui.set_width(width - 32.0);
                ui.set_min_height(150.0);
                if editing_here {
                    self.inspector_editor(ui);
                } else {
                    // A cell can hold megabytes: the start is shown.
                    let text = self.table.cell(row, col);
                    let shown = capped(text, SHOWN_CHARS);
                    let cut = shown.len() != text.len();
                    let job = cell_layout(&shown, width - 32.0, p);
                    let response = ui.add(egui::Label::new(job).sense(Sense::CLICK));
                    if cut {
                        ui.label(
                            egui::RichText::new(t!("inspector-truncated", count = SHOWN_CHARS))
                                .size(12.0)
                                .color(p.faint),
                        );
                    }
                    if response.double_clicked() {
                        self.start_editing(self.table.cell(row, col).to_owned(), true);
                    }
                }
            });
        ui.add_space(12.0);

        // Counts, worked out again only when the text changes.
        let (chars, words, brs) = self.cell_counts(editing_here);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let card = (width - 16.0) / 3.0;
            for (value, label) in [
                (chars, t!("inspector-chars", count = chars)),
                (words, t!("inspector-words", count = words)),
                (brs, t!("inspector-brs", count = brs)),
            ] {
                stat_card(ui, card, &crate::i18n::number(value as u64), &label);
            }
        });
        ui.add_space(14.0);

        // Edit and copy.
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let half = (width - 8.0) / 2.0;
            if editing_here {
                if theme::primary_button(ui, &t!("inspector-apply"), half).clicked() {
                    self.commit_editing();
                }
                if theme::secondary_button(ui, &t!("cancel"), half).clicked() {
                    self.editing = None;
                }
            } else {
                if theme::primary_button(ui, &t!("inspector-edit"), half).clicked() {
                    self.start_editing(self.table.cell(row, col).to_owned(), true);
                }
                let copy = format!("{}  {}", icon::COPY, t!("inspector-copy"));
                if theme::secondary_button(ui, &copy, half).clicked() {
                    ui.ctx().copy_text(self.table.cell(row, col).to_owned());
                }
            }
        });
        ui.add_space(20.0);
        // Across the whole inspector, beyond its margins.
        let (line, _) = ui.allocate_exact_size(vec2(width, 1.0), Sense::hover());
        let panel = ui.clip_rect();
        ui.painter().rect_filled(
            egui::Rect::from_x_y_ranges(panel.x_range(), line.y_range()),
            0.0,
            p.line,
        );
        ui.add_space(16.0);

        // The column.
        ui.spacing_mut().item_spacing.y = 10.0;
        ui.label(caption(t!(
            "inspector-column",
            letter = columns::label(col)
        )));
        let stats = self.column_stats(col);
        for (label, value) in [
            (
                t!("inspector-values"),
                crate::i18n::number(stats.filled as u64),
            ),
            (
                t!("inspector-empty"),
                crate::i18n::number(stats.empty as u64),
            ),
            (
                t!("inspector-longest"),
                t!("inspector-longest-value", count = stats.longest),
            ),
        ] {
            ui.horizontal(|ui| {
                ui.set_width(width);
                ui.label(egui::RichText::new(label).size(13.0).color(p.muted));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(value)
                            .font(theme::mono(13.0))
                            .color(p.text),
                    );
                });
            });
        }
    }

    /// The selected cell's text being typed in the inspector: Ctrl+Enter
    /// applies it, Escape cancels.
    fn inspector_editor(&mut self, ui: &mut egui::Ui) {
        // Ctrl+Enter applies; taken before the field could see it.
        let apply = ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, Key::Enter));
        let Some(text) = &mut self.editing else {
            return;
        };
        let response = ui.add(
            egui::TextEdit::multiline(text)
                .frame(egui::Frame::NONE)
                .font(theme::regular(14.0))
                .desired_rows(7)
                .desired_width(f32::INFINITY)
                .lock_focus(true),
        );
        if self.focus_editor {
            response.request_focus();
            self.focus_editor = false;
            return;
        }
        let escape = ui.input(|i| i.key_pressed(Key::Escape));
        if response.has_focus() && apply {
            self.commit_editing();
        } else if response.lost_focus() {
            // Escape cancels; going elsewhere keeps the text.
            if escape {
                self.editing = None;
            } else {
                self.commit_editing();
            }
        }
    }

    /// Characters (as seen on screen), words and `<BR>` tags of the text
    /// shown in the inspector.
    fn cell_counts(&mut self, editing: bool) -> Counts {
        let (row, col) = self.selected;
        if editing && let Some(text) = &self.editing {
            // Typed text: counted again only when it changed.
            if let Some((seen, counts)) = &self.editing_counts
                && seen == text
            {
                return *counts;
            }
            let counts = counts(text);
            self.editing_counts = Some((text.clone(), counts));
            return counts;
        }
        self.editing_counts = None;
        let key = (self.generation, row, col);
        if let Some((k, counts)) = self.cell_counts
            && k == key
        {
            return counts;
        }
        let counts = counts(self.table.cell(row, col));
        self.cell_counts = Some((key, counts));
        counts
    }
}

/// Characters (as seen on screen), words and `<BR>` tags of `text`.
fn counts(text: &str) -> Counts {
    let parts = split_breaks(text);
    let brs = parts.iter().filter(|(_, br)| *br).count();
    let chars = parts.iter().map(|(s, _)| grapheme_count(s)).sum();
    let words = parts
        .iter()
        .map(|(s, _)| s.split_whitespace().count())
        .sum();
    (chars, words, brs)
}

/// A cell's text for the inspector: wrapped, each `<BR>` tag shown as a
/// small "↵ BR" chip followed by a line break.
fn cell_layout(text: &str, width: f32, p: &theme::Palette) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = width;
    let body = egui::TextFormat {
        font_id: theme::regular(14.0),
        color: p.text,
        line_height: Some(23.0),
        ..Default::default()
    };
    let chip = egui::TextFormat {
        font_id: theme::mono_semibold(10.0),
        color: p.muted,
        background: p.raised,
        line_height: Some(23.0),
        valign: Align::Center,
        ..Default::default()
    };
    let parts = split_breaks(text);
    let last = parts.len() - 1;
    for (i, (piece, br)) in parts.into_iter().enumerate() {
        job.append(piece, 0.0, body.clone());
        if br {
            job.append(" \u{21b5} BR ", 4.0, chip.clone());
        }
        if i < last {
            job.append("\n", 0.0, body.clone());
        }
    }
    if text.is_empty() {
        let mut empty = body;
        empty.color = p.faint;
        job.append("\u{2014}", 0.0, empty);
    }
    job
}

/// A number and what it counts, in a small card.
fn stat_card(ui: &mut egui::Ui, width: f32, value: &str, label: &str) {
    let p = palette(ui);
    let (rect, _) = ui.allocate_exact_size(vec2(width, 54.0), Sense::hover());
    ui.painter().rect(
        rect,
        8.0,
        p.surface,
        Stroke::new(1.0, p.line),
        StrokeKind::Inside,
    );
    theme::paint_text(
        ui.painter(),
        rect.left_top() + vec2(12.0, 10.0),
        egui::Align2::LEFT_TOP,
        value,
        theme::mono_semibold(16.0),
        p.text,
    );
    theme::paint_text(
        ui.painter(),
        rect.left_top() + vec2(12.0, 33.0),
        egui::Align2::LEFT_TOP,
        label,
        theme::regular(11.0),
        p.faint,
    );
}
