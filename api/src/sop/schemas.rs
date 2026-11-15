//! Structured outputs of the two validated SOP stages: what the model returns (`Raw*`, with the
//! JSON schemas sent to it) and what the teacher is shown after the checks (`*Suggestion`).
//! The model schemas are the ones validated in research/sop-validation/schemas.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{CurriculumLink, LinkStatus, ObjectiveType};

pub const ANALYSIS_SCHEMA_NAME: &str = "sop_passage_analysis";
pub const OBJECTIVES_SCHEMA_NAME: &str = "sop_objectives";

pub const SIOP_CONTENT: &str = "內容目標";
pub const SIOP_LANGUAGE: &str = "語言目標";

// ---------------------------------------------------------------- (a) passage analysis

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawFeature {
    pub feature_zh: String,
    pub evidence: String,
    pub explanation_zh: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawParagraph {
    pub paragraph_no: i64,
    pub function_zh: String,
    pub summary_zh: String,
    pub writing_features: Vec<RawFeature>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawFocus {
    pub kind: String,
    pub item: String,
    pub paragraph_no: i64,
    pub evidence: String,
    pub note_zh: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawAnalysis {
    pub text_type: String,
    pub main_idea_zh: String,
    pub structure_zh: String,
    pub paragraphs: Vec<RawParagraph>,
    pub language_focus: Vec<RawFocus>,
}

pub fn analysis_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "text_type": { "type": "string",
                           "enum": ["記敘文", "描寫文", "說明文", "論說文", "討論文", "其他"] },
            "main_idea_zh": { "type": "string" },
            "structure_zh": { "type": "string" },
            "paragraphs": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "paragraph_no": { "type": "integer" },
                        "function_zh": { "type": "string" },
                        "summary_zh": { "type": "string" },
                        "writing_features": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "feature_zh": { "type": "string" },
                                    "evidence": { "type": "string" },
                                    "explanation_zh": { "type": "string" }
                                },
                                "required": ["feature_zh", "evidence", "explanation_zh"],
                                "additionalProperties": false
                            }
                        }
                    },
                    "required": ["paragraph_no", "function_zh", "summary_zh", "writing_features"],
                    "additionalProperties": false
                }
            },
            "language_focus": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "kind": { "type": "string", "enum": ["字彙", "句型", "篇章標記"] },
                        "item": { "type": "string" },
                        "paragraph_no": { "type": "integer" },
                        "evidence": { "type": "string" },
                        "note_zh": { "type": "string" }
                    },
                    "required": ["kind", "item", "paragraph_no", "evidence", "note_zh"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["text_type", "main_idea_zh", "structure_zh", "paragraphs", "language_focus"],
        "additionalProperties": false
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FeatureSuggestion {
    pub feature: String,
    pub evidence: String,
    pub explanation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ParagraphSuggestion {
    pub paragraph_id: String,
    pub paragraph_no: i32,
    pub function: String,
    pub summary: String,
    pub features: Vec<FeatureSuggestion>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FocusSuggestion {
    pub kind: String,
    pub item: String,
    pub paragraph_id: String,
    pub evidence: String,
    pub note: String,
}

/// What stage 2 shows: only paragraphs the model actually returned and only evidence that is
/// a verbatim piece of its paragraph.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct AnalysisSuggestion {
    pub text_type: String,
    pub main_idea: String,
    pub structure: String,
    pub language_focus: Vec<FocusSuggestion>,
    pub paragraphs: Vec<ParagraphSuggestion>,
    pub missing_paragraph_ids: Vec<String>,
    pub dropped_evidence: usize,
}

// ---------------------------------------------------------------- (b) objectives

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawLink {
    pub item_code: String,
    pub why_zh: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawObjective {
    pub objective_id: String,
    pub siop_type: String,
    pub objective_zh: String,
    pub student_facing_en: String,
    pub reason_zh: String,
    pub paragraph_refs: Vec<i64>,
    pub link_status: String,
    pub curriculum_links: Vec<RawLink>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawObjectives {
    pub objectives: Vec<RawObjective>,
}

pub fn objectives_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "objectives": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "objective_id": { "type": "string" },
                        "siop_type": { "type": "string", "enum": [SIOP_CONTENT, SIOP_LANGUAGE] },
                        "objective_zh": { "type": "string" },
                        "student_facing_en": { "type": "string" },
                        "reason_zh": { "type": "string" },
                        "paragraph_refs": { "type": "array", "items": { "type": "integer" } },
                        "link_status": { "type": "string", "enum": ["linked", "unlinked"] },
                        "curriculum_links": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "item_code": { "type": "string" },
                                    "why_zh": { "type": "string" }
                                },
                                "required": ["item_code", "why_zh"],
                                "additionalProperties": false
                            }
                        }
                    },
                    "required": ["objective_id", "siop_type", "objective_zh", "student_facing_en",
                                 "reason_zh", "paragraph_refs", "link_status", "curriculum_links"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["objectives"],
        "additionalProperties": false
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ObjectiveSuggestion {
    #[serde(rename = "type")]
    pub kind: ObjectiveType,
    pub text: String,
    pub student_facing_en: String,
    pub reason: String,
    pub paragraph_ids: Vec<String>,
    pub link_status: LinkStatus,
    /// Official text and source come from the database by code.
    pub links: Vec<CurriculumLink>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ObjectivesSuggestion {
    pub objectives: Vec<ObjectiveSuggestion>,
    /// How many curriculum items the model could choose from (0: none in the knowledge base).
    pub curriculum_available: usize,
}
