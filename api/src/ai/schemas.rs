//! Structured-output types and the JSON schemas sent to the model.
//!
//! Schemas follow the structured-output rules: every object has `additionalProperties: false`
//! and lists all properties in `required`; no numeric/string constraints (minLength etc.).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CoreAssistOutput {
    pub level: u8,
    pub text: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum IcapLevel {
    Passive,
    Active,
    Constructive,
    Interactive,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PlanSuggestion {
    pub title: String,
    pub body: String,
    pub activity_type: Option<String>,
    pub icap_level: Option<IcapLevel>,
    pub citations: Vec<String>,
    /// Estimated minutes (required when the teacher's P3 includes differentiation).
    #[serde(default)]
    pub duration_min: Option<i32>,
    /// Support for weaker students.
    #[serde(default)]
    pub support_note: Option<String>,
    /// Extension for stronger students.
    #[serde(default)]
    pub extension_note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PlanSuggestOutput {
    pub kb_empty: bool,
    pub suggestions: Vec<PlanSuggestion>,
}

/// 教材優先: one candidate learning focus drawn from the teacher's pasted material.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MaterialCandidate {
    pub big_idea: String,
    pub rationale: String,
    /// Must be a verbatim passage of the stored material (checked).
    pub excerpt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MaterialFocusesOutput {
    pub candidates: Vec<MaterialCandidate>,
}

pub const CORE_ASSIST_SCHEMA_NAME: &str = "core_assist";
pub const PLAN_SUGGEST_SCHEMA_NAME: &str = "plan_suggestions";
pub const MATERIAL_FOCUSES_SCHEMA_NAME: &str = "material_focuses";

pub fn core_assist_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "level": { "type": "integer", "enum": [1, 2, 3] },
            "text": { "type": "string" }
        },
        "required": ["level", "text"],
        "additionalProperties": false
    })
}

pub fn plan_suggest_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "kb_empty": { "type": "boolean" },
            "suggestions": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string" },
                        "body": { "type": "string" },
                        "activity_type": { "anyOf": [{ "type": "string" }, { "type": "null" }] },
                        "icap_level": {
                            "anyOf": [
                                { "type": "string", "enum": ["passive", "active", "constructive", "interactive"] },
                                { "type": "null" }
                            ]
                        },
                        "citations": { "type": "array", "items": { "type": "string" } },
                        "duration_min": { "anyOf": [{ "type": "integer" }, { "type": "null" }] },
                        "support_note": { "anyOf": [{ "type": "string" }, { "type": "null" }] },
                        "extension_note": { "anyOf": [{ "type": "string" }, { "type": "null" }] }
                    },
                    "required": ["title", "body", "activity_type", "icap_level", "citations",
                                 "duration_min", "support_note", "extension_note"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["kb_empty", "suggestions"],
        "additionalProperties": false
    })
}

pub fn material_focuses_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "candidates": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "big_idea": { "type": "string" },
                        "rationale": { "type": "string" },
                        "excerpt": { "type": "string" }
                    },
                    "required": ["big_idea", "rationale", "excerpt"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["candidates"],
        "additionalProperties": false
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every object node must close its properties and require all of them.
    fn assert_strict(v: &Value, path: &str) {
        match v {
            Value::Object(m) => {
                if m.get("type") == Some(&json!("object")) {
                    assert_eq!(m.get("additionalProperties"), Some(&json!(false)), "{path}");
                    let props: Vec<&String> = m["properties"].as_object().unwrap().keys().collect();
                    let req: Vec<&str> = m["required"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|r| r.as_str().unwrap())
                        .collect();
                    for p in props {
                        assert!(req.contains(&p.as_str()), "{path}.{p} not required");
                    }
                }
                for forbidden in ["minLength", "maxLength", "minimum", "maximum", "minItems"] {
                    assert!(!m.contains_key(forbidden), "{path} uses {forbidden}");
                }
                for (k, child) in m {
                    assert_strict(child, &format!("{path}.{k}"));
                }
            }
            Value::Array(a) => a.iter().for_each(|c| assert_strict(c, path)),
            _ => {}
        }
    }

    #[test]
    fn schemas_are_strict() {
        assert_strict(&core_assist_schema(), "core");
        assert_strict(&plan_suggest_schema(), "plan");
        assert_strict(&material_focuses_schema(), "material");
    }

    #[test]
    fn plan_schema_allows_null_for_differentiation_fields() {
        let s = plan_suggest_schema();
        let props = &s["properties"]["suggestions"]["items"]["properties"];
        for f in ["duration_min", "support_note", "extension_note"] {
            let any = props[f]["anyOf"].as_array().unwrap();
            assert!(any.contains(&json!({ "type": "null" })), "{f}");
        }
        assert_eq!(props["duration_min"]["anyOf"][0]["type"], "integer");
    }

    #[test]
    fn material_output_roundtrip() {
        let v = json!({ "candidates": [{ "big_idea": "倒裝句", "rationale": "r", "excerpt": "Not only" }] });
        let o: MaterialFocusesOutput = serde_json::from_value(v.clone()).unwrap();
        assert_eq!(serde_json::to_value(&o).unwrap(), v);
        let extra = json!({ "candidates": [{ "big_idea": "a", "rationale": "r", "excerpt": "e", "page": 1 }] });
        assert!(serde_json::from_value::<MaterialFocusesOutput>(extra).is_err());
        let missing = json!({ "candidates": [{ "big_idea": "a", "rationale": "r" }] });
        assert!(serde_json::from_value::<MaterialFocusesOutput>(missing).is_err());
    }

    #[test]
    fn plan_output_roundtrip() {
        let v = json!({
            "kb_empty": false,
            "suggestions": [{
                "title": "t", "body": "b", "activity_type": null,
                "icap_level": "constructive", "citations": ["X-1"],
                "duration_min": 15, "support_note": "s", "extension_note": null
            }]
        });
        let o: PlanSuggestOutput = serde_json::from_value(v.clone()).unwrap();
        assert_eq!(o.suggestions[0].icap_level, Some(IcapLevel::Constructive));
        assert_eq!(o.suggestions[0].duration_min, Some(15));
        assert_eq!(serde_json::to_value(&o).unwrap(), v);
        // v1-shaped output (no differentiation fields) still parses, as nulls.
        let v1 = json!({"kb_empty": false, "suggestions": [{"title": "t", "body": "b",
            "activity_type": null, "icap_level": null, "citations": ["X-1"]}]});
        let o: PlanSuggestOutput = serde_json::from_value(v1).unwrap();
        assert_eq!(o.suggestions[0].support_note, None);
        let bad = json!({"kb_empty": false, "suggestions": [{"title": "t", "body": "b",
            "activity_type": null, "icap_level": "deep", "citations": []}]});
        assert!(serde_json::from_value::<PlanSuggestOutput>(bad).is_err());
    }
}
