//! 我的學習單版型 (worksheet templates): a teacher's own past worksheet, read as a reusable
//! design. Contract: docs/API.md「Phase 2.5：我的學習單版型」; validated approach:
//! docs/WORKSHEET_TEMPLATE_PROBE.md (v4 extraction in two calls, gen2 one call per block).
//!
//! This module holds the template document ([`WsTemplate`]), its validation (a teacher's save
//! is checked against the spec's fixed label sets, like `WorksheetSettings`), the clean-up of
//! what the model extracted, the JSON schemas of the three model calls, the settings a template
//! implies and the expansion of a template into blocks for a given material. The extraction job
//! lives in [`extract`], storage in [`store`]; generation is part of the worksheet pipeline
//! (`worksheet::ai`).

pub mod extract;
pub mod store;

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{
    student_tasks, BlockKind, ScaffoldType, Settings, ANSWER_FORMATS, GENERATION_OPTIONS,
    GEN_OBJECTIVES, INSTRUCTIONAL_KINDS, MAX_ID_CHARS, PROCEDURAL_KINDS, VERBAL_KINDS,
};
use crate::routes::clip_chars;

// ---------------------------------------------------------------- limits and labels

pub const INSTRUCTION_LANGUAGES: [&str; 3] = ["英文", "中文", "中英並用"];
pub const MAX_SECTIONS: usize = 12;
pub const MAX_ITEM_COUNT: i64 = 12;
/// Most blocks a worksheet made from a template may have after per-paragraph expansion.
pub const MAX_BLOCKS: usize = 16;
pub const MAX_TITLE_CHARS: usize = 200;
pub const MAX_KIND_CHARS: usize = 200;
pub const MAX_PURPOSE_CHARS: usize = 300;
pub const MAX_TEXT_SCOPE_CHARS: usize = 100;
pub const MAX_INSTRUCTION_STYLE_CHARS: usize = 200;
pub const MAX_ITEM_PATTERNS: usize = 6;
pub const MAX_ITEM_PATTERN_CHARS: usize = 300;
pub const MAX_STYLE_NOTES: usize = 8;
pub const MAX_STYLE_NOTE_CHARS: usize = 200;
pub const MAX_HEADER_FIELDS: usize = 20;
pub const MAX_HEADER_FIELD_CHARS: usize = 100;

/// What a block that needs the teacher's own material says (contract, verbatim).
pub const MSG_EXTERNAL: &str = "這一區需要你提供素材（例如影片或另一篇文章），Manna 不會自己編造。";

pub const OUTLINE_SCHEMA_NAME: &str = "worksheet_template_outline";
pub const DETAILS_SCHEMA_NAME: &str = "worksheet_template_details";
pub const SECTION_SCHEMA_NAME: &str = "worksheet_template_section";

/// Every scaffold kind of the spec (§3): verbal, instructional and procedural.
pub fn scaffold_kinds() -> Vec<&'static str> {
    VERBAL_KINDS
        .iter()
        .chain(INSTRUCTIONAL_KINDS.iter())
        .chain(PROCEDURAL_KINDS.iter())
        .copied()
        .collect()
}

/// Which of the three scaffold types a kind belongs to.
pub fn scaffold_type_of(kind: &str) -> Option<ScaffoldType> {
    [
        ScaffoldType::Verbal,
        ScaffoldType::Instructional,
        ScaffoldType::Procedural,
    ]
    .into_iter()
    .find(|t| t.kinds().contains(&kind))
}

// ---------------------------------------------------------------- the document

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct WsTemplate {
    pub worksheet_kind: String,
    pub instruction_language: String,
    pub header_fields: Vec<String>,
    pub shows_objectives: bool,
    pub includes_passage_text: bool,
    pub sections: Vec<WsTemplateSection>,
    pub generation_options: Vec<String>,
    pub style_notes: Vec<String>,
}

impl Default for WsTemplate {
    fn default() -> Self {
        Self {
            worksheet_kind: String::new(),
            instruction_language: INSTRUCTION_LANGUAGES[0].to_string(),
            header_fields: Vec::new(),
            shows_objectives: false,
            includes_passage_text: false,
            sections: Vec::new(),
            generation_options: Vec::new(),
            style_notes: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct WsTemplateSection {
    /// "s1", "s2"…: stable across edits.
    pub id: String,
    pub title_pattern: String,
    pub purpose: String,
    pub block_kind: BlockKind,
    /// Repeated once per paragraph of the new material.
    pub per_paragraph: bool,
    pub student_tasks: Vec<String>,
    pub answer_formats: Vec<String>,
    pub item_count: i64,
    pub text_scope: String,
    pub scaffolds: Vec<String>,
    pub instruction_style: String,
    pub item_patterns: Vec<String>,
    /// Needs a video, recording, image or other text the teacher must supply.
    pub needs_external_material: bool,
}

/// Keeps the labels in the order given, without duplicates; one outside `allowed` is an error
/// naming it.
fn check_labels(what: &str, list: &[String], allowed: &[&str]) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    for label in list {
        let label = label.trim();
        if !allowed.contains(&label) {
            return Err(format!(
                "「{}」不是「{what}」的選項。",
                clip_chars(label, 40)
            ));
        }
        if !out.iter().any(|l| l == label) {
            out.push(label.to_string());
        }
    }
    Ok(out)
}

/// The labels of `list` that are in `allowed`, in the order given, without duplicates.
fn keep_labels(list: &[String], allowed: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for label in list {
        let label = label.trim();
        if allowed.contains(&label) && !out.iter().any(|l| l == label) {
            out.push(label.to_string());
        }
    }
    out
}

fn check_len(what: &str, s: &str, max: usize) -> Result<(), String> {
    if s.chars().count() > max {
        Err(format!("{what}最多 {max} 字。"))
    } else {
        Ok(())
    }
}

fn trimmed_lines(list: &[String]) -> Vec<String> {
    list.iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Lowest unused `s<n>` id.
fn next_id(taken: &HashSet<String>) -> String {
    (1..)
        .map(|n| format!("s{n}"))
        .find(|id| !taken.contains(id))
        .expect("an unused id")
}

impl WsTemplate {
    /// Checks a template the teacher saved. Labels must come from the spec's fixed sets (400
    /// otherwise, with a Chinese message); text is trimmed, empty lines are dropped and a
    /// section without an id gets a new one. Section ids must be unique.
    pub fn validate(&mut self) -> Result<(), String> {
        self.worksheet_kind = self.worksheet_kind.trim().to_string();
        check_len("學習單類型", &self.worksheet_kind, MAX_KIND_CHARS)?;
        if !INSTRUCTION_LANGUAGES.contains(&self.instruction_language.as_str()) {
            return Err(format!(
                "「{}」不是「指示語言」的選項（英文、中文、中英並用）。",
                clip_chars(&self.instruction_language, 40)
            ));
        }
        self.header_fields = trimmed_lines(&self.header_fields);
        if self.header_fields.len() > MAX_HEADER_FIELDS {
            return Err(format!("表頭欄位最多 {MAX_HEADER_FIELDS} 個。"));
        }
        for f in &self.header_fields {
            check_len("表頭欄位", f, MAX_HEADER_FIELD_CHARS)?;
        }
        if self.sections.is_empty() || self.sections.len() > MAX_SECTIONS {
            return Err(format!("版型需要 1 到 {MAX_SECTIONS} 個區塊。"));
        }
        let mut taken: HashSet<String> = HashSet::new();
        for s in &self.sections {
            let id = s.id.trim();
            if id.is_empty() {
                continue;
            }
            if id.chars().count() > MAX_ID_CHARS {
                return Err(format!("區塊的 id 最多 {MAX_ID_CHARS} 個字元。"));
            }
            if !taken.insert(id.to_string()) {
                return Err(format!("區塊的 id「{}」重複。", clip_chars(id, 40)));
            }
        }
        let all_tasks = student_tasks();
        let all_kinds = scaffold_kinds();
        for (i, s) in self.sections.iter_mut().enumerate() {
            let at = |m: String| format!("第 {} 個區塊：{m}", i + 1);
            s.id = s.id.trim().to_string();
            if s.id.is_empty() {
                s.id = next_id(&taken);
                taken.insert(s.id.clone());
            }
            s.title_pattern = s.title_pattern.trim().to_string();
            if s.title_pattern.is_empty() {
                return Err(at("標題不能是空的。".to_string()));
            }
            check_len("標題", &s.title_pattern, MAX_TITLE_CHARS).map_err(at)?;
            s.purpose = s.purpose.trim().to_string();
            check_len("用途說明", &s.purpose, MAX_PURPOSE_CHARS).map_err(at)?;
            s.student_tasks = check_labels("學生任務", &s.student_tasks, &all_tasks).map_err(at)?;
            s.answer_formats =
                check_labels("作答方式", &s.answer_formats, &ANSWER_FORMATS).map_err(at)?;
            if !(1..=MAX_ITEM_COUNT).contains(&s.item_count) {
                return Err(at(format!("題數需為 1 到 {MAX_ITEM_COUNT}。")));
            }
            s.text_scope = s.text_scope.trim().to_string();
            check_len("課文範圍", &s.text_scope, MAX_TEXT_SCOPE_CHARS).map_err(at)?;
            s.scaffolds = check_labels("鷹架", &s.scaffolds, &all_kinds).map_err(at)?;
            s.instruction_style = s.instruction_style.trim().to_string();
            check_len(
                "指示語寫法",
                &s.instruction_style,
                MAX_INSTRUCTION_STYLE_CHARS,
            )
            .map_err(at)?;
            s.item_patterns = trimmed_lines(&s.item_patterns);
            if s.item_patterns.len() > MAX_ITEM_PATTERNS {
                return Err(at(format!("題型樣式最多 {MAX_ITEM_PATTERNS} 條。")));
            }
            for p in &s.item_patterns {
                check_len("題型樣式", p, MAX_ITEM_PATTERN_CHARS).map_err(at)?;
            }
        }
        self.generation_options =
            check_labels("產生設定", &self.generation_options, &GENERATION_OPTIONS)?;
        self.style_notes = trimmed_lines(&self.style_notes);
        if self.style_notes.len() > MAX_STYLE_NOTES {
            return Err(format!("設計習慣最多 {MAX_STYLE_NOTES} 條。"));
        }
        for n in &self.style_notes {
            check_len("設計習慣", n, MAX_STYLE_NOTE_CHARS)?;
        }
        Ok(())
    }

    pub fn section(&self, id: &str) -> Option<&WsTemplateSection> {
        self.sections.iter().find(|s| s.id == id)
    }
}

// ---------------------------------------------------------------- what the model returns

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawOutlineItem {
    pub title_pattern: String,
    pub per_paragraph: bool,
}

/// Call A: the outline and the header facts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawOutline {
    pub worksheet_kind: String,
    pub instruction_language: String,
    pub header_fields: Vec<String>,
    pub shows_objectives: bool,
    pub includes_passage_text: bool,
    pub outline: Vec<RawOutlineItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawSection {
    pub title_pattern: String,
    pub purpose: String,
    pub block_kind: String,
    pub student_tasks: Vec<String>,
    pub answer_formats: Vec<String>,
    pub item_count: i64,
    pub text_scope: String,
    pub scaffolds: Vec<String>,
    pub instruction_style: String,
    pub item_patterns: Vec<String>,
    pub needs_external_material: bool,
}

/// Call B: one section per outline item.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawDetails {
    pub sections: Vec<RawSection>,
    pub generation_options: Vec<String>,
    pub style_notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawOutItem {
    pub answer_format: String,
    pub prompt: String,
    pub options: Vec<String>,
    pub answer: String,
}

/// One generated block.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawSectionOut {
    pub title: String,
    pub instructions: String,
    pub scaffold: String,
    pub items: Vec<RawOutItem>,
}

// ---------------------------------------------------------------- schemas (properties in written order)

fn s() -> Value {
    json!({ "type": "string" })
}

fn b() -> Value {
    json!({ "type": "boolean" })
}

fn strings() -> Value {
    json!({ "type": "array", "items": { "type": "string" } })
}

fn enum_of(labels: &[&str]) -> Value {
    json!({ "type": "string", "enum": labels })
}

fn enum_array(labels: &[&str]) -> Value {
    json!({ "type": "array", "items": enum_of(labels) })
}

/// An object whose properties are required, in the order written (the gateway emits fields in
/// schema order; alphabetical order made gemma run away in whitespace).
fn obj(props: Vec<(&str, Value)>) -> Value {
    let required: Vec<&str> = props.iter().map(|(k, _)| *k).collect();
    let mut map = serde_json::Map::new();
    for (k, v) in props {
        map.insert(k.to_string(), v);
    }
    json!({
        "type": "object",
        "properties": Value::Object(map),
        "required": required,
        "additionalProperties": false
    })
}

const BLOCK_KINDS: [&str; 3] = ["warmup", "activity", "exit_ticket"];

/// OUTLINE4_SCHEMA of the probe.
pub fn outline_schema() -> Value {
    obj(vec![
        ("worksheet_kind", s()),
        ("instruction_language", enum_of(&INSTRUCTION_LANGUAGES)),
        ("header_fields", strings()),
        ("shows_objectives", b()),
        ("includes_passage_text", b()),
        (
            "outline",
            json!({ "type": "array", "items": obj(vec![
                ("title_pattern", s()),
                ("per_paragraph", b()),
            ]) }),
        ),
    ])
}

fn section_schema() -> Value {
    obj(vec![
        ("title_pattern", s()),
        ("purpose", s()),
        ("block_kind", enum_of(&BLOCK_KINDS)),
        ("student_tasks", enum_array(&student_tasks())),
        ("answer_formats", enum_array(&ANSWER_FORMATS)),
        ("item_count", json!({ "type": "integer" })),
        ("text_scope", s()),
        ("scaffolds", enum_array(&scaffold_kinds())),
        ("instruction_style", s()),
        ("item_patterns", strings()),
        ("needs_external_material", b()),
    ])
}

/// `details_schema(n)` of the probe: exactly `n` sections (one per outline item).
pub fn details_schema(n: usize) -> Value {
    obj(vec![
        (
            "sections",
            json!({ "type": "array", "items": section_schema(), "minItems": n, "maxItems": n }),
        ),
        ("generation_options", enum_array(&GENERATION_OPTIONS)),
        ("style_notes", strings()),
    ])
}

/// SECTION_OUT of the probe (gen2).
pub fn section_out_schema() -> Value {
    obj(vec![
        ("title", s()),
        ("instructions", s()),
        ("scaffold", s()),
        (
            "items",
            json!({
                "type": "array",
                "items": obj(vec![
                    ("answer_format", enum_of(&ANSWER_FORMATS)),
                    ("prompt", s()),
                    ("options", strings()),
                    ("answer", s()),
                ]),
                "maxItems": MAX_ITEM_COUNT
            }),
        ),
    ])
}

// ---------------------------------------------------------------- clean-up of an extraction

static SPECIFIC_PARAGRAPHS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)第\s*[0-9０-９一二三四五六七八九十]+\s*(段|[-–~、,，至到和與及])|\b(paragraphs?|para\.?|p)\s*[0-9]+",
    )
    .expect("valid regex")
});

/// True when a section's text scope names one or a few specific paragraphs (「第 2 段」,
/// "Paragraphs 2–3", "P4"), not every paragraph.
pub fn names_specific_paragraphs(text_scope: &str) -> bool {
    let s = text_scope.to_lowercase();
    let every = [
        "逐段",
        "每段",
        "每一段",
        "各段",
        "each paragraph",
        "every paragraph",
        "per paragraph",
    ];
    if every.iter().any(|k| s.contains(k)) {
        return false;
    }
    SPECIFIC_PARAGRAPHS.is_match(&s)
}

fn clip_trim(s: &str, max: usize) -> String {
    s.trim()
        .chars()
        .take(max)
        .collect::<String>()
        .trim()
        .to_string()
}

fn clip_list(list: &[String], count: usize, max: usize) -> Vec<String> {
    list.iter()
        .map(|s| clip_trim(s, max))
        .filter(|s| !s.is_empty())
        .take(count)
        .collect()
}

/// Turns the two extraction outputs into a template, leniently: section ids are assigned,
/// `item_count` is clamped to 1–12, labels outside the fixed lists are dropped, text is cut to
/// its limit, at most 12 sections are kept, and `per_paragraph` is switched off when the text
/// scope names specific paragraphs. The result passes [`WsTemplate::validate`].
pub fn from_extraction(outline: &RawOutline, details: &RawDetails) -> WsTemplate {
    let all_tasks = student_tasks();
    let all_kinds = scaffold_kinds();
    let sections: Vec<WsTemplateSection> = outline
        .outline
        .iter()
        .zip(details.sections.iter())
        .take(MAX_SECTIONS)
        .enumerate()
        .map(|(i, (o, d))| {
            let mut title = clip_trim(&d.title_pattern, MAX_TITLE_CHARS);
            if title.is_empty() {
                title = clip_trim(&o.title_pattern, MAX_TITLE_CHARS);
            }
            if title.is_empty() {
                title = format!("區塊 {}", i + 1);
            }
            let text_scope = clip_trim(&d.text_scope, MAX_TEXT_SCOPE_CHARS);
            let block_kind = match d.block_kind.trim() {
                "warmup" => BlockKind::Warmup,
                "exit_ticket" => BlockKind::ExitTicket,
                _ => BlockKind::Activity,
            };
            WsTemplateSection {
                id: format!("s{}", i + 1),
                title_pattern: title,
                purpose: clip_trim(&d.purpose, MAX_PURPOSE_CHARS),
                block_kind,
                per_paragraph: o.per_paragraph && !names_specific_paragraphs(&text_scope),
                student_tasks: keep_labels(&d.student_tasks, &all_tasks),
                answer_formats: keep_labels(&d.answer_formats, &ANSWER_FORMATS),
                item_count: d.item_count.clamp(1, MAX_ITEM_COUNT),
                text_scope,
                scaffolds: keep_labels(&d.scaffolds, &all_kinds),
                instruction_style: clip_trim(&d.instruction_style, MAX_INSTRUCTION_STYLE_CHARS),
                item_patterns: clip_list(
                    &d.item_patterns,
                    MAX_ITEM_PATTERNS,
                    MAX_ITEM_PATTERN_CHARS,
                ),
                needs_external_material: d.needs_external_material,
            }
        })
        .collect();
    let language = outline.instruction_language.trim();
    WsTemplate {
        worksheet_kind: clip_trim(&outline.worksheet_kind, MAX_KIND_CHARS),
        instruction_language: if INSTRUCTION_LANGUAGES.contains(&language) {
            language.to_string()
        } else {
            INSTRUCTION_LANGUAGES[0].to_string()
        },
        header_fields: clip_list(
            &outline.header_fields,
            MAX_HEADER_FIELDS,
            MAX_HEADER_FIELD_CHARS,
        ),
        shows_objectives: outline.shows_objectives,
        includes_passage_text: outline.includes_passage_text,
        sections,
        generation_options: keep_labels(&details.generation_options, &GENERATION_OPTIONS),
        style_notes: clip_list(&details.style_notes, MAX_STYLE_NOTES, MAX_STYLE_NOTE_CHARS),
    }
}

// ---------------------------------------------------------------- settings a template implies

/// The worksheet settings a template implies, on top of `base`: the student tasks and answer
/// formats used by its sections, its generation options, and 「顯示學習目標」 when the
/// template shows objectives. Material types, versions and time stay as they were.
pub fn prefill_settings(base: &Settings, t: &WsTemplate) -> Settings {
    let used = |pick: fn(&WsTemplateSection) -> &Vec<String>, label: &str| {
        t.sections
            .iter()
            .any(|s| pick(s).iter().any(|l| l == label))
    };
    let mut generation: Vec<String> = t.generation_options.clone();
    generation.retain(|g| g != GEN_OBJECTIVES);
    if t.shows_objectives {
        generation.push(GEN_OBJECTIVES.to_string());
    }
    let settings = Settings {
        student_tasks: student_tasks()
            .into_iter()
            .filter(|l| used(|s| &s.student_tasks, l))
            .map(str::to_string)
            .collect(),
        answer_formats: ANSWER_FORMATS
            .into_iter()
            .filter(|l| used(|s| &s.answer_formats, l))
            .map(str::to_string)
            .collect(),
        generation: GENERATION_OPTIONS
            .into_iter()
            .filter(|l| generation.iter().any(|g| g == l))
            .map(str::to_string)
            .collect(),
        ..base.clone()
    };
    settings.normalised().unwrap_or(settings)
}

// ---------------------------------------------------------------- expansion into blocks

/// One block to generate: a section, and the paragraph it covers when it is repeated per
/// paragraph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// Index into `sections`.
    pub section: usize,
    /// 1-based paragraph number of the new material.
    pub paragraph: Option<usize>,
    /// A per-paragraph section generated once over the whole text (the cap was reached).
    pub collapsed: bool,
}

/// Each section becomes one block; a per-paragraph section one block per paragraph. The total
/// stays within [`MAX_BLOCKS`]: a per-paragraph section whose expansion would go over it is
/// generated once over the whole text instead, and a notice says why.
pub fn plan_blocks(t: &WsTemplate, paragraphs: usize) -> (Vec<Target>, Vec<String>) {
    let mut total = t.sections.len();
    let mut expand: Vec<bool> = Vec::new();
    let mut notices: Vec<String> = Vec::new();
    for s in &t.sections {
        if !s.per_paragraph || paragraphs == 0 {
            expand.push(false);
            continue;
        }
        let extra = paragraphs - 1;
        if total + extra <= MAX_BLOCKS {
            total += extra;
            expand.push(true);
        } else {
            expand.push(false);
            notices.push(format!(
                "「{}」在版型裡是每段各做一次，但這篇教材有 {paragraphs} 段，全部展開會超過 {MAX_BLOCKS} 個區塊，所以這一區改成對全文做一次。",
                clip_chars(&s.title_pattern, 60)
            ));
        }
    }
    let mut targets = Vec::new();
    for (i, s) in t.sections.iter().enumerate() {
        if expand[i] {
            targets.extend((1..=paragraphs).map(|p| Target {
                section: i,
                paragraph: Some(p),
                collapsed: false,
            }));
        } else {
            targets.push(Target {
                section: i,
                paragraph: None,
                collapsed: s.per_paragraph,
            });
        }
    }
    (targets, notices)
}

/// A block title for a paragraph, always in one form: 「Paragraph n: …」 (「第 n 段：…」 for a
/// Chinese template). The model numbers paragraphs in many ways (`[n]`, `[2]`, `2 `, `Para 2`,
/// `Paragraph 2`, `第 2 段`), so any leading number or paragraph label is removed before the
/// canonical prefix is added.
pub fn title_for(title: &str, paragraph: Option<usize>, instruction_language: &str) -> String {
    let title = title.trim();
    let Some(n) = paragraph else {
        return title.to_string();
    };
    static LEADING: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(
            r"(?i)^\s*(?:(?:paragraphs?|para\.?|p\.?)\s*(?:\[\s*(?:n|\d+)\s*\]|\d+)|第\s*(?:\[\s*(?:n|\d+)\s*\]|\d+)\s*段|\[\s*(?:n|\d+)\s*\]|\(\s*\d+\s*\)|\d+)\s*[:：.\-–)]?\s*",
        )
        .expect("valid regex")
    });
    let mut rest = title.to_string();
    // Strip repeated labels such as "[2] Paragraph 2:".
    for _ in 0..3 {
        let stripped = LEADING.replace(&rest, "").to_string();
        if stripped == rest {
            break;
        }
        rest = stripped;
    }
    let rest = rest.replace("[n]", &n.to_string());
    let rest = rest.trim_matches(|c: char| c.is_whitespace() || ":：-–.".contains(c));
    let prefix = if instruction_language == "中文" {
        format!("第 {n} 段")
    } else {
        format!("Paragraph {n}")
    };
    if rest.is_empty() {
        prefix
    } else if instruction_language == "中文" {
        format!("{prefix}：{rest}")
    } else {
        format!("{prefix}: {rest}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn section(title: &str, per_paragraph: bool) -> WsTemplateSection {
        WsTemplateSection {
            id: String::new(),
            title_pattern: title.to_string(),
            per_paragraph,
            item_count: 3,
            answer_formats: labels(&["簡答題"]),
            ..WsTemplateSection::default()
        }
    }

    fn template(sections: Vec<WsTemplateSection>) -> WsTemplate {
        WsTemplate {
            worksheet_kind: "課文深度閱讀".into(),
            sections,
            ..WsTemplate::default()
        }
    }

    #[test]
    fn specific_paragraphs_are_recognised() {
        for yes in ["第 2 段", "第1、2段", "Paragraphs 2–3", "P4", "第三段"] {
            assert!(names_specific_paragraphs(yes), "{yes}");
        }
        for no in ["逐段", "每一段", "全文", "課文外", "each paragraph", ""] {
            assert!(!names_specific_paragraphs(no), "{no}");
        }
    }

    #[test]
    fn validation_rejects_labels_outside_the_sets() {
        let mut t = template(vec![section("TASK 1", false), section("TASK 2", true)]);
        t.validate().unwrap();
        assert_eq!(
            t.sections.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            ["s1", "s2"]
        );
        let mut bad = t.clone();
        bad.sections[0].answer_formats = labels(&["探究"]);
        assert!(bad.validate().unwrap_err().contains("作答方式"));
        let mut bad = t.clone();
        bad.sections[1].item_count = 13;
        assert!(bad.validate().unwrap_err().contains("第 2 個區塊"));
        let mut bad = t.clone();
        bad.sections[1].id = "s1".into();
        assert!(bad.validate().is_err());
        let mut bad = t.clone();
        bad.sections.clear();
        assert!(bad.validate().is_err());
        let mut bad = t.clone();
        bad.instruction_language = "日文".into();
        assert!(bad.validate().is_err());
        let mut bad = t.clone();
        bad.sections[0].scaffolds = labels(&["鷹架X"]);
        assert!(bad.validate().is_err());
        let mut ok = t.clone();
        ok.sections[0].scaffolds = labels(&["句型框架", "清楚的分步指示及完成順序", "句型框架"]);
        ok.validate().unwrap();
        assert_eq!(ok.sections[0].scaffolds.len(), 2);
    }

    #[test]
    fn extraction_is_cleaned_up_and_valid() {
        let outline = RawOutline {
            worksheet_kind: "逐段提問單".into(),
            instruction_language: "火星文".into(),
            header_fields: labels(&["Name", " ", "No"]),
            shows_objectives: true,
            includes_passage_text: false,
            outline: (0..14)
                .map(|i| RawOutlineItem {
                    title_pattern: format!("Part {i}"),
                    per_paragraph: true,
                })
                .collect(),
        };
        let raw = |i: i64| RawSection {
            title_pattern: if i == 0 {
                String::new()
            } else {
                format!("Part {i}: [主題]")
            },
            purpose: "p".repeat(400),
            block_kind: "nonsense".into(),
            student_tasks: labels(&["理解內容", "探究", "理解內容"]),
            answer_formats: labels(&["簡答題", "SDGs"]),
            item_count: if i == 1 { 40 } else { 0 },
            text_scope: if i == 2 {
                "第 2 段".into()
            } else {
                "逐段".into()
            },
            scaffolds: labels(&["引導問題", "高層次思考"]),
            instruction_style: "英文".into(),
            item_patterns: labels(&["a", "b", "c", "d", "e", "f", "g"]),
            needs_external_material: i == 3,
        };
        let details = RawDetails {
            sections: (0..14).map(raw).collect(),
            generation_options: labels(&["提供引導問題", "SEL"]),
            style_notes: labels(&["n"; 10]),
        };
        let mut t = from_extraction(&outline, &details);
        assert_eq!(t.sections.len(), MAX_SECTIONS);
        assert_eq!(t.instruction_language, "英文");
        assert_eq!(t.header_fields, labels(&["Name", "No"]));
        let s = &t.sections;
        assert_eq!(
            s[0].title_pattern, "Part 0",
            "falls back to the outline title"
        );
        assert_eq!(s[1].item_count, 12);
        assert_eq!(s[0].item_count, 1);
        assert!(s[0].per_paragraph && !s[2].per_paragraph);
        assert!(s[3].needs_external_material);
        assert_eq!(s[0].student_tasks, labels(&["理解內容"]));
        assert_eq!(s[0].answer_formats, labels(&["簡答題"]));
        assert_eq!(s[0].scaffolds, labels(&["引導問題"]));
        assert_eq!(s[0].block_kind, BlockKind::Activity);
        assert_eq!(s[0].item_patterns.len(), MAX_ITEM_PATTERNS);
        assert_eq!(s[0].purpose.chars().count(), MAX_PURPOSE_CHARS);
        assert_eq!(t.generation_options, labels(&["提供引導問題"]));
        assert_eq!(t.style_notes.len(), MAX_STYLE_NOTES);
        let before = t.clone();
        t.validate().unwrap();
        assert_eq!(t, before, "a cleaned extraction is already valid");
    }

    #[test]
    fn expansion_is_capped() {
        let t = template(vec![
            section("Warm-up", false),
            section("Paragraph [n]", true),
            section("Details [n]", true),
            section("Wrap up", false),
        ]);
        let (targets, notices) = plan_blocks(&t, 5);
        // 4 sections; the first per-paragraph section expands to 5 (total 8), the second would
        // make 12: still within 16.
        assert_eq!(targets.len(), 12);
        assert!(notices.is_empty());
        let (targets, notices) = plan_blocks(&t, 9);
        // 4 + 8 = 12 for the first; the second would make 20, so it is done once.
        assert_eq!(targets.len(), 12);
        assert_eq!(notices.len(), 1);
        assert!(notices[0].contains("Details [n]"));
        assert!(targets
            .iter()
            .any(|x| x.section == 2 && x.paragraph.is_none() && x.collapsed));
        assert_eq!(
            targets
                .iter()
                .filter(|x| x.section == 1)
                .map(|x| x.paragraph)
                .collect::<Vec<_>>(),
            (1..=9).map(Some).collect::<Vec<_>>()
        );
        let (targets, _) = plan_blocks(&t, 30);
        assert_eq!(targets.len(), 4);
    }

    #[test]
    fn titles_name_the_paragraph() {
        assert_eq!(
            title_for("Paragraph [n]: Details", Some(3), "英文"),
            "Paragraph 3: Details"
        );
        assert_eq!(
            title_for("Details", Some(2), "英文"),
            "Paragraph 2: Details"
        );
        assert_eq!(title_for("細節", Some(2), "中文"), "第 2 段：細節");
        assert_eq!(
            title_for("Para 4 notes", Some(4), "英文"),
            "Paragraph 4: notes"
        );
        // The forms gemma produced in the real-model run all become one form.
        for raw in [
            "1 Paragraph Analysis",
            "[1] Paragraph Analysis",
            "Paragraph 1 Paragraph Analysis",
            "[n] Paragraph Analysis",
            "Paragraph [n]: Paragraph Analysis",
        ] {
            assert_eq!(
                title_for(raw, Some(1), "英文"),
                "Paragraph 1: Paragraph Analysis",
                "{raw}"
            );
        }
        assert_eq!(
            title_for("第 [n] 段 字彙", Some(2), "中文"),
            "第 2 段：字彙"
        );
        assert_eq!(title_for("[n]", Some(5), "英文"), "Paragraph 5");
        assert_eq!(title_for(" Wrap up ", None, "英文"), "Wrap up");
    }

    #[test]
    fn settings_follow_the_template() {
        let mut t = template(vec![section("A", false), section("B", false)]);
        t.sections[0].student_tasks = labels(&["寫作"]);
        t.sections[1].student_tasks = labels(&["理解內容"]);
        t.sections[1].answer_formats = labels(&["選擇題"]);
        t.generation_options = labels(&["提供句型框架", "顯示學習目標"]);
        t.shows_objectives = false;
        let base = Settings {
            material_types: labels(&["閱讀文本"]),
            generation: labels(&["加入暖身活動"]),
            levels: vec![super::super::Level::Standard],
            ..Settings::default()
        };
        let s = prefill_settings(&base, &t);
        assert_eq!(s.student_tasks, labels(&["理解內容", "寫作"]));
        assert_eq!(s.answer_formats, labels(&["選擇題", "簡答題"]));
        assert_eq!(s.generation, labels(&["提供句型框架"]));
        assert_eq!(s.material_types, base.material_types);
        t.shows_objectives = true;
        assert!(prefill_settings(&base, &t).wants(GEN_OBJECTIVES));
    }

    #[test]
    fn schemas_keep_the_written_order() {
        let s = details_schema(3);
        let keys: Vec<&str> = s["properties"]["sections"]["items"]["properties"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys.first(), Some(&"title_pattern"));
        assert_eq!(keys.last(), Some(&"needs_external_material"));
        assert_eq!(s["properties"]["sections"]["minItems"], 3);
        assert_eq!(s["properties"]["sections"]["maxItems"], 3);
        let o = outline_schema();
        let keys: Vec<&String> = o["properties"].as_object().unwrap().keys().collect();
        assert_eq!(keys.last().map(|k| k.as_str()), Some("outline"));
        for schema in [o, s, section_out_schema()] {
            let text = schema.to_string();
            for label in super::super::FORBIDDEN_LABELS {
                assert!(!text.contains(&format!("\"{label}\"")), "{label}");
            }
        }
    }
}
