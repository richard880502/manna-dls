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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PlanSuggestOutput {
    pub kb_empty: bool,
    pub suggestions: Vec<PlanSuggestion>,
}

pub const CORE_ASSIST_SCHEMA_NAME: &str = "core_assist";
pub const PLAN_SUGGEST_SCHEMA_NAME: &str = "plan_suggestions";

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
                        "citations": { "type": "array", "items": { "type": "string" } }
                    },
                    "required": ["title", "body", "activity_type", "icap_level", "citations"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["kb_empty", "suggestions"],
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
    }

    #[test]
    fn plan_output_roundtrip() {
        let v = json!({
            "kb_empty": false,
            "suggestions": [{
                "title": "t", "body": "b", "activity_type": null,
                "icap_level": "constructive", "citations": ["X-1"]
            }]
        });
        let o: PlanSuggestOutput = serde_json::from_value(v.clone()).unwrap();
        assert_eq!(o.suggestions[0].icap_level, Some(IcapLevel::Constructive));
        assert_eq!(serde_json::to_value(&o).unwrap(), v);
        let bad = json!({"kb_empty": false, "suggestions": [{"title": "t", "body": "b",
            "activity_type": null, "icap_level": "deep", "citations": []}]});
        assert!(serde_json::from_value::<PlanSuggestOutput>(bad).is_err());
    }
}
