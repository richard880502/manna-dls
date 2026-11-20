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

use super::bloom::Bloom;
use super::{
    student_tasks, BlockKind, ScaffoldType, Settings, ANSWER_FORMATS, GENERATION_OPTIONS,
    GEN_OBJECTIVES, INSTRUCTIONAL_KINDS, KIND_EXAMPLE, KIND_FRAMES, KIND_GUIDING, KIND_STEPS,
    KIND_WORD_LIST, MAX_ID_CHARS, PROCEDURAL_KINDS, VERBAL_KINDS,
};
use crate::text::clip_chars;

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
pub const MAX_SUPPORTS: usize = 6;
pub const MAX_SUPPORT_HEADING_CHARS: usize = 60;
pub const MAX_SUPPORT_PATTERN_CHARS: usize = 300;

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
    /// The sample's labelled support boxes in this section, in the sample's order, each with its
    /// own heading ("Directions", "Sentence Frame for Sharing", "Word Bank"…). A block made from
    /// the section prints each one separately under that heading. Empty for a section without
    /// any, and for a template read before supports existed (its block prints one scaffold of
    /// its first kind, as before); absent from the JSON when empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub supports: Vec<WsTemplateSupport>,
    pub instruction_style: String,
    /// Patterns of the numbered questions only: a frame is a support, never an item pattern.
    pub item_patterns: Vec<String>,
    /// Needs a video, recording, image or other text the teacher must supply.
    pub needs_external_material: bool,
}

/// What a support box of the sample is for. Each role prints as one scaffold kind of the spec
/// ([`SupportRole::kind`]); the box keeps the sample's heading.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SupportRole {
    /// Short imperative steps, no blanks (清楚的分步指示及完成順序, procedural).
    Directions,
    /// Sentences with blanks the student completes in writing (句型框架).
    #[default]
    SentenceFrame,
    /// A frame for sharing aloud; it may open with a greeting ("Hi! In Paragraph 2, …"). Never a
    /// numbered question (句型框架).
    SharingFrame,
    /// Words only (字詞表).
    WordBank,
    /// Short hints or prompts (提示與追問).
    Hints,
    /// Questions that guide the thinking, not answered on the sheet (引導問題).
    GuidingQuestions,
    /// A worked example (作答示例).
    Example,
}

impl SupportRole {
    pub const ALL: [Self; 7] = [
        Self::Directions,
        Self::SentenceFrame,
        Self::SharingFrame,
        Self::WordBank,
        Self::Hints,
        Self::GuidingQuestions,
        Self::Example,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Directions => "directions",
            Self::SentenceFrame => "sentence_frame",
            Self::SharingFrame => "sharing_frame",
            Self::WordBank => "word_bank",
            Self::Hints => "hints",
            Self::GuidingQuestions => "guiding_questions",
            Self::Example => "example",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.as_str() == s.trim())
    }

    /// The role a scaffold kind is printed for, if any (a sentence frame is a writing frame).
    pub fn of_kind(kind: &str) -> Option<Self> {
        [
            Self::Directions,
            Self::SentenceFrame,
            Self::WordBank,
            Self::Hints,
            Self::GuidingQuestions,
            Self::Example,
        ]
        .into_iter()
        .find(|r| r.kind() == kind)
    }

    /// The scaffold kind it prints as.
    pub fn kind(self) -> &'static str {
        match self {
            Self::Directions => KIND_STEPS,
            Self::SentenceFrame | Self::SharingFrame => KIND_FRAMES,
            Self::WordBank => KIND_WORD_LIST,
            Self::Hints => "提示與追問",
            Self::GuidingQuestions => KIND_GUIDING,
            Self::Example => KIND_EXAMPLE,
        }
    }

    pub fn kind_type(self) -> ScaffoldType {
        if self == Self::Directions {
            ScaffoldType::Procedural
        } else {
            ScaffoldType::Verbal
        }
    }

    /// The heading a box of this role gets when the sample gave it none.
    pub fn default_heading(self) -> &'static str {
        match self {
            Self::Directions => "Directions",
            Self::SentenceFrame => "Sentence Frames",
            Self::SharingFrame => "Sentence Frame for Sharing",
            Self::WordBank => "Word Bank",
            Self::Hints => "Hints",
            Self::GuidingQuestions => "Guiding Questions",
            Self::Example => "Example",
        }
    }

    /// A sentence frame (for writing or for sharing aloud).
    pub fn is_frame(self) -> bool {
        matches!(self, Self::SentenceFrame | Self::SharingFrame)
    }
}

/// One labelled support box of the sample.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct WsTemplateSupport {
    /// The sample's own heading, word for word (≤60).
    pub heading: String,
    pub role: SupportRole,
    /// What the box holds, as a reusable pattern with placeholders and blanks (≤300, may be
    /// empty).
    pub pattern: String,
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
            if s.supports.len() > MAX_SUPPORTS {
                return Err(at(format!("提示框最多 {MAX_SUPPORTS} 個。")));
            }
            for sup in &mut s.supports {
                sup.heading = sup.heading.trim().to_string();
                if sup.heading.is_empty() {
                    return Err(at("提示框的標題不能是空的。".to_string()));
                }
                check_len("提示框的標題", &sup.heading, MAX_SUPPORT_HEADING_CHARS).map_err(at)?;
                sup.pattern = sup.pattern.trim().to_string();
                check_len("提示框的樣式", &sup.pattern, MAX_SUPPORT_PATTERN_CHARS).map_err(at)?;
            }
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
    /// `worksheet_template_details` version 3; absent from an older output.
    #[serde(default)]
    pub supports: Vec<RawSupport>,
    pub instruction_style: String,
    pub item_patterns: Vec<String>,
    pub needs_external_material: bool,
}

/// One support box as the extraction wrote it (`role` is checked when it is cleaned up).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct RawSupport {
    pub heading: String,
    pub role: String,
    pub pattern: String,
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
    /// The item's Bloom level (`worksheet_template_section` version 2); a value outside the
    /// fixed six is dropped by the checks. Absent in a revision (`prep_revise`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bloom: Option<String>,
    /// 圖像組織 (`worksheet_template_section` version 3, when the section may use it): the
    /// organizer to draw; absent or `null` otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub organizer: Option<crate::worksheet::organizer::RawOrganizer>,
}

/// One generated block.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawSectionOut {
    pub title: String,
    pub instructions: String,
    /// One line per step, frame or question. The schema asks for an array of lines (version 2:
    /// gemma ends a JSON string where it wants a line break); a single string is read as well.
    /// A section with supports is asked for `supports` instead (absent then).
    #[serde(default, deserialize_with = "lines_or_string")]
    pub scaffold: String,
    /// One entry per support box of the section, in its order (sections with supports only).
    #[serde(default)]
    pub supports: Vec<RawSupportOut>,
    pub items: Vec<RawOutItem>,
}

/// One support box as generated: the heading it belongs to and its lines.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct RawSupportOut {
    #[serde(default)]
    pub heading: String,
    #[serde(default, deserialize_with = "line_list")]
    pub lines: Vec<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Lines {
    One(String),
    Many(Vec<String>),
}

impl Lines {
    fn list(self) -> Vec<String> {
        let all = match self {
            Lines::One(s) => s.lines().map(str::to_string).collect::<Vec<_>>(),
            Lines::Many(lines) => lines,
        };
        all.iter()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect()
    }
}

/// An array of lines joined with line breaks, or a string as it is.
fn lines_or_string<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(match Lines::deserialize(d)? {
        Lines::One(s) => s,
        many => many.list().join("\n"),
    })
}

/// An array of lines, or a string split at its line breaks; blank lines dropped.
fn line_list<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    Ok(Lines::deserialize(d)?.list())
}

// ---------------------------------------------------------------- schemas (properties in written order)

fn b() -> Value {
    json!({ "type": "boolean" })
}

/// A string with a length cap. The gateway enforces `maxLength`/`maxItems` while decoding, which
/// is what stops gemma's repetition runaways inside long strings (a real-model run lost about
/// half its blocks to an answer repeating one sentence until max_tokens).
pub(crate) fn s_max(max: usize) -> Value {
    json!({ "type": "string", "maxLength": max })
}

pub(crate) fn strings_max(items: usize, chars: usize) -> Value {
    json!({ "type": "array", "items": s_max(chars), "maxItems": items })
}

pub(crate) fn enum_of(labels: &[&str]) -> Value {
    json!({ "type": "string", "enum": labels })
}

fn enum_array(labels: &[&str]) -> Value {
    json!({ "type": "array", "items": enum_of(labels) })
}

/// An object whose properties are required, in the order written (the gateway emits fields in
/// schema order; alphabetical order made gemma run away in whitespace).
pub(crate) fn obj(props: Vec<(&str, Value)>) -> Value {
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
        ("worksheet_kind", s_max(MAX_KIND_CHARS)),
        ("instruction_language", enum_of(&INSTRUCTION_LANGUAGES)),
        (
            "header_fields",
            strings_max(MAX_HEADER_FIELDS, MAX_HEADER_FIELD_CHARS),
        ),
        ("shows_objectives", b()),
        ("includes_passage_text", b()),
        (
            "outline",
            json!({ "type": "array", "items": obj(vec![
                ("title_pattern", s_max(MAX_TITLE_CHARS)),
                ("per_paragraph", b()),
            ]), "maxItems": 20 }),
        ),
    ])
}

fn section_schema() -> Value {
    obj(vec![
        ("title_pattern", s_max(MAX_TITLE_CHARS)),
        ("purpose", s_max(MAX_PURPOSE_CHARS)),
        ("block_kind", enum_of(&BLOCK_KINDS)),
        ("student_tasks", enum_array(&student_tasks())),
        ("answer_formats", enum_array(&ANSWER_FORMATS)),
        ("item_count", json!({ "type": "integer" })),
        ("text_scope", s_max(MAX_TEXT_SCOPE_CHARS)),
        ("scaffolds", enum_array(&scaffold_kinds())),
        // Before the item patterns: what is a support box is decided first, so a frame does
        // not end up as a question pattern.
        (
            "supports",
            json!({ "type": "array", "items": obj(vec![
                ("heading", s_max(MAX_SUPPORT_HEADING_CHARS)),
                ("role", enum_of(&support_roles())),
                ("pattern", s_max(MAX_SUPPORT_PATTERN_CHARS)),
            ]), "maxItems": MAX_SUPPORTS }),
        ),
        ("instruction_style", s_max(MAX_INSTRUCTION_STYLE_CHARS)),
        (
            "item_patterns",
            strings_max(MAX_ITEM_PATTERNS, MAX_ITEM_PATTERN_CHARS),
        ),
        ("needs_external_material", b()),
    ])
}

fn support_roles() -> Vec<&'static str> {
    SupportRole::ALL.iter().map(|r| r.as_str()).collect()
}

/// `details_schema(n)` of the probe: exactly `n` sections (one per outline item).
pub fn details_schema(n: usize) -> Value {
    obj(vec![
        (
            "sections",
            json!({ "type": "array", "items": section_schema(), "minItems": n, "maxItems": n }),
        ),
        ("generation_options", enum_array(&GENERATION_OPTIONS)),
        (
            "style_notes",
            strings_max(MAX_STYLE_NOTES, MAX_STYLE_NOTE_CHARS),
        ),
    ])
}

/// Caps on one generated block. Generous for a worksheet item, small enough that a repetition
/// runaway is cut off early instead of eating the whole token budget.
pub const SECTION_TITLE_CHARS: usize = 150;
pub const SECTION_INSTRUCTIONS_CHARS: usize = 500;
/// The scaffold: at most this many lines of this many characters.
pub const SECTION_SCAFFOLD_LINES: usize = 6;
pub const SECTION_SCAFFOLD_LINE_CHARS: usize = 200;
pub const ITEM_PROMPT_CHARS: usize = 600;
pub const ITEM_OPTIONS: usize = 6;
pub const ITEM_OPTION_CHARS: usize = 150;
pub const ITEM_ANSWER_CHARS: usize = 400;

/// SECTION_OUT of the probe (gen2), with length caps; at most `item_count` items. Version 2 of
/// the prompt: the scaffold is an array of lines and every item ends with its Bloom level (an
/// enum of the six ids), written after the item so the label describes what was written.
/// `organizers` (the section may use 圖像組織): every item has an `organizer` after its options.
/// `supports` (the section's support boxes): instead of `scaffold`, exactly one entry per box,
/// its heading one of the boxes' headings, written before the items.
pub fn section_out_schema(
    item_count: usize,
    organizers: bool,
    supports: &[WsTemplateSupport],
) -> Value {
    let blooms: Vec<&str> = Bloom::ALL.iter().map(|b| b.as_str()).collect();
    let mut item = vec![
        ("answer_format", enum_of(&ANSWER_FORMATS)),
        ("prompt", s_max(ITEM_PROMPT_CHARS)),
        ("options", strings_max(ITEM_OPTIONS, ITEM_OPTION_CHARS)),
    ];
    if organizers {
        item.push(("organizer", crate::worksheet::organizer::schema(false)));
    }
    item.extend([
        ("answer", s_max(ITEM_ANSWER_CHARS)),
        ("bloom", enum_of(&blooms)),
    ]);
    let support = if supports.is_empty() {
        (
            "scaffold",
            strings_max(SECTION_SCAFFOLD_LINES, SECTION_SCAFFOLD_LINE_CHARS),
        )
    } else {
        let mut headings: Vec<&str> = Vec::new();
        for s in supports {
            if !headings.contains(&s.heading.as_str()) {
                headings.push(s.heading.as_str());
            }
        }
        (
            "supports",
            json!({
                "type": "array",
                "items": obj(vec![
                    ("heading", enum_of(&headings)),
                    ("lines", strings_max(SECTION_SCAFFOLD_LINES, SECTION_SCAFFOLD_LINE_CHARS)),
                ]),
                "minItems": supports.len(),
                "maxItems": supports.len(),
            }),
        )
    };
    obj(vec![
        ("title", s_max(SECTION_TITLE_CHARS)),
        ("instructions", s_max(SECTION_INSTRUCTIONS_CHARS)),
        support,
        (
            "items",
            json!({
                "type": "array",
                "items": obj(item),
                "maxItems": item_count.clamp(1, MAX_ITEM_COUNT as usize)
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
            let mut section = WsTemplateSection {
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
                supports: supports_from(&d.supports),
                instruction_style: clip_trim(&d.instruction_style, MAX_INSTRUCTION_STYLE_CHARS),
                item_patterns: clip_list(
                    &d.item_patterns,
                    MAX_ITEM_PATTERNS,
                    MAX_ITEM_PATTERN_CHARS,
                ),
                needs_external_material: d.needs_external_material,
            };
            settle_supports(&mut section);
            section
        })
        .collect();
    let sections = drop_header_section(sections);
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

/// The support boxes the extraction wrote, leniently: a role outside the set drops the box, an
/// empty heading becomes the role's default heading, text is cut to its limit, at most
/// [`MAX_SUPPORTS`].
fn supports_from(raw: &[RawSupport]) -> Vec<WsTemplateSupport> {
    raw.iter()
        .filter_map(|r| {
            let role = SupportRole::parse(&r.role)?;
            let mut heading = clip_trim(&r.heading, MAX_SUPPORT_HEADING_CHARS);
            if heading.is_empty() {
                heading = role.default_heading().to_string();
            }
            Some(WsTemplateSupport {
                heading,
                role,
                pattern: clip_trim(&r.pattern, MAX_SUPPORT_PATTERN_CHARS),
            })
        })
        .take(MAX_SUPPORTS)
        .collect()
}

/// A pattern that is a support box of the sample with its label: 「Directions: Underline …」,
/// 「Sentence Frame for Sharing: Hi! …」 (a template read before supports wrote them so). The
/// label as written, its role and what follows it.
static LABELLED_BOX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^\s*(directions?|sentence\s+frames?\s+for\s+sharing|sharing\s+frames?|sentence\s+frames?|sentence\s+starters?|word\s+banks?|hints?|tips|guiding\s+questions|examples?)\s*[:：]\s*(.+)$",
    )
    .expect("valid regex")
});

fn labelled_box(pattern: &str) -> Option<(String, SupportRole, String)> {
    let c = LABELLED_BOX.captures(pattern)?;
    let label = c[1].split_whitespace().collect::<Vec<_>>().join(" ");
    let key = label.to_lowercase();
    let role = if key.starts_with("direction") {
        SupportRole::Directions
    } else if key.contains("sharing") {
        SupportRole::SharingFrame
    } else if key.starts_with("sentence") {
        SupportRole::SentenceFrame
    } else if key.starts_with("word") {
        SupportRole::WordBank
    } else if key.starts_with("hint") || key == "tips" {
        SupportRole::Hints
    } else if key.starts_with("guiding") {
        SupportRole::GuidingQuestions
    } else {
        SupportRole::Example
    };
    Some((label, role, c[2].trim().to_string()))
}

/// A frame (or another support box) is never a question pattern (a teacher found her 「Sentence
/// Frame for Sharing」 numbered as a question in every paragraph block): an item pattern that is
/// a labelled box (「Directions: …」, [`labelled_box`]), a greeting-led fill-in frame
/// ([`crate::worksheet::checks::greeting_frame`]) or a frame in the student's voice
/// ([`crate::worksheet::checks::sentence_frame_only`]) leaves `item_patterns` and becomes the
/// pattern of the section's box of that role, which is added (under the label as written, or the
/// role's default heading) when the section has none. A section that had no boxes and whose
/// frames came without a label keeps, first, a box of its first scaffold kind (its one scaffold
/// printed before). When the patterns were one per question (as many as `item_count`), the count
/// goes down with them.
fn frames_out_of_patterns(s: &mut WsTemplateSection) {
    use crate::worksheet::checks::{greeting_frame, sentence_frame_only};
    let one_per_item = s.item_patterns.len() as i64 == s.item_count;
    let had_supports = !s.supports.is_empty();
    let mut moved: Vec<(Option<String>, SupportRole, String)> = Vec::new();
    let mut kept = Vec::new();
    for p in std::mem::take(&mut s.item_patterns) {
        if let Some((label, role, rest)) = labelled_box(&p) {
            let rest = match role {
                SupportRole::SharingFrame => greeting_frame(&rest, "").unwrap_or(rest),
                _ => rest,
            };
            moved.push((Some(label), role, rest));
        } else if let Some(frame) = greeting_frame(&p, "") {
            moved.push((None, SupportRole::SharingFrame, frame));
        } else if sentence_frame_only(&p, "") {
            moved.push((None, SupportRole::SentenceFrame, p));
        } else {
            kept.push(p);
        }
    }
    s.item_patterns = kept;
    if moved.is_empty() {
        return;
    }
    if !had_supports && moved.iter().all(|(label, _, _)| label.is_none()) {
        let first = s.scaffolds.iter().find_map(|k| SupportRole::of_kind(k));
        if let Some(first) = first.filter(|r| moved.iter().all(|(_, role, _)| role != r)) {
            s.supports.push(WsTemplateSupport {
                heading: first.default_heading().to_string(),
                role: first,
                pattern: String::new(),
            });
        }
    }
    let count = moved.len() as i64;
    for (label, role, text) in moved {
        let text = clip_trim(&text, MAX_SUPPORT_PATTERN_CHARS);
        match s.supports.iter().position(|x| x.role == role) {
            Some(at) if s.supports[at].pattern.is_empty() => s.supports[at].pattern = text,
            Some(_) => {}
            None if s.supports.len() < MAX_SUPPORTS => s.supports.push(WsTemplateSupport {
                heading: label
                    .map(|l| clip_trim(&l, MAX_SUPPORT_HEADING_CHARS))
                    .unwrap_or_else(|| role.default_heading().to_string()),
                role,
                pattern: text,
            }),
            None => {}
        }
    }
    if one_per_item {
        s.item_count = (s.item_count - count).max(1);
    }
}

/// A section read before supports existed, brought up to date: its labelled boxes and frames
/// move out of the question patterns ([`frames_out_of_patterns`]) and its kinds follow
/// ([`kinds_of_supports`]). A section whose patterns hold no box stays as it is.
fn settle_supports(s: &mut WsTemplateSection) {
    frames_out_of_patterns(s);
    kinds_of_supports(s);
}

impl WsTemplate {
    /// The template as a worksheet is generated from it: sections read before supports existed
    /// get the boxes their question patterns hold ([`settle_supports`]). The stored versions are
    /// not changed; a section that has boxes is used as it is.
    pub fn for_generation(mut self) -> Self {
        for s in self.sections.iter_mut().filter(|s| s.supports.is_empty()) {
            settle_supports(s);
        }
        self
    }
}

/// The scaffold kinds of a section include those its support boxes print as.
fn kinds_of_supports(s: &mut WsTemplateSection) {
    for sup in &s.supports {
        let kind = sup.role.kind();
        if !s.scaffolds.iter().any(|k| k == kind) {
            s.scaffolds.push(kind.to_string());
        }
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

/// The worksheet's own title line (「[單元名稱] Vocabulary Worksheet」) sometimes comes back as the
/// first section, although the prompt says it is not one; generating it gives a block like
/// 「write your name here」. A first section that looks like a worksheet title is dropped (when
/// others remain) and the ids are renumbered.
fn drop_header_section(mut sections: Vec<WsTemplateSection>) -> Vec<WsTemplateSection> {
    static HEADER: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)(worksheet|handout|學習單|講義|提問單)\s*$").expect("valid regex")
    });
    static ACTIVITY: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)\b(task|part|step|activity|exercise)\b|任務|活動|練習")
            .expect("valid regex")
    });
    let looks_like_header = sections.first().is_some_and(|s| {
        let t = s.title_pattern.trim();
        HEADER.is_match(t) && !ACTIVITY.is_match(t)
    });
    if looks_like_header && sections.len() > 1 {
        sections.remove(0);
        for (i, s) in sections.iter_mut().enumerate() {
            s.id = format!("s{}", i + 1);
        }
    }
    sections
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
    static TRAILING: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(
            r"(?i)\s*[(（]\s*(?:(?:paragraphs?|para\.?|p\.?)\s*(?:\[\s*(?:n|\d+)\s*\]|\d+)|第\s*(?:\[\s*(?:n|\d+)\s*\]|\d+)\s*段)\s*[)）]\s*$",
        )
        .expect("valid regex")
    });
    let rest = TRAILING.replace(&rest, "").to_string();
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
            supports: Vec::new(),
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
    fn a_leading_worksheet_title_is_not_a_section() {
        let kept = drop_header_section(vec![
            section("[單元名稱] Vocabulary Worksheet", false),
            section("Task I. Syllables", false),
        ]);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].title_pattern, "Task I. Syllables");
        assert_eq!(kept[0].id, "s1");
        // A real activity, or the only section, stays.
        assert_eq!(
            drop_header_section(vec![
                section("Task 1 Reading Worksheet", false),
                section("B", false)
            ])
            .len(),
            2
        );
        assert_eq!(
            drop_header_section(vec![section("Worksheet", false)]).len(),
            1
        );
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
        assert_eq!(
            title_for("Vocabulary (第 1 段)", Some(1), "中文"),
            "第 1 段：Vocabulary"
        );
        assert_eq!(
            title_for("Reading (Paragraph [n])", Some(2), "英文"),
            "Paragraph 2: Reading"
        );
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
    fn a_block_reads_scaffold_lines_and_bloom_levels() {
        let v2: RawSectionOut = serde_json::from_value(json!({
            "title": "Details", "instructions": "Answer.",
            "scaffold": ["Step 1: find the key words.", " ", "Step 2: answer in a sentence."],
            "items": [{ "answer_format": "簡答題", "prompt": "Why?", "options": [],
                        "answer": "Because.", "bloom": "analyze" }],
        }))
        .unwrap();
        assert_eq!(
            v2.scaffold,
            "Step 1: find the key words.\nStep 2: answer in a sentence."
        );
        assert_eq!(v2.items[0].bloom.as_deref(), Some("analyze"));
        // Version 1 (a scaffold string, no Bloom level) and a revision still read.
        let v1: RawSectionOut = serde_json::from_value(json!({
            "title": "Details", "instructions": "Answer.", "scaffold": "Look first.",
            "items": [{ "answer_format": "簡答題", "prompt": "Why?", "options": [],
                        "answer": "Because." }],
        }))
        .unwrap();
        assert_eq!(v1.scaffold, "Look first.");
        assert_eq!(v1.items[0].bloom, None);
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
        let out = section_out_schema(20, false, &[]);
        assert_eq!(out["properties"]["items"]["maxItems"], 12, "capped at 12");
        assert_eq!(
            section_out_schema(3, false, &[])["properties"]["items"]["maxItems"],
            3
        );
        // A section that may use 圖像組織: an organizer after the options.
        let with = section_out_schema(3, true, &[]);
        let keys: Vec<&String> = with["properties"]["items"]["items"]["properties"]
            .as_object()
            .unwrap()
            .keys()
            .collect();
        assert_eq!(
            keys,
            [
                "answer_format",
                "prompt",
                "options",
                "organizer",
                "answer",
                "bloom"
            ]
        );
        assert_eq!(
            out["properties"]["items"]["items"]["properties"]["answer"]["maxLength"],
            ITEM_ANSWER_CHARS
        );
        // Version 2: a Bloom level per item, last; the scaffold is an array of capped lines.
        let item_keys: Vec<&String> = out["properties"]["items"]["items"]["properties"]
            .as_object()
            .unwrap()
            .keys()
            .collect();
        assert_eq!(
            item_keys,
            ["answer_format", "prompt", "options", "answer", "bloom"]
        );
        assert_eq!(
            out["properties"]["items"]["items"]["properties"]["bloom"]["enum"],
            json!([
                "remember",
                "understand",
                "apply",
                "analyze",
                "evaluate",
                "create"
            ])
        );
        assert_eq!(out["properties"]["scaffold"]["type"], "array");
        assert_eq!(
            out["properties"]["scaffold"]["maxItems"],
            SECTION_SCAFFOLD_LINES
        );
        assert_eq!(
            out["properties"]["scaffold"]["items"]["maxLength"],
            SECTION_SCAFFOLD_LINE_CHARS
        );
        assert_eq!(
            out["required"],
            json!(["title", "instructions", "scaffold", "items"])
        );
        for schema in [o, s, out] {
            let text = schema.to_string();
            for label in super::super::FORBIDDEN_LABELS {
                assert!(!text.contains(&format!("\"{label}\"")), "{label}");
            }
        }
    }

    fn raw_section(supports: Vec<RawSupport>, patterns: &[&str], count: i64) -> RawSection {
        RawSection {
            title_pattern: "[n] Paragraph Analysis".into(),
            purpose: "p".into(),
            block_kind: "activity".into(),
            student_tasks: labels(&["理解內容"]),
            answer_formats: labels(&["是非題", "段落寫作"]),
            item_count: count,
            text_scope: "逐段".into(),
            scaffolds: labels(&["句型框架", "提示與追問"]),
            supports,
            instruction_style: "英文祈使句".into(),
            item_patterns: labels(patterns),
            needs_external_material: false,
        }
    }

    fn extracted(section: RawSection) -> WsTemplateSection {
        let outline = RawOutline {
            worksheet_kind: "逐段分析".into(),
            instruction_language: "英文".into(),
            header_fields: Vec::new(),
            shows_objectives: false,
            includes_passage_text: false,
            outline: vec![RawOutlineItem {
                title_pattern: "[n] Paragraph Analysis".into(),
                per_paragraph: true,
            }],
        };
        let details = RawDetails {
            sections: vec![section],
            generation_options: Vec::new(),
            style_notes: Vec::new(),
        };
        let mut t = from_extraction(&outline, &details);
        let before = t.clone();
        t.validate().unwrap();
        assert_eq!(t, before, "a cleaned extraction is already valid");
        t.sections.remove(0)
    }

    const SHARING_PATTERN: &str = "Hi! In the [段落編號] paragraph, the author describes _____. After reading it carefully, I identified _____ as one of the main characteristics...";

    #[test]
    fn the_samples_support_boxes_are_kept_with_their_headings() {
        let raw = |heading: &str, role: &str, pattern: &str| RawSupport {
            heading: heading.into(),
            role: role.into(),
            pattern: pattern.into(),
        };
        let s = extracted(raw_section(
            vec![
                raw(
                    "Directions",
                    "directions",
                    "Identify [關鍵概念] in the first sentence.",
                ),
                raw(
                    " Sentence Frame for Sharing ",
                    "sharing_frame",
                    SHARING_PATTERN,
                ),
                raw("", "word_bank", "[字詞], [字詞]"),
                raw("Tips", "nonsense", "x"),
            ],
            &["Do you think [句子] is the topic sentence?"],
            3,
        ));
        assert_eq!(
            s.supports,
            [
                WsTemplateSupport {
                    heading: "Directions".into(),
                    role: SupportRole::Directions,
                    pattern: "Identify [關鍵概念] in the first sentence.".into(),
                },
                WsTemplateSupport {
                    heading: "Sentence Frame for Sharing".into(),
                    role: SupportRole::SharingFrame,
                    pattern: SHARING_PATTERN.into(),
                },
                WsTemplateSupport {
                    heading: "Word Bank".into(),
                    role: SupportRole::WordBank,
                    pattern: "[字詞], [字詞]".into(),
                },
            ]
        );
        // The kinds the boxes print as join the section's kinds.
        assert_eq!(
            s.scaffolds,
            labels(&["句型框架", "提示與追問", KIND_STEPS, KIND_WORD_LIST])
        );
        assert_eq!(s.item_count, 3);
        // Stored as it was read; a template without boxes keeps its JSON as before.
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["supports"][1]["role"], "sharing_frame");
        let old: WsTemplateSection = serde_json::from_value(json!({
            "id": "s1", "title_pattern": "Paragraph [n]", "item_count": 2,
        }))
        .unwrap();
        assert!(old.supports.is_empty());
        assert!(serde_json::to_value(&old)
            .unwrap()
            .get("supports")
            .is_none());
    }

    #[test]
    fn a_frame_read_as_a_question_pattern_becomes_a_box() {
        // As the teacher's template was read: the sharing frame is the 4th of 4 question patterns.
        let s = extracted(raw_section(
            Vec::new(),
            &[
                "What is the common idea about [主題] in the first sentence?",
                "Which sentence introduces the main point of paragraph [n]?",
                "Do you think [句子] serves as a topic sentence? (True/False)",
                SHARING_PATTERN,
            ],
            4,
        ));
        assert_eq!(s.item_patterns.len(), 3);
        assert_eq!(s.item_count, 3, "the frame was not a question");
        // The box of its first kind stays (a template read without boxes printed it), then the
        // sharing frame's.
        assert_eq!(
            s.supports
                .iter()
                .map(|b| (b.heading.as_str(), b.role))
                .collect::<Vec<_>>(),
            [
                ("Sentence Frames", SupportRole::SentenceFrame),
                ("Sentence Frame for Sharing", SupportRole::SharingFrame),
            ]
        );
        assert_eq!(s.supports[1].pattern, SHARING_PATTERN);
        // A wrapped frame among more patterns than questions: the count stays.
        let s = extracted(raw_section(
            vec![RawSupport {
                heading: "Sentence Frame for Sharing".into(),
                role: "sharing_frame".into(),
                pattern: String::new(),
            }],
            &[
                "What does [人物] do?",
                &format!("Write a paragraph using the frame: {SHARING_PATTERN}"),
            ],
            4,
        ));
        assert_eq!(s.item_patterns, labels(&["What does [人物] do?"]));
        assert_eq!(s.item_count, 4);
        assert_eq!(s.supports.len(), 1);
        assert_eq!(s.supports[0].pattern, SHARING_PATTERN);
    }

    #[test]
    fn a_template_read_before_supports_gets_its_boxes_when_generating() {
        // As a real-model extraction on main read a sample with a Directions part and a
        // Sentence Frame for Sharing part: both among the question patterns, with their labels.
        let old = WsTemplateSection {
            id: "s1".into(),
            title_pattern: "Part A. Paragraph [n] Analysis".into(),
            per_paragraph: true,
            item_count: 3,
            scaffolds: labels(&["透過上下文解釋詞義", "句型框架"]),
            item_patterns: labels(&[
                "Directions: Underline the topic sentence, circle two supporting details.",
                "Do you think the [第一] sentence serves as the topic sentence? (A) True (B) False",
                "Which detail best supports the main idea of Paragraph [n]?",
                "In your own words, what does the word \"[關鍵字]\" mean? ______",
                "Sentence Frame for Sharing: Hi! In Paragraph [n], the author describes ______.",
            ]),
            ..WsTemplateSection::default()
        };
        let plain = section("Wrap-up", false);
        let t = template(vec![old, plain.clone()]).for_generation();
        let s = &t.sections[0];
        assert_eq!(
            s.supports,
            [
                WsTemplateSupport {
                    heading: "Directions".into(),
                    role: SupportRole::Directions,
                    pattern: "Underline the topic sentence, circle two supporting details.".into(),
                },
                WsTemplateSupport {
                    heading: "Sentence Frame for Sharing".into(),
                    role: SupportRole::SharingFrame,
                    pattern: "Hi! In Paragraph [n], the author describes ______.".into(),
                },
            ]
        );
        assert_eq!(s.item_patterns.len(), 3);
        assert_eq!(
            s.item_count, 3,
            "five patterns for three questions: the count stays"
        );
        assert!(s.scaffolds.contains(&KIND_STEPS.to_string()));
        // Nothing to move, or boxes already there: as stored.
        assert_eq!(t.sections[1], plain);
        let read = template(vec![s.clone()]);
        assert_eq!(read.clone().for_generation(), read);
    }

    #[test]
    fn support_boxes_are_validated() {
        let mut t = template(vec![section("TASK 1", false)]);
        t.sections[0].supports = vec![WsTemplateSupport {
            heading: " Directions ".into(),
            role: SupportRole::Directions,
            pattern: String::new(),
        }];
        t.validate().unwrap();
        assert_eq!(t.sections[0].supports[0].heading, "Directions");
        let mut bad = t.clone();
        bad.sections[0].supports[0].heading = " ".into();
        assert!(bad.validate().unwrap_err().contains("提示框"));
        let mut bad = t.clone();
        bad.sections[0].supports[0].heading = "H".repeat(61);
        assert!(bad.validate().is_err());
        let mut bad = t.clone();
        bad.sections[0].supports = vec![t.sections[0].supports[0].clone(); 7];
        assert!(bad.validate().is_err());
        // A role outside the set is not a template.
        let parsed: Result<WsTemplateSupport, _> =
            serde_json::from_value(json!({ "heading": "Tips", "role": "tips" }));
        assert!(parsed.is_err());
    }

    #[test]
    fn a_section_with_boxes_is_asked_for_each_box() {
        let boxes = vec![
            WsTemplateSupport {
                heading: "Directions".into(),
                role: SupportRole::Directions,
                pattern: String::new(),
            },
            WsTemplateSupport {
                heading: "Sentence Frame for Sharing".into(),
                role: SupportRole::SharingFrame,
                pattern: String::new(),
            },
        ];
        let out = section_out_schema(3, false, &boxes);
        let keys: Vec<&String> = out["properties"].as_object().unwrap().keys().collect();
        assert_eq!(keys, ["title", "instructions", "supports", "items"]);
        let sup = &out["properties"]["supports"];
        assert_eq!(
            (sup["minItems"].clone(), sup["maxItems"].clone()),
            (json!(2), json!(2))
        );
        assert_eq!(
            sup["items"]["properties"]["heading"]["enum"],
            json!(["Directions", "Sentence Frame for Sharing"])
        );
        // Read back: supports as lines (or one string), no scaffold.
        let v: RawSectionOut = serde_json::from_value(json!({
            "title": "P1", "instructions": "Read.",
            "supports": [
                { "heading": "Directions", "lines": ["Underline it.", " "] },
                { "heading": "Sentence Frame for Sharing", "lines": "Hi! ______\nThanks." },
            ],
            "items": [],
        }))
        .unwrap();
        assert_eq!(v.scaffold, "");
        assert_eq!(v.supports[0].lines, ["Underline it."]);
        assert_eq!(v.supports[1].lines, ["Hi! ______", "Thanks."]);
        // The details schema asks for the boxes before the question patterns.
        let d = details_schema(1);
        let keys: Vec<&String> = d["properties"]["sections"]["items"]["properties"]
            .as_object()
            .unwrap()
            .keys()
            .collect();
        let at = |k: &str| keys.iter().position(|x| *x == k).unwrap();
        assert!(at("supports") < at("item_patterns"));
    }
}
