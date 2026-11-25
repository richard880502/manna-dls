//! Tool calling for the prep chat (docs/PREP_CHAT.md): messages with tool calls and tool results,
//! and the OpenAI Responses API request and response for them. Only OpenAI's own API (and the
//! fake provider) supports it; every other provider answers with an error. The strict
//! `json_schema` calls every other feature makes are in [`super::openai`] and are not touched.

use serde_json::{json, Value};

use super::{clip, LlmImage, ProviderError};

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// The arguments as the model wrote them (a JSON text; may be malformed).
    pub arguments: String,
}

#[derive(Debug, Clone)]
pub enum ChatMessage {
    System(String),
    User {
        text: String,
        images: Vec<LlmImage>,
    },
    Assistant {
        content: Option<String>,
        tool_calls: Vec<ToolCall>,
        /// The output items the model returned for this step (reasoning, function calls), kept
        /// opaque and sent back unchanged. Empty: spelled out from the two fields above.
        replay: Vec<Value>,
    },
    Tool {
        call_id: String,
        content: String,
    },
}

impl ChatMessage {
    pub fn role(&self) -> &'static str {
        match self {
            Self::System(_) => "system",
            Self::User { .. } => "user",
            Self::Assistant { .. } => "assistant",
            Self::Tool { .. } => "tool",
        }
    }

    /// Characters of the text the message carries (for the fake provider's token estimate).
    pub fn chars(&self) -> usize {
        match self {
            Self::System(t) => t.chars().count(),
            Self::User { text, .. } => text.chars().count(),
            Self::Assistant {
                content,
                tool_calls,
                ..
            } => {
                content.as_deref().map_or(0, |c| c.chars().count())
                    + tool_calls
                        .iter()
                        .map(|c| c.name.len() + c.arguments.chars().count())
                        .sum::<usize>()
            }
            Self::Tool { content, .. } => content.chars().count(),
        }
    }
}

/// A function the model may call (strict JSON schema parameters).
#[derive(Debug, Clone)]
pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolChoice {
    Auto,
    /// The model must answer in words (the last call of a turn).
    None,
}

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<ToolSpec>,
    pub tool_choice: ToolChoice,
    /// The schema of the final answer (strict). It is the same on every call of a turn, so the
    /// request prefix stays the same.
    pub answer_schema_name: &'static str,
    pub answer_schema: Value,
    pub max_tokens: u32,
    /// `prompt_cache_key`: a stable, non-identifying string per prep session, so the calls of a
    /// session land on the same cache.
    pub cache_key: Option<String>,
    /// Structured inputs only the fake provider reads.
    pub fake_context: Value,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    /// `usage.input_tokens_details.cached_tokens`: the prompt tokens served from OpenAI's
    /// automatic prompt cache.
    pub cached_tokens: u64,
    /// `usage.output_tokens_details.reasoning_tokens` (already part of `completion_tokens`).
    pub reasoning_tokens: u64,
}

impl Usage {
    pub fn to_json(self) -> Value {
        json!({
            "prompt_tokens": self.prompt_tokens,
            "completion_tokens": self.completion_tokens,
            "cached_tokens": self.cached_tokens,
            "reasoning_tokens": self.reasoning_tokens,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChatResponse {
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    /// The output items to send back with the next request when the model asked for tools.
    pub replay: Vec<Value>,
    pub model_version: String,
    pub usage: Option<Usage>,
}

fn user_content(text: &str, images: &[LlmImage]) -> Value {
    let mut parts = vec![json!({ "type": "input_text", "text": text })];
    parts.extend(
        images
            .iter()
            .map(|i| json!({ "type": "input_image", "image_url": i.data_url() })),
    );
    Value::Array(parts)
}

/// The input items one message becomes. An assistant message that carries the items the model
/// returned (`replay`) sends them back unchanged (reasoning items included: the request is
/// stateless); one without (built by hand) is spelled out from its text and tool calls.
fn message_items(m: &ChatMessage) -> Vec<Value> {
    match m {
        ChatMessage::System(t) => vec![json!({ "role": "developer", "content": t })],
        ChatMessage::User { text, images } => {
            vec![json!({ "role": "user", "content": user_content(text, images) })]
        }
        ChatMessage::Assistant {
            content,
            tool_calls,
            replay,
        } => {
            if !replay.is_empty() {
                return replay.clone();
            }
            let mut items = Vec::new();
            if let Some(text) = content {
                items.push(json!({
                    "role": "assistant",
                    "content": [{ "type": "output_text", "text": text }]
                }));
            }
            items.extend(tool_calls.iter().map(|c| {
                json!({
                    "type": "function_call", "call_id": c.id,
                    "name": c.name, "arguments": c.arguments
                })
            }));
            items
        }
        ChatMessage::Tool { call_id, content } => {
            vec![json!({ "type": "function_call_output", "call_id": call_id, "output": content })]
        }
    }
}

/// The request body for the Responses API: `input` items (instructions, the fixed prefix, the
/// history, this turn: the cache-stable order), `tools` (strict function schemas), `tool_choice`,
/// `parallel_tool_calls: false`, a strict `json_schema` for the final message, the reasoning
/// effort and a token limit that leaves room for the reasoning. `store: false` keeps the
/// teacher's material out of OpenAI's storage; `include` returns the reasoning as encrypted
/// content so it can be sent back with the next request of the turn.
pub fn build_body(model: &str, effort: &str, req: &ChatRequest) -> Value {
    let mut body = json!({
        "model": model,
        "input": req.messages.iter().flat_map(message_items).collect::<Vec<_>>(),
        "text": {
            "format": {
                "type": "json_schema",
                "name": req.answer_schema_name, "schema": req.answer_schema, "strict": true
            }
        },
        "reasoning": { "effort": super::openai::reasoning_effort(effort) },
        "max_output_tokens": req.max_tokens.saturating_add(super::openai::REASONING_ROOM),
        "store": false,
        "include": ["reasoning.encrypted_content"],
    });
    if let Some(key) = &req.cache_key {
        body["prompt_cache_key"] = json!(key);
    }
    if !req.tools.is_empty() {
        body["tools"] = json!(req
            .tools
            .iter()
            .map(|t| json!({
                "type": "function",
                "name": t.name, "description": t.description,
                "parameters": t.parameters, "strict": true
            }))
            .collect::<Vec<_>>());
        body["tool_choice"] = json!(match req.tool_choice {
            ToolChoice::Auto => "auto",
            ToolChoice::None => "none",
        });
        body["parallel_tool_calls"] = json!(false);
    }
    body
}

pub fn parse_response(status: u16, body: &str) -> Result<ChatResponse, ProviderError> {
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
    let output: Vec<Value> = v["output"].as_array().cloned().unwrap_or_default();

    let mut tool_calls = Vec::new();
    let mut text = String::new();
    let mut refusal: Option<String> = None;
    for item in &output {
        match item["type"].as_str() {
            Some("function_call") => tool_calls.push(ToolCall {
                id: item["call_id"].as_str().unwrap_or_default().to_string(),
                name: item["name"].as_str().unwrap_or_default().to_string(),
                arguments: item["arguments"].as_str().unwrap_or_default().to_string(),
            }),
            Some("message") => {
                for part in item["content"].as_array().into_iter().flatten() {
                    match part["type"].as_str() {
                        Some("output_text") => text.push_str(part["text"].as_str().unwrap_or("")),
                        Some("refusal") => {
                            refusal = Some(part["refusal"].as_str().unwrap_or("").to_string())
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    if text.is_empty() {
        // The convenience field some SDKs and proxies add.
        text = v["output_text"].as_str().unwrap_or_default().to_string();
    }
    if let Some(r) = refusal {
        return Err(ProviderError::Refusal(clip(&r, 500)));
    }
    match v["status"].as_str() {
        Some("incomplete") => {
            return Err(match v["incomplete_details"]["reason"].as_str() {
                Some("content_filter") => ProviderError::Refusal("content_filter".into()),
                _ => ProviderError::MaxTokens { partial: text },
            })
        }
        Some("failed") | Some("cancelled") => {
            let msg = v["error"]["message"]
                .as_str()
                .unwrap_or("the response failed");
            return Err(ProviderError::InvalidOutput(clip(msg, 500)));
        }
        _ => {}
    }
    let content = Some(text.replace('\0', "")).filter(|c| !c.trim().is_empty());
    if content.is_none() && tool_calls.is_empty() {
        return Err(ProviderError::InvalidOutput("no message content".into()));
    }
    let usage = v["usage"].is_object().then(|| Usage {
        prompt_tokens: v["usage"]["input_tokens"].as_u64().unwrap_or(0),
        completion_tokens: v["usage"]["output_tokens"].as_u64().unwrap_or(0),
        cached_tokens: v["usage"]["input_tokens_details"]["cached_tokens"]
            .as_u64()
            .unwrap_or(0),
        reasoning_tokens: v["usage"]["output_tokens_details"]["reasoning_tokens"]
            .as_u64()
            .unwrap_or(0),
    });
    // Only a turn that goes on needs the items back; they are sent as returned.
    let replay = if tool_calls.is_empty() {
        Vec::new()
    } else {
        output
    };
    Ok(ChatResponse {
        content,
        tool_calls,
        replay,
        model_version,
        usage,
    })
}

pub async fn call(
    http: &reqwest::Client,
    api_key: Option<&str>,
    model: &str,
    effort: &str,
    req: &ChatRequest,
) -> Result<ChatResponse, ProviderError> {
    let mut rb = http
        .post(format!(
            "{}/responses",
            super::openai::OPENAI_BASE_URL.trim_end_matches('/')
        ))
        .json(&build_body(model, effort, req));
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

    fn reasoning_item() -> Value {
        json!({"type": "reasoning", "id": "rs_1", "summary": [], "encrypted_content": "ENC"})
    }

    fn call_item() -> Value {
        json!({"type": "function_call", "id": "fc_1", "call_id": "c1", "name": "read_block",
               "arguments": "{\"block_id\":\"b1\"}", "status": "completed"})
    }

    fn request() -> ChatRequest {
        ChatRequest {
            messages: vec![
                ChatMessage::System("s".into()),
                ChatMessage::User {
                    text: "prefix".into(),
                    images: vec![],
                },
                ChatMessage::User {
                    text: "u".into(),
                    images: vec![],
                },
            ],
            tools: vec![ToolSpec {
                name: "read_block",
                description: "d",
                parameters: json!({"type": "object"}),
            }],
            tool_choice: ToolChoice::Auto,
            answer_schema_name: "prep_chat",
            answer_schema: json!({"type": "object"}),
            max_tokens: 1000,
            cache_key: Some("prep-chat-7".into()),
            fake_context: Value::Null,
        }
    }

    fn second_request() -> ChatRequest {
        let mut r = request();
        r.messages.push(ChatMessage::Assistant {
            content: None,
            tool_calls: vec![ToolCall {
                id: "c1".into(),
                name: "read_block".into(),
                arguments: "{\"block_id\":\"b1\"}".into(),
            }],
            replay: vec![reasoning_item(), call_item()],
        });
        r.messages.push(ChatMessage::Tool {
            call_id: "c1".into(),
            content: "ok".into(),
        });
        r
    }

    #[test]
    fn the_body_is_a_stateless_responses_request() {
        let b = build_body("gpt", "medium", &request());
        assert!(b.get("messages").is_none() && b.get("response_format").is_none());
        assert_eq!(b["model"], "gpt");
        assert_eq!(b["store"], false);
        assert_eq!(b["include"], json!(["reasoning.encrypted_content"]));
        assert_eq!(b["reasoning"], json!({"effort": "medium"}));
        assert!(b.get("reasoning_effort").is_none());
        assert_eq!(
            b["max_output_tokens"],
            1000 + super::super::openai::REASONING_ROOM
        );
        assert_eq!(b["prompt_cache_key"], "prep-chat-7");
        assert_eq!(b["text"]["format"]["type"], "json_schema");
        assert_eq!(b["text"]["format"]["name"], "prep_chat");
        assert_eq!(b["text"]["format"]["strict"], true);
        assert_eq!(b["text"]["format"]["schema"], json!({"type": "object"}));
        assert_eq!(b["tool_choice"], "auto");
        assert_eq!(b["parallel_tool_calls"], false);
        assert_eq!(
            b["tools"][0],
            json!({"type": "function", "name": "read_block", "description": "d",
                   "parameters": {"type": "object"}, "strict": true})
        );
        // Instructions, then the fixed prefix, then the history and this turn.
        assert_eq!(b["input"][0], json!({"role": "developer", "content": "s"}));
        assert_eq!(b["input"][1]["content"][0]["text"], "prefix");
        assert_eq!(b["input"][2]["content"][0]["text"], "u");
        assert_eq!(b["input"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn effort_follows_the_setting_and_tool_choice_none() {
        for (setting, sent) in [("low", "low"), ("medium", "medium"), ("high", "high")] {
            assert_eq!(
                build_body("g", setting, &request())["reasoning"]["effort"],
                sent
            );
        }
        let mut last = request();
        last.tool_choice = ToolChoice::None;
        let b = build_body("g", "low", &last);
        assert_eq!(b["tool_choice"], "none");
        assert_eq!(b["reasoning"]["effort"], "low");
        last.tools.clear();
        let plain = build_body("g", "low", &last);
        assert!(plain.get("tools").is_none() && plain.get("tool_choice").is_none());
        last.cache_key = None;
        assert!(build_body("g", "low", &last)
            .get("prompt_cache_key")
            .is_none());
    }

    #[test]
    fn images_are_input_image_parts() {
        let mut r = request();
        r.messages.push(ChatMessage::User {
            text: "look".into(),
            images: vec![LlmImage {
                media_type: "image/png",
                data: bytes::Bytes::from_static(&[1, 2, 3]),
            }],
        });
        let b = build_body("g", "low", &r);
        let content = &b["input"][3]["content"];
        assert_eq!(content[0], json!({"type": "input_text", "text": "look"}));
        assert_eq!(content[1]["type"], "input_image");
        assert!(content[1]["image_url"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,"));
    }

    #[test]
    fn the_second_request_replays_reasoning_and_calls_then_the_output() {
        let b = build_body("gpt", "high", &second_request());
        let input = b["input"].as_array().unwrap();
        assert_eq!(input.len(), 6);
        assert_eq!(input[3], reasoning_item());
        assert_eq!(input[4], call_item());
        assert_eq!(
            input[5],
            json!({"type": "function_call_output", "call_id": "c1", "output": "ok"})
        );
        // The first three items (the cached prefix) are the same as in the first request.
        let first = build_body("gpt", "high", &request());
        assert_eq!(
            first["input"].as_array().unwrap()[..],
            b["input"].as_array().unwrap()[..3]
        );
    }

    #[test]
    fn an_assistant_message_without_replay_is_spelled_out() {
        let items = message_items(&ChatMessage::Assistant {
            content: Some("hi".into()),
            tool_calls: vec![ToolCall {
                id: "c2".into(),
                name: "n".into(),
                arguments: "{}".into(),
            }],
            replay: vec![],
        });
        assert_eq!(
            items[0]["content"][0],
            json!({"type": "output_text", "text": "hi"})
        );
        assert_eq!(
            items[1],
            json!({"type": "function_call", "call_id": "c2", "name": "n", "arguments": "{}"})
        );
    }

    #[test]
    fn function_calls_reasoning_and_usage_are_parsed() {
        let body = json!({
            "model": "gpt-x", "status": "completed",
            "output": [reasoning_item(), call_item()],
            "usage": {"input_tokens": 2000, "output_tokens": 130,
                      "input_tokens_details": {"cached_tokens": 1536},
                      "output_tokens_details": {"reasoning_tokens": 100}}
        })
        .to_string();
        let r = parse_response(200, &body).unwrap();
        assert_eq!(r.model_version, "gpt-x");
        assert_eq!(r.content, None);
        assert_eq!(
            r.tool_calls,
            vec![ToolCall {
                id: "c1".into(),
                name: "read_block".into(),
                arguments: "{\"block_id\":\"b1\"}".into()
            }]
        );
        assert_eq!(r.replay, vec![reasoning_item(), call_item()]);
        let u = r.usage.unwrap();
        assert_eq!(
            (
                u.prompt_tokens,
                u.completion_tokens,
                u.cached_tokens,
                u.reasoning_tokens
            ),
            (2000, 130, 1536, 100)
        );
        assert_eq!(
            u.to_json(),
            json!({"prompt_tokens": 2000, "completion_tokens": 130,
                   "cached_tokens": 1536, "reasoning_tokens": 100})
        );
    }

    #[test]
    fn the_final_text_comes_from_message_items() {
        let body = json!({"model": "m", "status": "completed", "output": [
            reasoning_item(),
            {"type": "message", "role": "assistant", "content": [
                {"type": "output_text", "text": "{\"paragraphs\":"},
                {"type": "output_text", "text": "[\"hi\"]}"}]}]})
        .to_string();
        let r = parse_response(200, &body).unwrap();
        assert_eq!(r.content.as_deref(), Some("{\"paragraphs\":[\"hi\"]}"));
        assert!(r.tool_calls.is_empty() && r.replay.is_empty() && r.usage.is_none());
        let conv = json!({"model": "m", "output": [], "output_text": "{}"}).to_string();
        assert_eq!(
            parse_response(200, &conv).unwrap().content.as_deref(),
            Some("{}")
        );
    }

    #[test]
    fn incomplete_refusals_and_errors_map_as_before() {
        let cut = json!({"model": "m", "status": "incomplete",
            "incomplete_details": {"reason": "max_output_tokens"},
            "output": [{"type": "message", "content": [{"type": "output_text", "text": "{\"a"}]}]})
        .to_string();
        match parse_response(200, &cut) {
            Err(ProviderError::MaxTokens { partial }) => assert_eq!(partial, "{\"a"),
            other => panic!("{other:?}"),
        }
        let filtered = json!({"status": "incomplete",
            "incomplete_details": {"reason": "content_filter"}, "output": []})
        .to_string();
        assert!(matches!(
            parse_response(200, &filtered),
            Err(ProviderError::Refusal(_))
        ));
        let refused = json!({"status": "completed", "output": [{"type": "message", "content": [
            {"type": "refusal", "refusal": "no"}]}]})
        .to_string();
        assert!(matches!(
            parse_response(200, &refused),
            Err(ProviderError::Refusal(_))
        ));
        let failed =
            json!({"status": "failed", "error": {"message": "boom"}, "output": []}).to_string();
        assert!(matches!(
            parse_response(200, &failed),
            Err(ProviderError::InvalidOutput(_))
        ));
        let empty = json!({"status": "completed", "output": []}).to_string();
        assert!(matches!(
            parse_response(200, &empty),
            Err(ProviderError::InvalidOutput(_))
        ));
        assert!(matches!(
            parse_response(200, "nope"),
            Err(ProviderError::InvalidOutput(_))
        ));
        assert!(matches!(
            parse_response(504, ""),
            Err(ProviderError::Timeout)
        ));
        assert!(matches!(
            parse_response(408, ""),
            Err(ProviderError::Timeout)
        ));
        match parse_response(400, "{\"error\":{}}") {
            Err(ProviderError::Http { status, body }) => {
                assert_eq!(status, 400);
                assert!(body.contains("error"));
            }
            other => panic!("{other:?}"),
        }
    }
}
