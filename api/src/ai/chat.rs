//! Tool calling for the prep chat (docs/PREP_CHAT.md): messages with tool calls and tool results,
//! and the OpenAI chat-completions request and response for them. Only OpenAI's own API (and the
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
    /// Structured inputs only the fake provider reads.
    pub fake_context: Value,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    /// `usage.prompt_tokens_details.cached_tokens`: the prompt tokens served from OpenAI's
    /// automatic prompt cache.
    pub cached_tokens: u64,
}

impl Usage {
    pub fn to_json(self) -> Value {
        json!({
            "prompt_tokens": self.prompt_tokens,
            "completion_tokens": self.completion_tokens,
            "cached_tokens": self.cached_tokens,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChatResponse {
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub model_version: String,
    pub usage: Option<Usage>,
}

fn user_content(text: &str, images: &[LlmImage]) -> Value {
    if images.is_empty() {
        return json!(text);
    }
    let mut parts = vec![json!({ "type": "text", "text": text })];
    parts.extend(
        images
            .iter()
            .map(|i| json!({ "type": "image_url", "image_url": { "url": i.data_url() } })),
    );
    Value::Array(parts)
}

fn message_json(m: &ChatMessage) -> Value {
    match m {
        ChatMessage::System(t) => json!({ "role": "system", "content": t }),
        ChatMessage::User { text, images } => {
            json!({ "role": "user", "content": user_content(text, images) })
        }
        ChatMessage::Assistant {
            content,
            tool_calls,
        } => {
            let mut v = json!({ "role": "assistant", "content": content });
            if !tool_calls.is_empty() {
                v["tool_calls"] = json!(tool_calls
                    .iter()
                    .map(|c| json!({
                        "id": c.id, "type": "function",
                        "function": { "name": c.name, "arguments": c.arguments }
                    }))
                    .collect::<Vec<_>>());
            }
            v
        }
        ChatMessage::Tool { call_id, content } => {
            json!({ "role": "tool", "tool_call_id": call_id, "content": content })
        }
    }
}

/// The request body for OpenAI's own API: `tools` (strict function schemas), `tool_choice`,
/// `parallel_tool_calls: false`, a strict `json_schema` for the final message, the reasoning
/// effort and a token limit that leaves room for the reasoning.
///
/// With tools the effort is `none`: OpenAI refuses function tools together with a reasoning
/// effort on `/v1/chat/completions` for the GPT-5.6 models (HTTP 400 "Function tools with
/// reasoning_effort are not supported … use /v1/responses or set reasoning_effort to 'none'",
/// first real turn on 2026-11-25). Moving this path to the Responses API would allow both.
pub fn build_body(model: &str, effort: &str, req: &ChatRequest) -> Value {
    let mut body = json!({
        "model": model,
        "messages": req.messages.iter().map(message_json).collect::<Vec<_>>(),
        "response_format": {
            "type": "json_schema",
            "json_schema": {
                "name": req.answer_schema_name, "schema": req.answer_schema, "strict": true
            }
        },
        "reasoning_effort": if req.tools.is_empty() {
            super::openai::reasoning_effort(effort)
        } else {
            "none"
        },
        "max_completion_tokens": req.max_tokens.saturating_add(super::openai::REASONING_ROOM),
    });
    if !req.tools.is_empty() {
        body["tools"] = json!(req
            .tools
            .iter()
            .map(|t| json!({
                "type": "function",
                "function": {
                    "name": t.name, "description": t.description,
                    "parameters": t.parameters, "strict": true
                }
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
    let choice = &v["choices"][0];
    let message = &choice["message"];
    if let Some(refusal) = message["refusal"].as_str() {
        return Err(ProviderError::Refusal(clip(refusal, 500)));
    }
    let tool_calls: Vec<ToolCall> = message["tool_calls"]
        .as_array()
        .map(|calls| {
            calls
                .iter()
                .map(|c| ToolCall {
                    id: c["id"].as_str().unwrap_or_default().to_string(),
                    name: c["function"]["name"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                    arguments: c["function"]["arguments"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                })
                .collect()
        })
        .unwrap_or_default();
    match choice["finish_reason"].as_str() {
        Some("length") => {
            return Err(ProviderError::MaxTokens {
                partial: message["content"].as_str().unwrap_or_default().to_string(),
            })
        }
        Some("content_filter") => return Err(ProviderError::Refusal("content_filter".into())),
        _ => {}
    }
    let content = message["content"]
        .as_str()
        .map(|c| c.replace('\0', ""))
        .filter(|c| !c.trim().is_empty());
    if content.is_none() && tool_calls.is_empty() {
        return Err(ProviderError::InvalidOutput("no message content".into()));
    }
    let usage = v["usage"].is_object().then(|| Usage {
        prompt_tokens: v["usage"]["prompt_tokens"].as_u64().unwrap_or(0),
        completion_tokens: v["usage"]["completion_tokens"].as_u64().unwrap_or(0),
        cached_tokens: v["usage"]["prompt_tokens_details"]["cached_tokens"]
            .as_u64()
            .unwrap_or(0),
    });
    Ok(ChatResponse {
        content,
        tool_calls,
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
        .post(super::openai::endpoint(super::openai::OPENAI_BASE_URL))
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

    fn request() -> ChatRequest {
        ChatRequest {
            messages: vec![
                ChatMessage::System("s".into()),
                ChatMessage::User {
                    text: "u".into(),
                    images: vec![],
                },
                ChatMessage::Assistant {
                    content: None,
                    tool_calls: vec![ToolCall {
                        id: "c1".into(),
                        name: "read_block".into(),
                        arguments: "{\"block_id\":\"b1\"}".into(),
                    }],
                },
                ChatMessage::Tool {
                    call_id: "c1".into(),
                    content: "ok".into(),
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
            fake_context: Value::Null,
        }
    }

    #[test]
    fn the_body_has_tools_tool_choice_and_tool_messages() {
        let b = build_body("gpt", "medium", &request());
        assert_eq!(b["tool_choice"], "auto");
        assert_eq!(b["parallel_tool_calls"], false);
        assert_eq!(b["tools"][0]["type"], "function");
        assert_eq!(b["tools"][0]["function"]["strict"], true);
        assert_eq!(b["response_format"]["json_schema"]["strict"], true);
        assert_eq!(b["messages"][2]["role"], "assistant");
        assert_eq!(b["messages"][2]["content"], Value::Null);
        assert_eq!(b["messages"][2]["tool_calls"][0]["id"], "c1");
        assert_eq!(
            b["messages"][2]["tool_calls"][0]["function"]["arguments"],
            "{\"block_id\":\"b1\"}"
        );
        assert_eq!(
            b["messages"][3],
            json!({"role": "tool", "tool_call_id": "c1", "content": "ok"})
        );
        // OpenAI refuses function tools with a reasoning effort on chat completions.
        assert_eq!(b["reasoning_effort"], "none");
        let mut last = request();
        last.tool_choice = ToolChoice::None;
        assert_eq!(build_body("gpt", "low", &last)["tool_choice"], "none");
        assert_eq!(build_body("gpt", "low", &last)["reasoning_effort"], "none");
        last.tools.clear();
        let plain = build_body("gpt", "low", &last);
        assert!(plain.get("tools").is_none());
        assert_eq!(plain["reasoning_effort"], "low");
    }

    #[test]
    fn tool_calls_and_cached_tokens_are_parsed() {
        let body = json!({
            "model": "gpt-x",
            "choices": [{"finish_reason": "tool_calls", "message": {"role": "assistant",
                "content": null, "tool_calls": [{"id": "c9", "type": "function",
                "function": {"name": "edit_block", "arguments": "{\"block_id\":\"b2\"}"}}]}}],
            "usage": {"prompt_tokens": 2000, "completion_tokens": 30,
                      "prompt_tokens_details": {"cached_tokens": 1536}}
        })
        .to_string();
        let r = parse_response(200, &body).unwrap();
        assert_eq!(r.tool_calls[0].name, "edit_block");
        assert_eq!(r.content, None);
        assert_eq!(r.usage.unwrap().cached_tokens, 1536);
        let text = json!({"model": "m", "choices": [{"finish_reason": "stop",
            "message": {"content": "{\"paragraphs\":[\"hi\"]}"}}]})
        .to_string();
        let r = parse_response(200, &text).unwrap();
        assert!(r.tool_calls.is_empty() && r.usage.is_none());
        assert!(matches!(
            parse_response(500, "x"),
            Err(ProviderError::Http { .. })
        ));
    }
}
