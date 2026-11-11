//! UI translations. Each language has its own Fluent file in `locales/`
//! (same keys, in the same order), embedded in the executable. English is the
//! fallback: a key missing from a translation is shown in English.
//!
//! In code, `t!("key")` or `t!("key", name = value)` gives the text in the
//! chosen language.

use std::path::PathBuf;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource, FluentValue};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    En,
    Fr,
}

impl Lang {
    /// English first: it is the fallback language.
    pub const ALL: [Lang; 2] = [Lang::En, Lang::Fr];

    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Fr => "fr",
        }
    }

    /// Name of the language in that language, for the menu.
    pub fn native_name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Fr => "Français",
        }
    }

    fn source(self) -> &'static str {
        match self {
            Lang::En => include_str!("../locales/en.ftl"),
            Lang::Fr => include_str!("../locales/fr.ftl"),
        }
    }

    /// Accepts "fr", "fr-CA", "fr_FR.UTF-8"…
    fn from_code(code: &str) -> Option<Lang> {
        let primary = code.split(['-', '_', '.']).next()?.to_ascii_lowercase();
        Lang::ALL.into_iter().find(|l| l.code() == primary)
    }

    /// What goes between each group of three digits: a narrow no-break
    /// space in French, a comma in English.
    fn thousands(self) -> &'static str {
        match self {
            Lang::En => ",",
            Lang::Fr => "\u{202f}",
        }
    }

    fn index(self) -> usize {
        Lang::ALL
            .iter()
            .position(|&l| l == self)
            .expect("known language")
    }
}

static CURRENT: AtomicUsize = AtomicUsize::new(0);

static BUNDLES: LazyLock<Vec<FluentBundle<FluentResource>>> =
    LazyLock::new(|| Lang::ALL.iter().map(|&l| bundle(l)).collect());

fn bundle(lang: Lang) -> FluentBundle<FluentResource> {
    let id = lang.code().parse().expect("valid language code");
    let mut bundle = FluentBundle::new_concurrent(vec![id]);
    // No Unicode isolation marks around values: egui's fonts show them as
    // empty boxes.
    bundle.set_use_isolating(false);
    // Counts with their thousands separators ("240,567", "240 567").
    match lang {
        Lang::En => bundle.set_formatter(Some(format_number::<En, _>)),
        Lang::Fr => bundle.set_formatter(Some(format_number::<Fr, _>)),
    }
    // Syntax errors are caught by the tests; whatever parses is kept.
    let resource = FluentResource::try_new(lang.source().to_owned()).unwrap_or_else(|(r, _)| r);
    let _ = bundle.add_resource(resource);
    bundle
}

/// The languages as types, for the bundles' formatters (plain functions).
trait Language {
    const LANG: Lang;
}
struct En;
struct Fr;
impl Language for En {
    const LANG: Lang = Lang::En;
}
impl Language for Fr {
    const LANG: Lang = Lang::Fr;
}

/// A whole number given to a message, written with the language's
/// thousands separators; anything else as Fluent writes it.
fn format_number<L: Language, M>(value: &FluentValue, _: &M) -> Option<String> {
    let FluentValue::Number(number) = value else {
        return None;
    };
    let whole = number.value.fract() == 0.0 && number.value.abs() < 9e15;
    let plain = number.options.minimum_fraction_digits.is_none()
        && number.options.minimum_integer_digits.is_none();
    (whole && plain && number.options.use_grouping).then(|| {
        let digits = grouped_in(number.value.abs() as u64, L::LANG);
        if number.value < 0.0 {
            format!("-{digits}")
        } else {
            digits
        }
    })
}

/// `n` with the chosen language's thousands separators, for counts shown
/// outside messages.
pub fn number(n: impl Into<u64>) -> String {
    grouped_in(n.into(), lang())
}

/// `n` with `lang`'s thousands separators.
fn grouped_in(n: u64, lang: Lang) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 * 3);
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push_str(lang.thousands());
        }
        out.push(digit);
    }
    out
}

pub fn lang() -> Lang {
    Lang::ALL[CURRENT.load(Ordering::Relaxed)]
}

/// Language at startup: the one picked in the menu, else the system's, else
/// English.
pub fn init() {
    let saved = std::fs::read_to_string(settings_file()).ok();
    let lang = saved
        .as_deref()
        .map(str::trim)
        .and_then(Lang::from_code)
        .or_else(|| {
            sys_locale::get_locale()
                .as_deref()
                .and_then(Lang::from_code)
        })
        .unwrap_or(Lang::En);
    CURRENT.store(lang.index(), Ordering::Relaxed);
}

/// Switches language and remembers it for the next launches.
pub fn choose(lang: Lang) {
    CURRENT.store(lang.index(), Ordering::Relaxed);
    let file = settings_file();
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(file, lang.code());
}

fn settings_file() -> PathBuf {
    crate::config::dir().join("language")
}

/// Text of key `id` in the chosen language, falling back to English, then to
/// the key itself if it exists nowhere.
pub fn tr(id: &str, args: Option<&FluentArgs>) -> String {
    let bundles = &*BUNDLES;
    let current = &bundles[CURRENT.load(Ordering::Relaxed)];
    for bundle in [current, &bundles[0]] {
        if let Some(pattern) = bundle.get_message(id).and_then(|m| m.value()) {
            let mut errors = Vec::new();
            return bundle
                .format_pattern(pattern, args, &mut errors)
                .into_owned();
        }
    }
    id.to_owned()
}

macro_rules! t {
    ($id:literal) => {
        $crate::i18n::tr($id, None)
    };
    ($id:literal, $($name:ident = $value:expr),+ $(,)?) => {{
        let mut args = fluent_bundle::FluentArgs::new();
        $(args.set(stringify!($name), $value);)+
        $crate::i18n::tr($id, Some(&args))
    }};
}
pub(crate) use t;

#[cfg(test)]
mod tests {
    use super::*;
    use fluent_syntax::ast;
    use std::collections::BTreeSet;

    /// Each message of a language, in file order: its key and the
    /// variables it uses.
    fn messages(lang: Lang) -> Vec<(String, BTreeSet<String>)> {
        let resource = fluent_syntax::parser::parse(lang.source())
            .unwrap_or_else(|(_, errors)| panic!("{}.ftl: {errors:?}", lang.code()));
        resource
            .body
            .iter()
            .filter_map(|entry| match entry {
                ast::Entry::Message(m) => {
                    let mut vars = BTreeSet::new();
                    if let Some(value) = &m.value {
                        collect_vars(value, &mut vars);
                    }
                    Some((m.id.name.to_owned(), vars))
                }
                _ => None,
            })
            .collect()
    }

    fn collect_vars(pattern: &ast::Pattern<&str>, vars: &mut BTreeSet<String>) {
        for element in &pattern.elements {
            if let ast::PatternElement::Placeable { expression } = element {
                collect_expression(expression, vars);
            }
        }
    }

    fn collect_expression(expression: &ast::Expression<&str>, vars: &mut BTreeSet<String>) {
        match expression {
            ast::Expression::Inline(ast::InlineExpression::VariableReference { id }) => {
                vars.insert(id.name.to_owned());
            }
            ast::Expression::Select { selector, variants } => {
                if let ast::InlineExpression::VariableReference { id } = selector {
                    vars.insert(id.name.to_owned());
                }
                for v in variants {
                    collect_vars(&v.value, vars);
                }
            }
            _ => {}
        }
    }

    /// Keys given to `t!` in the sources.
    fn used_keys() -> BTreeSet<String> {
        let mut keys = BTreeSet::new();
        let mut dirs = vec![std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src")];
        let mut files = Vec::new();
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(dir).expect("source folder") {
                let path = entry.expect("entry").path();
                if path.is_dir() {
                    dirs.push(path);
                } else {
                    files.push(path);
                }
            }
        }
        for file in files {
            let source = std::fs::read_to_string(file).expect("source");
            let code: Vec<&str> = source
                .lines()
                .filter(|l| !l.trim_start().starts_with("//"))
                .collect();
            let code = code.join("\n");
            for (i, _) in code.match_indices("t!(") {
                // Not the end of `format!(` and the like, nor this string.
                if code[..i].ends_with(|c: char| c.is_alphanumeric() || c == '_' || c == '"') {
                    continue;
                }
                if let Some(rest) = code[i + 3..].trim_start().strip_prefix('"') {
                    keys.insert(rest[..rest.find('"').expect("closing quote")].to_owned());
                }
            }
        }
        keys
    }

    #[test]
    fn every_language_has_the_same_keys_in_the_same_order() {
        let en = messages(Lang::En);
        for lang in Lang::ALL {
            let other = messages(lang);
            let keys = |m: &[(String, BTreeSet<String>)]| {
                m.iter().map(|(k, _)| k.clone()).collect::<Vec<_>>()
            };
            assert_eq!(keys(&other), keys(&en), "{}: keys", lang.code());
            for ((key, vars), (_, other_vars)) in en.iter().zip(&other) {
                assert_eq!(vars, other_vars, "{}: variables of {key}", lang.code());
            }
        }
    }

    #[test]
    fn every_key_is_translated_and_used() {
        let en: BTreeSet<String> = messages(Lang::En).into_iter().map(|(k, _)| k).collect();
        let used = used_keys();
        let undefined: Vec<_> = used.difference(&en).collect();
        let unused: Vec<_> = en.difference(&used).collect();
        assert!(
            undefined.is_empty(),
            "keys missing from en.ftl: {undefined:?}"
        );
        assert!(unused.is_empty(), "unused keys: {unused:?}");
    }

    /// French puts a no-break space before : ; ! ? » and after «: never a
    /// breakable one, nor none at all. Checked in fr.ftl and in the French
    /// texts of the Flatpak's description.
    #[test]
    fn french_typography() {
        let metainfo = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("packaging/flatpak/io.github.Shhu.CsvEdit.metainfo.xml"),
        )
        .expect("metainfo");
        let ftl = Lang::Fr
            .source()
            .lines()
            .filter(|l| !l.starts_with('#'))
            // The text after the key.
            .map(|l| l.split_once(" = ").map_or(l, |(_, v)| v));
        let xml = metainfo.lines().filter(|l| l.contains("xml:lang=\"fr\""));
        let no_break = |c: char| c == '\u{a0}' || c == '\u{202f}';
        for line in ftl.chain(xml) {
            let chars: Vec<char> = line.chars().collect();
            for (i, &c) in chars.iter().enumerate() {
                let before = i.checked_sub(1).map(|j| chars[j]);
                let after = chars.get(i + 1).copied();
                // "!#" and "(!" are not punctuation of a sentence.
                let mark = matches!(c, ':' | ';' | '!' | '?' | '»')
                    && after.is_none_or(char::is_whitespace)
                    && before.is_some_and(|b| b != '(');
                if mark {
                    assert!(
                        before.is_some_and(no_break),
                        "fr: no no-break space before {c:?}: {line}"
                    );
                }
                if c == '«' {
                    assert!(
                        after.is_some_and(no_break),
                        "fr: no no-break space after «: {line}"
                    );
                }
            }
        }
    }

    #[test]
    fn groups_thousands() {
        assert_eq!(grouped_in(0, Lang::En), "0");
        assert_eq!(grouped_in(999, Lang::En), "999");
        assert_eq!(grouped_in(1000, Lang::En), "1,000");
        assert_eq!(grouped_in(240_567, Lang::En), "240,567");
        assert_eq!(grouped_in(3_608_505, Lang::Fr), "3\u{202f}608\u{202f}505");
        assert_eq!(grouped_in(12_345, Lang::Fr), "12\u{202f}345");
    }

    #[test]
    fn messages_group_counts_and_keep_plurals() {
        let format = |lang: Lang, rows: usize| {
            let bundle = bundle(lang);
            let pattern = bundle
                .get_message("import-data-rows")
                .and_then(|m| m.value())
                .expect("import-data-rows");
            let mut args = FluentArgs::new();
            args.set("count", rows);
            bundle
                .format_pattern(pattern, Some(&args), &mut Vec::new())
                .into_owned()
        };
        assert_eq!(format(Lang::En, 1), "1 data row");
        assert_eq!(format(Lang::En, 240_567), "240,567 data rows");
        assert!(format(Lang::Fr, 240_567).starts_with("240\u{202f}567 "));
    }

    #[test]
    fn recognizes_system_locales() {
        assert_eq!(Lang::from_code("fr-FR"), Some(Lang::Fr));
        assert_eq!(Lang::from_code("fr_CA.UTF-8"), Some(Lang::Fr));
        assert_eq!(Lang::from_code("en-US"), Some(Lang::En));
        assert_eq!(Lang::from_code("de-DE"), None);
    }
}
