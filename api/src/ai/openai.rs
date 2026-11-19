//! OpenAI-compatible chat completions: the NCHC / 國網中心 model, and OpenAI's own API
//! (`AI_PROVIDER=openai`, GPT-5 and later).

use serde_json::{json, Value};

use super::{clip, LlmRequest, LlmResponse, ProviderError};

/// OpenAI's own API. The text model's address is fixed: only the model and key are settings.
pub const OPENAI_BASE_URL: &str = "https://api.openai.com/v1";

/// Room added to an OpenAI reasoning model's token limit: `max_completion_tokens` counts the
/// reasoning as well as the answer, and a limit sized for the answer alone ends with nothing.
pub const REASONING_ROOM: u32 = 16_000;

/// How a reasoning model is asked: `Some(effort)` for OpenAI's own API (`AI_EFFORT`, mapped to
/// what it takes), `None` for an OpenAI-compatible server (NCHC), which takes `max_tokens` and a
/// temperature.
pub fn reasoning_effort(effort: &str) -> &'static str {
    match effort {
        "low" => "low",
        "medium" => "medium",
        _ => "high",
    }
}

pub fn build_body(model: &str, req: &LlmRequest) -> Value {
    build_body_for(model, req, None)
}

pub fn build_body_for(model: &str, req: &LlmRequest, reasoning: Option<&str>) -> Value {
    let mut body = json!({
        "model": model,
        "messages": [
            { "role": "system", "content": req.system },
            { "role": "user", "content": req.user }
        ],
        "response_format": {
            "type": "json_schema",
            "json_schema": { "name": req.schema_name, "schema": req.schema, "strict": true }
        }
    });
    match reasoning {
        Some(effort) => {
            // GPT-5 and later: their own token field (reasoning included), their own sampling.
            body["reasoning_effort"] = json!(reasoning_effort(effort));
            if let Some(max_tokens) = req.max_tokens {
                body["max_completion_tokens"] = json!(max_tokens.saturating_add(REASONING_ROOM));
            }
        }
        None => {
            if let Some(max_tokens) = req.max_tokens {
                body["max_tokens"] = json!(max_tokens);
            }
            if let Some(temperature) = req.temperature {
                body["temperature"] = json!(temperature);
            }
        }
    }
    body
}

pub fn endpoint(base_url: &str) -> String {
    format!("{}/chat/completions", base_url.trim_end_matches('/'))
}

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
    let choice = &v["choices"][0];
    if let Some(refusal) = choice["message"]["refusal"].as_str() {
        return Err(ProviderError::Refusal(clip(refusal, 500)));
    }
    match choice["finish_reason"].as_str() {
        Some("length") => {
            let partial = choice["message"]["content"].as_str().unwrap_or_default();
            return Err(ProviderError::MaxTokens {
                partial: partial.to_string(),
            });
        }
        Some("content_filter") => return Err(ProviderError::Refusal("content_filter".into())),
        _ => {}
    }
    let content = choice["message"]["content"]
        .as_str()
        .ok_or_else(|| ProviderError::InvalidOutput("no message content".into()))?;
    let mut json: Value = serde_json::from_str(content.trim()).map_err(|e| {
        ProviderError::InvalidOutput(format!(
            "output is not valid JSON: {e}; raw: {}",
            clip(content, 4000)
        ))
    })?;
    // A NUL character (`\u0000`) is valid JSON but PostgreSQL refuses it in JSONB and text, which
    // failed a whole worksheet run (gpt-5.6-sol, 2026-11-19). It never carries meaning: drop it.
    super::strip_nul(&mut json);
    Ok(LlmResponse {
        json,
        model_version,
    })
}

pub async fn call(
    http: &reqwest::Client,
    base_url: &str,
    api_key: Option<&str>,
    model: &str,
    reasoning: Option<&str>,
    req: &LlmRequest,
) -> Result<LlmResponse, ProviderError> {
    let mut rb = http
        .post(endpoint(base_url))
        .json(&build_body_for(model, req, reasoning));
    if let Some(key) = api_key {
        rb = rb.bearer_auth(key);
    }
    let resp = rb.send().await.map_err(ProviderError::from_reqwest)?;
    let status = resp.status().as_u16();
    let body = resp.text().await.map_err(ProviderError::from_reqwest)?;
    parse_response(status, &body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_uses_strict_json_schema() {
        let req = LlmRequest {
            system: "s".into(),
            user: "u".into(),
            schema_name: "plan_suggestions",
            schema: json!({"type": "object"}),
            fake_context: json!({}),
            attempt: 1,
            max_tokens: None,
            temperature: None,
        };
        let b = build_body("nchc-model", &req);
        assert_eq!(b["messages"][0]["role"], "system");
        assert_eq!(b["messages"][1]["content"], "u");
        assert_eq!(b["response_format"]["type"], "json_schema");
        assert_eq!(
            b["response_format"]["json_schema"]["name"],
            "plan_suggestions"
        );
        assert_eq!(b["response_format"]["json_schema"]["strict"], true);
        assert_eq!(endpoint("http://h/v1/"), "http://h/v1/chat/completions");
        // Neither is sent unless the caller sets it.
        assert!(b.get("max_tokens").is_none());
        assert!(b.get("temperature").is_none());
        let bounded = LlmRequest {
            max_tokens: Some(6000),
            temperature: Some(0.5),
            ..req
        };
        let b = build_body("nchc-model", &bounded);
        assert_eq!(b["max_tokens"], 6000);
        assert_eq!(b["temperature"], 0.5);
    }

    #[test]
    fn parses_message_content() {
        let body = json!({
            "model": "nchc-model-2026",
            "choices": [{"finish_reason": "stop",
                         "message": {"role": "assistant", "content": "{\"level\":2,\"text\":\"面向\"}"}}]
        })
        .to_string();
        let r = parse_response(200, &body).unwrap();
        assert_eq!(r.model_version, "nchc-model-2026");
        assert_eq!(r.json["level"], 2);
    }

    #[test]
    fn length_refusal_and_errors() {
        let length = json!({"model": "m", "choices": [{"finish_reason": "length",
            "message": {"content": "{"}}]})
        .to_string();
        // What was written before the cut is handed back for diagnosis.
        assert_eq!(
            parse_response(200, &length),
            Err(ProviderError::MaxTokens {
                partial: "{".into()
            })
        );
        let refusal = json!({"model": "m", "choices": [{"finish_reason": "stop",
            "message": {"content": null, "refusal": "no"}}]})
        .to_string();
        assert!(matches!(
            parse_response(200, &refusal),
            Err(ProviderError::Refusal(_))
        ));
        assert!(matches!(
            parse_response(503, "busy"),
            Err(ProviderError::Http { status: 503, .. })
        ));
    }
    #[test]
    fn openai_reasoning_models_get_their_own_token_field_and_no_temperature() {
        let req = LlmRequest {
            system: "s".into(),
            user: "u".into(),
            schema_name: "x",
            schema: json!({"type": "object"}),
            fake_context: json!({}),
            attempt: 1,
            max_tokens: Some(4000),
            temperature: Some(0.4),
        };
        let b = build_body_for("gpt-5.6-terra", &req, Some("medium"));
        assert_eq!(b["reasoning_effort"], "medium");
        assert_eq!(b["max_completion_tokens"], 4000 + REASONING_ROOM);
        assert!(b.get("max_tokens").is_none());
        assert!(b.get("temperature").is_none());
        assert_eq!(b["response_format"]["json_schema"]["strict"], true);
        // NCHC is unchanged.
        let n = build_body("gemma", &req);
        assert_eq!(n["max_tokens"], 4000);
        assert_eq!(n["temperature"], json!(0.4f32));
        assert!(n.get("reasoning_effort").is_none());
        // The settings' efforts map onto what OpenAI takes.
        assert_eq!(reasoning_effort("low"), "low");
        assert_eq!(reasoning_effort("xhigh"), "high");
        assert_eq!(reasoning_effort("max"), "high");
    }

    #[test]
    fn nul_characters_are_dropped_from_the_output() {
        let body = json!({
            "model": "gpt-5.6-sol",
            "choices": [{"finish_reason": "stop", "message": {"content": "{\"items\":[{\"prompt\":\"a\\u0000b\",\"k\\u0000\":[\"\\u0000\"]}]}"}}]
        })
        .to_string();
        let r = parse_response(200, &body).unwrap();
        assert_eq!(r.json, json!({"items": [{"prompt": "ab", "k": [""]}]}));
    }
}
