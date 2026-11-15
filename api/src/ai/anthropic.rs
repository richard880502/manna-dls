//! Anthropic Messages API over raw HTTP (there is no official Rust SDK).
//!
//! Demo environment only — `Config` refuses to start with this provider unless `DEMO_MODE=true`.
//!
//! Request rules for the configured model (claude-opus-5-5 by default):
//! * structured output through `output_config.format` (JSON schema); depth through
//!   `output_config.effort`;
//! * no `thinking` field (adaptive thinking is on by default; `disabled`/`budget_tokens` 400);
//! * no `temperature`/`top_p`/`top_k` (400 on this API version), no assistant prefill;
//! * no server-side `fallbacks`: the spec forbids silently switching models, every output must
//!   be attributable to one model version.

use serde_json::{json, Value};

use super::{clip, LlmRequest, LlmResponse, ProviderError};

pub const API_URL: &str = "https://api.anthropic.com/v1/messages";
pub const API_VERSION: &str = "2026-10-21";
pub const MAX_TOKENS: u32 = 16000;

pub fn build_body(model: &str, effort: &str, req: &LlmRequest) -> Value {
    json!({
        "model": model,
        "max_tokens": MAX_TOKENS,
        "system": req.system,
        "messages": [{ "role": "user", "content": req.user }],
        "output_config": {
            "effort": effort,
            "format": { "type": "json_schema", "schema": req.schema }
        }
    })
}

/// Interprets an HTTP response from the Messages API.
pub fn parse_response(status: u16, body: &str) -> Result<LlmResponse, ProviderError> {
    if status == 408 || status == 504 {
        return Err(ProviderError::Timeout);
    }
    if !(200..300).contains(&status) {
        return Err(ProviderError::Http {
            status,
            body: clip(body, 2000),
        });
    }
    let v: Value = serde_json::from_str(body)
        .map_err(|e| ProviderError::InvalidOutput(format!("response is not JSON: {e}")))?;
    let model_version = v["model"].as_str().unwrap_or_default().to_string();

    // stop_reason first: a refusal or truncated output may not match the schema.
    if v["stop_reason"] == "refusal" {
        let detail = v["stop_details"]["category"]
            .as_str()
            .map(|c| format!("category={c}"))
            .unwrap_or_else(|| "refusal".into());
        return Err(ProviderError::Refusal(detail));
    }

    let text: String = v["content"]
        .as_array()
        .map(|blocks| {
            blocks
                .iter()
                .filter(|b| b["type"] == "text")
                .filter_map(|b| b["text"].as_str())
                .collect()
        })
        .unwrap_or_default();
    if v["stop_reason"] == "max_tokens" {
        // Kept for diagnosis only.
        return Err(ProviderError::MaxTokens { partial: text });
    }
    if text.trim().is_empty() {
        return Err(ProviderError::InvalidOutput("no text content".into()));
    }
    let json: Value = serde_json::from_str(text.trim()).map_err(|e| {
        ProviderError::InvalidOutput(format!(
            "output is not valid JSON: {e}; raw: {}",
            clip(&text, 4000)
        ))
    })?;
    Ok(LlmResponse {
        json,
        model_version,
    })
}

pub async fn call(
    http: &reqwest::Client,
    api_key: &str,
    model: &str,
    effort: &str,
    req: &LlmRequest,
) -> Result<LlmResponse, ProviderError> {
    let resp = http
        .post(API_URL)
        .header("x-api-key", api_key)
        .header("anthropic-version", API_VERSION)
        .header("content-type", "application/json")
        .json(&build_body(model, effort, req))
        .send()
        .await
        .map_err(ProviderError::from_reqwest)?;
    let status = resp.status().as_u16();
    let body = resp.text().await.map_err(ProviderError::from_reqwest)?;
    parse_response(status, &body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req() -> LlmRequest {
        LlmRequest {
            system: "sys".into(),
            user: "hello".into(),
            schema_name: "core_assist",
            schema: json!({"type": "object"}),
            fake_context: json!({}),
            attempt: 1,
            max_tokens: None,
            temperature: None,
        }
    }

    #[test]
    fn body_has_structured_output_and_no_forbidden_fields() {
        let b = build_body("claude-opus-5-5", "medium", &req());
        assert_eq!(b["model"], "claude-opus-5-5");
        assert_eq!(b["max_tokens"], 16000);
        assert_eq!(b["system"], "sys");
        assert_eq!(b["messages"], json!([{"role": "user", "content": "hello"}]));
        assert_eq!(b["output_config"]["effort"], "medium");
        assert_eq!(b["output_config"]["format"]["type"], "json_schema");
        assert_eq!(
            b["output_config"]["format"]["schema"],
            json!({"type": "object"})
        );
        for forbidden in ["thinking", "temperature", "top_p", "top_k", "fallbacks"] {
            assert!(b.get(forbidden).is_none(), "{forbidden} must not be sent");
        }
        // single user message, no assistant prefill
        assert_eq!(b["messages"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn parses_text_blocks_and_ignores_thinking() {
        let body = json!({
            "id": "msg_1",
            "type": "message",
            "role": "assistant",
            "model": "claude-opus-5-5",
            "stop_reason": "end_turn",
            "stop_details": null,
            "content": [
                {"type": "thinking", "thinking": "", "signature": "sig"},
                {"type": "text", "text": "{\"level\": 1, "},
                {"type": "text", "text": "\"text\": \"這一題想了解的是…\"}"}
            ],
            "usage": {"input_tokens": 10, "output_tokens": 20}
        })
        .to_string();
        let r = parse_response(200, &body).unwrap();
        assert_eq!(r.model_version, "claude-opus-5-5");
        assert_eq!(r.json, json!({"level": 1, "text": "這一題想了解的是…"}));
    }

    #[test]
    fn refusal_is_reported_before_content() {
        let body = json!({
            "model": "claude-opus-5-5",
            "stop_reason": "refusal",
            "stop_details": {"type": "refusal", "category": "cyber", "explanation": null},
            "content": [{"type": "text", "text": "not json"}]
        })
        .to_string();
        assert_eq!(
            parse_response(200, &body),
            Err(ProviderError::Refusal("category=cyber".into()))
        );
    }

    #[test]
    fn max_tokens_is_an_error() {
        let body = json!({
            "model": "claude-opus-5-5",
            "stop_reason": "max_tokens",
            "content": [{"type": "text", "text": "{\"level\": 1, \"te"}]
        })
        .to_string();
        assert_eq!(
            parse_response(200, &body),
            Err(ProviderError::MaxTokens {
                partial: "{\"level\": 1, \"te".into()
            })
        );
    }

    #[test]
    fn http_errors_and_timeouts() {
        assert_eq!(parse_response(504, ""), Err(ProviderError::Timeout));
        assert!(matches!(
            parse_response(429, "{\"type\":\"error\"}"),
            Err(ProviderError::Http { status: 429, .. })
        ));
        assert!(matches!(
            parse_response(529, "overloaded"),
            Err(ProviderError::Http { status: 529, .. })
        ));
        assert!(matches!(
            parse_response(400, "bad"),
            Err(ProviderError::Http { status: 400, .. })
        ));
    }

    #[test]
    fn invalid_json_output() {
        let body = json!({
            "model": "claude-opus-5-5",
            "stop_reason": "end_turn",
            "content": [{"type": "text", "text": "sorry, here is prose"}]
        })
        .to_string();
        assert!(matches!(
            parse_response(200, &body),
            Err(ProviderError::InvalidOutput(_))
        ));
        let empty = json!({"model": "m", "stop_reason": "end_turn", "content": []}).to_string();
        assert!(matches!(
            parse_response(200, &empty),
            Err(ProviderError::InvalidOutput(_))
        ));
        assert!(matches!(
            parse_response(200, "<html>"),
            Err(ProviderError::InvalidOutput(_))
        ));
    }
}
