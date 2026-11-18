//! Structured outputs of the four worksheet steps: what the model returns (`Raw*`) and the JSON
//! schemas sent to it. Derived from research/sop-validation/schemas (steps (c)–(f)); label
//! fields are enums built from the spec's fixed sets, so a strict-schema provider cannot leave
//! them. The `Raw*` structs keep labels as strings so that the checks, not the parser, decide
//! what happens to an output from a provider that does not enforce the schema.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{
    student_tasks, Bloom, Practice, ANSWER_FORMATS, EXIT_PURPOSES, INFO_FUNCTIONS,
    INSTRUCTIONAL_KINDS, PROCEDURAL_KINDS, VERBAL_KINDS,
};

pub const TASKS_SCHEMA_NAME: &str = "worksheet_tasks";
pub const WARMUP_SCHEMA_NAME: &str = "worksheet_warmup";
pub const SCAFFOLDS_SCHEMA_NAME: &str = "worksheet_scaffolds";
pub const EXIT_SCHEMA_NAME: &str = "worksheet_exit_ticket";

pub const TIER_CORE: &str = "core";
pub const TIER_SUPPORT: &str = "support";
pub const TIER_CHALLENGE: &str = "challenge";
pub const TARGET_WARMUP: &str = "warm_up";

// ---------------------------------------------------------------- (c) tasks

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawItem {
    pub item_id: String,
    pub answer_format: String,
    pub tier: String,
    pub prompt_en: String,
    pub options: Vec<String>,
    /// 圖像組織 (prompt version 4): the organizer to draw; `null` for the other formats.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub organizer: Option<super::organizer::RawOrganizer>,
    pub answer_en: String,
    pub evidence: Option<String>,
    /// The item's Bloom level (prompt version 2); checked against the fixed set.
    #[serde(default)]
    pub bloom: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawBlock {
    pub block_id: String,
    pub paragraph_nos: Vec<i64>,
    pub title_en: String,
    pub info_function: String,
    /// SIOP Q4 (prompt version 3): 示範／共同練習／同儕練習／獨立應用; checked by the server.
    #[serde(default)]
    pub practice: Option<String>,
    pub student_tasks: Vec<String>,
    pub objective_refs: Vec<String>,
    pub instructions_en: String,
    pub items: Vec<RawItem>,
    pub design_reason_zh: String,
    /// Pictures the block needs (only asked for while pictures are on).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub image_slots: Vec<super::images::RawImageSlot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawTasks {
    pub blocks: Vec<RawBlock>,
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

fn nullable_string() -> Value {
    json!({ "anyOf": [{ "type": "string" }, { "type": "null" }] })
}

pub fn tasks_schema() -> Value {
    let functions: Vec<&str> = INFO_FUNCTIONS.iter().map(|(_, l)| *l).collect();
    let blooms: Vec<&str> = Bloom::ALL.iter().map(|b| b.as_str()).collect();
    let stages: Vec<&str> = Practice::ALL.iter().map(|p| p.label()).collect();
    json!({
        "type": "object",
        "properties": {
            "blocks": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "block_id": { "type": "string" },
                        "paragraph_nos": { "type": "array", "items": { "type": "integer" } },
                        "title_en": { "type": "string" },
                        "info_function": enum_of(&functions),
                        "practice": enum_of(&stages),
                        "student_tasks": enum_array(&student_tasks()),
                        "objective_refs": strings(),
                        "instructions_en": { "type": "string" },
                        "items": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "item_id": { "type": "string" },
                                    "answer_format": enum_of(&ANSWER_FORMATS),
                                    "tier": enum_of(&[TIER_CORE, TIER_SUPPORT, TIER_CHALLENGE]),
                                    "prompt_en": { "type": "string" },
                                    "options": strings(),
                                    "organizer": super::organizer::schema(false),
                                    "answer_en": { "type": "string" },
                                    "evidence": nullable_string(),
                                    "bloom": enum_of(&blooms)
                                },
                                "required": ["item_id", "answer_format", "tier", "prompt_en",
                                             "options", "organizer", "answer_en", "evidence",
                                             "bloom"],
                                "additionalProperties": false
                            }
                        },
                        "design_reason_zh": { "type": "string" }
                    },
                    "required": ["block_id", "paragraph_nos", "title_en", "info_function",
                                 "practice", "student_tasks", "objective_refs", "instructions_en",
                                 "items", "design_reason_zh"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["blocks"],
        "additionalProperties": false
    })
}

// ---------------------------------------------------------------- (d) warm-up

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawBackground {
    pub title_en: String,
    pub instructions_en: String,
    pub prompts_en: Vec<String>,
    pub student_tasks: Vec<String>,
    pub answer_format: String,
    pub purpose_zh: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawKeyWord {
    pub word: String,
    pub paragraph_no: i64,
    pub context_sentence: String,
    pub student_friendly_definition_en: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawWarmup {
    pub background_activities: Vec<RawBackground>,
    pub key_vocabulary: Vec<RawKeyWord>,
}

pub fn warmup_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "background_activities": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "title_en": { "type": "string" },
                        "instructions_en": { "type": "string" },
                        "prompts_en": strings(),
                        "student_tasks": enum_array(&student_tasks()),
                        "answer_format": enum_of(&ANSWER_FORMATS),
                        "purpose_zh": { "type": "string" }
                    },
                    "required": ["title_en", "instructions_en", "prompts_en", "student_tasks",
                                 "answer_format", "purpose_zh"],
                    "additionalProperties": false
                }
            },
            "key_vocabulary": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "word": { "type": "string" },
                        "paragraph_no": { "type": "integer" },
                        "context_sentence": { "type": "string" },
                        "student_friendly_definition_en": { "type": "string" }
                    },
                    "required": ["word", "paragraph_no", "context_sentence",
                                 "student_friendly_definition_en"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["background_activities", "key_vocabulary"],
        "additionalProperties": false
    })
}

// ---------------------------------------------------------------- (e) scaffolds

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawSupport {
    pub kind: String,
    pub content_en: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawProcedure {
    pub kind: String,
    pub steps_en: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawScaffoldSet {
    pub target: String,
    pub verbal: Vec<RawSupport>,
    pub instructional: Vec<RawSupport>,
    pub procedural: Vec<RawProcedure>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawScaffolds {
    pub scaffolds: Vec<RawScaffoldSet>,
}

pub fn scaffolds_schema() -> Value {
    let support = |kinds: &[&str]| {
        json!({
            "type": "array",
            "items": {
                "type": "object",
                "properties": { "kind": enum_of(kinds), "content_en": { "type": "string" } },
                "required": ["kind", "content_en"],
                "additionalProperties": false
            }
        })
    };
    json!({
        "type": "object",
        "properties": {
            "scaffolds": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "target": { "type": "string" },
                        "verbal": support(&VERBAL_KINDS),
                        "instructional": support(&INSTRUCTIONAL_KINDS),
                        "procedural": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "kind": enum_of(&PROCEDURAL_KINDS),
                                    "steps_en": strings()
                                },
                                "required": ["kind", "steps_en"],
                                "additionalProperties": false
                            }
                        }
                    },
                    "required": ["target", "verbal", "instructional", "procedural"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["scaffolds"],
        "additionalProperties": false
    })
}

// ---------------------------------------------------------------- (f) exit ticket

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawExitItem {
    pub item_id: String,
    pub purpose: String,
    pub prompt_en: String,
    pub answer_format: String,
    pub options: Vec<String>,
    pub student_tasks: Vec<String>,
    pub objective_refs: Vec<String>,
    pub sample_answer_en: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawExit {
    pub estimated_minutes: i64,
    pub items: Vec<RawExitItem>,
}

pub fn exit_schema() -> Value {
    let purposes: Vec<&str> = EXIT_PURPOSES.iter().map(|(_, l)| *l).collect();
    json!({
        "type": "object",
        "properties": {
            "estimated_minutes": { "type": "integer" },
            "items": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "item_id": { "type": "string" },
                        "purpose": enum_of(&purposes),
                        "prompt_en": { "type": "string" },
                        "answer_format": enum_of(&ANSWER_FORMATS),
                        "options": strings(),
                        "student_tasks": enum_array(&student_tasks()),
                        "objective_refs": strings(),
                        "sample_answer_en": nullable_string()
                    },
                    "required": ["item_id", "purpose", "prompt_en", "answer_format", "options",
                                 "student_tasks", "objective_refs", "sample_answer_en"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["estimated_minutes", "items"],
        "additionalProperties": false
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schemas_only_offer_spec_labels() {
        let s = tasks_schema().to_string();
        for label in super::super::FORBIDDEN_LABELS {
            assert!(!s.contains(&format!("\"{label}\"")), "{label}");
        }
        assert!(s.contains("整理／分析資料") && s.contains("段落寫作") && s.contains("創造資訊"));
        assert!(scaffolds_schema()
            .to_string()
            .contains("清楚的分步指示及完成順序"));
        assert!(exit_schema().to_string().contains("蒐集問題"));
    }

    #[test]
    fn tasks_items_carry_a_nullable_organizer_after_the_options() {
        let s = tasks_schema();
        let item = &s["properties"]["blocks"]["items"]["properties"]["items"]["items"];
        let keys: Vec<&str> = item["properties"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        let at = |k: &str| keys.iter().position(|x| *x == k).unwrap();
        assert_eq!(at("organizer"), at("options") + 1);
        assert!(at("organizer") < at("answer_en"));
        assert!(item["required"]
            .as_array()
            .unwrap()
            .contains(&json!("organizer")));
        assert_eq!(
            item["properties"]["organizer"]["anyOf"][1]["type"],
            json!("null")
        );
    }
}
