//! Automatic checks for the two validated SOP stages, ported from
//! research/sop-validation/chain/checks.py (docs/LLM_VALIDATION.md §5–§6). All pure.
//!
//! Unlike the other AI checks, a failure here does not have to reject the whole output: the
//! parts that cannot be wrong are enforced in code. Evidence that is not a verbatim piece of its
//! paragraph is dropped, a curriculum code that was not offered is dropped, and only what
//! survives is shown. The `needs_repair` flags say whether a second call is worth making.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::checks::{contains_cjk, CheckResult};
use crate::curriculum::CurriculumItem;

use super::schemas::{
    AnalysisSuggestion, FeatureSuggestion, FocusSuggestion, ObjectiveSuggestion,
    ObjectivesSuggestion, ParagraphSuggestion, RawAnalysis, RawObjectives, SIOP_CONTENT,
    SIOP_LANGUAGE,
};
use super::{CurriculumLink, LinkStatus, ObjectiveType, Paragraph};

/// Most links kept per objective.
pub const MAX_LINKS: usize = 3;
/// Most language-focus items shown.
pub const MAX_FOCUS: usize = 6;
/// An objective that shares this share of its CJK bigrams with one curriculum item was copied.
pub const COPIED_OVERLAP: f64 = 0.8;

// ---------------------------------------------------------------- text helpers

/// Whitespace and typographic normalisation used for verbatim comparison (case is kept).
pub fn norm(s: &str) -> String {
    let s: String = s
        .chars()
        .map(|c| match c {
            '’' | '‘' => '\'',
            '“' | '”' => '"',
            '—' | '–' => '-',
            '\u{a0}' => ' ',
            c => c,
        })
        .collect();
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// True when `quote` is one continuous piece of `source`. Surrounding quotation marks and a
/// trailing punctuation mark are forgiven; a quote spliced with "..." is not a substring.
pub fn is_verbatim(quote: &str, source: &str) -> bool {
    locate_verbatim(quote, source).is_some()
}

/// The piece of `source` that `quote` copies, exactly as `source` writes it. Besides what
/// [`is_verbatim`] forgives, double quotation marks are ignored on both sides: the model copies
/// `the words "Trust Stand" on the side` as `the words Trust Stand on the side` (real-model run:
/// such quotes failed the check, cost a repair call, and some questions were dropped).
pub fn locate_verbatim(quote: &str, source: &str) -> Option<String> {
    // Normalised characters, each with the byte range it came from in `source`.
    fn chars(s: &str) -> Vec<(char, usize, usize)> {
        let mut out: Vec<(char, usize, usize)> = Vec::new();
        for (i, c) in s.char_indices() {
            let end = i + c.len_utf8();
            let c = match c {
                '\u{2019}' | '\u{2018}' => '\'',
                '\u{201c}' | '\u{201d}' | '"' => continue,
                '\u{2014}' | '\u{2013}' => '-',
                c if c.is_whitespace() => ' ',
                c => c,
            };
            if c == ' ' && out.last().is_none_or(|(p, _, _)| *p == ' ') {
                continue;
            }
            out.push((c, i, end));
        }
        while out.last().is_some_and(|(c, _, _)| *c == ' ') {
            out.pop();
        }
        out
    }
    let q: String = chars(quote).into_iter().map(|(c, _, _)| c).collect();
    let q = q.trim_matches(|c| c == ' ' || c == '\'');
    // A trailing punctuation mark is forgiven, but kept when the source has it too.
    let punct = q
        .chars()
        .last()
        .filter(|c| matches!(c, '.' | ',' | ';' | ':' | '!' | '?'));
    let q = q
        .trim_end_matches(['.', ',', ';', ':', '!', '?'])
        .trim_end();
    let needle: Vec<char> = q.chars().collect();
    if needle.len() < 3 {
        return None;
    }
    let hay = chars(source);
    let at = hay
        .windows(needle.len())
        .position(|w| w.iter().zip(&needle).all(|((c, _, _), n)| c == n))?;
    let (start, mut end) = (hay[at].1, hay[at + needle.len() - 1].2);
    if let Some(p) = punct.filter(|p| source[end..].starts_with(*p)) {
        end += p.len_utf8();
    }
    // Keep a closing quotation mark that belongs to the copied words.
    let tail = source[end..]
        .chars()
        .next()
        .filter(|c| matches!(c, '"' | '\u{201d}'));
    let opens = source[start..end].matches(['"', '\u{201c}']).count();
    let closes = source[start..end].matches(['"', '\u{201d}']).count();
    let end = match tail {
        Some(c)
            if opens > closes
                || (opens == closes && source[start..end].matches('"').count() % 2 == 1) =>
        {
            end + c.len_utf8()
        }
        _ => end,
    };
    Some(source[start..end].to_string())
}

pub fn english_only(s: &str) -> bool {
    !contains_cjk(s)
}

/// Removes the LaTeX and Markdown leftovers the model sometimes writes (`$\rightarrow$`, which
/// JSON-decodes to a carriage return plus `ightarrow$`; `**`).
pub fn clean_markup(s: &str) -> String {
    let mut out = s.to_string();
    for arrow in [
        "$\\rightarrow$",
        "$\rightarrow$",
        "\\rightarrow",
        "\rightarrow",
        "$\\to$",
    ] {
        out = out.replace(arrow, "→");
    }
    out = out.replace("**", "").replace(['\r', '\u{8}', '\u{c}'], "");
    out.trim().to_string()
}

fn cjk_bigrams(s: &str) -> HashSet<(char, char)> {
    let chars: Vec<char> = s.chars().filter(|c| crate::retrieval::is_cjk(*c)).collect();
    chars.windows(2).map(|w| (w[0], w[1])).collect()
}

/// The largest share of `objective`'s CJK bigrams found in a single curriculum text.
pub fn copied_share<'a>(objective: &str, texts: impl Iterator<Item = &'a str>) -> f64 {
    let own = cjk_bigrams(objective);
    if own.is_empty() {
        return 0.0;
    }
    texts
        .map(|t| {
            let other = cjk_bigrams(t);
            own.intersection(&other).count() as f64 / own.len() as f64
        })
        .fold(0.0, f64::max)
}

fn clip(s: &str) -> String {
    crate::text::clip_chars(s, 120)
}

// ---------------------------------------------------------------- (a) passage analysis

#[derive(Debug, Clone, PartialEq)]
pub struct EvalParagraph {
    /// Only the features whose evidence is verbatim.
    pub suggestion: ParagraphSuggestion,
    /// Evidence strings that were not a continuous piece of the paragraph.
    pub bad_evidence: Vec<String>,
    /// function and summary are in Chinese.
    pub zh_ok: bool,
}

impl EvalParagraph {
    /// Worth asking the model to redo: spliced evidence, wrong language, or no feature at all.
    pub fn needs_redo(&self) -> bool {
        !self.bad_evidence.is_empty() || !self.zh_ok || self.suggestion.features.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct AnalysisEval {
    /// By paragraph number: the first entry the model gave for each paragraph it was asked for.
    pub paragraphs: BTreeMap<i32, EvalParagraph>,
    /// Asked for, not returned (the model sometimes stops after two paragraphs).
    pub missing: Vec<i32>,
    pub focus: Vec<FocusSuggestion>,
    pub focus_dropped: usize,
    /// (text_type, main_idea, structure) when both sentences are in Chinese.
    pub overall: Option<(String, String, String)>,
    pub results: Vec<CheckResult>,
}

impl AnalysisEval {
    pub fn redo(&self) -> Vec<i32> {
        self.paragraphs
            .iter()
            .filter(|(_, p)| p.needs_redo())
            .map(|(no, _)| *no)
            .collect()
    }

    pub fn needs_repair(&self) -> bool {
        !self.missing.is_empty() || !self.redo().is_empty() || self.overall.is_none()
    }
}

/// Checks one analysis output against the paragraphs split in code. `expected` are the
/// paragraph numbers this call was asked for (all of them on the first call).
pub fn evaluate_analysis(
    raw: &RawAnalysis,
    paragraphs: &[Paragraph],
    expected: &[i32],
) -> AnalysisEval {
    let by_no: HashMap<i32, &Paragraph> = paragraphs.iter().map(|p| (p.no, p)).collect();
    let mut eval = AnalysisEval::default();
    let mut out_of_range = Vec::new();
    for rp in &raw.paragraphs {
        let no = i32::try_from(rp.paragraph_no).unwrap_or(-1);
        let Some(p) = by_no.get(&no) else {
            out_of_range.push(rp.paragraph_no.to_string());
            continue;
        };
        if !expected.contains(&no) || eval.paragraphs.contains_key(&no) {
            continue;
        }
        let mut features = Vec::new();
        let mut bad_evidence = Vec::new();
        for f in &rp.writing_features {
            if is_verbatim(&f.evidence, &p.text) {
                features.push(FeatureSuggestion {
                    feature: clean_markup(&f.feature_zh),
                    evidence: f.evidence.trim().to_string(),
                    explanation: clean_markup(&f.explanation_zh),
                });
            } else {
                bad_evidence.push(f.evidence.clone());
            }
        }
        let zh_ok = contains_cjk(&rp.function_zh) && contains_cjk(&rp.summary_zh);
        eval.paragraphs.insert(
            no,
            EvalParagraph {
                suggestion: ParagraphSuggestion {
                    paragraph_id: p.id.clone(),
                    paragraph_no: no,
                    function: clean_markup(&rp.function_zh),
                    summary: clean_markup(&rp.summary_zh),
                    features,
                },
                bad_evidence,
                zh_ok,
            },
        );
    }
    eval.missing = expected
        .iter()
        .copied()
        .filter(|no| !eval.paragraphs.contains_key(no))
        .collect();

    let mut seen_items = HashSet::new();
    let mut bad_focus = Vec::new();
    for f in &raw.language_focus {
        let no = i32::try_from(f.paragraph_no).unwrap_or(-1);
        match by_no.get(&no) {
            Some(p) if is_verbatim(&f.evidence, &p.text) && !f.item.trim().is_empty() => {
                if seen_items.insert(f.item.trim().to_lowercase()) {
                    eval.focus.push(FocusSuggestion {
                        kind: f.kind.trim().to_string(),
                        item: f.item.trim().to_string(),
                        paragraph_id: p.id.clone(),
                        evidence: f.evidence.trim().to_string(),
                        note: clean_markup(&f.note_zh),
                    });
                }
            }
            _ => bad_focus.push(format!("P{}: {}", f.paragraph_no, clip(&f.evidence))),
        }
    }
    eval.focus_dropped = bad_focus.len();
    if contains_cjk(&raw.main_idea_zh) && contains_cjk(&raw.structure_zh) {
        eval.overall = Some((
            raw.text_type.trim().to_string(),
            clean_markup(&raw.main_idea_zh),
            clean_markup(&raw.structure_zh),
        ));
    }

    let bad_evidence: Vec<String> = eval
        .paragraphs
        .iter()
        .flat_map(|(no, p)| {
            p.bad_evidence
                .iter()
                .map(move |e| format!("P{no}: {}", clip(e)))
        })
        .collect();
    let not_zh: Vec<String> = eval
        .paragraphs
        .iter()
        .filter(|(_, p)| !p.zh_ok)
        .map(|(no, _)| format!("P{no}"))
        .collect();
    let no_feature: Vec<String> = eval
        .paragraphs
        .iter()
        .filter(|(_, p)| p.suggestion.features.is_empty() && p.bad_evidence.is_empty())
        .map(|(no, _)| format!("P{no}"))
        .collect();
    let check = |name: &'static str, bad: &[String]| {
        if bad.is_empty() {
            CheckResult::pass(name)
        } else {
            CheckResult::fail(name, bad.join("；"))
        }
    };
    let mut one_to_one = Vec::new();
    if !eval.missing.is_empty() {
        one_to_one.push(format!(
            "missing {}",
            eval.missing
                .iter()
                .map(|n| format!("P{n}"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !out_of_range.is_empty() {
        one_to_one.push(format!("unknown paragraph_no {}", out_of_range.join(", ")));
    }
    eval.results = vec![
        check("paragraphs_one_to_one", &one_to_one),
        check("each_paragraph_has_feature", &no_feature),
        check("evidence_verbatim", &bad_evidence),
        check("focus_evidence_verbatim", &bad_focus),
        check(
            "zh_fields_in_chinese",
            &[
                not_zh,
                if eval.overall.is_none() {
                    vec!["main_idea_zh / structure_zh".to_string()]
                } else {
                    Vec::new()
                },
            ]
            .concat(),
        ),
    ];
    eval
}

/// Puts the (at most two) checked outputs together. For each paragraph the repair call's entry
/// wins when it has one in Chinese; otherwise the first call's entry is kept with its bad
/// evidence dropped. Paragraphs neither call delivered are reported as missing.
pub fn merge_analysis(
    first: Option<&AnalysisEval>,
    second: Option<&AnalysisEval>,
    paragraphs: &[Paragraph],
) -> AnalysisSuggestion {
    let mut out = AnalysisSuggestion::default();
    for p in paragraphs {
        let pick = [second, first]
            .into_iter()
            .flatten()
            .filter_map(|e| e.paragraphs.get(&p.no))
            .find(|e| e.zh_ok);
        match pick {
            Some(e) => {
                out.dropped_evidence += e.bad_evidence.len();
                out.paragraphs.push(e.suggestion.clone());
            }
            None => out.missing_paragraph_ids.push(p.id.clone()),
        }
    }
    let mut seen = HashSet::new();
    for e in [first, second].into_iter().flatten() {
        out.dropped_evidence += e.focus_dropped;
        for f in &e.focus {
            if out.language_focus.len() < MAX_FOCUS && seen.insert(f.item.to_lowercase()) {
                out.language_focus.push(f.clone());
            }
        }
    }
    if let Some((text_type, main_idea, structure)) = [first, second]
        .into_iter()
        .flatten()
        .find_map(|e| e.overall.clone())
    {
        out.text_type = text_type;
        out.main_idea = main_idea;
        out.structure = structure;
    }
    out
}

// ---------------------------------------------------------------- (b) objectives

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ObjectivesEval {
    /// Objectives that may be shown, with only the links whose code was offered.
    pub kept: Vec<ObjectiveSuggestion>,
    /// Codes the model cited that were not in the list it was given.
    pub invalid_codes: Vec<String>,
    /// Objectives left out, with the reason.
    pub dropped: Vec<String>,
    pub results: Vec<CheckResult>,
}

impl ObjectivesEval {
    pub fn has_both_types(&self) -> bool {
        self.kept.iter().any(|o| o.kind == ObjectiveType::Content)
            && self.kept.iter().any(|o| o.kind == ObjectiveType::Language)
    }

    pub fn needs_repair(&self) -> bool {
        !self.invalid_codes.is_empty() || !self.dropped.is_empty() || !self.has_both_types()
    }

    pub fn into_suggestion(self, curriculum_available: usize) -> ObjectivesSuggestion {
        ObjectivesSuggestion {
            objectives: self.kept,
            curriculum_available,
        }
    }
}

/// Checks one objectives output. `allowed` are exactly the items shown to the model; link text
/// and source are taken from them, never from the output.
pub fn evaluate_objectives(
    raw: &RawObjectives,
    allowed: &HashMap<String, CurriculumItem>,
    paragraphs: &[Paragraph],
) -> ObjectivesEval {
    let by_no: HashMap<i64, &Paragraph> = paragraphs.iter().map(|p| (i64::from(p.no), p)).collect();
    let mut eval = ObjectivesEval::default();
    let mut status_mismatch = Vec::new();
    let mut not_zh = Vec::new();
    let mut not_i_can = Vec::new();
    let mut copied = Vec::new();
    let mut bad_type = Vec::new();
    for o in &raw.objectives {
        let id = o.objective_id.trim();
        let mut links: Vec<CurriculumLink> = Vec::new();
        for l in &o.curriculum_links {
            let code = l.item_code.trim();
            match allowed.get(code) {
                Some(item) => {
                    if links.len() < MAX_LINKS && !links.iter().any(|x| x.item_code == code) {
                        links.push(CurriculumLink::from_item(item, &clean_markup(&l.why_zh)));
                    }
                }
                None => eval.invalid_codes.push(code.to_string()),
            }
        }
        if (o.link_status == "linked") != !o.curriculum_links.is_empty() {
            status_mismatch.push(id.to_string());
        }
        let kind = match o.siop_type.trim() {
            SIOP_CONTENT => Some(ObjectiveType::Content),
            SIOP_LANGUAGE => Some(ObjectiveType::Language),
            _ => None,
        };
        let text = clean_markup(&o.objective_zh);
        let en = clean_markup(&o.student_facing_en);
        let share = copied_share(&text, allowed.values().map(|i| i.content.as_str()));
        let problem = if kind.is_none() {
            bad_type.push(format!("{id}: {}", o.siop_type));
            Some("siop_type")
        } else if !contains_cjk(&text) || !contains_cjk(&o.reason_zh) {
            not_zh.push(id.to_string());
            Some("not in Chinese")
        } else if !english_only(&en) || !en.to_lowercase().starts_with("i can") {
            not_i_can.push(format!("{id}: {}", clip(&en)));
            Some("student_facing_en")
        } else if share >= COPIED_OVERLAP {
            copied.push(format!(
                "{id} overlaps a curriculum item {:.0}%",
                share * 100.0
            ));
            Some("copied from the curriculum")
        } else {
            None
        };
        match (problem, kind) {
            (None, Some(kind)) => {
                let mut seen = HashSet::new();
                let paragraph_ids = o
                    .paragraph_refs
                    .iter()
                    .filter_map(|r| by_no.get(r))
                    .filter(|p| seen.insert(p.id.clone()))
                    .map(|p| p.id.clone())
                    .collect();
                eval.kept.push(ObjectiveSuggestion {
                    kind,
                    text,
                    student_facing_en: en,
                    reason: clean_markup(&o.reason_zh),
                    paragraph_ids,
                    link_status: if links.is_empty() {
                        LinkStatus::Unlinked
                    } else {
                        LinkStatus::Linked
                    },
                    links,
                });
            }
            (problem, _) => eval
                .dropped
                .push(format!("{id}: {}", problem.unwrap_or("siop_type"))),
        }
    }
    let check = |name: &'static str, bad: &[String]| {
        if bad.is_empty() {
            CheckResult::pass(name)
        } else {
            CheckResult::fail(name, bad.join("；"))
        }
    };
    eval.results = vec![
        if eval.has_both_types() {
            CheckResult::pass("both_siop_types_present")
        } else {
            CheckResult::fail(
                "both_siop_types_present",
                format!("{} usable objectives", eval.kept.len()),
            )
        },
        check("siop_type_valid", &bad_type),
        check("cited_codes_exist", &eval.invalid_codes),
        check("link_status_consistent", &status_mismatch),
        check("objective_not_copied_from_curriculum", &copied),
        check("objective_zh_in_chinese", &not_zh),
        check("student_facing_english_i_can", &not_i_can),
    ];
    eval
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sop::schemas::{RawFeature, RawFocus, RawLink, RawObjective, RawParagraph};

    const P1: &str =
        "Mrs. Lin put an umbrella stand outside her shop. A sign said, “Take one if you need it.”";
    const P2: &str = "In the first month, twelve of the thirty umbrellas did disappear. However, something unexpected followed.";

    fn paragraphs() -> Vec<Paragraph> {
        vec![
            Paragraph {
                id: "p1".into(),
                no: 1,
                text: P1.into(),
                page: None,
            },
            Paragraph {
                id: "p7".into(),
                no: 2,
                text: P2.into(),
                page: None,
            },
        ]
    }

    #[test]
    fn verbatim_forgives_quotes_and_punctuation_but_not_splices() {
        assert!(is_verbatim("Take one if you need it.", P1));
        assert!(is_verbatim(
            "\"A sign said, \"Take one if you need it.\"",
            P1
        ));
        assert!(is_verbatim("  umbrella   stand outside ", P1));
        assert!(!is_verbatim(
            "Mrs. Lin put an umbrella stand ... Take one if you need it",
            P1
        ));
        assert!(!is_verbatim(
            "Mrs. Lin put an umbrella stand… A sign said",
            P1
        ));
        assert!(!is_verbatim("mrs. lin put an umbrella stand", P1));
        assert!(!is_verbatim("it", P1));
        assert!(!is_verbatim("However, something unexpected followed", P1));
    }

    #[test]
    fn quotation_marks_inside_a_copy_are_forgiven_and_the_source_is_returned() {
        let src = "The noodle seller painted the words \"Trust Stand\" on the side. \u{201c}Science is about noticing a problem,\u{201d} she says.";
        assert_eq!(
            locate_verbatim("painted the words Trust Stand on the side", src).as_deref(),
            Some("painted the words \"Trust Stand\" on the side")
        );
        assert_eq!(
            locate_verbatim("Science is about noticing a problem, she says", src).as_deref(),
            Some("Science is about noticing a problem,\u{201d} she says")
        );
        assert!(locate_verbatim("painted the words Trust Shop on the side", src).is_none());
    }

    #[test]
    fn markup_leftovers_are_removed() {
        assert_eq!(clean_markup("原因 $\\rightarrow$ 結果"), "原因 → 結果");
        assert_eq!(clean_markup("原因 $\rightarrow$ 結果"), "原因 → 結果");
        assert_eq!(clean_markup(" **重點** "), "重點");
    }

    fn feature(evidence: &str) -> RawFeature {
        RawFeature {
            feature_zh: "轉折語".into(),
            evidence: evidence.into(),
            explanation_zh: "用轉折帶出意外".into(),
        }
    }

    fn para(no: i64, evidence: &[&str]) -> RawParagraph {
        RawParagraph {
            paragraph_no: no,
            function_zh: "開場".into(),
            summary_zh: "介紹傘架".into(),
            writing_features: evidence.iter().map(|e| feature(e)).collect(),
        }
    }

    fn analysis(paragraphs: Vec<RawParagraph>, focus: Vec<RawFocus>) -> RawAnalysis {
        RawAnalysis {
            text_type: "記敘文".into(),
            main_idea_zh: "信任".into(),
            structure_zh: "開場、轉折".into(),
            paragraphs,
            language_focus: focus,
        }
    }

    #[test]
    fn analysis_reports_missing_paragraphs_and_spliced_evidence() {
        let raw = analysis(
            vec![
                para(
                    1,
                    &[
                        "Take one if you need it",
                        "an umbrella stand ... A sign said",
                    ],
                ),
                para(9, &["x"]),
            ],
            vec![
                RawFocus {
                    kind: "篇章標記".into(),
                    item: "However".into(),
                    paragraph_no: 2,
                    evidence: "However, something unexpected followed.".into(),
                    note_zh: "轉折".into(),
                },
                RawFocus {
                    kind: "字彙".into(),
                    item: "trust".into(),
                    paragraph_no: 2,
                    evidence: "trust is like an umbrella".into(),
                    note_zh: "不在這一段".into(),
                },
            ],
        );
        let e = evaluate_analysis(&raw, &paragraphs(), &[1, 2]);
        assert_eq!(e.missing, vec![2]);
        assert_eq!(e.redo(), vec![1]);
        assert!(e.needs_repair());
        assert_eq!(e.paragraphs[&1].suggestion.features.len(), 1);
        assert_eq!(e.paragraphs[&1].bad_evidence.len(), 1);
        assert_eq!(e.focus.len(), 1);
        assert_eq!(e.focus[0].paragraph_id, "p7");
        assert_eq!(e.focus_dropped, 1);
        let failed: Vec<&str> = e
            .results
            .iter()
            .filter(|r| !r.passed)
            .map(|r| r.name)
            .collect();
        assert_eq!(
            failed,
            [
                "paragraphs_one_to_one",
                "evidence_verbatim",
                "focus_evidence_verbatim"
            ]
        );

        // The repair call delivers paragraph 2 and a clean paragraph 1.
        let repair = analysis(
            vec![
                para(1, &["A sign said"]),
                para(2, &["However, something unexpected followed"]),
            ],
            vec![],
        );
        let e2 = evaluate_analysis(&repair, &paragraphs(), &[1, 2]);
        assert!(!e2.needs_repair());
        let merged = merge_analysis(Some(&e), Some(&e2), &paragraphs());
        assert_eq!(merged.paragraphs.len(), 2);
        assert_eq!(merged.paragraphs[0].features[0].evidence, "A sign said");
        assert_eq!(merged.paragraphs[1].paragraph_id, "p7");
        assert!(merged.missing_paragraph_ids.is_empty());
        assert_eq!(merged.dropped_evidence, 1); // the language-focus item
        assert_eq!(merged.main_idea, "信任");

        // Without a second call the first one is salvaged: bad evidence dropped, P2 missing.
        let salvaged = merge_analysis(Some(&e), None, &paragraphs());
        assert_eq!(salvaged.paragraphs.len(), 1);
        assert_eq!(salvaged.paragraphs[0].features.len(), 1);
        assert_eq!(salvaged.missing_paragraph_ids, vec!["p7"]);
        assert_eq!(salvaged.dropped_evidence, 2);
    }

    #[test]
    fn a_clean_analysis_needs_no_repair() {
        let raw = analysis(
            vec![
                para(1, &["Take one if you need it"]),
                para(2, &["However, something unexpected followed"]),
                para(2, &["duplicate entry is ignored"]),
            ],
            vec![],
        );
        let e = evaluate_analysis(&raw, &paragraphs(), &[1, 2]);
        assert!(!e.needs_repair(), "{:?}", e.results);
        assert!(crate::checks::all_passed(&e.results));
        let mut english = raw.clone();
        english.paragraphs[0].function_zh = "Opening".into();
        let e = evaluate_analysis(&english, &paragraphs(), &[1, 2]);
        assert_eq!(e.redo(), vec![1]);
        assert_eq!(
            merge_analysis(Some(&e), None, &paragraphs()).missing_paragraph_ids,
            vec!["p1"]
        );
    }

    fn item(code: &str, content: &str) -> CurriculumItem {
        CurriculumItem {
            item_code: code.into(),
            version: 1,
            category: "curriculum".into(),
            school_track: "general".into(),
            title: format!("標題 {code}"),
            content: content.into(),
            source_doc: "課綱".into(),
            source_version: "107".into(),
            source_page: "8".into(),
            official_code: "3-Ⅴ-7".into(),
            item_kind: "learning_performance".into(),
            strand_name: String::new(),
            stage: "V".into(),
            subject: "英語文".into(),
            advanced: false,
            recurring: false,
        }
    }

    fn objective(id: &str, kind: &str, zh: &str, links: &[&str]) -> RawObjective {
        RawObjective {
            objective_id: id.into(),
            siop_type: kind.into(),
            objective_zh: zh.into(),
            student_facing_en: "I can explain why the umbrellas came back.".into(),
            reason_zh: "第 2 段有轉折".into(),
            paragraph_refs: vec![2, 5, 2],
            link_status: if links.is_empty() {
                "unlinked"
            } else {
                "linked"
            }
            .into(),
            curriculum_links: links
                .iter()
                .map(|c| RawLink {
                    item_code: (*c).into(),
                    why_zh: "模型寫的理由".into(),
                })
                .collect(),
        }
    }

    #[test]
    fn objectives_keep_only_offered_codes_and_take_text_from_the_database() {
        let allowed = HashMap::from([(
            "CUR-1".to_string(),
            item("CUR-1", "能閱讀短文並找出主要論點與支持理由"),
        )]);
        let raw = RawObjectives {
            objectives: vec![
                objective(
                    "O1",
                    SIOP_CONTENT,
                    "學生能說明傘為什麼回來",
                    &["CUR-1", "9-V-2"],
                ),
                objective("O2", SIOP_LANGUAGE, "學生能用 however 說出轉折", &[]),
                objective(
                    "O3",
                    SIOP_CONTENT,
                    "能閱讀短文並找出主要論點與支持理由",
                    &["CUR-1"],
                ),
            ],
        };
        let e = evaluate_objectives(&raw, &allowed, &paragraphs());
        assert_eq!(e.invalid_codes, vec!["9-V-2"]);
        assert_eq!(e.dropped.len(), 1, "{:?}", e.dropped);
        assert!(e.needs_repair());
        assert!(e.has_both_types());
        assert_eq!(e.kept.len(), 2);
        assert_eq!(e.kept[0].links.len(), 1);
        assert_eq!(
            e.kept[0].links[0].text,
            "能閱讀短文並找出主要論點與支持理由"
        );
        assert_eq!(e.kept[0].links[0].source_version, "107");
        assert_eq!(e.kept[0].link_status, LinkStatus::Linked);
        assert_eq!(e.kept[0].paragraph_ids, vec!["p7"]);
        assert_eq!(e.kept[1].link_status, LinkStatus::Unlinked);
        let failed: Vec<&str> = e
            .results
            .iter()
            .filter(|r| !r.passed)
            .map(|r| r.name)
            .collect();
        assert_eq!(
            failed,
            ["cited_codes_exist", "objective_not_copied_from_curriculum"]
        );
    }

    #[test]
    fn objectives_need_both_types_chinese_text_and_an_i_can_line() {
        let allowed = HashMap::new();
        let mut english = objective("O2", SIOP_LANGUAGE, "Students can use however", &[]);
        english.student_facing_en = "學生可以".into();
        let raw = RawObjectives {
            objectives: vec![
                objective("O1", SIOP_CONTENT, "學生能說明原因", &[]),
                english,
            ],
        };
        let e = evaluate_objectives(&raw, &allowed, &paragraphs());
        assert!(!e.has_both_types());
        assert!(e.needs_repair());
        assert_eq!(e.kept.len(), 1);
        // A link the model marked "linked" but could not back up ends up unlinked.
        let raw = RawObjectives {
            objectives: vec![
                objective("O1", SIOP_CONTENT, "學生能說明原因", &["MADE-UP"]),
                objective("O2", SIOP_LANGUAGE, "學生能用 however 說出轉折", &[]),
            ],
        };
        let e = evaluate_objectives(&raw, &allowed, &paragraphs());
        assert_eq!(e.kept[0].link_status, LinkStatus::Unlinked);
        assert!(e.kept[0].links.is_empty());
        assert_eq!(e.clone().into_suggestion(0).objectives.len(), 2);
    }
}
