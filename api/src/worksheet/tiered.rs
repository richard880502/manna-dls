//! 差異化學習單 (`output = "tiered"`, formerly 分層練習): the rules of
//! docs/SIOP_DESIGN_RULES.md section 三 (T0–T9) that are decided in code. Contract: docs/API.md
//! 「差異化學習單」.
//!
//! The three versions (基礎版／一般版／進階版) share the objectives, the questions and so the
//! Bloom distribution (T1, T2); they differ only in the support printed under each question and
//! in the answer expected (T3–T6). The model writes each question once, as one object with a part
//! per version ([`RawTieredItem`]); the server checks it
//! ([`super::checks::evaluate_tiered_level`]) and assembles the three versions in code
//! ([`super::assemble::project`]), so they cannot drift apart. (Asking the model for three
//! separate versions was the slowest and least stable design in docs/LLM_VALIDATION.md.)
//!
//! Students never see the level names: their sheets carry a neutral code, 「Version A／B／C」
//! (T9; [`version_code`]). Teachers keep 基礎版／一般版／進階版 (T0).

use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::bloom::Bloom;
use super::template::{enum_of, obj, s_max, strings_max};
use super::{Level, FMT_ORGANIZER, FMT_PARAGRAPH, FMT_SHORT, FMT_TABLE};

// ---------------------------------------------------------------- support forms (T3)

/// The support printed under a question, from least to most (T3: 預填較多字的克漏字 → 只給字庫 →
/// 句型框架 → 句首 → 不給支持). The order of the variants is the amount of support.
#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum SupportKind {
    #[default]
    None,
    SentenceStarter,
    SentenceFrame,
    WordBank,
    Cloze,
}

impl SupportKind {
    pub const ALL: [SupportKind; 5] = [
        SupportKind::None,
        SupportKind::SentenceStarter,
        SupportKind::SentenceFrame,
        SupportKind::WordBank,
        SupportKind::Cloze,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::SentenceStarter => "sentence_starter",
            Self::SentenceFrame => "sentence_frame",
            Self::WordBank => "word_bank",
            Self::Cloze => "cloze",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s.trim())
    }

    /// The teacher's name of the form.
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "不給支持",
            Self::SentenceStarter => "句首",
            Self::SentenceFrame => "句型框架",
            Self::WordBank => "字庫",
            Self::Cloze => "預填克漏字",
        }
    }

    /// The English heading printed above the support on the sheet.
    pub fn heading(self) -> &'static str {
        match self {
            Self::None => "",
            Self::SentenceStarter => "Start like this",
            Self::SentenceFrame => "Sentence frame",
            Self::WordBank => "Word bank",
            Self::Cloze => "Fill in the blanks",
        }
    }

    /// A cloze and a sentence frame are cut from a model sentence (T4): they need blanks, the
    /// sentence and a word bank of what was taken out.
    pub fn is_cut_from_sentence(self) -> bool {
        matches!(self, Self::Cloze | Self::SentenceFrame)
    }
}

/// The forms each version may use: 基礎版 the most support, 一般版 less, 進階版 none or a starter
/// (T3). On top of this, every question's support goes strictly down from 基礎版 to 進階版.
pub fn allowed_kinds(level: Level) -> &'static [SupportKind] {
    match level {
        Level::Basic => &[
            SupportKind::Cloze,
            SupportKind::WordBank,
            SupportKind::SentenceFrame,
        ],
        Level::Standard => &[
            SupportKind::WordBank,
            SupportKind::SentenceFrame,
            SupportKind::SentenceStarter,
        ],
        Level::Advanced => &[SupportKind::SentenceStarter, SupportKind::None],
    }
}

/// One version of a question of a 差異化學習單: the same question, with this version's support
/// and the answer this version expects (T3–T6).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ItemTier {
    pub level: Level,
    #[serde(default)]
    pub support_kind: SupportKind,
    /// The complete sentence a cloze or sentence frame is cut from (T4). Teacher version only.
    #[serde(default)]
    pub model_sentence: String,
    /// What this version prints under the question, one line each; blanks are 「___」.
    #[serde(default)]
    pub support: Vec<String>,
    /// Words printed as a word bank: for a cloze or frame, the words it took out.
    #[serde(default)]
    pub word_bank: Vec<String>,
    /// The answer expected in this version (基礎版 accepts a phrase). Teacher version only.
    #[serde(default)]
    pub answer: String,
    /// How to score the answer in this version (評分說明, Chinese). Teacher version only.
    #[serde(default)]
    pub scoring: String,
}

pub const MAX_SUPPORT_LINES: usize = 4;
pub const SUPPORT_LINE_CHARS: usize = 200;
pub const MAX_WORD_BANK: usize = 10;
pub const WORD_CHARS: usize = 40;
pub const MODEL_SENTENCE_CHARS: usize = 300;
pub const TIER_ANSWER_CHARS: usize = 400;
pub const SCORING_CHARS: usize = 150;
pub const MAX_LINKING_WORDS: usize = 4;
pub const LINKING_WORD_CHARS: usize = 30;
pub const FUNCTION_CHARS: usize = 40;
pub const STEP_CHARS: usize = 150;
pub const MIN_STEPS: usize = 2;
pub const MAX_STEPS: usize = 5;
pub const EXAMPLE_CHARS: usize = 300;

// ---------------------------------------------------------------- what a hint may not do (T3)

static LOCATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?ix)
        \b(?:paragraphs?|paras?\.?|lines?|sentences?)\s*\(?\s*(?:\d+|one|two|three|four|five|six|seven|eight|nine|ten)\b
        | \b(?:first|second|third|fourth|fifth|sixth|seventh|eighth|ninth|tenth|last|final|next|previous|opening|closing|middle)\s+(?:paragraph|sentence|line)s?\b
        | \[?\bP\d+\b\]?
        | \b(?:look|go\ back|go|refer|turn|check|see|read)\b[^.!?]{0,40}?\b(?:at|to|in)\s+(?:the\s+)?(?:paragraph|text|passage|story|article|line|sentence|reading)\b
        | \bfind\b[^.!?]{0,40}?\bin\s+(?:the\s+)?(?:paragraph|text|passage|story|article|reading)\b
        | 第\s*[0-9一二三四五六七八九十]+\s*[段行句]
        ",
    )
    .expect("valid regex")
});

/// Whether a support line points to where the answer is (「Look at paragraph 2.」): T3 forbids
/// giving the answer's location instead of language support.
pub fn points_to_answer_location(text: &str) -> bool {
    LOCATION.is_match(text)
}

// ---------------------------------------------------------------- answer formats

/// The formats a 差異化學習單 uses: the open ones, where the support forms and the per-version
/// answers (phrase or full sentences, T6) mean something.
pub const OPEN_FORMATS: [&str; 4] = [FMT_SHORT, FMT_PARAGRAPH, FMT_TABLE, FMT_ORGANIZER];

/// The formats a level may use in a 差異化學習單: its open formats that the teacher selected;
/// when none of them is selected (or she selected nothing), all of its open formats.
pub fn level_formats(b: Bloom, selected: &[String]) -> Vec<&'static str> {
    let open: Vec<&'static str> = b
        .formats()
        .iter()
        .copied()
        .filter(|f| OPEN_FORMATS.contains(f))
        .collect();
    let both: Vec<&'static str> = open
        .iter()
        .copied()
        .filter(|f| selected.iter().any(|s| s == f))
        .collect();
    if both.is_empty() {
        open
    } else {
        both
    }
}

// ---------------------------------------------------------------- the neutral code (T9)

/// The code a version's student sheet carries: 「A」, 「B」, 「C」 by the version's place in the
/// worksheet's `student_order` (when it lists exactly the worksheet's versions), else among
/// `levels`; `None` when the worksheet has only one version (nothing to tell apart).
pub fn version_code(levels: &[Level], student_order: &[Level], level: Level) -> Option<String> {
    if levels.len() < 2 || !levels.contains(&level) {
        return None;
    }
    let order = if same_versions(levels, student_order) {
        student_order
    } else {
        levels
    };
    order
        .iter()
        .position(|l| *l == level)
        .map(|i| ((b'A' + i as u8) as char).to_string())
}

/// Whether `order` lists each of `levels` exactly once.
fn same_versions(levels: &[Level], order: &[Level]) -> bool {
    order.len() == levels.len()
        && levels
            .iter()
            .all(|l| order.iter().filter(|o| *o == l).count() == 1)
}

/// The students' order for a newly generated 差異化學習單: the one it already has when that still
/// lists its versions (a regenerated worksheet keeps the codes printed before), else a random
/// order (T9: 「A」 must not mean 「the most support」).
pub fn student_order_for(levels: &[Level], existing: &[Level]) -> Vec<Level> {
    if levels.len() < 2 {
        return Vec::new();
    }
    if same_versions(levels, existing) {
        return existing.to_vec();
    }
    use rand::seq::SliceRandom;
    let mut order = levels.to_vec();
    order.shuffle(&mut rand::thread_rng());
    order
}

/// What the student sheet prints: 「Version A」. 「Version」 keeps it apart from the block letters
/// (「A. Find the Facts」, 「Part A · …」).
pub fn student_version_name(code: &str) -> String {
    format!("Version {code}")
}

/// The code in the teacher's words: 「A 版」.
pub fn version_label(code: &str) -> String {
    format!("{code} 版")
}

/// 分層說明 (teacher version only): why the versions differ.
pub const TIERED_REASON: &str = "差異化學習單：三個版本的學習目標、題目與 Bloom 分布相同，只差在支持的多寡。基礎版附預填克漏字、字庫或句型框架；一般版附較少的支持（字庫、句型框架或句首）；進階版只附句首或不附支持。答案示例與評分說明分版本寫：基礎版可接受片語，一般版與進階版要寫 1–2 個完整句。學生版只印 A／B／C 版，不印程度名稱。";

// ---------------------------------------------------------------- model output and schema

pub const LEVEL_SCHEMA_NAME: &str = "worksheet_tiered_level";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct RawVersion {
    pub support_kind: String,
    pub model_sentence: String,
    pub support: Vec<String>,
    pub word_bank: Vec<String>,
    pub answer: String,
    pub scoring_zh: String,
}

/// One question, written once, with a part per version.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawTieredItem {
    pub answer_format: String,
    pub prompt: String,
    #[serde(default)]
    pub evidence: String,
    #[serde(default)]
    pub paragraph_ids: Vec<String>,
    #[serde(default)]
    pub advanced: RawVersion,
    #[serde(default)]
    pub standard: RawVersion,
    #[serde(default)]
    pub basic: RawVersion,
}

impl RawTieredItem {
    pub fn version(&self, level: Level) -> &RawVersion {
        match level {
            Level::Basic => &self.basic,
            Level::Standard => &self.standard,
            Level::Advanced => &self.advanced,
        }
    }
}

/// Linking words for one language function, graded (T5): 基礎版 gets `basic`, 一般版 adds
/// `standard_more`, 進階版 adds `advanced_more` (the more formal ones).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct RawLinkingWords {
    pub function: String,
    pub basic: Vec<String>,
    pub standard_more: Vec<String>,
    pub advanced_more: Vec<String>,
}

/// The questions of one Bloom level for a 差異化學習單, with the block's steps (R5).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawTieredLevel {
    pub items: Vec<RawTieredItem>,
    #[serde(default)]
    pub steps: Vec<String>,
    #[serde(default)]
    pub example: String,
    #[serde(default)]
    pub linking_words: RawLinkingWords,
}

fn version_schema(level: Level) -> Value {
    let kinds: Vec<&str> = allowed_kinds(level).iter().map(|k| k.as_str()).collect();
    obj(vec![
        ("support_kind", enum_of(&kinds)),
        ("model_sentence", s_max(MODEL_SENTENCE_CHARS)),
        (
            "support",
            strings_max(MAX_SUPPORT_LINES, SUPPORT_LINE_CHARS),
        ),
        ("word_bank", strings_max(MAX_WORD_BANK, WORD_CHARS)),
        ("answer", s_max(TIER_ANSWER_CHARS)),
        ("scoring_zh", s_max(SCORING_CHARS)),
    ])
}

/// Exactly `count` questions of the given formats, each with its three versions (written 進階版
/// first: the full answer, then less and less of it as support), then the block's steps, an
/// optional example and the graded linking words. Every string and array is capped.
pub fn level_schema(formats: &[&str], paragraph_codes: &[String], count: usize) -> Value {
    let codes: Vec<&str> = paragraph_codes.iter().map(String::as_str).collect();
    let count = count.max(1);
    obj(vec![
        (
            "items",
            json!({
                "type": "array",
                "items": obj(vec![
                    ("answer_format", enum_of(formats)),
                    ("prompt", s_max(super::bloom::PROMPT_CHARS)),
                    ("evidence", s_max(super::bloom::EVIDENCE_CHARS)),
                    ("paragraph_ids", json!({
                        "type": "array", "items": enum_of(&codes),
                        "maxItems": super::bloom::PARAGRAPHS_PER_ITEM
                    })),
                    ("advanced", version_schema(Level::Advanced)),
                    ("standard", version_schema(Level::Standard)),
                    ("basic", version_schema(Level::Basic)),
                ]),
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
        (
            "linking_words",
            obj(vec![
                ("function", s_max(FUNCTION_CHARS)),
                ("basic", strings_max(MAX_LINKING_WORDS, LINKING_WORD_CHARS)),
                (
                    "standard_more",
                    strings_max(MAX_LINKING_WORDS, LINKING_WORD_CHARS),
                ),
                (
                    "advanced_more",
                    strings_max(MAX_LINKING_WORDS, LINKING_WORD_CHARS),
                ),
            ]),
        ),
    ])
}

/// Output budget of one level call: a question with its three versions is about four times a
/// plain one.
pub fn level_max_tokens(count: usize) -> u32 {
    (800 + 900 * count.min(12) as u32).clamp(2500, 8000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn support_goes_down_from_basic_to_advanced() {
        assert!(SupportKind::Cloze > SupportKind::WordBank);
        assert!(SupportKind::WordBank > SupportKind::SentenceFrame);
        assert!(SupportKind::SentenceFrame > SupportKind::SentenceStarter);
        assert!(SupportKind::SentenceStarter > SupportKind::None);
        let most = |l: Level| allowed_kinds(l).iter().max().copied().unwrap();
        let least = |l: Level| allowed_kinds(l).iter().min().copied().unwrap();
        assert!(least(Level::Basic) >= most(Level::Advanced));
        assert!(most(Level::Basic) > most(Level::Standard));
        assert!(most(Level::Standard) > most(Level::Advanced));
        assert_eq!(least(Level::Advanced), SupportKind::None);
        for k in SupportKind::ALL {
            assert_eq!(SupportKind::parse(k.as_str()), Some(k));
        }
    }

    #[test]
    fn hints_that_give_the_answer_location_are_caught() {
        for bad in [
            "Look at paragraph 2.",
            "Read paragraph two again.",
            "Look at the second paragraph.",
            "The answer is in the last sentence.",
            "Go back to the text and find the word.",
            "See line 4.",
            "Find it in the story.",
            "Check [P3].",
            "看第 2 段。",
        ] {
            assert!(points_to_answer_location(bad), "{bad}");
        }
        for fine in [
            "The neighbors thought that ___.",
            "I think Mrs. Lin was ___ because ___.",
            "First, … Then, …",
            "trust, return, borrow",
            "The writer gives an example to show ___.",
        ] {
            assert!(!points_to_answer_location(fine), "{fine}");
        }
    }

    #[test]
    fn only_open_formats_are_used() {
        assert_eq!(level_formats(Bloom::Remember, &[]), [FMT_SHORT]);
        assert_eq!(
            level_formats(Bloom::Analyze, &[]),
            [FMT_TABLE, FMT_ORGANIZER, FMT_SHORT]
        );
        let sel = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            level_formats(Bloom::Evaluate, &sel(&[FMT_PARAGRAPH, "選擇題"])),
            [FMT_PARAGRAPH]
        );
        // Only closed formats selected: the level's open formats.
        assert_eq!(
            level_formats(Bloom::Create, &sel(&["選擇題"])),
            [FMT_PARAGRAPH, FMT_SHORT]
        );
    }

    #[test]
    fn codes_follow_the_versions() {
        let all = Level::ALL.to_vec();
        assert_eq!(version_code(&all, &[], Level::Basic).as_deref(), Some("A"));
        assert_eq!(
            version_code(&all, &[], Level::Advanced).as_deref(),
            Some("C")
        );
        let two = [Level::Standard, Level::Advanced];
        assert_eq!(
            version_code(&two, &[], Level::Standard).as_deref(),
            Some("A")
        );
        assert_eq!(version_code(&[Level::Standard], &[], Level::Standard), None);
        // A stored order decides the letters; one that doesn't list the versions is ignored.
        let order = [Level::Advanced, Level::Basic, Level::Standard];
        assert_eq!(
            version_code(&all, &order, Level::Basic).as_deref(),
            Some("B")
        );
        assert_eq!(
            version_code(&all, &order, Level::Advanced).as_deref(),
            Some("A")
        );
        assert_eq!(
            version_code(&all, &order, Level::Standard).as_deref(),
            Some("C")
        );
        let partial = [Level::Advanced, Level::Basic];
        assert_eq!(
            version_code(&all, &partial, Level::Basic).as_deref(),
            Some("A")
        );
        // A new order lists every version once; an existing one is kept.
        for _ in 0..20 {
            let fresh = student_order_for(&all, &[]);
            assert_eq!(fresh.len(), 3);
            assert!(all.iter().all(|l| fresh.contains(l)));
        }
        assert_eq!(student_order_for(&all, &order), order.to_vec());
        assert!(student_order_for(&[Level::Standard], &[]).is_empty());
        assert_eq!(student_version_name("B"), "Version B");
        assert_eq!(version_label("C"), "C 版");
    }

    #[test]
    fn the_schema_is_capped_and_ordered() {
        let s = level_schema(&[FMT_SHORT], &super::super::bloom::paragraph_codes(2), 3);
        let keys: Vec<&str> = s["properties"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, ["items", "steps", "example", "linking_words"]);
        let item = &s["properties"]["items"]["items"]["properties"];
        let keys: Vec<&str> = item
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
                "evidence",
                "paragraph_ids",
                "advanced",
                "standard",
                "basic"
            ]
        );
        assert_eq!(
            item["basic"]["properties"]["support_kind"]["enum"],
            json!(["cloze", "word_bank", "sentence_frame"])
        );
        assert_eq!(
            item["advanced"]["properties"]["support_kind"]["enum"],
            json!(["sentence_starter", "none"])
        );
        assert_eq!(s["properties"]["steps"]["minItems"], 2);
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
    }
}
