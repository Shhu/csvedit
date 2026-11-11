//! Colors, fonts and widget style, in a dark and a light theme, and the
//! small painted pieces the interface is made of (icon buttons, chips…).

use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Rect,
    Sense, Shadow, Stroke, StrokeKind, TextStyle, Theme, ThemePreference, Vec2, vec2,
};

/// The colors of a theme.
pub struct Palette {
    pub dark: bool,
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
    /// The accent under the pointer: a little darker, opaque.
    pub accent_hover: Color32,
}

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

const fn rgba(r: u8, g: u8, b: u8, alpha: f32) -> Color32 {
    Color32::from_rgba_unmultiplied_const(r, g, b, (alpha * 255.0) as u8)
}

pub const DARK: Palette = Palette {
    dark: true,
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
    accent_hover: rgb(0x35BD7B),
};

pub const LIGHT: Palette = Palette {
    dark: false,
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
    accent_hover: rgb(0x34B878),
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
    ctx.set_fonts(fonts(&lock(&SYSTEM)));
    ctx.style_mut_of(Theme::Dark, |s| style(s, &DARK));
    ctx.style_mut_of(Theme::Light, |s| style(s, &LIGHT));
    ctx.set_theme(saved_theme());
    // Ctrl+plus and Ctrl+minus insert and delete rows, not zoom.
    ctx.options_mut(|o| o.zoom_with_keyboard = false);
}

/// The fonts loaded: (name, bytes, from the system).
static LOADED_FONTS: Mutex<Vec<(String, u64, bool)>> = Mutex::new(Vec::new());

/// The system's fonts read so far (they take megabytes each: only those
/// of the scripts some text needs), by name.
static SYSTEM: Mutex<Vec<(String, Arc<FontData>)>> = Mutex::new(Vec::new());

/// The scripts whose system fonts were asked for (`script` bits).
static REQUESTED: AtomicU16 = AtomicU16::new(0);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The fonts loaded, for the diagnostics.
pub fn loaded_fonts() -> Vec<(String, u64, bool)> {
    lock(&LOADED_FONTS).clone()
}

/// The system's fonts of every script were asked for: no text needs
/// checking any more.
pub fn system_fonts_all_requested() -> bool {
    REQUESTED.load(Ordering::Relaxed) == script::ALL
}

/// The scripts, one bit each, that the system's fonts are looked for by.
mod script {
    pub const HAN: u16 = 1;
    pub const KANA: u16 = 1 << 1;
    pub const HANGUL: u16 = 1 << 2;
    pub const ARABIC: u16 = 1 << 3;
    pub const HEBREW: u16 = 1 << 4;
    pub const THAI: u16 = 1 << 5;
    pub const INDIC: u16 = 1 << 6;
    /// Anything else (symbols, other scripts).
    pub const OTHER: u16 = 1 << 7;
    pub const ALL: u16 = (1 << 8) - 1;
    pub const CJK: u16 = HAN | KANA | HANGUL;

    pub const NAMES: [(u16, &str); 8] = [
        (HAN, "Chinese characters"),
        (KANA, "Japanese kana"),
        (HANGUL, "Korean"),
        (ARABIC, "Arabic"),
        (HEBREW, "Hebrew"),
        (THAI, "Thai"),
        (INDIC, "Indian scripts"),
        (OTHER, "other characters"),
    ];

    /// The script of `c`, for its fallback font.
    pub fn of(c: char) -> u16 {
        match u32::from(c) {
            0x0590..=0x05FF | 0xFB1D..=0xFB4F => HEBREW,
            0x0600..=0x06FF | 0x0750..=0x077F | 0x0870..=0x08FF => ARABIC,
            0xFB50..=0xFDFF | 0xFE70..=0xFEFF => ARABIC,
            0x0900..=0x0DFF => INDIC,
            0x0E00..=0x0E7F => THAI,
            0x1100..=0x11FF | 0x3130..=0x318F | 0xA960..=0xA97F | 0xAC00..=0xD7FF => HANGUL,
            0xFFA0..=0xFFDF => HANGUL,
            0x3040..=0x30FF | 0x31F0..=0x31FF | 0xFF65..=0xFF9F | 0x1B000..=0x1B16F => KANA,
            0x2E80..=0x2FDF | 0x3000..=0x303F | 0x3100..=0x312F | 0x31A0..=0x31EF => HAN,
            0x3200..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0xFE30..=0xFE4F => HAN,
            0xFF00..=0xFFEF | 0x20000..=0x3FFFF => HAN,
            _ => OTHER,
        }
    }

    /// "Chinese characters, Korean".
    pub fn names(scripts: u16) -> String {
        NAMES
            .iter()
            .filter(|(bit, _)| scripts & bit != 0)
            .map(|(_, name)| *name)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// The characters the built-in fonts of the interface's text have
/// (Inter, then egui's fallbacks and emoji, and Phosphor), sorted.
///
/// Read from their character maps: egui's `has_glyph` counts the
/// characters of its replacement font (Noto Emoji, the first with "◻")
/// as missing.
static BUILT_IN: LazyLock<Vec<u32>> = LazyLock::new(|| {
    use skrifa::MetadataProvider as _;
    let fonts = built_in_fonts();
    let mut chars: Vec<u32> = fonts.families[&FontFamily::Proportional]
        .iter()
        .filter_map(|name| fonts.font_data.get(name))
        .filter_map(|data| skrifa::FontRef::from_index(&data.font, data.index).ok())
        .flat_map(|font| {
            font.charmap()
                .mappings()
                .map(|(c, _)| c)
                .collect::<Vec<_>>()
        })
        .collect();
    chars.sort_unstable();
    chars.dedup();
    chars
});

/// The scripts of `texts`' characters that the built-in fonts lack,
/// leaving out those already asked for.
pub fn scripts_needed<'a>(texts: impl IntoIterator<Item = &'a str>) -> u16 {
    scripts_lacking(texts, REQUESTED.load(Ordering::Relaxed))
}

fn scripts_lacking<'a>(texts: impl IntoIterator<Item = &'a str>, requested: u16) -> u16 {
    if requested == script::ALL {
        return 0;
    }
    let mut known = requested;
    for text in texts.into_iter().filter(|text| !text.is_ascii()) {
        for c in text.chars().filter(|c| !c.is_ascii() && !invisible(*c)) {
            let script = script::of(c);
            if known & script == 0 && BUILT_IN.binary_search(&u32::from(c)).is_err() {
                known |= script;
                if known == script::ALL {
                    return known & !requested;
                }
            }
        }
    }
    known & !requested
}

/// Loads the system's fonts for the scripts of `texts`' characters that
/// the built-in fonts lack (in the background).
pub fn cover<'a>(ctx: &egui::Context, texts: impl IntoIterator<Item = &'a str>) {
    load(ctx, scripts_needed(texts));
}

/// Loads the system's fonts for `scripts`, those not asked for yet, in
/// the background: reading them takes tens of megabytes at most and
/// walks the font folders. They are given to egui once read.
pub fn load(ctx: &egui::Context, scripts: u16) {
    let new = scripts & !REQUESTED.fetch_or(scripts, Ordering::Relaxed);
    if new == 0 {
        return;
    }
    crate::diag::log(
        crate::diag::Level::Info,
        &format!("loading the system's fonts for {}", script::names(new)),
    );
    let ctx = ctx.clone();
    let spawned = std::thread::Builder::new()
        .name("system fonts".to_owned())
        .spawn(move || {
            let found = crate::diag::measure("fonts.system", || read_system_fonts(new));
            if found.is_empty() {
                return;
            }
            // Held while egui is given the fonts, so that two loads give
            // it every font.
            let mut system = lock(&SYSTEM);
            for (name, data) in found {
                if !system.iter().any(|(n, _)| *n == name) {
                    system.push((name, Arc::new(FontData::from_owned(data))));
                }
            }
            ctx.set_fonts(fonts(&system));
            ctx.request_repaint();
        });
    if spawned.is_err() {
        crate::diag::log(
            crate::diag::Level::Warning,
            "couldn't start reading the system's fonts",
        );
    }
}

/// Characters drawn as nothing, which no font needs to have.
fn invisible(c: char) -> bool {
    matches!(c, '\u{200B}'..='\u{200F}' | '\u{2028}'..='\u{202E}' | '\u{2060}'..='\u{2064}' | '\u{FEFF}')
}

/// Inter and JetBrains Mono (assets/fonts, OFL), with Phosphor icons and
/// egui's fonts as fallback.
fn built_in_fonts() -> FontDefinitions {
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

/// The built-in fonts, then `system`'s for the scripts Inter lacks
/// (Chinese, Japanese, Korean, Arabic, Hebrew, Thai, Indian scripts).
fn fonts(system: &[(String, Arc<FontData>)]) -> FontDefinitions {
    let mut fonts = built_in_fonts();
    let mut loaded: Vec<(String, u64, bool)> = fonts
        .font_data
        .iter()
        .map(|(name, data)| (name.clone(), data.font.len() as u64, false))
        .collect();
    for (name, data) in system {
        loaded.push((
            name.trim_start_matches("system-").to_owned(),
            data.font.len() as u64,
            true,
        ));
        fonts.font_data.insert(name.clone(), Arc::clone(data));
        for family in fonts.families.values_mut() {
            family.push(name.clone());
        }
    }
    *lock(&LOADED_FONTS) = loaded;
    fonts
}

/// The system's fonts that may cover other scripts: groups of file
/// names, the first found of each group taken, and the scripts it is
/// read for.
fn system_candidates() -> &'static [(&'static [&'static str], u16)] {
    use script::*;
    if cfg!(windows) {
        &[
            (&["segoeui.ttf"], OTHER | ARABIC | HEBREW),
            (&["msyh.ttc"], HAN),
            (&["YuGothR.ttc"], KANA),
            (&["malgun.ttf"], HANGUL),
            (&["Nirmala.ttf"], INDIC),
            (&["LeelawUI.ttf"], THAI),
        ]
    } else {
        &[
            (
                &[
                    "NotoSansCJK-Regular.ttc",
                    // Variable fonts (Fedora), read at their default
                    // instance.
                    "NotoSansCJK-VF.ttc",
                    "NotoSansCJK-VF.otf.ttc",
                    "NotoSansCJKsc-Regular.otf",
                    "NotoSansCJKsc-VF.otf",
                    "DroidSansFallbackFull.ttf",
                    "wqy-microhei.ttc",
                ],
                CJK,
            ),
            (
                &[
                    "NotoSansArabic-Regular.ttf",
                    "NotoSansArabic[wdth,wght].ttf",
                ],
                ARABIC,
            ),
            (
                &[
                    "NotoSansHebrew-Regular.ttf",
                    "NotoSansHebrew[wdth,wght].ttf",
                ],
                HEBREW,
            ),
            (
                &["NotoSansThai-Regular.ttf", "NotoSansThai[wdth,wght].ttf"],
                THAI,
            ),
            (
                &[
                    "NotoSansDevanagari-Regular.ttf",
                    "NotoSansDevanagari[wdth,wght].ttf",
                ],
                INDIC,
            ),
            (&["DejaVuSans.ttf"], OTHER | ARABIC | HEBREW),
        ]
    }
}

/// The folders the system's fonts are in.
fn font_dirs() -> Vec<std::path::PathBuf> {
    if cfg!(windows) {
        let windir = std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into());
        return vec![std::path::Path::new(&windir).join("Fonts")];
    }
    let mut dirs: Vec<std::path::PathBuf> = [
        "/usr/share/fonts",
        "/usr/local/share/fonts",
        // The host's fonts, inside the Flatpak sandbox.
        "/run/host/fonts",
        "/run/host/user-fonts",
    ]
    .iter()
    .map(Into::into)
    .collect();
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(std::path::Path::new(&home).join(".local/share/fonts"));
    }
    dirs
}

/// Where the candidate files are (by lowercase name), found in one walk
/// of the font folders, the first time.
fn font_files() -> &'static std::collections::HashMap<String, std::path::PathBuf> {
    static FILES: std::sync::OnceLock<std::collections::HashMap<String, std::path::PathBuf>> =
        std::sync::OnceLock::new();
    FILES.get_or_init(|| {
        let wanted: std::collections::HashSet<String> = system_candidates()
            .iter()
            .flat_map(|(names, _)| names.iter())
            .map(|name| name.to_ascii_lowercase())
            .collect();
        let mut found = std::collections::HashMap::new();
        for dir in font_dirs() {
            walk(&dir, 4, &wanted, &mut found);
        }
        found
    })
}

/// Notes the `wanted` files in `dir` and its subfolders, `depth` levels
/// down at most, the shallower first.
fn walk(
    dir: &std::path::Path,
    depth: usize,
    wanted: &std::collections::HashSet<String>,
    found: &mut std::collections::HashMap<String, std::path::PathBuf>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut subdirs = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if kind.is_dir() {
            if depth > 0 {
                subdirs.push(path);
            }
        } else if let Some(name) = entry.file_name().to_str()
            && wanted.contains(&name.to_ascii_lowercase())
            && path.is_file()
        {
            found.entry(name.to_ascii_lowercase()).or_insert(path);
        }
    }
    subdirs.sort();
    for subdir in subdirs {
        walk(&subdir, depth - 1, wanted, found);
    }
}

/// The system's fonts for `scripts` not read yet: (name, bytes).
fn read_system_fonts(scripts: u16) -> Vec<(String, Vec<u8>)> {
    let files = font_files();
    let loaded: Vec<String> = lock(&SYSTEM).iter().map(|(n, _)| n.clone()).collect();
    let mut found = Vec::new();
    for (names, covers) in system_candidates() {
        if covers & scripts == 0 {
            continue;
        }
        let Some((file, path)) = names
            .iter()
            .find_map(|name| Some((*name, files.get(&name.to_ascii_lowercase())?)))
        else {
            continue;
        };
        let name = format!("system-{file}");
        if loaded.contains(&name) || found.iter().any(|(n, _)| *n == name) {
            continue;
        }
        if let Ok(data) = std::fs::read(path) {
            found.push((name, data));
        }
    }
    found
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
    // Floating bars (the only ones egui fades), given their own room so
    // that they don't cover the cells.
    s.scroll.floating = true;
    s.scroll.floating_width = 6.0;
    s.scroll.floating_allocated_width = 6.0;
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
pub fn icon_button(
    ui: &mut egui::Ui,
    size: f32,
    enabled: bool,
    on: bool,
    label: &str,
) -> egui::Response {
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
    focus_ring(ui, &response, 8.0);
    // Painted: named for screen readers by its label.
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    response
}

/// A ring around a widget that has the keyboard.
pub fn focus_ring(ui: &egui::Ui, response: &egui::Response, radius: f32) {
    if response.has_focus() {
        ui.painter().rect_stroke(
            response.rect,
            radius,
            Stroke::new(2.0, palette(ui).ring),
            StrokeKind::Inside,
        );
    }
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
    label: &str,
) -> egui::Response {
    let response = icon_button(ui, size, enabled, on, label);
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
        p.accent_hover
    } else {
        p.accent
    };
    ui.painter().rect_filled(rect, 8.0, fill);
    focus_ring(ui, &response, 8.0);
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), text));
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
    focus_ring(ui, &response, 8.0);
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), text));
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

/// `key` was pressed, for the dialog `ui` is in, when it is the topmost
/// one: the key is then taken, so that a dialog under it doesn't get it
/// too.
pub fn dialog_key(ui: &egui::Ui, key: egui::Key) -> bool {
    let top = ui.ctx().memory(|m| m.top_modal_layer()) == Some(ui.layer_id());
    top && ui
        .ctx()
        .input_mut(|i| i.consume_key(egui::Modifiers::NONE, key))
}

/// The largest size a dialog's frame (its margins in) may take: the
/// window less a margin around it, and at most `max`.
pub fn dialog_room(ctx: &egui::Context, max: egui::Vec2) -> egui::Vec2 {
    let screen = ctx.content_rect().size();
    (screen - egui::vec2(32.0, 32.0))
        .max(egui::vec2(200.0, 120.0))
        .min(max)
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
    text: impl Into<String>,
    font: FontId,
    color: Color32,
    align: egui::Align,
) -> bool {
    let mut job = egui::text::LayoutJob::simple_singleline(text.into(), font, color);
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

#[cfg(test)]
mod tests {
    use super::script::{self, *};

    #[test]
    fn system_fonts_wait_for_text_that_needs_them() {
        let lacking = |texts: &[&str]| super::scripts_lacking(texts.iter().copied(), 0);
        // Accents, Greek, Cyrillic, emoji (Noto Emoji, egui's) are built in.
        assert_eq!(
            lacking(&[
                "plain",
                "café, Ελλάδα, Москва, 25 €",
                "\u{FEFF}x",
                "👍 😀 ✅"
            ]),
            0
        );
        assert_eq!(lacking(&["東京"]), HAN);
        assert_eq!(lacking(&["東京", "ひらがな", "서울"]), HAN | KANA | HANGUL);
        assert_eq!(lacking(&["שלום", "مرحبا"]), HEBREW | ARABIC);
        // Scripts already asked for are left out.
        assert_eq!(super::scripts_lacking(["東京 ไทย"], HAN), THAI);
        assert_eq!(super::scripts_lacking(["東京"], script::ALL), 0);
    }

    #[test]
    fn characters_belong_to_their_script() {
        for (c, s) in [
            ('東', HAN),
            ('。', HAN),
            ('あ', KANA),
            ('ｱ', KANA),
            ('한', HANGUL),
            ('ש', HEBREW),
            ('ش', ARABIC),
            ('ก', THAI),
            ('क', INDIC),
            ('ᚠ', OTHER),
        ] {
            assert_eq!(script::of(c), s, "{c}");
        }
        assert_eq!(script::names(HAN | THAI), "Chinese characters, Thai");
    }

    #[test]
    fn each_script_has_a_candidate_font() {
        let covered = super::system_candidates()
            .iter()
            .fold(0, |all, (_, scripts)| all | scripts);
        assert_eq!(covered, script::ALL);
    }
}
