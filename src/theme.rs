//! Colors, fonts and widget style, in a dark and a light theme, and the
//! small painted pieces the interface is made of (icon buttons, chips…).

use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Rect,
    Sense, Shadow, Stroke, StrokeKind, TextStyle, Theme, ThemePreference, Vec2, vec2,
};

/// The colors of a theme.
pub struct Palette {
    /// Behind the grid.
    pub bg: Color32,
    /// Title bar, toolbar, status bar, inspector.
    pub chrome: Color32,
    /// Fields and cards.
    pub surface: Color32,
    /// Hovered buttons and cells.
    pub raised: Color32,
    pub line: Color32,
    /// Grid lines.
    pub line_soft: Color32,
    pub text: Color32,
    pub muted: Color32,
    /// Row numbers, labels.
    pub faint: Color32,
    /// Every other row.
    pub zebra: Color32,
    /// Search matches.
    pub hl: Color32,
    pub hl_ink: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    /// Text in the accent color.
    pub accent_ink: Color32,
    /// Ring around the selected cell.
    pub ring: Color32,
    /// Text on the accent.
    pub on_accent: Color32,
    /// Over the window behind a dialog.
    pub scrim: Color32,
}

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

const fn rgba(r: u8, g: u8, b: u8, alpha: f32) -> Color32 {
    Color32::from_rgba_unmultiplied_const(r, g, b, (alpha * 255.0) as u8)
}

pub const DARK: Palette = Palette {
    bg: rgb(0x15171C),
    chrome: rgb(0x1B1E24),
    surface: rgb(0x22262E),
    raised: rgb(0x2A2F38),
    line: rgb(0x2C313A),
    line_soft: rgb(0x22252C),
    text: rgb(0xE7E9EE),
    muted: rgb(0x9AA3B2),
    faint: rgb(0x7D8696),
    zebra: rgb(0x181A1F),
    hl: rgba(245, 184, 64, 0.20),
    hl_ink: rgb(0xF5C26B),
    accent: rgb(0x3DD68C),
    accent_soft: rgba(0x3D, 0xD6, 0x8C, 0.15),
    accent_ink: rgb(0x3DD68C),
    ring: rgb(0x3DD68C),
    on_accent: rgb(0x0C1A13),
    scrim: rgba(8, 9, 12, 0.62),
};

pub const LIGHT: Palette = Palette {
    bg: rgb(0xFFFFFF),
    chrome: rgb(0xF6F7F9),
    surface: rgb(0xFFFFFF),
    raised: rgb(0xECEEF2),
    line: rgb(0xE2E5EA),
    line_soft: rgb(0xEEF0F3),
    text: rgb(0x1A1D23),
    muted: rgb(0x576070),
    faint: rgb(0x6B7280),
    zebra: rgb(0xFAFBFC),
    hl: rgba(245, 170, 30, 0.24),
    hl_ink: rgb(0x8A5300),
    accent: rgb(0x3DD68C),
    accent_soft: rgba(0x3D, 0xD6, 0x8C, 0.13),
    // The accent darkened (× 0.55 and × 0.8) to stand out on white.
    accent_ink: rgb(0x22764D),
    ring: rgb(0x31AB70),
    on_accent: rgb(0x0C1A13),
    scrim: rgba(20, 24, 32, 0.38),
};

/// The palette of the theme shown.
pub fn palette(ui: &egui::Ui) -> &'static Palette {
    if ui.visuals().dark_mode {
        &DARK
    } else {
        &LIGHT
    }
}

const SEMIBOLD: &str = "inter-semibold";
const MEDIUM: &str = "inter-medium";
const MONO_SEMIBOLD: &str = "mono-semibold";
const ICONS: &str = "icons";

/// Inter, the interface's font.
pub fn regular(size: f32) -> FontId {
    FontId::proportional(size)
}

pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(MEDIUM.into()))
}

/// Titles, column names.
pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}

/// JetBrains Mono: cell references, row numbers, column letters.
pub fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}

pub fn mono_semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(MONO_SEMIBOLD.into()))
}

/// Phosphor icons first: Inter has glyphs of its own where Phosphor puts
/// its icons (Unicode's private use area).
pub fn icons(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(ICONS.into()))
}

/// `text` laid out in `font`, its icons (private use characters) in the
/// icon font.
pub fn with_icons(text: &str, font: FontId, color: Color32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let format = |font: FontId| egui::TextFormat {
        font_id: font,
        color,
        valign: egui::Align::Center,
        ..Default::default()
    };
    let is_icon = |c: char| ('\u{e000}'..='\u{f8ff}').contains(&c);
    let mut start = 0;
    for (i, c) in text.char_indices() {
        if is_icon(c) {
            if start < i {
                job.append(&text[start..i], 0.0, format(font.clone()));
            }
            let end = i + c.len_utf8();
            job.append(&text[i..end], 0.0, format(icons(font.size)));
            start = end;
        }
    }
    if start < text.len() {
        job.append(&text[start..], 0.0, format(font));
    }
    job
}

/// Fonts, styles of both themes, and the theme chosen last time.
pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    ctx.style_mut_of(Theme::Dark, |s| style(s, &DARK));
    ctx.style_mut_of(Theme::Light, |s| style(s, &LIGHT));
    ctx.set_theme(saved_theme());
}

/// Inter and JetBrains Mono (assets/fonts, OFL), with Phosphor icons and
/// egui's fonts as fallback.
fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    let fallback = fonts.families[&FontFamily::Proportional].clone();
    let mut mono_fallback = fonts.families[&FontFamily::Monospace].clone();
    mono_fallback.push("phosphor".into());
    let faces: [(&str, &'static [u8]); 5] = [
        ("inter", include_bytes!("../assets/fonts/Inter-Regular.ttf")),
        (MEDIUM, include_bytes!("../assets/fonts/Inter-Medium.ttf")),
        (
            SEMIBOLD,
            include_bytes!("../assets/fonts/Inter-SemiBold.ttf"),
        ),
        (
            "mono",
            include_bytes!("../assets/fonts/JetBrainsMono-Medium.ttf"),
        ),
        (
            MONO_SEMIBOLD,
            include_bytes!("../assets/fonts/JetBrainsMono-SemiBold.ttf"),
        ),
    ];
    for (name, data) in faces {
        fonts
            .font_data
            .insert(name.into(), Arc::new(FontData::from_static(data)));
    }
    let family = |first: &str, rest: &[String]| {
        std::iter::once(first.to_owned())
            .chain(rest.iter().cloned())
            .collect::<Vec<_>>()
    };
    let mut icon_fallback = vec!["phosphor".to_owned(), "inter".to_owned()];
    icon_fallback.extend(fallback.iter().filter(|f| *f != "phosphor").cloned());
    let families = [
        (FontFamily::Name(ICONS.into()), icon_fallback),
        (FontFamily::Proportional, family("inter", &fallback)),
        (FontFamily::Name(MEDIUM.into()), family(MEDIUM, &fallback)),
        (
            FontFamily::Name(SEMIBOLD.into()),
            family(SEMIBOLD, &fallback),
        ),
        (FontFamily::Monospace, family("mono", &mono_fallback)),
        (
            FontFamily::Name(MONO_SEMIBOLD.into()),
            family(MONO_SEMIBOLD, &mono_fallback),
        ),
    ];
    fonts.families.extend(families);
    fonts
}

fn style(style: &mut egui::Style, p: &Palette) {
    style.text_styles = [
        (TextStyle::Heading, semibold(17.0)),
        (TextStyle::Body, regular(13.0)),
        (TextStyle::Button, regular(13.0)),
        (TextStyle::Small, regular(11.0)),
        (TextStyle::Monospace, mono(12.0)),
    ]
    .into();
    let s = &mut style.spacing;
    s.item_spacing = vec2(8.0, 6.0);
    s.button_padding = vec2(10.0, 4.0);
    s.interact_size.y = 26.0;
    s.menu_margin = Margin::same(6);
    s.scroll.bar_width = 6.0;
    s.scroll.floating = false;
    s.scroll.dormant_handle_opacity = 0.3;
    s.scroll.active_handle_opacity = 0.45;
    s.scroll.interact_handle_opacity = 0.6;
    s.scroll.dormant_background_opacity = 0.0;
    s.scroll.active_background_opacity = 0.0;
    s.scroll.interact_background_opacity = 0.0;

    let v = &mut style.visuals;
    // Crisp text: glyphs snapped to the pixel (fractional positions blur
    // them) and, on dark backgrounds, a softer contrast than egui's, which
    // thickens light text.
    v.text_options.subpixel_binning = false;
    if v.dark_mode {
        v.text_options.color_transfer_function =
            egui::epaint::FontColorTransferFunction::Gamma(0.8);
    }
    v.override_text_color = None;
    v.weak_text_color = Some(p.faint);
    v.hyperlink_color = p.accent_ink;
    v.faint_bg_color = p.zebra;
    v.extreme_bg_color = p.surface;
    v.text_edit_bg_color = Some(p.surface);
    v.code_bg_color = p.raised;
    v.panel_fill = p.chrome;
    v.window_fill = p.chrome;
    v.window_stroke = Stroke::new(1.0, p.line);
    v.window_corner_radius = CornerRadius::same(14);
    v.menu_corner_radius = CornerRadius::same(10);
    v.window_shadow = Shadow {
        offset: [0, 16],
        blur: 48,
        spread: 0,
        color: Color32::from_black_alpha(if v.dark_mode { 110 } else { 40 }),
    };
    v.popup_shadow = Shadow {
        offset: [0, 6],
        blur: 18,
        spread: 0,
        color: Color32::from_black_alpha(if v.dark_mode { 90 } else { 30 }),
    };
    v.selection.bg_fill = p.accent_soft;
    v.selection.stroke = Stroke::new(1.0, p.accent_ink);
    v.text_cursor.stroke = Stroke::new(2.0, p.accent_ink);
    v.striped = true;

    let radius = CornerRadius::same(8);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.chrome;
    w.noninteractive.weak_bg_fill = p.chrome;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.line);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    w.noninteractive.corner_radius = radius;
    for (state, fill, stroke, text) in [
        (&mut w.inactive, p.surface, p.line, p.text),
        (&mut w.hovered, p.raised, p.line, p.text),
        (&mut w.active, p.raised, p.ring, p.text),
        (&mut w.open, p.raised, p.line, p.text),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.bg_stroke = Stroke::new(1.0, stroke);
        state.fg_stroke = Stroke::new(1.0, text);
        state.corner_radius = radius;
        state.expansion = 0.0;
    }
}

// Theme choice

pub fn saved_theme() -> ThemePreference {
    match std::fs::read_to_string(theme_file())
        .as_deref()
        .map(str::trim)
    {
        Ok("dark") => ThemePreference::Dark,
        Ok("light") => ThemePreference::Light,
        _ => ThemePreference::System,
    }
}

pub fn choose_theme(ctx: &egui::Context, theme: ThemePreference) {
    ctx.set_theme(theme);
    let name = match theme {
        ThemePreference::Dark => "dark",
        ThemePreference::Light => "light",
        ThemePreference::System => "system",
    };
    let file = theme_file();
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(file, name);
}

fn theme_file() -> std::path::PathBuf {
    crate::config::dir().join("theme")
}

// Painted pieces

/// A square icon button, transparent until hovered; `on` gives it the
/// accent's soft fill.
pub fn icon_button(ui: &mut egui::Ui, size: f32, enabled: bool, on: bool) -> egui::Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), sense);
    let p = palette(ui);
    let fill = if on {
        p.accent_soft
    } else if enabled && response.hovered() {
        p.raised
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 8.0, fill);
    response
}

/// The color of an icon button's glyph.
pub fn icon_color(ui: &egui::Ui, response: &egui::Response, enabled: bool, on: bool) -> Color32 {
    let p = palette(ui);
    if !enabled {
        p.faint.gamma_multiply(0.5)
    } else if on {
        p.accent_ink
    } else if response.hovered() {
        p.text
    } else {
        p.muted
    }
}

/// An icon button with a Phosphor glyph.
pub fn glyph_button(
    ui: &mut egui::Ui,
    glyph: &str,
    size: f32,
    glyph_size: f32,
    enabled: bool,
    on: bool,
) -> egui::Response {
    let response = icon_button(ui, size, enabled, on);
    let color = icon_color(ui, &response, enabled, on);
    paint_text(
        ui.painter(),
        response.rect.center(),
        egui::Align2::CENTER_CENTER,
        glyph,
        icons(glyph_size),
        color,
    );
    response
}

/// A thin vertical line between groups.
pub fn vertical_separator(ui: &mut egui::Ui, height: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(1.0, height), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, palette(ui).line);
}

/// A dot between status bar items.
pub fn dot(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(3.0, 3.0), Sense::hover());
    ui.painter()
        .circle_filled(rect.center(), 1.5, palette(ui).faint);
}

/// A filled button in the accent color.
pub fn primary_button(ui: &mut egui::Ui, text: &str, width: f32) -> egui::Response {
    let p = palette(ui);
    let galley = ui
        .painter()
        .layout_job(with_icons(text, semibold(13.0), p.on_accent));
    let size = vec2(width.max(galley.size().x + 36.0), 36.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let fill = if response.hovered() {
        p.accent.gamma_multiply(0.9)
    } else {
        p.accent
    };
    ui.painter().rect_filled(rect, 8.0, fill);
    paint_galley(
        ui.painter(),
        rect.center() - galley.size() / 2.0,
        galley,
        p.on_accent,
    );
    response
}

/// A bordered button on the surface color.
pub fn secondary_button(ui: &mut egui::Ui, text: &str, width: f32) -> egui::Response {
    let p = palette(ui);
    let galley = ui
        .painter()
        .layout_job(with_icons(text, medium(13.0), p.text));
    let size = vec2(width.max(galley.size().x + 32.0), 36.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
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
    paint_galley(
        ui.painter(),
        rect.center() - galley.size() / 2.0,
        galley,
        p.text,
    );
    response
}

/// The frame of dialogs.
pub fn modal(id: &str, ctx: &egui::Context) -> egui::Modal {
    let dark = ctx.global_style().visuals.dark_mode;
    let p = if dark { &DARK } else { &LIGHT };
    egui::Modal::new(egui::Id::new(id))
        .backdrop_color(p.scrim)
        .frame(
            egui::Frame::new()
                .fill(p.chrome)
                .stroke(Stroke::new(1.0, p.line))
                .corner_radius(14)
                .shadow(ctx.global_style().visuals.window_shadow)
                .inner_margin(Margin::same(24)),
        )
}

/// Paints `text` anchored at `pos`, on whole pixels (sharper).
pub fn paint_text(
    painter: &egui::Painter,
    pos: egui::Pos2,
    align: egui::Align2,
    text: impl ToString,
    font: FontId,
    color: Color32,
) {
    let galley = painter.layout_no_wrap(text.to_string(), font, color);
    let rect = align.anchor_size(pos, galley.size());
    paint_galley(painter, rect.min, galley, color);
}

/// Paints `galley` at `pos` rounded to whole pixels (sharper).
pub fn paint_galley(
    painter: &egui::Painter,
    pos: egui::Pos2,
    galley: Arc<egui::Galley>,
    color: Color32,
) {
    use egui::emath::GuiRounding as _;
    painter.galley(
        pos.round_to_pixels(painter.pixels_per_point()),
        galley,
        color,
    );
}

/// Paints `text` in `rect`, cut with "…" when it doesn't fit.
pub fn truncated_text(
    ui: &egui::Ui,
    rect: Rect,
    text: &str,
    font: FontId,
    color: Color32,
    align: egui::Align,
) -> bool {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(rect.width().max(0.0));
    let galley = ui.painter().layout_job(job);
    let truncated = galley.elided;
    let x = match align {
        egui::Align::Max => rect.right() - galley.size().x,
        egui::Align::Center => rect.center().x - galley.size().x / 2.0,
        egui::Align::Min => rect.left(),
    };
    let pos = egui::pos2(x, rect.center().y - galley.size().y / 2.0);
    paint_galley(&ui.painter().with_clip_rect(rect), pos, galley, color);
    truncated
}
