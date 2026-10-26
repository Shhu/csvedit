//! Title bar: app icon, menus, the file's name and the window buttons. The
//! window has no system frame: the title bar moves it and its borders
//! resize it (`resize_grips`).

use eframe::egui::viewport::ResizeDirection;
use eframe::egui::{
    self, Align, Color32, CursorIcon, KeyboardShortcut, Layout, Pos2, Rect, Sense, Stroke,
    StrokeKind, ThemePreference, ViewportCommand, pos2, vec2,
};

use super::*;
use crate::i18n::{self, Lang, t};
use crate::theme::{self, palette};

/// Window buttons, as wide as Windows'.
const WINDOW_BUTTON_WIDTH: f32 = 46.0;
/// Width of the window border that resizes it.
const RESIZE_BORDER: f32 = 5.0;
/// Length of the corners' grip along each border.
const RESIZE_CORNER: f32 = 12.0;

impl CsvApp {
    pub(super) fn title_bar(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let p = palette(ui);
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));

        // Empty space moves the window; double-click maximizes. Registered
        // first so that the widgets on top of it take precedence.
        let bar = ui.max_rect();
        let drag_rect = Rect::from_x_y_ranges(ui.clip_rect().x_range(), bar.y_range());
        let background = ui.interact(drag_rect, ui.id().with("drag"), Sense::click_and_drag());
        if background.double_clicked() {
            ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
        } else if background.drag_started_by(egui::PointerButton::Primary) {
            ctx.send_viewport_cmd(ViewportCommand::StartDrag);
        }

        // File name, a dot when modified, and the app's name, centered.
        let name = ui
            .painter()
            .layout_no_wrap(self.display_name(), theme::semibold(13.0), p.text);
        let app = ui
            .painter()
            .layout_no_wrap(APP_NAME.to_owned(), theme::regular(13.0), p.faint);
        let dirty = self.dirty();
        let width = name.size().x + 8.0 + if dirty { 7.0 + 8.0 } else { 0.0 } + app.size().x;
        let mut x = bar.center().x - width / 2.0;
        let y = bar.center().y;
        let name_width = name.size().x;
        theme::paint_galley(ui.painter(), pos2(x, y - name.size().y / 2.0), name, p.text);
        x += name_width + 8.0;
        if dirty {
            ui.painter().circle_filled(pos2(x + 3.5, y), 3.5, p.accent);
            x += 7.0 + 8.0;
        }
        theme::paint_galley(ui.painter(), pos2(x, y - app.size().y / 2.0), app, p.faint);

        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            logo(ui);
            ui.add_space(6.0);
            ui.scope(|ui| {
                menu_bar_style(ui);
                ui.spacing_mut().item_spacing.x = 2.0;
                let config = egui::containers::menu::MenuConfig::new().style(menu_style);
                egui::containers::menu::MenuButton::new(t!("menu-file"))
                    .config(config.clone())
                    .ui(ui, |ui| self.file_menu(ui));
                egui::containers::menu::MenuButton::new(t!("menu-edit"))
                    .config(config.clone())
                    .ui(ui, |ui| self.edit_menu(ui));
                egui::containers::menu::MenuButton::new(t!("menu-table"))
                    .config(config.clone())
                    .ui(ui, |ui| self.table_menu(ui));
                egui::containers::menu::MenuButton::new(t!("menu-help"))
                    .config(config)
                    .ui(ui, |ui| self.help_menu(ui));
            });

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                if window_button(ui, WindowButton::Close, &t!("title-close")).clicked() {
                    // Through the usual close flow (unsaved changes).
                    ctx.send_viewport_cmd(ViewportCommand::Close);
                }
                let (kind, tip) = if maximized {
                    (WindowButton::Restore, t!("title-restore"))
                } else {
                    (WindowButton::Maximize, t!("title-maximize"))
                };
                if window_button(ui, kind, &tip).clicked() {
                    ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
                }
                if window_button(ui, WindowButton::Minimize, &t!("title-minimize")).clicked() {
                    ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
                }
            });
        });
    }

    fn file_menu(&mut self, ui: &mut egui::Ui) {
        self.menu_item(ui, t!("command-new"), Some(SHORTCUT_NEW), Command::New);
        self.menu_item(ui, t!("command-open"), Some(SHORTCUT_OPEN), Command::Open);
        self.menu_item(ui, t!("command-save"), Some(SHORTCUT_SAVE), Command::Save);
        self.menu_item(
            ui,
            t!("command-save-as"),
            Some(SHORTCUT_SAVE_AS),
            Command::SaveAs,
        );
        ui.separator();
        self.menu_item(ui, t!("command-quit"), Some(SHORTCUT_QUIT), Command::Quit);
    }

    fn edit_menu(&mut self, ui: &mut egui::Ui) {
        self.menu_item(ui, t!("command-undo"), Some(SHORTCUT_UNDO), Command::Undo);
        self.menu_item(ui, t!("command-redo"), Some(SHORTCUT_REDO), Command::Redo);
        ui.separator();
        self.menu_item(ui, t!("command-find"), Some(SHORTCUT_FIND), Command::Find);
        self.menu_item(
            ui,
            t!("command-inspector"),
            Some(SHORTCUT_INSPECTOR),
            Command::ToggleInspector,
        );
    }

    pub(super) fn table_menu(&mut self, ui: &mut egui::Ui) {
        self.row_and_column_items(ui);
        ui.separator();
        self.menu_item(
            ui,
            t!("columns-choose"),
            Some(SHORTCUT_COLUMNS),
            Command::ChooseColumns,
        );
        ui.menu_button(t!("column-labels"), |ui| {
            for labels in columns::Labels::ALL {
                let text = egui::RichText::new(labels.example()).font(theme::mono(13.0));
                if ui.radio(columns::labels() == labels, text).clicked() {
                    columns::choose(labels);
                }
            }
        });
        if self.shown.is_some() && ui.button(t!("columns-show-all")).clicked() {
            self.show_columns(Vec::new());
        }
        ui.separator();
        ui.checkbox(&mut self.header, t!("header-row"));
        ui.menu_button(t!("menu-density"), |ui| {
            for (density, name) in [
                (Density::Compact, t!("density-compact")),
                (Density::Normal, t!("density-normal")),
                (Density::Comfortable, t!("density-comfortable")),
            ] {
                if ui.radio(self.density == density, name).clicked() {
                    self.density = density;
                }
            }
        });
    }

    /// Inserting, deleting and sorting: in the Table menu and the column
    /// headers' menus.
    pub(super) fn row_and_column_items(&mut self, ui: &mut egui::Ui) {
        self.menu_item(
            ui,
            t!("command-insert-row-above"),
            None,
            Command::InsertRowAbove,
        );
        self.menu_item(
            ui,
            t!("command-insert-row-below"),
            Some(SHORTCUT_INSERT_ROW),
            Command::InsertRowBelow,
        );
        self.menu_item(
            ui,
            t!("command-delete-row"),
            Some(SHORTCUT_DELETE_ROW),
            Command::DeleteRow,
        );
        ui.separator();
        self.menu_item(
            ui,
            t!("command-insert-col-left"),
            None,
            Command::InsertColLeft,
        );
        self.menu_item(
            ui,
            t!("command-insert-col-right"),
            None,
            Command::InsertColRight,
        );
        self.menu_item(ui, t!("command-delete-col"), None, Command::DeleteCol);
        ui.separator();
        self.menu_item(
            ui,
            t!("command-sort-ascending"),
            None,
            Command::SortAscending,
        );
        self.menu_item(
            ui,
            t!("command-sort-descending"),
            None,
            Command::SortDescending,
        );
    }

    fn help_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button(t!("menu-language"), |ui| {
            for lang in Lang::ALL {
                if ui.radio(i18n::lang() == lang, lang.native_name()).clicked() {
                    i18n::choose(lang);
                }
            }
        });
        ui.menu_button(t!("menu-theme"), |ui| {
            let current = ui.ctx().options(|o| o.theme_preference);
            for (theme, name) in [
                (ThemePreference::System, t!("theme-system")),
                (ThemePreference::Dark, t!("theme-dark")),
                (ThemePreference::Light, t!("theme-light")),
            ] {
                if ui.radio(current == theme, name).clicked() {
                    theme::choose_theme(ui.ctx(), theme);
                }
            }
        });
        ui.separator();
        if ui.button(t!("command-about")).clicked() {
            self.about = true;
        }
    }

    fn menu_item(
        &mut self,
        ui: &mut egui::Ui,
        text: String,
        shortcut: Option<KeyboardShortcut>,
        command: Command,
    ) {
        let mut button = egui::Button::new(text);
        if let Some(shortcut) = shortcut {
            button = button.shortcut_text(ui.ctx().format_shortcut(&shortcut));
        }
        if ui.add_enabled(self.can_run(command), button).clicked() {
            self.run(command);
        }
    }
}

/// The app icon: an accent square with a table.
fn logo(ui: &mut egui::Ui) {
    let p = palette(ui);
    let (rect, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
    ui.painter().rect_filled(rect, 6.0, p.accent);
    let stroke = Stroke::new(1.6, p.on_accent);
    let inner = Rect::from_center_size(rect.center(), vec2(12.0, 10.0));
    ui.painter()
        .rect_stroke(inner, 1.5, stroke, StrokeKind::Middle);
    let y = inner.top() + 3.5;
    ui.painter()
        .line_segment([pos2(inner.left(), y), pos2(inner.right(), y)], stroke);
    let x = inner.left() + 4.0;
    ui.painter()
        .line_segment([pos2(x, y), pos2(x, inner.bottom())], stroke);
}

/// Menu titles: frameless, muted text that lights up on hover.
fn menu_bar_style(ui: &mut egui::Ui) {
    let p = palette(ui);
    let style = ui.style_mut();
    style.spacing.button_padding = vec2(10.0, 5.0);
    let w = &mut style.visuals.widgets;
    for (state, fill, text) in [
        (&mut w.inactive, Color32::TRANSPARENT, p.muted),
        (&mut w.hovered, p.raised, p.text),
        (&mut w.active, p.raised, p.text),
        (&mut w.open, p.raised, p.text),
    ] {
        state.weak_bg_fill = fill;
        state.bg_stroke = Stroke::NONE;
        state.fg_stroke.color = text;
        state.corner_radius = 6.into();
    }
}

/// Menu popups and their submenus.
fn menu_style(style: &mut egui::Style) {
    egui::containers::menu::menu_style(style);
    style.spacing.button_padding = vec2(8.0, 4.0);
    style.spacing.interact_size.y = 28.0;
    style.spacing.item_spacing.y = 2.0;
    for state in [
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
        &mut style.visuals.widgets.open,
    ] {
        state.corner_radius = 6.into();
    }
}

#[derive(Clone, Copy, PartialEq)]
enum WindowButton {
    Minimize,
    Maximize,
    Restore,
    Close,
}

/// Window button, full title bar height, its glyph drawn in a 12 px box.
fn window_button(ui: &mut egui::Ui, kind: WindowButton, tip: &str) -> egui::Response {
    let p = palette(ui);
    let size = vec2(WINDOW_BUTTON_WIDTH, ui.available_height());
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let (fill, fg) = match (response.hovered(), kind) {
        (true, WindowButton::Close) => (Color32::from_rgb(0xE5, 0x48, 0x4D), Color32::WHITE),
        (true, _) => (p.raised, p.text),
        (false, _) => (Color32::TRANSPARENT, p.muted),
    };
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, fill);
    let stroke = Stroke::new(1.2, fg);
    // Glyph coordinates in a 12 × 12 box centered in the button.
    let o = rect.center() - vec2(6.0, 6.0);
    let at = |x: f32, y: f32| o + vec2(x, y);
    match kind {
        WindowButton::Minimize => {
            painter.line_segment([at(1.5, 6.0), at(10.5, 6.0)], stroke);
        }
        WindowButton::Maximize => {
            painter.rect_stroke(
                Rect::from_min_max(at(1.5, 1.5), at(10.5, 10.5)),
                2.0,
                stroke,
                StrokeKind::Middle,
            );
        }
        WindowButton::Restore => {
            painter.rect_stroke(
                Rect::from_min_max(at(1.5, 3.5), at(8.5, 10.5)),
                1.5,
                stroke,
                StrokeKind::Middle,
            );
            painter.add(egui::Shape::line(
                vec![
                    at(3.5, 3.5),
                    at(3.5, 1.5),
                    at(10.5, 1.5),
                    at(10.5, 8.5),
                    at(8.5, 8.5),
                ],
                stroke,
            ));
        }
        WindowButton::Close => {
            painter.line_segment([at(2.0, 2.0), at(10.0, 10.0)], stroke);
            painter.line_segment([at(10.0, 2.0), at(2.0, 10.0)], stroke);
        }
    }
    // Painted: named by its tooltip for screen readers.
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tip));
    response.on_hover_text(tip)
}

/// Invisible strips along the window borders that resize it (the window
/// has no system frame). Nothing to do when maximized or full screen.
pub fn resize_grips(ctx: &egui::Context) {
    let fixed = ctx.input(|i| {
        let v = i.viewport();
        v.maximized.unwrap_or(false) || v.fullscreen.unwrap_or(false)
    });
    if fixed {
        return;
    }
    let r = ctx.content_rect();
    let b = RESIZE_BORDER;
    let strips = [
        ("n", Rect::from_min_max(r.min, pos2(r.max.x, r.min.y + b))),
        ("s", Rect::from_min_max(pos2(r.min.x, r.max.y - b), r.max)),
        ("w", Rect::from_min_max(r.min, pos2(r.min.x + b, r.max.y))),
        ("e", Rect::from_min_max(pos2(r.max.x - b, r.min.y), r.max)),
    ];
    for (name, strip) in strips {
        egui::Area::new(egui::Id::new("resize_grip").with(name))
            .order(egui::Order::Foreground)
            .fixed_pos(strip.min)
            .show(ctx, |ui| {
                let (_, response) = ui.allocate_exact_size(strip.size(), Sense::drag());
                let Some(pos) = response.hover_pos() else {
                    return;
                };
                let direction = resize_direction(r, pos);
                ctx.set_cursor_icon(resize_cursor(direction));
                if response.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_pressed())
                {
                    ctx.send_viewport_cmd(ViewportCommand::BeginResize(direction));
                }
            });
    }
}

/// Which border or corner of `window` is under `pos`.
fn resize_direction(window: Rect, pos: Pos2) -> ResizeDirection {
    let c = RESIZE_CORNER;
    let west = pos.x < window.left() + c;
    let east = pos.x > window.right() - c;
    let north = pos.y < window.top() + c;
    let south = pos.y > window.bottom() - c;
    match (north, south, west, east) {
        (true, _, true, _) => ResizeDirection::NorthWest,
        (true, _, _, true) => ResizeDirection::NorthEast,
        (_, true, true, _) => ResizeDirection::SouthWest,
        (_, true, _, true) => ResizeDirection::SouthEast,
        (true, ..) => ResizeDirection::North,
        (_, true, ..) => ResizeDirection::South,
        (_, _, true, _) => ResizeDirection::West,
        _ => ResizeDirection::East,
    }
}

fn resize_cursor(direction: ResizeDirection) -> CursorIcon {
    match direction {
        ResizeDirection::North => CursorIcon::ResizeNorth,
        ResizeDirection::South => CursorIcon::ResizeSouth,
        ResizeDirection::East => CursorIcon::ResizeEast,
        ResizeDirection::West => CursorIcon::ResizeWest,
        ResizeDirection::NorthEast => CursorIcon::ResizeNorthEast,
        ResizeDirection::SouthEast => CursorIcon::ResizeSouthEast,
        ResizeDirection::NorthWest => CursorIcon::ResizeNorthWest,
        ResizeDirection::SouthWest => CursorIcon::ResizeSouthWest,
    }
}
