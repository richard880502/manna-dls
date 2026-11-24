//! Bloom's cognitive levels (Bloom 認知層次): the fixed set of six, what each level asks of the
//! students, the answer formats that suit it, and the rules that turn a worksheet's settings into
//! the questions to generate per level. Contract: docs/API.md「Phase 2.7」.
//!
//! The model never decides the distribution, the tiers or the block titles: they are all here,
//! in code. The model writes the questions of one level per call (`worksheet_bloom_level`) and a
//! separate judge call (`worksheet_bloom_check`) classifies every question independently.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};

use super::template::{enum_of, obj, s_max, strings_max};
use super::{
    Settings, ANSWER_FORMATS, FMT_BLANK, FMT_CHOICE, FMT_MATCHING, FMT_ORGANIZER, FMT_PARAGRAPH,
    FMT_SHORT, FMT_TABLE, FMT_TRUE_FALSE,
};
use crate::text::clip_chars;

// ---------------------------------------------------------------- the six levels

/// One of Bloom's six cognitive levels, ordered from remember (lowest) to create (highest).
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Bloom {
    Remember,
    Understand,
    Apply,
    Analyze,
    Evaluate,
    Create,
}

impl Bloom {
    pub const ALL: [Bloom; 6] = [
        Bloom::Remember,
        Bloom::Understand,
        Bloom::Apply,
        Bloom::Analyze,
        Bloom::Evaluate,
        Bloom::Create,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Remember => "remember",
            Self::Understand => "understand",
            Self::Apply => "apply",
            Self::Analyze => "analyze",
            Self::Evaluate => "evaluate",
            Self::Create => "create",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|b| b.as_str() == s.trim())
    }

    /// 0 for remember … 5 for create.
    pub fn rank(self) -> usize {
        self as usize
    }

    /// The Chinese label shown to teachers: 記憶、理解…
    pub fn label(self) -> &'static str {
        match self {
            Self::Remember => "記憶",
            Self::Understand => "理解",
            Self::Apply => "應用",
            Self::Analyze => "分析",
            Self::Evaluate => "評鑑",
            Self::Create => "創造",
        }
    }

    pub fn english(self) -> &'static str {
        match self {
            Self::Remember => "Remember",
            Self::Understand => "Understand",
            Self::Apply => "Apply",
            Self::Analyze => "Analyze",
            Self::Evaluate => "Evaluate",
            Self::Create => "Create",
        }
    }

    /// What the students do at this level (prompt text).
    pub fn definition(self) -> &'static str {
        match self {
            Self::Remember => "回想或找出文本明白寫出的事實（人物、事件、時間、地點、數字、細節）。",
            Self::Understand => {
                "解釋、改述、摘要文本的意思，或從上下文推知詞義（例如段落主旨、用自己的話說明）。"
            }
            Self::Apply => "把文本的概念或語言用在新的情境（例如自己的生活、學校，或用文中的詞語造句）。",
            Self::Analyze => {
                "分析關係：因果、比較、文章結構、寫作目的，或分辨事實與意見（例如作者為什麼這樣寫、第 2 段和第 3 段有什麼不同）。"
            }
            Self::Evaluate => "根據文本的證據做判斷並說明理由（例如是否同意、哪一個做法最好，並舉出文中的理由）。",
            Self::Create => "根據文本產出新的東西（例如改寫結局、設計海報或計畫、寫出想問作者的問題）。",
        }
    }

    /// English question stems for EFL high-school students (prompt text).
    pub fn stems(self) -> &'static str {
        match self {
            Self::Remember => {
                "Who / What / When / Where …? According to paragraph n, …? List the …"
            }
            Self::Understand => {
                "What is the main idea of paragraph n? In your own words, …? What does \"X\" mean in paragraph n?"
            }
            Self::Apply => {
                "How could you use … at your school? Give an example from your own life of … Use \"X\" in a sentence about …"
            }
            Self::Analyze => {
                "Why does the writer …? Compare paragraph 2 and 3. What is the purpose of …? Which sentence is an opinion?"
            }
            Self::Evaluate => {
                "Do you agree that …? Give two reasons from the text. Which solution is the best, and why?"
            }
            Self::Create => {
                "Write a new ending … Design a poster/plan … Write three questions you would ask the writer."
            }
        }
    }

    /// The answer formats that suit this level (fixed labels, in the contract's order).
    pub fn formats(self) -> &'static [&'static str] {
        match self {
            Self::Remember => &[
                FMT_CHOICE,
                FMT_TRUE_FALSE,
                FMT_BLANK,
                FMT_MATCHING,
                FMT_SHORT,
            ],
            Self::Understand => &[FMT_CHOICE, FMT_BLANK, FMT_SHORT, FMT_MATCHING],
            Self::Apply => &[FMT_SHORT, FMT_BLANK],
            Self::Analyze => &[FMT_TABLE, FMT_ORGANIZER, FMT_SHORT, FMT_CHOICE],
            Self::Evaluate => &[FMT_SHORT, FMT_PARAGRAPH],
            Self::Create => &[FMT_PARAGRAPH, FMT_SHORT],
        }
    }

    /// Remember and understand questions must quote the text as their evidence.
    pub fn needs_evidence(self) -> bool {
        matches!(self, Self::Remember | Self::Understand)
    }

    /// The 學生任務 labels of a block of this level.
    pub fn student_tasks(self) -> &'static [&'static str] {
        match self {
            Self::Remember | Self::Understand => &["理解內容"],
            Self::Apply => &["字彙練習"],
            Self::Analyze => &["推論／預測", "比較／分類"],
            Self::Evaluate => &["評估／提出理由"],
            Self::Create => &["寫作"],
        }
    }

    /// The fixed part of a block's title (after 「Part A · 」).
    pub fn title_name(self) -> &'static str {
        match self {
            Self::Remember => "Find the Facts",
            Self::Understand => "Understand the Text",
            Self::Apply => "Use It",
            Self::Analyze => "Look Closer",
            Self::Evaluate => "What Do You Think?",
            Self::Create => "Make Something New",
        }
    }

    /// The tag printed after a question in the teacher version: 〔Bloom：分析 Analyze〕.
    pub fn tag(self) -> String {
        format!("〔Bloom：{} {}〕", self.label(), self.english())
    }
}

impl<'de> Deserialize<'de> for Bloom {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Bloom::parse(&s).ok_or_else(|| {
            serde::de::Error::custom(format!(
                "「{}」不是 Bloom 認知層次的選項（remember、understand、apply、analyze、evaluate、create）。",
                clip_chars(&s, 40)
            ))
        })
    }
}

/// A block title: 「Find the Facts」. The sheet, the editor and the exports print the block's
/// letter in front of it (「A. Find the Facts」), so the title does not carry one: it would be
/// printed twice and go stale when the teacher reorders blocks.
pub fn block_title(b: Bloom) -> String {
    b.title_name().to_string()
}

/// A title of the form 「Part A · Find the Facts」 as (letter, name). The sheet renumbers the
/// letter to the printed one.
pub fn split_part_title(title: &str) -> Option<(&str, &str)> {
    let rest = title.strip_prefix("Part ")?;
    let (letter, name) = rest.split_once(" · ")?;
    (!letter.is_empty() && letter.chars().all(|c| c.is_ascii_uppercase())).then_some((letter, name))
}

/// The level of a block from its fixed title, when the title is one of them (also the older
/// 「Part A · Find the Facts」 form).
pub fn level_of_title(title: &str) -> Option<Bloom> {
    let title = title.trim();
    let name = split_part_title(title).map_or(title, |(_, name)| name);
    Bloom::ALL.into_iter().find(|b| b.title_name() == name)
}

/// 「Bloom：記憶 2、理解 2」
pub fn summary(counts: &[(Bloom, usize)]) -> String {
    format!(
        "Bloom：{}",
        counts
            .iter()
            .filter(|(_, n)| *n > 0)
            .map(|(b, n)| format!("{} {n}", b.label()))
            .collect::<Vec<_>>()
            .join("、")
    )
}

// ---------------------------------------------------------------- what to generate

/// What a worksheet generation makes: the full worksheet (the existing pipeline), reading
/// comprehension questions, or a 差異化學習單 (`tiered`, formerly 分層練習).
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Output {
    #[default]
    Worksheet,
    Comprehension,
    Tiered,
}

impl Output {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Worksheet => "worksheet",
            Self::Comprehension => "comprehension",
            Self::Tiered => "tiered",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Self::Worksheet, Self::Comprehension, Self::Tiered]
            .into_iter()
            .find(|o| o.as_str() == s.trim())
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Worksheet => "學習單",
            Self::Comprehension => "閱讀理解題",
            Self::Tiered => "差異化學習單",
        }
    }

    /// Generated per Bloom level (`worksheet::ai::bloom`).
    pub fn by_bloom(self) -> bool {
        self != Self::Worksheet
    }

    pub fn default_count(self) -> usize {
        match self {
            Self::Tiered => DEFAULT_TIERED_COUNT,
            _ => DEFAULT_COMPREHENSION_COUNT,
        }
    }
}

impl<'de> Deserialize<'de> for Output {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Output::parse(&s).ok_or_else(|| {
            serde::de::Error::custom(format!(
                "「{}」不是「要做什麼」的選項（worksheet、comprehension、tiered）。",
                clip_chars(&s, 40)
            ))
        })
    }
}

pub const DEFAULT_COMPREHENSION_COUNT: usize = 8;
/// Every question of a 差異化學習單 is in all three versions (T2), so one per level by default
/// (before the redesign the 12 questions were spread over the versions).
pub const DEFAULT_TIERED_COUNT: usize = 6;
pub const QUESTION_COUNT_RANGE: std::ops::RangeInclusive<i32> = 3..=30;
/// Most questions of one level in `bloom_counts`.
pub const MAX_LEVEL_COUNT: i32 = 12;
pub const MAX_OBJECTIVE_CHARS: usize = 300;

/// Keeps the levels in Bloom order, without duplicates.
pub fn normalise_levels(levels: &[Bloom]) -> Vec<Bloom> {
    Bloom::ALL
        .into_iter()
        .filter(|b| levels.contains(b))
        .collect()
}

/// Checks `bloom_counts`: every count 0–12 and 3–30 in all.
pub fn check_counts(counts: &BTreeMap<Bloom, i32>) -> Result<(), String> {
    let total: i32 = counts.values().sum();
    if counts.values().any(|n| !(0..=MAX_LEVEL_COUNT).contains(n))
        || !QUESTION_COUNT_RANGE.contains(&total)
    {
        return Err(format!(
            "每個 Bloom 層次的題數需為 0 到 {MAX_LEVEL_COUNT} 題，合計 {} 到 {} 題。",
            QUESTION_COUNT_RANGE.start(),
            QUESTION_COUNT_RANGE.end()
        ));
    }
    Ok(())
}

/// Spreads `total` questions over `levels` (in Bloom order): every level gets one, and the rest
/// go to the lower levels first, round-robin upward (8 over six levels: 2, 2, 1, 1, 1, 1).
pub fn distribute(levels: &[Bloom], total: usize) -> Vec<(Bloom, usize)> {
    let levels = normalise_levels(levels);
    let n = levels.len();
    if n == 0 {
        return Vec::new();
    }
    let rest = total.saturating_sub(n);
    levels
        .into_iter()
        .enumerate()
        .map(|(i, b)| (b, 1 + rest / n + usize::from(i < rest % n)))
        .collect()
}

/// The questions to generate per level, in Bloom order: `bloom_counts` when it is set (the
/// levels with a count above 0), otherwise `question_count` (or the output's default) spread
/// over `bloom` (empty: all six).
pub fn plan(settings: &Settings) -> Vec<(Bloom, usize)> {
    if let Some(counts) = &settings.bloom_counts {
        return Bloom::ALL
            .into_iter()
            .filter_map(|b| counts.get(&b).filter(|n| **n > 0).map(|n| (b, *n as usize)))
            .collect();
    }
    let levels = if settings.bloom.is_empty() {
        Bloom::ALL.to_vec()
    } else {
        settings.bloom.clone()
    };
    let total = settings
        .question_count
        .map_or(settings.output.default_count(), |n| n.max(1) as usize);
    distribute(&levels, total)
}

/// The answer formats a level may use: its suitable formats that the teacher selected; when
/// none of them is selected, the teacher's selection; when she selected nothing, all of its
/// suitable formats.
pub fn level_formats(b: Bloom, selected: &[String]) -> Vec<&'static str> {
    if selected.is_empty() {
        return b.formats().to_vec();
    }
    let both: Vec<&'static str> = b
        .formats()
        .iter()
        .copied()
        .filter(|f| selected.iter().any(|s| s == f))
        .collect();
    if !both.is_empty() {
        return both;
    }
    ANSWER_FORMATS
        .into_iter()
        .filter(|f| selected.iter().any(|s| s == f))
        .collect()
}

/// Whether the judge's level is too far from the target: any lower level, or two levels or more
/// higher.
pub fn is_mismatch(target: Bloom, judged: Bloom) -> bool {
    // Any question judged lower than asked is a mismatch: in the real-model run 9 of 90 questions
    // were one level lower (a 理解 question that only looked up a fact). One level higher is fine.
    judged < target || judged.rank() >= target.rank() + 2
}

/// The notice for a question that is relabelled.
pub fn relabel_notice(no: usize, target: Bloom, judged: Bloom) -> String {
    format!(
        "第 {no} 題原本要出『{}』，Manna 判斷是『{}』，已改標為『{}』。",
        target.label(),
        judged.label(),
        judged.label()
    )
}

/// Paragraph codes the model sees ("P1"…), matching the `[P1]` markers of the material.
pub fn paragraph_codes(paragraphs: usize) -> Vec<String> {
    (1..=paragraphs).map(|n| format!("P{n}")).collect()
}

// ---------------------------------------------------------------- model outputs and schemas

pub const LEVEL_SCHEMA_NAME: &str = "worksheet_bloom_level";
pub const CHECK_SCHEMA_NAME: &str = "worksheet_bloom_check";

pub const INSTRUCTIONS_CHARS: usize = 200;
pub const PROMPT_CHARS: usize = 500;
pub const OPTIONS: usize = 6;
pub const OPTION_CHARS: usize = 150;
pub const ANSWER_CHARS: usize = 400;
pub const EVIDENCE_CHARS: usize = 300;
pub const PARAGRAPHS_PER_ITEM: usize = 3;
pub const HINT_CHARS: usize = 150;
pub const REASON_CHARS: usize = 80;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawBloomItem {
    pub answer_format: String,
    pub prompt: String,
    pub options: Vec<String>,
    pub answer: String,
    #[serde(default)]
    pub evidence: String,
    #[serde(default)]
    pub paragraph_ids: Vec<String>,
    #[serde(default)]
    pub hint: String,
    /// 圖像組織 (prompt version 3): the organizer; `null` for the other formats.
    #[serde(default)]
    pub organizer: Option<super::organizer::RawOrganizer>,
    /// 表格整理: the column headings and row labels; `null` for the other formats.
    #[serde(default)]
    pub table: Option<super::table::RawTable>,
}

/// The questions of one level, then the block's written steps (R5, prompt version 2) and an
/// optional worked example. Version 1 wrote one sentence of `instructions` instead.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawBloomLevel {
    #[serde(default)]
    pub instructions: String,
    pub items: Vec<RawBloomItem>,
    #[serde(default)]
    pub steps: Vec<String>,
    #[serde(default)]
    pub example: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawJudgement {
    pub id: String,
    pub bloom: String,
    #[serde(default)]
    pub reason: String,
}

/// The judge's classification of every question.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawBloomCheck {
    pub items: Vec<RawJudgement>,
}

/// Exactly `count` questions of the given formats, then the block's steps (2–5, one array item
/// per step: gemma ends a string where it wants a line break) and an optional example. Every
/// string and array is capped: the gateway enforces the caps while decoding, which stops
/// repetition runaways. When 圖像組織 is one of the formats, every question has an `organizer`
/// after its options (`null` unless it is an organizer question); otherwise there is none.
pub fn level_schema(formats: &[&str], paragraph_codes: &[String], count: usize) -> Value {
    use super::tiered::{EXAMPLE_CHARS, MAX_STEPS, MIN_STEPS, STEP_CHARS};
    let codes: Vec<&str> = paragraph_codes.iter().map(String::as_str).collect();
    let count = count.max(1);
    let mut item = vec![
        ("answer_format", enum_of(formats)),
        ("prompt", s_max(PROMPT_CHARS)),
        ("options", strings_max(OPTIONS, OPTION_CHARS)),
    ];
    if formats.contains(&FMT_ORGANIZER) {
        item.push(("organizer", super::organizer::schema(false)));
    }
    if formats.contains(&super::FMT_TABLE) {
        item.push(("table", super::table::schema()));
    }
    item.extend([
        ("answer", s_max(ANSWER_CHARS)),
        ("evidence", s_max(EVIDENCE_CHARS)),
        (
            "paragraph_ids",
            json!({
                "type": "array", "items": enum_of(&codes), "maxItems": PARAGRAPHS_PER_ITEM
            }),
        ),
        ("hint", s_max(HINT_CHARS)),
    ]);
    obj(vec![
        (
            "items",
            json!({
                "type": "array",
                "items": obj(item),
                "minItems": count,
                "maxItems": count
            }),
        ),
        (
            "steps",
            json!({
                "type": "array",
                "items": s_max(STEP_CHARS),
                "minItems": MIN_STEPS,
                "maxItems": MAX_STEPS
            }),
        ),
        ("example", s_max(EXAMPLE_CHARS)),
    ])
}

/// One judgement per question id.
pub fn check_schema(ids: &[String]) -> Value {
    let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
    let levels: Vec<&str> = Bloom::ALL.iter().map(|b| b.as_str()).collect();
    obj(vec![(
        "items",
        json!({
            "type": "array",
            "items": obj(vec![
                ("id", enum_of(&ids)),
                ("bloom", enum_of(&levels)),
                ("reason", s_max(REASON_CHARS)),
            ]),
            "minItems": ids.len(),
            "maxItems": ids.len()
        }),
    )])
}

/// Output budget of one level call.
pub fn level_max_tokens(count: usize) -> u32 {
    (400 + 300 * count.min(30) as u32).clamp(1500, 6000)
}

/// Output budget of the judge call.
pub fn check_max_tokens(count: usize) -> u32 {
    (300 + 80 * count.min(60) as u32).clamp(1000, 3000)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts(plan: &[(Bloom, usize)]) -> Vec<usize> {
        plan.iter().map(|(_, n)| *n).collect()
    }

    #[test]
    fn eight_over_six_levels_is_two_two_one_one_one_one() {
        assert_eq!(
            counts(&distribute(&Bloom::ALL, 8)),
            [2, 2, 1, 1, 1, 1],
            "lower levels first"
        );
        assert_eq!(counts(&distribute(&Bloom::ALL, 12)), [2; 6]);
        assert_eq!(
            counts(&distribute(&Bloom::ALL, 3)),
            [1; 6],
            "at least one each"
        );
        assert_eq!(counts(&distribute(&Bloom::ALL, 15)), [3, 3, 3, 2, 2, 2]);
        let picked = distribute(&[Bloom::Evaluate, Bloom::Understand, Bloom::Evaluate], 5);
        assert_eq!(picked, [(Bloom::Understand, 3), (Bloom::Evaluate, 2)]);
        assert!(distribute(&[], 8).is_empty());
    }

    #[test]
    fn the_plan_follows_the_settings() {
        let mut s = Settings {
            output: Output::Comprehension,
            ..Settings::default()
        };
        assert_eq!(counts(&plan(&s)), [2, 2, 1, 1, 1, 1]);
        s.output = Output::Tiered;
        assert_eq!(counts(&plan(&s)), [1; 6], "差異化學習單 default is 6");
        s.bloom = vec![Bloom::Analyze, Bloom::Remember];
        s.question_count = Some(7);
        assert_eq!(plan(&s), [(Bloom::Remember, 4), (Bloom::Analyze, 3)]);
        // Explicit counts win over bloom and question_count; zero means not generated.
        s.bloom_counts = Some(BTreeMap::from([
            (Bloom::Create, 2),
            (Bloom::Understand, 0),
            (Bloom::Apply, 5),
        ]));
        assert_eq!(plan(&s), [(Bloom::Apply, 5), (Bloom::Create, 2)]);
    }

    #[test]
    fn counts_are_checked() {
        assert!(check_counts(&BTreeMap::from([(Bloom::Remember, 3)])).is_ok());
        assert!(check_counts(&BTreeMap::from([(Bloom::Remember, 2)])).is_err());
        assert!(check_counts(&BTreeMap::from([(Bloom::Remember, 13)])).is_err());
        assert!(check_counts(&BTreeMap::from([(Bloom::Remember, -1), (Bloom::Apply, 5)])).is_err());
        let many: BTreeMap<Bloom, i32> = Bloom::ALL.into_iter().map(|b| (b, 6)).collect();
        assert!(check_counts(&many).is_err(), "36 in all");
    }

    #[test]
    fn mismatch_rule() {
        use Bloom::*;
        // Higher targets: any lower judgement is a mismatch.
        assert!(is_mismatch(Analyze, Apply));
        assert!(is_mismatch(Evaluate, Analyze));
        assert!(is_mismatch(Create, Evaluate));
        assert!(!is_mismatch(Analyze, Analyze));
        assert!(!is_mismatch(Analyze, Evaluate));
        assert!(is_mismatch(Analyze, Create), "two levels higher");
        // Lower targets: any lower judgement, or two levels higher.
        assert!(!is_mismatch(Remember, Understand));
        assert!(is_mismatch(Understand, Remember));
        assert!(is_mismatch(Apply, Understand));
        assert!(is_mismatch(Remember, Apply));
        assert!(is_mismatch(Apply, Remember));
        assert!(is_mismatch(Understand, Evaluate));
    }

    #[test]
    fn formats_follow_the_teachers_selection() {
        let sel = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(level_formats(Bloom::Apply, &[]), [FMT_SHORT, FMT_BLANK]);
        assert_eq!(
            level_formats(
                Bloom::Remember,
                &sel(&[FMT_PARAGRAPH, FMT_CHOICE, FMT_SHORT])
            ),
            [FMT_CHOICE, FMT_SHORT]
        );
        // Nothing suitable selected: the teacher's selection, in label order.
        assert_eq!(
            level_formats(Bloom::Create, &sel(&[FMT_CHOICE, FMT_BLANK])),
            [FMT_BLANK, FMT_CHOICE]
        );
    }

    #[test]
    fn labels_ids_and_titles() {
        assert_eq!(Bloom::parse("analyze"), Some(Bloom::Analyze));
        assert_eq!(Bloom::parse("Analyze"), None);
        assert_eq!(Bloom::Analyze.tag(), "〔Bloom：分析 Analyze〕");
        assert_eq!(block_title(Bloom::Remember), "Find the Facts");
        assert_eq!(level_of_title("Part C · Look Closer"), Some(Bloom::Analyze));
        assert_eq!(level_of_title("Look Closer"), Some(Bloom::Analyze));
        assert_eq!(level_of_title("My own title"), None);
        assert_eq!(split_part_title("Part AB · x"), Some(("AB", "x")));
        assert_eq!(split_part_title("Part a · x"), None);
        assert_eq!(
            summary(&[(Bloom::Remember, 2), (Bloom::Apply, 0), (Bloom::Create, 1)]),
            "Bloom：記憶 2、創造 1"
        );
        let err = serde_json::from_str::<Bloom>("\"thinking\"").unwrap_err();
        assert!(err.to_string().contains("不是 Bloom 認知層次的選項"));
        assert!(serde_json::from_str::<Output>("\"quiz\"").is_err());
        assert_eq!(
            serde_json::to_value(BTreeMap::from([(Bloom::Create, 1), (Bloom::Remember, 2)]))
                .unwrap()
                .to_string(),
            r#"{"remember":2,"create":1}"#
        );
    }

    #[test]
    fn schemas_are_capped_and_keep_the_written_order() {
        let s = level_schema(&[FMT_CHOICE, FMT_SHORT], &paragraph_codes(3), 4);
        let top: Vec<&str> = s["properties"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            top,
            ["items", "steps", "example"],
            "R5: steps, no one-line instructions"
        );
        let items = &s["properties"]["items"];
        assert_eq!(items["maxItems"], 4);
        assert_eq!(items["minItems"], 4);
        let keys: Vec<&str> = items["items"]["properties"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            [
                "answer_format",
                "prompt",
                "options",
                "answer",
                "evidence",
                "paragraph_ids",
                "hint"
            ]
        );
        // Every string and array has a cap.
        fn capped(v: &Value) {
            match v.get("type").and_then(Value::as_str) {
                Some("string") if v.get("enum").is_none() => {
                    assert!(v.get("maxLength").is_some(), "{v}")
                }
                Some("array") => {
                    assert!(v.get("maxItems").is_some(), "{v}");
                    capped(&v["items"]);
                }
                Some("object") => {
                    for p in v["properties"].as_object().unwrap().values() {
                        capped(p);
                    }
                }
                _ => {}
            }
        }
        capped(&s);
        assert!(items["items"]["properties"].get("organizer").is_none());
        // With 圖像組織 allowed, an organizer after the options (capped in turn).
        let o = level_schema(&[FMT_TABLE, FMT_ORGANIZER], &paragraph_codes(2), 2);
        let props = &o["properties"]["items"]["items"]["properties"];
        let keys: Vec<&str> = props
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        // With 表格整理 too, a table after the organizer.
        assert_eq!(
            &keys[..6],
            [
                "answer_format",
                "prompt",
                "options",
                "organizer",
                "table",
                "answer"
            ]
        );
        capped(&props["organizer"]["anyOf"][0]);
        capped(&props["table"]["anyOf"][0]);
        assert_eq!(props["table"]["anyOf"][1]["type"], "null");
        let only_table = level_schema(&[FMT_TABLE], &paragraph_codes(2), 2);
        assert!(only_table["properties"]["items"]["items"]["properties"]
            .get("table")
            .is_some());
        assert!(items["items"]["properties"].get("table").is_none());
        let c = check_schema(&["Q1".to_string(), "Q2".to_string()]);
        capped(&c);
        assert_eq!(c["properties"]["items"]["maxItems"], 2);
        assert_eq!(
            c["properties"]["items"]["items"]["properties"]["id"]["enum"],
            json!(["Q1", "Q2"])
        );
    }
}
