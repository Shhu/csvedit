//! How columns are named in the headers (A B C, 1 2 3 or 0 1 2), and the
//! lists of columns typed to choose which ones are shown ("3, 157, 10-20").

use std::sync::atomic::{AtomicU8, Ordering};

use crate::i18n::t;

/// How columns are named.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Labels {
    /// A, B, … Z, AA…
    Letters,
    /// 1, 2, 3…
    FromOne,
    /// 0, 1, 2… (as programs count)
    FromZero,
}

impl Labels {
    pub const ALL: [Labels; 3] = [Labels::Letters, Labels::FromOne, Labels::FromZero];

    /// The first three names, for menus.
    pub fn example(self) -> &'static str {
        match self {
            Labels::Letters => "A B C",
            Labels::FromOne => "1 2 3",
            Labels::FromZero => "0 1 2",
        }
    }

    fn code(self) -> &'static str {
        match self {
            Labels::Letters => "letters",
            Labels::FromOne => "from-one",
            Labels::FromZero => "from-zero",
        }
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

pub fn labels() -> Labels {
    Labels::ALL[usize::from(CURRENT.load(Ordering::Relaxed))]
}

fn set(labels: Labels) {
    let index = Labels::ALL
        .iter()
        .position(|&l| l == labels)
        .expect("known");
    CURRENT.store(index as u8, Ordering::Relaxed);
}

/// The naming chosen last time.
pub fn init() {
    let saved = std::fs::read_to_string(settings_file()).unwrap_or_default();
    if let Some(labels) = Labels::ALL.into_iter().find(|l| l.code() == saved.trim()) {
        set(labels);
    }
}

/// Switches naming and remembers it for the next launches.
pub fn choose(labels: Labels) {
    set(labels);
    let file = settings_file();
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(file, labels.code());
}

fn settings_file() -> std::path::PathBuf {
    crate::config::dir().join("column-labels")
}

/// A, B, … Z, AA, AB…
pub fn letters(mut col: usize) -> String {
    let mut name = Vec::new();
    loop {
        name.push(b'A' + (col % 26) as u8);
        if col < 26 {
            break;
        }
        col = col / 26 - 1;
    }
    name.reverse();
    String::from_utf8(name).expect("ASCII letters")
}

/// Column `col`'s name in the chosen naming.
pub fn label(col: usize) -> String {
    match labels() {
        Labels::Letters => letters(col),
        Labels::FromOne => (col + 1).to_string(),
        Labels::FromZero => col.to_string(),
    }
}

/// A cell's reference: "D4", or "R4 C3" when columns are numbered.
pub fn reference(row: usize, col: usize) -> String {
    match labels() {
        Labels::Letters => format!("{}{}", letters(col), row + 1),
        _ => t!("cell-reference", row = row + 1, col = label(col)),
    }
}

/// The columns a list names, in its order and without repeats: numbers in
/// the chosen naming (counted from 1 with letters), letters, and ranges
/// ("10-20", "C-F"), separated by commas, semicolons or spaces. Gives the
/// first item that names no column when there is one.
pub fn parse_list(text: &str, count: usize) -> Result<Vec<usize>, String> {
    let one = |item: &str| -> Option<usize> {
        let item = item.trim();
        let col = if item.chars().all(|c| c.is_ascii_digit()) && !item.is_empty() {
            let n: usize = item.parse().ok()?;
            match labels() {
                Labels::FromZero => n,
                _ => n.checked_sub(1)?,
            }
        } else if item.chars().all(|c| c.is_ascii_alphabetic()) && !item.is_empty() {
            item.to_ascii_uppercase().bytes().try_fold(0usize, |n, b| {
                n.checked_mul(26)?.checked_add(usize::from(b - b'A') + 1)
            })? - 1
        } else {
            return None;
        };
        (col < count).then_some(col)
    };
    let mut cols = Vec::new();
    for token in text
        .split([',', ';', ' ', '\t'])
        .filter(|t| !t.trim().is_empty())
    {
        let range = token
            .split_once("..")
            .or_else(|| token.split_once('-'))
            .or_else(|| token.split_once(':'));
        let found = match range {
            Some((from, to)) => one(from).zip(one(to)).map(|(a, b)| {
                if a <= b {
                    (a..=b).collect::<Vec<_>>()
                } else {
                    (b..=a).rev().collect()
                }
            }),
            None => one(token).map(|c| vec![c]),
        };
        let Some(found) = found else {
            return Err(token.trim().to_owned());
        };
        for col in found {
            if !cols.contains(&col) {
                cols.push(col);
            }
        }
    }
    Ok(cols)
}

/// A list of columns as `parse_list` reads it, with runs as ranges.
pub fn format_list(cols: &[usize]) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut i = 0;
    while i < cols.len() {
        let mut j = i;
        while j + 1 < cols.len() && cols[j + 1] == cols[j] + 1 {
            j += 1;
        }
        parts.push(if j >= i + 2 {
            format!("{}-{}", label(cols[i]), label(cols[j]))
        } else {
            (i..=j)
                .map(|k| label(cols[k]))
                .collect::<Vec<_>>()
                .join(", ")
        });
        i = j + 1;
    }
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    // One test: the naming is shared by the whole program.
    #[test]
    fn names_and_lists_of_columns() {
        set(Labels::Letters);
        assert_eq!(letters(0), "A");
        assert_eq!(letters(25), "Z");
        assert_eq!(letters(26), "AA");
        assert_eq!(letters(701), "ZZ");
        assert_eq!(letters(702), "AAA");
        assert_eq!(reference(3, 3), "D4");
        assert_eq!(parse_list("3, 157", 334), Ok(vec![2, 156]));
        assert_eq!(parse_list("c;fa  b-d", 334), Ok(vec![2, 156, 1, 3]));
        assert_eq!(parse_list("5-3", 10), Ok(vec![4, 3, 2]));
        assert_eq!(parse_list("2, 400", 334), Err("400".into()));
        assert_eq!(parse_list("0", 10), Err("0".into()));
        assert_eq!(parse_list("x1", 10), Err("x1".into()));
        assert_eq!(format_list(&[0, 1, 2, 3, 9, 11, 12]), "A-D, J, L, M");

        set(Labels::FromZero);
        assert_eq!(label(3), "3");
        assert_eq!(parse_list("0, 2..4", 10), Ok(vec![0, 2, 3, 4]));
        assert_eq!(format_list(&[0, 2, 3, 4]), "0, 2-4");

        set(Labels::FromOne);
        assert_eq!(label(3), "4");
        assert_eq!(parse_list("1:2", 10), Ok(vec![0, 1]));
        set(Labels::Letters);
    }
}
