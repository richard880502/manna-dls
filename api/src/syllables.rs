//! Syllable division from a dictionary, so a 「拆音節」 answer never depends on the model.
//!
//! The model gets syllable division wrong often enough to matter (it wrote "dra-ma-tic" for
//! dra-mat-ic). The bundled list is Moby Hyphenator II (public domain; see
//! `research/syllables/README.md`): lowercase single words, about 137,000 of them. A few regular
//! inflections that the list leaves out (habits, reminder, sorrowful) are derived from their stem.

use std::collections::HashMap;
use std::io::Read;
use std::sync::LazyLock;

use regex::Regex;

static DATA: &[u8] = include_bytes!("../data/syllables.txt.gz");

/// Word (lowercase letters) → its divisions joined with "-"; several for heteronyms
/// (rec-ord / re-cord).
static DICT: LazyLock<HashMap<String, Vec<String>>> = LazyLock::new(|| {
    let mut text = String::new();
    flate2::read::GzDecoder::new(DATA)
        .read_to_string(&mut text)
        .expect("the bundled syllable dictionary is valid gzip");
    text.lines()
        .filter_map(|line| {
            let splits: Vec<String> = line.split(' ').map(str::to_string).collect();
            let key = splits.first()?.replace('-', "");
            Some((key, splits))
        })
        .collect()
});

/// A syllable division as written: letters joined by a hyphen or a middle dot, with optional
/// spaces around the mark ("dra-ma-tic", "dra · mat · ic").
static WRITTEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z]+(?:[ ]?[-·•‧][ ]?[A-Za-z]+)+").expect("valid regex"));

static MENTIONS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)syllab|音節").expect("valid regex"));

/// Whether a text is about syllables (a section title, its instructions or one item's prompt).
pub fn mentions_syllables(text: &str) -> bool {
    MENTIONS.is_match(text)
}

/// The dictionary's divisions of `word`, joined with "-" (several for heteronyms), or `None`
/// when the word is not in the dictionary and is not a regular inflection of one that is.
pub fn divisions(word: &str) -> Option<Vec<String>> {
    let w = word.to_ascii_lowercase();
    if w.is_empty() || !w.bytes().all(|b| b.is_ascii_lowercase()) {
        return None;
    }
    if let Some(v) = DICT.get(&w) {
        return Some(v.clone());
    }
    derived(&w)
}

fn stem_divisions(stem: &str) -> Option<&'static Vec<String>> {
    (stem.len() >= 2).then(|| DICT.get(stem)).flatten()
}

/// Regular inflections and derivations the list leaves out, built from the stem's divisions.
fn derived(w: &str) -> Option<Vec<String>> {
    let map = |stem: &str, f: &dyn Fn(&str) -> String| {
        stem_divisions(stem).map(|v| v.iter().map(|s| f(s)).collect::<Vec<_>>())
    };
    // A suffix that is a syllable of its own: stem-suffix.
    for suffix in ["ful", "less", "ment", "ness", "ship"] {
        if let Some(stem) = w.strip_suffix(suffix) {
            if let Some(v) = map(stem, &|s| format!("{s}-{suffix}")) {
                return Some(v);
            }
        }
    }
    // -er / -ing after a stem ending in two consonants (re-mind-er, read-ing is listed anyway):
    // the boundary is clear. After one consonant or a dropped "e" it is not, so leave those.
    for suffix in ["er", "ing"] {
        if let Some(stem) = w.strip_suffix(suffix) {
            let b = stem.as_bytes();
            let two_consonants =
                b.len() >= 2 && !is_vowel(b[b.len() - 1]) && !is_vowel(b[b.len() - 2]);
            if two_consonants && b[b.len() - 1] != b[b.len() - 2] {
                if let Some(v) = map(stem, &|s| format!("{s}-{suffix}")) {
                    return Some(v);
                }
            }
        }
    }
    // -es after a sibilant is a syllable (box-es); a plain -s joins the last syllable (hab-its).
    if let Some(stem) = w.strip_suffix("es") {
        if ["s", "x", "z", "ch", "sh"]
            .iter()
            .any(|e| stem.ends_with(e))
        {
            return map(stem, &|s| format!("{s}-es"));
        }
    }
    if let Some(stem) = w.strip_suffix('s') {
        let sibilant = ["s", "x", "z", "ch", "sh"]
            .iter()
            .any(|e| stem.ends_with(e));
        if !sibilant && !stem.ends_with('e') {
            return map(stem, &|s| format!("{s}s"));
        }
    }
    // -ed is a syllable after t/d (want-ed) and joins the last syllable otherwise (played).
    if let Some(stem) = w.strip_suffix("ed") {
        if stem.ends_with('t') || stem.ends_with('d') {
            return map(stem, &|s| format!("{s}-ed"));
        }
        if !stem.ends_with('e') {
            return map(stem, &|s| format!("{s}ed"));
        }
    }
    None
}

fn is_vowel(b: u8) -> bool {
    matches!(b, b'a' | b'e' | b'i' | b'o' | b'u' | b'y')
}

/// One division the dictionary disagrees with.
#[derive(Debug, Clone, PartialEq)]
pub struct Correction {
    pub written: String,
    pub dictionary: String,
}

/// The syllable pieces of a written division, without the marks or spaces.
fn pieces(written: &str) -> Vec<String> {
    written
        .split(['-', '·', '•', '‧'])
        .map(|p| p.trim().to_string())
        .collect()
}

/// How the dictionary would write the same word with the writer's mark: "dra-mat-ic" stays as
/// it is for "dra-ma-tic", "dra·mat·ic" for "dra·ma·tic"; heteronyms are given as "a / b".
fn in_writers_style(written: &str, splits: &[String]) -> String {
    let mark = written
        .chars()
        .find(|c| matches!(c, '-' | '·' | '•' | '‧'))
        .unwrap_or('-');
    let spaced = written.contains(&format!(" {mark}")) || written.contains(&format!("{mark} "));
    let joiner = if spaced {
        format!(" {mark} ")
    } else {
        mark.to_string()
    };
    let capital = written.chars().next().is_some_and(char::is_uppercase);
    splits
        .iter()
        .map(|s| {
            let mut out = s.split('-').collect::<Vec<_>>().join(&joiner);
            if capital {
                if let Some(first) = out.get(0..1) {
                    out = first.to_uppercase() + &out[1..];
                }
            }
            out
        })
        .collect::<Vec<_>>()
        .join(" / ")
}

/// Whether a written division agrees with the dictionary. `None` when the word cannot be
/// checked (not a dictionary word: "well-known", a name, a typo).
pub fn check(written: &str) -> Option<Result<(), String>> {
    let parts = pieces(written);
    let word: String = parts.concat();
    let splits = divisions(&word)?;
    let normalized = parts.join("-").to_lowercase();
    if splits.contains(&normalized) {
        Some(Ok(()))
    } else {
        Some(Err(in_writers_style(written, &splits)))
    }
}

/// Rewrites every syllable division in `text` that the dictionary disagrees with. Words the
/// dictionary does not know are left as they are.
pub fn correct(text: &str) -> (String, Vec<Correction>) {
    let mut out = String::with_capacity(text.len());
    let mut corrections = Vec::new();
    let mut last = 0;
    for m in WRITTEN.find_iter(text) {
        // Part of a longer token ("x-ray2", "co-op's"): leave it.
        let before = text[..m.start()].chars().next_back();
        let after = text[m.end()..].chars().next();
        if before.is_some_and(|c| c.is_alphanumeric()) || after.is_some_and(|c| c.is_alphanumeric())
        {
            continue;
        }
        if let Some(Err(dictionary)) = check(m.as_str()) {
            out.push_str(&text[last..m.start()]);
            out.push_str(&dictionary);
            last = m.end();
            corrections.push(Correction {
                written: m.as_str().to_string(),
                dictionary,
            });
        }
    }
    out.push_str(&text[last..]);
    (out, corrections)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dictionary_words() {
        assert_eq!(divisions("dramatic").unwrap(), ["dra-mat-ic"]);
        assert_eq!(divisions("Automatic").unwrap(), ["au-to-mat-ic"]);
        assert_eq!(divisions("school").unwrap(), ["school"]);
        assert_eq!(divisions("record").unwrap(), ["rec-ord", "re-cord"]);
        assert!(divisions("qzxv").is_none());
        assert!(divisions("well-known").is_none());
        assert!(divisions("").is_none());
    }

    #[test]
    fn regular_inflections_come_from_the_stem() {
        assert_eq!(divisions("habits").unwrap(), ["hab-its"]);
        assert_eq!(divisions("reminder").unwrap(), ["re-mind-er"]);
        assert_eq!(divisions("sorrowful").unwrap(), ["sor-row-ful"]);
        assert_eq!(divisions("sponsorship").unwrap(), ["spon-sor-ship"]);
    }

    #[test]
    fn checks_a_written_division() {
        assert_eq!(check("dra-mat-ic"), Some(Ok(())));
        assert_eq!(check("dra-ma-tic"), Some(Err("dra-mat-ic".into())));
        assert_eq!(check("dra · ma · tic"), Some(Err("dra · mat · ic".into())));
        assert_eq!(check("Dra·ma·tic"), Some(Err("Dra·mat·ic".into())));
        assert_eq!(check("re-cord"), Some(Ok(())));
        assert_eq!(check("reco-rd"), Some(Err("rec-ord / re-cord".into())));
        assert_eq!(check("well-known"), None);
    }

    #[test]
    fn corrects_divisions_inside_a_sentence() {
        let (text, fixes) =
            correct("dramatic → dra-ma-tic (3 syllables); in-vis-i-ble is right; well-known.");
        assert_eq!(
            text,
            "dramatic → dra-mat-ic (3 syllables); in-vis-i-ble is right; well-known."
        );
        assert_eq!(
            fixes,
            [Correction {
                written: "dra-ma-tic".into(),
                dictionary: "dra-mat-ic".into()
            }]
        );
        let (same, none) = correct("No divisions here.");
        assert_eq!(same, "No divisions here.");
        assert!(none.is_empty());
    }
}
