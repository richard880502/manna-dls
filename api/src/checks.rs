//! Automatic checks run on every AI output before it is shown (spec §5.2).
//!
//! All functions are pure; the pipeline records the full report in
//! `ai_interactions.check_results`.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;
use serde_json::{json, Value};

use crate::ai::schemas::{CoreAssistOutput, MaterialFocusesOutput, PlanSuggestOutput};
use crate::retrieval::is_cjk;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CheckResult {
    pub name: &'static str,
    pub passed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl CheckResult {
    pub fn pass(name: &'static str) -> Self {
        Self::new(name, true, None)
    }

    pub fn fail(name: &'static str, detail: impl Into<String>) -> Self {
        Self::new(name, false, Some(detail.into()))
    }

    /// Passed because the code fixed what was wrong; `detail` says what it changed.
    pub fn repaired(name: &'static str, detail: impl Into<String>) -> Self {
        Self::new(name, true, Some(detail.into()))
    }

    fn new(name: &'static str, passed: bool, detail: Option<String>) -> Self {
        Self {
            name,
            passed,
            item_index: None,
            detail,
        }
    }
    fn at(mut self, i: usize) -> Self {
        self.item_index = Some(i);
        self
    }
}

pub fn all_passed(results: &[CheckResult]) -> bool {
    results.iter().all(|r| r.passed)
}

pub fn report(results: &[CheckResult]) -> Value {
    json!({ "passed": all_passed(results), "checks": results })
}

pub fn contains_cjk(s: &str) -> bool {
    s.chars().any(is_cjk)
}

// ---------------------------------------------------------------- level mapping

// Heuristic for "級別是否錯換" (spec §5.2): a sentence that mentions a 大考 vocabulary level
// AND a CEFR level or a school grade AND an equating word, without a negation, is treated as
// equating the two systems. Examples flagged: 「大考第三級單字相當於 CEFR B1」,
// 「Level 4 = 高二程度」. Not flagged: 「大考六級與 CEFR 是不同系統，不能直接對應」.
static LEVEL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)level\s*[1-6]|第\s*[1-6一二三四五六]\s*級|[1-6一二三四五六]\s*級\s*(?:字|單字|詞彙|字彙)|大考\s*[1-6一二三四五六]\s*級|大考.{0,6}級別",
    )
    .expect("valid regex")
});
static CEFR_OR_GRADE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)cefr|(?:^|[^A-Za-z0-9])[abc][12](?:$|[^A-Za-z0-9])|高[一二三](?:程度|學生|生)?|國[一二三]|[七八九十]\s*年級|1[0-2]\s*年級|grade\s*(?:[7-9]|1[0-2])|(?:國小|國中|高中|高職)\s*程度",
    )
    .expect("valid regex")
});
static EQUATE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)[=＝≈]|等於|等同|相當於|相當|對應|對照為|即為|就是|equals?|equivalent|corresponds?|maps?\s+to")
        .expect("valid regex")
});
static NEGATION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)不能|不可|不宜|不應|不等於|不等同|不是|並非|並不|無法直接|不直接|不要|避免|\bnot\b|n't")
        .expect("valid regex")
});

/// Returns the offending sentence if `text` equates 大考 levels with CEFR levels or grades.
pub fn level_mapping_violation(text: &str) -> Option<String> {
    text.split(['。', '！', '？', '!', '?', '\n', '；', ';'])
        .map(str::trim)
        .find(|s| {
            LEVEL_RE.is_match(s)
                && CEFR_OR_GRADE_RE.is_match(s)
                && EQUATE_RE.is_match(s)
                && !NEGATION_RE.is_match(s)
        })
        .map(str::to_string)
}

// ---------------------------------------------------------------- CoRe assist

const ANSWER_PREFIXES: &[&str] = &[
    "答案",
    "參考答案",
    "範例答案",
    "建議答案",
    "標準答案",
    "示範答案",
    "你可以寫",
    "妳可以寫",
    "您可以寫",
    "可以寫成",
    "可以這樣寫",
    "你可以這樣寫",
    "範例：",
    "範例:",
    "例如你可以寫",
    "以下是範例",
    "以下是參考",
    "answer:",
    "sample answer",
    "model answer",
];
const ANSWER_ANYWHERE: &[&str] = &[
    "範例答案",
    "參考答案",
    "建議答案",
    "標準答案",
    "可直接貼上",
    "直接貼上",
    "直接填入",
    "直接複製",
];

/// Heuristic: does the text look like a filled-in answer the teacher could paste?
pub fn looks_like_pasteable_answer(text: &str) -> bool {
    let trimmed = text
        .trim_start_matches(|c: char| c.is_whitespace() || "「『\"'【(（*#-・".contains(c))
        .to_lowercase();
    ANSWER_PREFIXES.iter().any(|p| trimmed.starts_with(p))
        || ANSWER_ANYWHERE.iter().any(|p| text.contains(p))
}

/// Removes text inside 「」『』“” quotes (quoted teacher text may itself contain question marks).
fn strip_quoted(text: &str) -> String {
    let mut out = String::new();
    let mut depth = 0usize;
    for c in text.chars() {
        match c {
            '「' | '『' | '“' => depth += 1,
            '」' | '』' | '”' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

pub fn question_mark_count(text: &str) -> usize {
    strip_quoted(text)
        .chars()
        .filter(|c| matches!(c, '？' | '?'))
        .count()
}

/// Checks for a CoRe assist output at `requested_level`:
/// * level 1 restates the question, level 2 offers thinking dimensions — neither may be an answer;
/// * level 3 asks exactly one concrete follow-up question (ends with ？ / ?, one question mark
///   outside quotes).
pub fn check_core_assist(out: &CoreAssistOutput, requested_level: u8) -> Vec<CheckResult> {
    let text = out.text.trim();
    let mut r = vec![
        CheckResult::new(
            "level_matches",
            out.level == requested_level,
            (out.level != requested_level)
                .then(|| format!("requested {requested_level}, got {}", out.level)),
        ),
        CheckResult::new("non_empty", !text.is_empty(), None),
        CheckResult::new(
            "language_zh",
            contains_cjk(text),
            (!contains_cjk(text)).then(|| "說明需以繁體中文撰寫".to_string()),
        ),
        CheckResult::new(
            "not_pasteable_answer",
            !looks_like_pasteable_answer(text),
            looks_like_pasteable_answer(text).then(|| "輸出看起來像可直接貼上的答案".to_string()),
        ),
    ];
    let mapping = level_mapping_violation(text);
    r.push(CheckResult::new(
        "level_mapping",
        mapping.is_none(),
        mapping,
    ));
    if requested_level == 3 {
        let ends = text.ends_with('？') || text.ends_with('?');
        let n = question_mark_count(text);
        r.push(CheckResult::new(
            "single_question",
            ends && n == 1,
            (!(ends && n == 1)).then(|| format!("ends_with_question={ends}, question_marks={n}")),
        ));
    }
    r
}

// ---------------------------------------------------------------- plan suggestions

/// What the checks need to know about one retrieved item.
#[derive(Debug, Clone)]
pub struct RetrievedRef {
    pub item_code: String,
    pub school_track: String,
}

/// Checks for plan suggestions:
/// * kb_empty ⇔ no suggestions;
/// * each suggestion has ≥ 1 citation, every cited code was retrieved for this request, and
///   cited items belong to the teacher's track (or `both`);
/// * body is written in Chinese (contains CJK; a fully ASCII body is rejected);
/// * no 大考-level ↔ CEFR/grade equating in title or body.
pub fn check_plan(
    out: &PlanSuggestOutput,
    retrieved: &[RetrievedRef],
    teacher_track: &str,
) -> Vec<CheckResult> {
    let tracks: HashMap<&str, &str> = retrieved
        .iter()
        .map(|r| (r.item_code.as_str(), r.school_track.as_str()))
        .collect();
    let mut r = Vec::new();
    let consistent = out.kb_empty == out.suggestions.is_empty();
    r.push(CheckResult::new(
        "kb_empty_consistent",
        consistent,
        (!consistent).then(|| {
            format!(
                "kb_empty={} with {} suggestions",
                out.kb_empty,
                out.suggestions.len()
            )
        }),
    ));
    for (i, s) in out.suggestions.iter().enumerate() {
        r.push(CheckResult::new("has_citation", !s.citations.is_empty(), None).at(i));
        let unknown: Vec<&str> = s
            .citations
            .iter()
            .map(String::as_str)
            .filter(|c| !tracks.contains_key(c))
            .collect();
        r.push(
            CheckResult::new(
                "citations_retrieved",
                unknown.is_empty(),
                (!unknown.is_empty())
                    .then(|| format!("not in this retrieval: {}", unknown.join(", "))),
            )
            .at(i),
        );
        let wrong_track: Vec<&str> = s
            .citations
            .iter()
            .map(String::as_str)
            .filter(|c| {
                tracks
                    .get(c)
                    .is_some_and(|t| *t != "both" && *t != teacher_track)
            })
            .collect();
        r.push(
            CheckResult::new(
                "track_match",
                wrong_track.is_empty(),
                (!wrong_track.is_empty())
                    .then(|| format!("other track: {}", wrong_track.join(", "))),
            )
            .at(i),
        );
        let zh = contains_cjk(&s.body) && !s.title.trim().is_empty();
        r.push(
            CheckResult::new(
                "language_zh",
                zh,
                (!zh).then(|| "建議說明需以繁體中文撰寫".to_string()),
            )
            .at(i),
        );
        let mapping = level_mapping_violation(&format!("{}\n{}", s.title, s.body));
        r.push(CheckResult::new("level_mapping", mapping.is_none(), mapping).at(i));
    }
    r
}

/// Plausible range for an AI-estimated activity duration, in minutes.
pub const DURATION_RANGE: std::ops::RangeInclusive<i32> = 1..=120;

/// Differentiation fields of plan suggestions (docs/API.md Phase 1.1):
/// * when the teacher's P3 includes differentiation, every suggestion must carry
///   `duration_min`, a non-blank `support_note` and a non-blank `extension_note`;
/// * any `duration_min` given must lie in [`DURATION_RANGE`] (≤ 0 or > 120 is rejected).
pub fn check_plan_differentiation(out: &PlanSuggestOutput, required: bool) -> Vec<CheckResult> {
    let mut r = Vec::new();
    for (i, s) in out.suggestions.iter().enumerate() {
        if required {
            let blank = |v: &Option<String>| v.as_deref().is_none_or(|t| t.trim().is_empty());
            let mut missing = Vec::new();
            if s.duration_min.is_none() {
                missing.push("duration_min");
            }
            if blank(&s.support_note) {
                missing.push("support_note");
            }
            if blank(&s.extension_note) {
                missing.push("extension_note");
            }
            r.push(
                CheckResult::new(
                    "differentiation_fields",
                    missing.is_empty(),
                    (!missing.is_empty()).then(|| format!("missing: {}", missing.join(", "))),
                )
                .at(i),
            );
        }
        if let Some(d) = s.duration_min {
            let ok = DURATION_RANGE.contains(&d);
            r.push(
                CheckResult::new(
                    "duration_plausible",
                    ok,
                    (!ok).then(|| format!("duration_min={d} outside 1–120")),
                )
                .at(i),
            );
        }
    }
    r
}

/// Distinct cited codes in first-cited order.
pub fn cited_codes(out: &PlanSuggestOutput) -> Vec<String> {
    let mut seen = HashSet::new();
    out.suggestions
        .iter()
        .flat_map(|s| s.citations.iter())
        .filter(|c| seen.insert(c.as_str()))
        .cloned()
        .collect()
}

// ---------------------------------------------------------------- material focuses

pub const MAX_MATERIAL_CANDIDATES: usize = 5;

/// Whitespace-insensitive form used to compare excerpts with the material: every whitespace
/// character is removed, so line breaks or doubled spaces in either text do not matter.
pub fn normalize_ws(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// Checks for 教材優先 candidate focuses:
/// * at most [`MAX_MATERIAL_CANDIDATES`] candidates (fewer than three, even none, is allowed);
/// * every excerpt is non-empty and, whitespace-normalised, a verbatim substring of the stored
///   material text;
/// * every big_idea contains Chinese; no 大考-level ↔ CEFR/grade equating.
pub fn check_material_focuses(out: &MaterialFocusesOutput, material: &str) -> Vec<CheckResult> {
    let n = out.candidates.len();
    let mut r = vec![CheckResult::new(
        "max_candidates",
        n <= MAX_MATERIAL_CANDIDATES,
        (n > MAX_MATERIAL_CANDIDATES).then(|| format!("{n} candidates")),
    )];
    let haystack = normalize_ws(material);
    for (i, c) in out.candidates.iter().enumerate() {
        let needle = normalize_ws(&c.excerpt);
        let verbatim = !needle.is_empty() && haystack.contains(&needle);
        r.push(
            CheckResult::new(
                "excerpt_verbatim",
                verbatim,
                (!verbatim).then(|| "excerpt 不是教材原文的一段".to_string()),
            )
            .at(i),
        );
        let zh = contains_cjk(&c.big_idea);
        r.push(
            CheckResult::new(
                "big_idea_zh",
                zh,
                (!zh).then(|| "學習重點需以中文撰寫".to_string()),
            )
            .at(i),
        );
        let mapping = level_mapping_violation(&format!("{}\n{}", c.big_idea, c.rationale));
        r.push(CheckResult::new("level_mapping", mapping.is_none(), mapping).at(i));
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::schemas::{IcapLevel, MaterialCandidate, PlanSuggestion};

    fn core(level: u8, text: &str) -> CoreAssistOutput {
        CoreAssistOutput {
            level,
            text: text.into(),
        }
    }

    fn failed(r: &[CheckResult]) -> Vec<&'static str> {
        r.iter().filter(|c| !c.passed).map(|c| c.name).collect()
    }

    #[test]
    fn cjk_detection() {
        assert!(contains_cjk("Think-Pair-Share 活動"));
        assert!(!contains_cjk("Think-Pair-Share, 100%!"));
        assert!(
            !contains_cjk("（）。，"),
            "punctuation alone is not Chinese text"
        );
    }

    #[test]
    fn level_mapping_flags_equating() {
        assert!(level_mapping_violation("大考第三級單字相當於 CEFR B1。").is_some());
        assert!(level_mapping_violation("Level 4 = 高二程度").is_some());
        assert!(level_mapping_violation("這些是第五級詞彙，對應B2").is_some());
        assert!(level_mapping_violation("4級單字就是高中程度的字").is_some());
    }

    #[test]
    fn level_mapping_allows_disclaimers_and_unrelated_text() {
        assert!(level_mapping_violation("大考六級與 CEFR 是不同系統，不能直接對應。").is_none());
        assert!(level_mapping_violation("本課重點是 B1 常見的句型。").is_none());
        assert!(level_mapping_violation("請挑出第四級單字讓學生練習。").is_none());
        assert!(level_mapping_violation("學生的 L1 是中文，可以對照母語").is_none());
        assert!(level_mapping_violation("").is_none());
    }

    #[test]
    fn pasteable_answer_heuristic() {
        assert!(looks_like_pasteable_answer("答案：學生要學會倒裝句。"));
        assert!(looks_like_pasteable_answer(
            "「你可以寫：學生能用 Not only 造句」"
        ));
        assert!(looks_like_pasteable_answer("範例答案如下……"));
        assert!(looks_like_pasteable_answer(
            "Sample answer: students will..."
        ));
        assert!(!looks_like_pasteable_answer(
            "這一題想了解你希望學生最後真正學會什麼。"
        ));
        assert!(!looks_like_pasteable_answer("可以從學生的先備經驗想想。"));
    }

    #[test]
    fn core_level1_and_2_pass_with_chinese_explanation() {
        assert!(all_passed(&check_core_assist(
            &core(1, "這一題想了解的是學生最後要真正學會什麼。"),
            1
        )));
        assert!(all_passed(&check_core_assist(
            &core(2, "・學生的先備經驗？\n・真實溝通情境"),
            2
        )));
    }

    #[test]
    fn core_rejects_wrong_level_english_empty_and_answers() {
        assert_eq!(
            failed(&check_core_assist(&core(2, "這題在問什麼。"), 1)),
            vec!["level_matches"]
        );
        assert_eq!(
            failed(&check_core_assist(
                &core(1, "This question asks what students learn."),
                1
            )),
            vec!["language_zh"]
        );
        assert!(failed(&check_core_assist(&core(1, "   "), 1)).contains(&"non_empty"));
        assert_eq!(
            failed(&check_core_assist(&core(1, "答案：學生要會倒裝句。"), 1)),
            vec!["not_pasteable_answer"]
        );
    }

    #[test]
    fn core_level3_requires_exactly_one_question() {
        let ok = core(
            3,
            "你提到「學生會搞混？」，你會用哪個課堂情境讓學生用到它？",
        );
        assert!(
            all_passed(&check_core_assist(&ok, 3)),
            "{:?}",
            check_core_assist(&ok, 3)
        );
        let two = core(3, "你為什麼這樣安排？學生會怎麼回應？");
        assert_eq!(failed(&check_core_assist(&two, 3)), vec!["single_question"]);
        let statement = core(3, "你可以再想想學生的反應。");
        assert_eq!(
            failed(&check_core_assist(&statement, 3)),
            vec!["single_question"]
        );
    }

    fn sug(body: &str, cites: &[&str]) -> PlanSuggestion {
        PlanSuggestion {
            title: "配對練習".into(),
            body: body.into(),
            activity_type: Some("配對討論".into()),
            icap_level: Some(IcapLevel::Interactive),
            citations: cites.iter().map(|c| c.to_string()).collect(),
            duration_min: None,
            support_note: None,
            extension_note: None,
        }
    }

    fn refs() -> Vec<RetrievedRef> {
        vec![
            RetrievedRef {
                item_code: "DEMO-GRM-001".into(),
                school_track: "both".into(),
            },
            RetrievedRef {
                item_code: "DEMO-CUR-V01".into(),
                school_track: "vocational".into(),
            },
        ]
    }

    #[test]
    fn plan_valid_output_passes() {
        let out = PlanSuggestOutput {
            kb_empty: false,
            suggestions: vec![sug("讓學生兩兩比較一般語序與倒裝句。", &["DEMO-GRM-001"])],
        };
        let r = check_plan(&out, &refs(), "general");
        assert!(all_passed(&r), "{r:?}");
        assert_eq!(cited_codes(&out), vec!["DEMO-GRM-001"]);
    }

    #[test]
    fn plan_rejects_missing_unknown_and_wrong_track_citations() {
        let out = PlanSuggestOutput {
            kb_empty: false,
            suggestions: vec![
                sug("沒有引用的建議。", &[]),
                sug("引用不存在的條目。", &["FAKE-999"]),
                sug("引用技高條目。", &["DEMO-CUR-V01"]),
            ],
        };
        let r = check_plan(&out, &refs(), "general");
        let f: Vec<(&str, Option<usize>)> = r
            .iter()
            .filter(|c| !c.passed)
            .map(|c| (c.name, c.item_index))
            .collect();
        assert_eq!(
            f,
            vec![
                ("has_citation", Some(0)),
                ("citations_retrieved", Some(1)),
                ("track_match", Some(2))
            ]
        );
        // a vocational teacher may cite the vocational item
        let ok = PlanSuggestOutput {
            kb_empty: false,
            suggestions: vec![sug("引用技高條目。", &["DEMO-CUR-V01"])],
        };
        assert!(all_passed(&check_plan(&ok, &refs(), "vocational")));
    }

    #[test]
    fn plan_rejects_english_only_body_and_level_mapping() {
        let out = PlanSuggestOutput {
            kb_empty: false,
            suggestions: vec![
                sug("Have students compare word order.", &["DEMO-GRM-001"]),
                sug(
                    "這些第四級單字相當於 CEFR B1，適合高二。",
                    &["DEMO-GRM-001"],
                ),
            ],
        };
        let f: Vec<&str> = failed(&check_plan(&out, &refs(), "general"));
        assert_eq!(f, vec!["language_zh", "level_mapping"]);
    }

    #[test]
    fn plan_kb_empty_consistency() {
        let empty = PlanSuggestOutput {
            kb_empty: true,
            suggestions: vec![],
        };
        assert!(all_passed(&check_plan(&empty, &refs(), "general")));
        let contradictory = PlanSuggestOutput {
            kb_empty: true,
            suggestions: vec![sug("內容。", &["DEMO-GRM-001"])],
        };
        assert_eq!(
            failed(&check_plan(&contradictory, &refs(), "general")),
            vec!["kb_empty_consistent"]
        );
        let nothing = PlanSuggestOutput {
            kb_empty: false,
            suggestions: vec![],
        };
        assert_eq!(
            failed(&check_plan(&nothing, &refs(), "general")),
            vec!["kb_empty_consistent"]
        );
    }

    #[test]
    fn report_shape() {
        let r = check_core_assist(&core(1, "說明題意。"), 1);
        let v = report(&r);
        assert_eq!(v["passed"], json!(true));
        assert!(v["checks"].as_array().unwrap().len() >= 4);
    }

    fn diff(d: Option<i32>, support: Option<&str>, ext: Option<&str>) -> PlanSuggestOutput {
        let mut s = sug("讓學生兩兩練習。", &["DEMO-GRM-001"]);
        s.duration_min = d;
        s.support_note = support.map(str::to_string);
        s.extension_note = ext.map(str::to_string);
        PlanSuggestOutput {
            kb_empty: false,
            suggestions: vec![s],
        }
    }

    #[test]
    fn differentiation_fields_required_only_when_preferred() {
        let full = diff(Some(15), Some("提供句型框架"), Some("改寫成正式語氣"));
        assert!(all_passed(&check_plan_differentiation(&full, true)));
        let none = diff(None, None, None);
        assert!(all_passed(&check_plan_differentiation(&none, false)));
        let r = check_plan_differentiation(&none, true);
        assert_eq!(failed(&r), vec!["differentiation_fields"]);
        assert!(r[0].detail.as_deref().unwrap().contains("extension_note"));
        let blank = diff(Some(10), Some("  "), Some("延伸"));
        assert_eq!(
            failed(&check_plan_differentiation(&blank, true)),
            vec!["differentiation_fields"]
        );
    }

    #[test]
    fn absurd_durations_are_rejected() {
        for d in [0, -5, 121, 600] {
            let out = diff(Some(d), Some("支援"), Some("延伸"));
            assert_eq!(
                failed(&check_plan_differentiation(&out, false)),
                vec!["duration_plausible"],
                "{d}"
            );
        }
        for d in [1, 45, 120] {
            assert!(all_passed(&check_plan_differentiation(
                &diff(Some(d), Some("支援"), Some("延伸")),
                true
            )));
        }
    }

    const MATERIAL: &str = "Not only does Taiwan have tall mountains,\n  but it also has beautiful beaches.\n\n台灣的夜市 文化很有名。";

    fn cand(big_idea: &str, excerpt: &str) -> MaterialCandidate {
        MaterialCandidate {
            big_idea: big_idea.into(),
            rationale: "這段可以讓學生觀察倒裝句。".into(),
            excerpt: excerpt.into(),
        }
    }

    #[test]
    fn material_excerpts_must_be_verbatim_whitespace_normalised() {
        let out = MaterialFocusesOutput {
            candidates: vec![
                cand(
                    "倒裝句 Not only",
                    "Not only does Taiwan have tall mountains, but it also has",
                ),
                cand("夜市文化", "台灣的夜市文化很有名。"),
            ],
        };
        let r = check_material_focuses(&out, MATERIAL);
        assert!(all_passed(&r), "{r:?}");
        let bad = MaterialFocusesOutput {
            candidates: vec![
                cand("改寫的句子", "Taiwan has not only mountains"),
                cand("空白", "   "),
            ],
        };
        let f: Vec<(&str, Option<usize>)> = check_material_focuses(&bad, MATERIAL)
            .iter()
            .filter(|c| !c.passed)
            .map(|c| (c.name, c.item_index))
            .collect();
        assert_eq!(
            f,
            vec![("excerpt_verbatim", Some(0)), ("excerpt_verbatim", Some(1))]
        );
    }

    #[test]
    fn material_big_idea_needs_chinese_and_at_most_five() {
        let english = MaterialFocusesOutput {
            candidates: vec![cand("Inversion", "台灣的夜市")],
        };
        assert_eq!(
            failed(&check_material_focuses(&english, MATERIAL)),
            vec!["big_idea_zh"]
        );
        let six = MaterialFocusesOutput {
            candidates: (0..6).map(|_| cand("夜市", "夜市")).collect(),
        };
        assert_eq!(
            failed(&check_material_focuses(&six, MATERIAL)),
            vec!["max_candidates"]
        );
        let none = MaterialFocusesOutput { candidates: vec![] };
        assert!(all_passed(&check_material_focuses(&none, MATERIAL)));
        let two = MaterialFocusesOutput {
            candidates: vec![cand("夜市", "夜市"), cand("山", "mountains")],
        };
        assert!(all_passed(&check_material_focuses(&two, MATERIAL)));
    }
}
