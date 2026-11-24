//! 表格整理: the table a question prints must say what it compares. The model writes the column
//! headings (the things compared: 「Unconventional Path as a Positive Choice」 | 「Speaker
//! Justifying His Decision」) and the row labels (what to write about each: 「Claim」,
//! 「Supporting detail」); the sheet prints the headings over a first column of row labels. A
//! placeholder (「Column 1」) never reaches the student: the server repairs the headings from the
//! question text when it names the columns, and asks the model again when it does not.
//!
//! The model's part is [`RawTable`] (one nullable `table` property of a question, `null` for
//! every other format); [`check`] cleans it into the [`ItemTable`] the worksheet stores.

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::template::{obj, s_max};
use super::{ItemTable, MAX_TABLE_COLUMNS, TABLE_ROWS};
use crate::sop::checks::{clean_markup, english_only};

/// Most row labels a generated table has (the editor allows [`super::MAX_TABLE_ROWS`]).
pub const MAX_ROW_LABELS: usize = 8;
/// Longest column heading or row label the model may write.
pub const LABEL_CHARS: usize = 60;
/// Longest heading taken from a question's own words.
const NAMED_COLUMN_WORDS: usize = 8;

/// The table as the model writes it. Stored (cleaned) in the raw question, so older stored
/// outputs without the key still parse.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct RawTable {
    #[serde(default)]
    pub columns: Vec<String>,
    #[serde(default)]
    pub row_labels: Vec<String>,
}

impl RawTable {
    /// The table the worksheet stores: one empty row per label (three free rows without any).
    pub fn to_item_table(&self) -> ItemTable {
        ItemTable {
            columns: self.columns.clone(),
            rows: if self.row_labels.is_empty() {
                TABLE_ROWS
            } else {
                self.row_labels
                    .len()
                    .min(usize::from(super::MAX_TABLE_ROWS)) as u8
            },
            row_labels: self.row_labels.clone(),
        }
    }
}

impl From<&ItemTable> for RawTable {
    fn from(t: &ItemTable) -> Self {
        Self {
            columns: t.columns.clone(),
            row_labels: t.row_labels.clone(),
        }
    }
}

/// `table` of a question: the headings first, then what each row is about. `null` for a
/// question of another format.
pub fn schema() -> Value {
    let table = obj(vec![
        (
            "columns",
            json!({
                "type": "array", "items": s_max(LABEL_CHARS),
                "minItems": 2, "maxItems": MAX_TABLE_COLUMNS
            }),
        ),
        (
            "row_labels",
            json!({
                "type": "array", "items": s_max(LABEL_CHARS), "maxItems": MAX_ROW_LABELS
            }),
        ),
    ]);
    json!({ "anyOf": [table, { "type": "null" }] })
}

fn re_placeholder() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(concat!(
            r"(?i)^(?:",
            r"(?:column|col|header|heading|row|item|field|label|title|category|topic|name)\s*[#_\-]?\s*\d+",
            r"|column(?:\s+(?:name|header))?|header|heading|tbd|n/?a|none|blank|untitled",
            r"|\.{2,}|…+|\?+|-+|_+|x+",
            r"|(?:欄位?|列|標題|欄名)\s*\d*",
            r")$"
        ))
        .expect("placeholder regex")
    })
}

/// A heading or label that says nothing: empty, 「Column 1」, 「Header」, 「Row 2」, 「…」.
pub fn is_placeholder(s: &str) -> bool {
    let t = s.trim();
    t.is_empty() || re_placeholder().is_match(t)
}

/// One line of model text: markup and line breaks gone, capped.
fn line(s: &str) -> String {
    let flat = clean_markup(s)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    flat.chars().take(LABEL_CHARS).collect()
}

fn number_word(w: &str) -> Option<usize> {
    match w.to_lowercase().as_str() {
        "two" | "2" => Some(2),
        "three" | "3" => Some(3),
        "four" | "4" => Some(4),
        "five" | "5" => Some(5),
        "six" | "6" => Some(6),
        _ => None,
    }
}

/// The headings a question names in its own words: 「Complete a T-chart with two columns:
/// Unconventional Path as a Positive Choice and Speaker Justifying His Decision. For each …」.
/// Reads the list after 「column(s)」 (and an optional 「labeled/titled/called/headed」) up to the
/// end of the sentence, split at 「/」, 「|」, 「;」 or commas, else at a single 「and」. A stated
/// number (「two columns」) must match what was found. Two to six short headings, else `None`.
pub fn columns_named_in_prompt(prompt: &str) -> Option<Vec<String>> {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(
            r"(?i)\b(?:(\w+)\s+)?columns?\s*(?:(?:labell?ed|titled|called|headed|named)\b\s*[:：]?|[:：])\s*([^\n]+)",
        )
        .expect("columns regex")
    });
    let caps = re.captures(prompt)?;
    let stated = caps.get(1).and_then(|m| number_word(m.as_str()));
    // A closing 「(Question 3)」 is not part of the list.
    let mut rest = caps.get(2)?.as_str().trim_end();
    if rest.ends_with(')') {
        if let Some(i) = rest.rfind('(') {
            rest = rest[..i].trim_end();
        }
    }
    // The list ends where the sentence does (a period followed by a space and a capital).
    let end = Regex::new(r"[.!?](?:\s+\p{Lu}|\s*$)")
        .ok()?
        .find(rest)
        .map_or(rest.len(), |m| m.start());
    let list = rest[..end].trim().trim_end_matches(['.', '。']);
    let mut parts: Vec<String> = list
        .split(['/', '|', '／', ';', '；', ','])
        .map(|p| {
            let p = p.trim();
            let p = p
                .strip_prefix("and ")
                .or_else(|| p.strip_prefix("And "))
                .unwrap_or(p);
            p.trim_matches(|c: char| matches!(c, '"' | '\'' | '“' | '”' | '‘' | '’'))
                .trim()
                .to_string()
        })
        .filter(|p| !p.is_empty())
        .collect();
    if parts.len() == 1 {
        let lower = parts[0].to_lowercase();
        if let Some(at) = lower.find(" and ") {
            let (a, b) = (&parts[0][..at], &parts[0][at + 5..]);
            parts = vec![a.trim().to_string(), b.trim().to_string()];
        }
    }
    let fits = (2..=MAX_TABLE_COLUMNS).contains(&parts.len())
        && stated.is_none_or(|n| n == parts.len())
        && parts.iter().all(|p| {
            p.chars().count() <= LABEL_CHARS
                && p.split_whitespace().count() <= NAMED_COLUMN_WORDS
                && !is_placeholder(p)
        });
    fits.then_some(parts)
}

/// What [`check`] found: the cleaned table (`None`: use the one the question's words give, or
/// none) and the problems. `needs_model`: the code could not repair it, so the model is asked
/// again (the question stays as it is if the second try fails too).
#[derive(Debug, Default)]
pub struct Checked {
    pub table: Option<RawTable>,
    pub problems: Vec<Problem>,
}

#[derive(Debug, PartialEq)]
pub struct Problem {
    /// What to tell the model.
    pub text: String,
    /// What the code did about it.
    pub repair: &'static str,
    pub needs_model: bool,
}

fn problem(text: impl Into<String>, repair: &'static str, needs_model: bool) -> Problem {
    Problem {
        text: text.into(),
        repair,
        needs_model,
    }
}

/// Cleans the table of a 表格整理 question (`raw`: what the model wrote; `prompt`: the question):
/// - headings are one capped line each; a placeholder or empty heading is replaced by the
///   headings the question names (none are kept half and half: all or nothing), else the table
///   keeps the real headings it has and the model is asked for the rest;
/// - row labels are cleaned, deduplicated and capped; placeholders (「Row 1」) go. A table of
///   two columns without any label asks the model for them (what is each row about?); three
///   columns or more may leave the rows free, the headings then name what goes in each cell;
/// - `english`: students read English only, so a Chinese heading or label drops the table.
pub fn check(raw: Option<&RawTable>, prompt: &str, english: bool, iname: &str) -> Checked {
    let mut out = Checked::default();
    let raw = raw.cloned().unwrap_or_default();
    let mut columns: Vec<String> = raw
        .columns
        .iter()
        .map(|c| line(c))
        .take(MAX_TABLE_COLUMNS)
        .collect();
    let named = || super::columns_from_prompt(prompt).or_else(|| columns_named_in_prompt(prompt));
    let bad: Vec<usize> = columns
        .iter()
        .enumerate()
        .filter(|(_, c)| is_placeholder(c))
        .map(|(i, _)| i)
        .collect();
    if columns.len() < 2 || !bad.is_empty() {
        let was = if columns.is_empty() {
            "沒有欄名".to_string()
        } else {
            format!("欄名「{}」是空白或占位字", columns.join(" | "))
        };
        match named() {
            Some(from_prompt) => {
                out.problems.push(problem(
                    format!(
                        "{iname} 的表格{was}；改用題目寫出的欄名「{}」。",
                        from_prompt.join(" | ")
                    ),
                    "欄名改用題目裡寫的",
                    false,
                ));
                columns = from_prompt;
            }
            None => {
                out.problems.push(problem(
                    format!(
                        "{iname} 的表格{was}。table.columns 要寫出要比較的兩件事（例如 \"Unconventional Path as a Positive Choice\"、\"Speaker Justifying His Decision\"），不可以寫 Column 1 這種占位字。"
                    ),
                    "欄名留白，教師要自己補上",
                    true,
                ));
                for i in bad {
                    columns[i].clear();
                }
                while columns.len() < 2 {
                    columns.push(String::new());
                }
            }
        }
    }
    let mut labels: Vec<String> = Vec::new();
    for l in &raw.row_labels {
        let l = line(l);
        if !is_placeholder(&l) && !labels.iter().any(|x| x.eq_ignore_ascii_case(&l)) {
            labels.push(l);
        }
    }
    labels.truncate(MAX_ROW_LABELS);
    if labels.is_empty() && columns.len() == 2 {
        out.problems.push(problem(
            format!(
                "{iname} 的表格沒有列標籤。table.row_labels 要寫出每一列要比較或寫下的面向（例如 \"Claim\"、\"Supporting detail\"），學生才知道每一列要寫什麼。"
            ),
            "印成沒有列標籤的空白列，教師要自己補上",
            true,
        ));
    }
    if english && columns.iter().chain(&labels).any(|s| !english_only(s)) {
        out.problems.push(problem(
            format!("{iname} 的表格欄名或列標籤含有中文；學生看得到的欄位只能用英文。"),
            "拿掉這一題的表格，欄名取自題目",
            true,
        ));
        return out;
    }
    out.table = Some(RawTable {
        columns,
        row_labels: labels,
    });
    out
}

/// A table on a question that is not 表格整理: dropped, said once.
pub fn stray(raw: Option<&RawTable>, format: &str, iname: &str) -> Option<Problem> {
    raw.filter(|t| !t.columns.is_empty() || !t.row_labels.is_empty())
        .map(|_| {
            problem(
                format!("{iname}（{format}）不是表格整理，table 要填 null。"),
                "拿掉 table",
                false,
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(columns: &[&str], rows: &[&str]) -> RawTable {
        RawTable {
            columns: columns.iter().map(|s| s.to_string()).collect(),
            row_labels: rows.iter().map(|s| s.to_string()).collect(),
        }
    }

    const PI_PROMPT: &str = "Complete a T-chart with two columns: Unconventional Path as a Positive Choice and Speaker Justifying His Decision. For each interpretation, record its claim and one supporting detail.";

    #[test]
    fn placeholders_are_recognised() {
        for p in [
            "",
            " ",
            "Column 1",
            "column2",
            "Col #3",
            "Header",
            "Row 1",
            "Heading 2",
            "欄 1",
            "...",
            "N/A",
        ] {
            assert!(is_placeholder(p), "{p:?}");
        }
        for p in [
            "Claim",
            "Who?",
            "Cause",
            "Unconventional Path",
            "Item 1 of the poem",
            "Column of smoke",
        ] {
            assert!(!is_placeholder(p), "{p:?}");
        }
    }

    #[test]
    fn the_prompt_names_its_columns() {
        assert_eq!(
            columns_named_in_prompt(PI_PROMPT).unwrap(),
            [
                "Unconventional Path as a Positive Choice",
                "Speaker Justifying His Decision"
            ]
        );
        assert_eq!(
            columns_named_in_prompt(
                "Fill in a table with three columns: Who, What happened, and Why. (Question 2)"
            )
            .unwrap(),
            ["Who", "What happened", "Why"]
        );
        assert_eq!(
            columns_named_in_prompt("Complete the chart with two columns labeled Pros / Cons.")
                .unwrap(),
            ["Pros", "Cons"]
        );
        // A stated number that does not match, or no list at all.
        assert_eq!(
            columns_named_in_prompt("Make a chart with three columns: Pros and Cons."),
            None
        );
        assert_eq!(
            columns_named_in_prompt("Complete the table about the speaker."),
            None
        );
    }

    #[test]
    fn good_headings_and_labels_are_kept() {
        let c = check(
            Some(&raw(
                &["Path A", "Path B"],
                &["Claim", "Supporting detail", "claim"],
            )),
            PI_PROMPT,
            true,
            "第 1 題",
        );
        assert!(c.problems.is_empty());
        let t = c.table.unwrap();
        assert_eq!(t.columns, ["Path A", "Path B"]);
        assert_eq!(t.row_labels, ["Claim", "Supporting detail"]);
        assert_eq!(t.to_item_table().rows, 2);
    }

    #[test]
    fn placeholder_headings_are_replaced_from_the_question() {
        let c = check(
            Some(&raw(
                &["Column 1", "Column 2"],
                &["Claim", "Supporting detail"],
            )),
            PI_PROMPT,
            true,
            "第 1 題",
        );
        assert_eq!(c.problems.len(), 1);
        assert!(!c.problems[0].needs_model);
        assert_eq!(
            c.table.unwrap().columns,
            [
                "Unconventional Path as a Positive Choice",
                "Speaker Justifying His Decision"
            ]
        );
        // No table at all: the same.
        let c = check(None, PI_PROMPT, true, "第 1 題");
        assert_eq!(
            c.table.unwrap().columns[1],
            "Speaker Justifying His Decision"
        );
    }

    #[test]
    fn unrepairable_headings_and_missing_labels_ask_the_model() {
        let c = check(
            Some(&raw(&["Column 1", "Column 2"], &[])),
            "Complete the table about the poem.",
            true,
            "第 2 題",
        );
        assert_eq!(c.problems.len(), 2);
        assert!(c.problems.iter().all(|p| p.needs_model));
        let t = c.table.unwrap();
        assert_eq!(t.columns, ["", ""]);
        assert_eq!(t.to_item_table().rows, TABLE_ROWS);
        // Three columns may leave the rows free; real headings stay while a placeholder goes.
        let c = check(
            Some(&raw(&["Who", "Column 2", "Why"], &[])),
            "Complete the table.",
            true,
            "第 3 題",
        );
        assert_eq!(c.problems.len(), 1);
        assert_eq!(c.table.unwrap().columns, ["Who", "", "Why"]);
        let c = check(
            Some(&raw(&["Who", "What", "Why"], &[])),
            "Complete the table.",
            true,
            "第 3 題",
        );
        assert!(c.problems.is_empty());
    }

    #[test]
    fn chinese_in_a_student_table_drops_it() {
        let c = check(
            Some(&raw(&["優點", "Cons"], &["Claim"])),
            "x",
            true,
            "第 1 題",
        );
        assert!(c.table.is_none());
        assert!(c.problems[0].needs_model);
        // A template may be in Chinese.
        let c = check(
            Some(&raw(&["優點", "缺點"], &["理由"])),
            "x",
            false,
            "第 1 題",
        );
        assert!(c.problems.is_empty());
    }

    #[test]
    fn a_table_on_another_format_is_a_stray() {
        assert!(stray(None, "簡答題", "q").is_none());
        assert!(stray(Some(&raw(&[], &[])), "簡答題", "q").is_none());
        assert!(stray(Some(&raw(&["A", "B"], &[])), "簡答題", "q").is_some());
    }
}
