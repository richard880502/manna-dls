//! Tool calling for the prep chat (docs/PREP_CHAT.md): messages with tool calls and tool results,
//! and the OpenAI Responses API request and response for them (`POST /v1/responses`: function
//! tools and a reasoning effort together, which chat completions refuses for GPT-5.6). Only
//! OpenAI's own API (and the fake provider, through the same shapes: [`fake_call`]) supports it;
//! every other provider answers with an error. The strict `json_schema` calls every other
//! feature makes stay on chat completions ([`super::openai`]) and are not touched.

use serde_json::{json, Value};

use super::openai::REASONING_ROOM;
use super::{clip, LlmImage, ProviderError};

/// `input_snapshot.request.api` of a tool call: which OpenAI API it went to.
pub const API: &str = "responses";

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
    /// How many times the guard repeated the call (429 or 5xx) before it ended.
    pub transport_retries: u64,
}

impl Usage {
    pub fn to_json(self) -> Value {
        let mut v = json!({
            "prompt_tokens": self.prompt_tokens,
            "completion_tokens": self.completion_tokens,
            "cached_tokens": self.cached_tokens,
            "reasoning_tokens": self.reasoning_tokens,
        });
        if self.transport_retries > 0 {
            v["transport_retries"] = json!(self.transport_retries);
        }
        v
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

/// One returned output item as it goes back in the next request of the turn. With `store: false`
/// OpenAI keeps nothing, so an item must not point at a stored one by its `id`: a function call
/// goes back as its call (`call_id`, `name`, `arguments`), a message as its role and text. A
/// reasoning item goes back whole: its `encrypted_content` is what lets the model continue.
/// Anything else is dropped.
fn replay_item(item: &Value) -> Option<Value> {
    match item["type"].as_str() {
        Some("reasoning") => {
            let mut r = json!({ "type": "reasoning", "summary": item["summary"].clone() });
            if r["summary"].is_null() {
                r["summary"] = json!([]);
            }
            for key in ["id", "encrypted_content"] {
                if let Some(v) = item.get(key).filter(|v| !v.is_null()) {
                    r[key] = v.clone();
                }
            }
            Some(r)
        }
        Some("function_call") => Some(json!({
            "type": "function_call",
            "call_id": item["call_id"],
            "name": item["name"],
            "arguments": item["arguments"],
        })),
        Some("message") => Some(json!({
            "role": "assistant",
            "content": item["content"]
                .as_array()
                .map(|parts| {
                    parts
                        .iter()
                        .filter(|p| p["type"] == "output_text")
                        .map(|p| json!({ "type": "output_text", "text": p["text"] }))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
        })),
        _ => None,
    }
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
                return replay.iter().filter_map(replay_item).collect();
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
        "max_output_tokens": req.max_tokens.saturating_add(REASONING_ROOM),
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
        body["tool_choice"] = json!(req.tool_choice.as_str());
        body["parallel_tool_calls"] = json!(false);
    }
    body
}

impl ToolChoice {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::None => "none",
        }
    }
}

/// What the research log keeps about a request (`ai_interactions.input_snapshot.request`), next
/// to the messages as lengths and hashes: the API, the reasoning effort actually sent, whether
/// OpenAI may keep it, the limits and the tool switches. No text.
pub fn request_record(effort: &str, req: &ChatRequest) -> Value {
    json!({
        "api": API,
        "reasoning_effort": super::openai::reasoning_effort(effort),
        "store": false,
        "max_tokens": req.max_tokens,
        "max_output_tokens": req.max_tokens.saturating_add(REASONING_ROOM),
        "tool_choice": req.tool_choice.as_str(),
        "parallel_tool_calls": false,
    })
}

pub fn endpoint(base_url: &str) -> String {
    format!("{}/responses", base_url.trim_end_matches('/'))
}

pub fn parse_response(status: u16, body: &str) -> Result<ChatResponse, ProviderError> {
    if !(200..300).contains(&status) {
        return Err(super::http_error(status, body, None));
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
        transport_retries: 0,
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

/// One request to `{base_url}/responses` (OpenAI's own address, [`super::openai::OPENAI_BASE_URL`],
/// outside tests). Errors are classified like every other call ([`super::http_error`]: 429 rate
/// limit or quota, 5xx, 408/504 timeouts), so the guard retries and cools down the same way.
pub async fn call(
    http: &reqwest::Client,
    base_url: &str,
    api_key: Option<&str>,
    model: &str,
    effort: &str,
    req: &ChatRequest,
) -> Result<ChatResponse, ProviderError> {
    let mut rb = http
        .post(endpoint(base_url))
        .json(&build_body(model, effort, req));
    if let Some(key) = api_key {
        rb = rb.bearer_auth(key);
    }
    let resp = rb.send().await.map_err(ProviderError::from_reqwest)?;
    let status = resp.status().as_u16();
    let delay = super::retry_after(resp.headers());
    let body = resp.text().await.map_err(ProviderError::from_reqwest)?;
    if !(200..300).contains(&status) {
        return Err(super::http_error(status, &body, delay));
    }
    parse_response(status, &body)
}

// ---------------------------------------------------------------- the fake provider

/// The fake provider's tool call, through the same request and response shapes as [`call`]: the
/// body is built and checked the way OpenAI reads it ([`check_body`]; a body it would refuse is
/// answered with HTTP 400, classified like a real one), the scripted answer is written as a
/// Responses body (a reasoning item first, as a reasoning model returns it) and read back with
/// [`parse_response`]. The tool loop's tests thus send the reasoning and the calls back the way
/// production does, without the network.
pub(crate) fn fake_call(
    model: &str,
    effort: &str,
    req: &ChatRequest,
    script: impl FnOnce(&ChatRequest, &str) -> Result<ChatResponse, ProviderError>,
) -> Result<ChatResponse, ProviderError> {
    if let Err(why) = check_body(&build_body(model, effort, req)) {
        let body = json!({ "error": { "message": why, "type": "invalid_request_error" } });
        return Err(super::http_error(400, &body.to_string(), None));
    }
    let answer = script(req, model)?;
    let step = req
        .messages
        .iter()
        .filter(|m| matches!(m, ChatMessage::Assistant { .. }))
        .count();
    parse_response(200, &fake_wire(&answer, step).to_string())
}

/// What a stateless (`store: false`) request with tools must not contain: an output with no call
/// before it or a call with no output after it (OpenAI answers 400), reasoning sent back without
/// its encrypted content (nothing is stored to look it up), an item that refers to a stored one
/// by `id` (the replay rule of [`replay_item`]: only reasoning keeps its id), a non-strict tool.
pub(crate) fn check_body(body: &Value) -> Result<(), String> {
    if body["store"] != json!(false) {
        return Err("store must be false".into());
    }
    if !body["include"]
        .as_array()
        .is_some_and(|i| i.contains(&json!("reasoning.encrypted_content")))
    {
        return Err("include must ask for reasoning.encrypted_content".into());
    }
    if body["reasoning"]["effort"].as_str().is_none() {
        return Err("no reasoning effort".into());
    }
    for tool in body["tools"].as_array().into_iter().flatten() {
        if tool["type"] != "function" || tool["strict"] != json!(true) {
            return Err(format!("tool {} is not a strict function", tool["name"]));
        }
    }
    let input = body["input"].as_array().cloned().unwrap_or_default();
    let mut calls: Vec<&str> = Vec::new();
    for (k, item) in input.iter().enumerate() {
        let call_id = item["call_id"].as_str().unwrap_or_default();
        match item["type"].as_str() {
            Some("reasoning") => {
                if item["encrypted_content"].as_str().is_none() {
                    return Err(format!(
                        "reasoning item {k} has no encrypted_content (items are not stored)"
                    ));
                }
                continue;
            }
            Some("function_call") => {
                let answered = input[k + 1..].iter().any(|later| {
                    later["type"] == "function_call_output" && later["call_id"] == call_id
                });
                if !answered {
                    return Err(format!("No tool output found for function call {call_id}."));
                }
                calls.push(call_id);
            }
            Some("function_call_output") if !calls.contains(&call_id) => {
                return Err(format!(
                    "No tool call found for function call output with call_id {call_id}."
                ));
            }
            _ => {}
        }
        if item.get("id").is_some() {
            return Err(format!("input item {k} refers to a stored item by id"));
        }
    }
    Ok(())
}

/// A scripted answer as the body `POST /v1/responses` returns.
fn fake_wire(answer: &ChatResponse, step: usize) -> Value {
    let mut output = vec![json!({
        "type": "reasoning", "id": format!("rs_fake_{step}"), "summary": [],
        "encrypted_content": format!("fake-encrypted-reasoning-{step}"),
    })];
    if answer.tool_calls.is_empty() {
        output.push(json!({
            "type": "message", "id": format!("msg_fake_{step}"), "status": "completed",
            "role": "assistant",
            "content": [{ "type": "output_text", "annotations": [],
                          "text": answer.content.clone().unwrap_or_default() }],
        }));
    }
    output.extend(answer.tool_calls.iter().enumerate().map(|(k, c)| {
        json!({
            "type": "function_call", "id": format!("fc_fake_{step}_{k}"), "status": "completed",
            "call_id": c.id, "name": c.name, "arguments": c.arguments,
        })
    }));
    let mut wire = json!({
        "id": format!("resp_fake_{step}"), "object": "response", "status": "completed",
        "model": answer.model_version, "output": output,
    });
    if let Some(u) = answer.usage {
        wire["usage"] = json!({
            "input_tokens": u.prompt_tokens,
            "input_tokens_details": { "cached_tokens": u.cached_tokens },
            "output_tokens": u.completion_tokens,
            "output_tokens_details": { "reasoning_tokens": u.reasoning_tokens },
            "total_tokens": u.prompt_tokens + u.completion_tokens,
        });
    }
    wire
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
        // The reasoning item goes back whole (its encrypted content); the call without the ids
        // of stored items (`store: false`).
        assert_eq!(input[3], reasoning_item());
        assert_eq!(
            input[4],
            json!({"type": "function_call", "call_id": "c1", "name": "read_block",
                   "arguments": "{\"block_id\":\"b1\"}"})
        );
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
    fn replayed_messages_lose_stored_ids_and_unknown_items_are_dropped() {
        let msg = json!({"type": "message", "id": "msg_1", "status": "completed", "role": "assistant",
                         "content": [{"type": "output_text", "text": "hi", "annotations": []}]});
        assert_eq!(
            replay_item(&msg),
            Some(json!({"role": "assistant", "content": [{"type": "output_text", "text": "hi"}]}))
        );
        assert_eq!(
            replay_item(&json!({"type": "web_search_call", "id": "ws_1"})),
            None
        );
        let bare = json!({"type": "reasoning", "id": "rs_2"});
        assert_eq!(
            replay_item(&bare),
            Some(json!({"type": "reasoning", "id": "rs_2", "summary": []}))
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

    #[test]
    fn a_400_and_the_two_kinds_of_429_are_classified_like_every_other_call() {
        // What OpenAI answers a request it will not take (the 2026-11-25 chat completions error).
        let refused = json!({"error": {
            "message": "Function tools with reasoning_effort are not supported for gpt-5.6-sol in /v1/chat/completions. Please use /v1/responses instead.",
            "type": "invalid_request_error", "param": "reasoning_effort", "code": null}})
        .to_string();
        match parse_response(400, &refused) {
            Err(ProviderError::Http { status: 400, body }) => {
                assert!(body.contains("/v1/responses"))
            }
            other => panic!("{other:?}"),
        }
        let rate = json!({"error": {"message": "Rate limit reached for gpt-5.6-sol on tokens per min (TPM)",
            "type": "tokens", "param": null, "code": "rate_limit_exceeded"}})
        .to_string();
        assert!(matches!(
            parse_response(429, &rate),
            Err(ProviderError::RateLimited { .. })
        ));
        let quota = json!({"error": {"message": "You exceeded your current quota",
            "type": "insufficient_quota", "param": null, "code": "insufficient_quota"}})
        .to_string();
        assert!(matches!(
            parse_response(429, &quota),
            Err(ProviderError::QuotaExceeded { .. })
        ));
        // The guard and the teacher's message read the same kinds as for chat completions.
        let quota_err = parse_response(429, &quota).unwrap_err();
        assert_eq!(
            super::super::ErrorKind::of(&quota_err),
            super::super::ErrorKind::Quota
        );
        assert!(matches!(
            parse_response(502, "bad gateway"),
            Err(ProviderError::Overloaded { status: 502, .. })
        ));
        assert_eq!(endpoint("http://h/v1/"), "http://h/v1/responses");
        assert_eq!(
            endpoint(super::super::openai::OPENAI_BASE_URL),
            "https://api.openai.com/v1/responses"
        );
    }

    #[test]
    fn the_research_record_names_the_api_and_the_effort_sent() {
        let mut r = request();
        let rec = request_record("xhigh", &r);
        assert_eq!(
            rec,
            json!({"api": "responses", "reasoning_effort": "high", "store": false,
                   "max_tokens": 1000, "max_output_tokens": 1000 + REASONING_ROOM,
                   "tool_choice": "auto", "parallel_tool_calls": false})
        );
        // It says what the body says.
        let b = build_body("g", "xhigh", &r);
        assert_eq!(rec["reasoning_effort"], b["reasoning"]["effort"]);
        assert_eq!(rec["max_output_tokens"], b["max_output_tokens"]);
        assert_eq!(rec["store"], b["store"]);
        r.tool_choice = ToolChoice::None;
        assert_eq!(request_record("low", &r)["tool_choice"], "none");
    }

    /// The fake's scripted tool loop: one `read_block` call, then the answer.
    fn script(req: &ChatRequest, model: &str) -> Result<ChatResponse, ProviderError> {
        let answered = req
            .messages
            .iter()
            .any(|m| matches!(m, ChatMessage::Tool { .. }));
        let usage = Some(Usage {
            prompt_tokens: 900,
            completion_tokens: 40,
            cached_tokens: if answered { 768 } else { 0 },
            reasoning_tokens: 25,
            transport_retries: 0,
        });
        Ok(if answered {
            ChatResponse {
                content: Some("{\"paragraphs\":[\"done\"],\"follow_ups\":[]}".into()),
                tool_calls: vec![],
                replay: vec![],
                model_version: format!("{model}-1"),
                usage,
            }
        } else {
            ChatResponse {
                content: None,
                tool_calls: vec![ToolCall {
                    id: "call_0".into(),
                    name: "read_block".into(),
                    arguments: "{\"block_id\":\"b1\"}".into(),
                }],
                replay: vec![],
                model_version: format!("{model}-1"),
                usage,
            }
        })
    }

    #[test]
    fn the_fake_runs_a_two_step_tool_loop_through_the_responses_shapes() {
        let mut req = request();
        // 1: a function call, with the reasoning item to send back.
        let first = fake_call("fake-model", "medium", &req, script).unwrap();
        assert_eq!(first.content, None);
        assert_eq!(first.tool_calls[0].id, "call_0");
        assert_eq!(first.model_version, "fake-model-1");
        let kinds: Vec<&str> = first
            .replay
            .iter()
            .map(|i| i["type"].as_str().unwrap())
            .collect();
        assert_eq!(kinds, ["reasoning", "function_call"]);
        let u = first.usage.unwrap();
        assert_eq!((u.cached_tokens, u.reasoning_tokens), (0, 25));
        // 2: the call and its output go back after the reasoning; the answer is the message.
        req.messages.push(ChatMessage::Assistant {
            content: first.content.clone(),
            tool_calls: first.tool_calls.clone(),
            replay: first.replay.clone(),
        });
        req.messages.push(ChatMessage::Tool {
            call_id: "call_0".into(),
            content: "區塊內容".into(),
        });
        let body = build_body("fake-model", "medium", &req);
        let input = body["input"].as_array().unwrap();
        assert_eq!(
            input[3],
            json!({"type": "reasoning", "id": "rs_fake_0", "summary": [],
                   "encrypted_content": "fake-encrypted-reasoning-0"})
        );
        assert_eq!(
            input[4],
            json!({"type": "function_call", "call_id": "call_0", "name": "read_block",
                   "arguments": "{\"block_id\":\"b1\"}"})
        );
        assert_eq!(
            input[5],
            json!({"type": "function_call_output", "call_id": "call_0", "output": "區塊內容"})
        );
        assert_eq!(check_body(&body), Ok(()));
        let last = fake_call("fake-model", "medium", &req, script).unwrap();
        assert!(last.tool_calls.is_empty() && last.replay.is_empty());
        assert_eq!(
            last.content.as_deref(),
            Some("{\"paragraphs\":[\"done\"],\"follow_ups\":[]}")
        );
        let u = last.usage.unwrap();
        assert_eq!(
            (
                u.prompt_tokens,
                u.completion_tokens,
                u.cached_tokens,
                u.reasoning_tokens
            ),
            (900, 40, 768, 25)
        );
    }

    #[test]
    fn the_fake_refuses_what_a_stateless_request_must_not_send() {
        // An output whose call is missing.
        let mut orphan = request();
        orphan.messages.push(ChatMessage::Tool {
            call_id: "c9".into(),
            content: "x".into(),
        });
        match fake_call("m", "low", &orphan, script) {
            Err(ProviderError::Http { status: 400, body }) => {
                assert!(body.contains("No tool call found"), "{body}")
            }
            other => panic!("{other:?}"),
        }
        // A call whose output is missing.
        let mut unanswered = request();
        unanswered.messages.push(ChatMessage::Assistant {
            content: None,
            tool_calls: vec![ToolCall {
                id: "c1".into(),
                name: "read_block".into(),
                arguments: "{}".into(),
            }],
            replay: vec![],
        });
        assert!(matches!(
            fake_call("m", "low", &unanswered, script),
            Err(ProviderError::Http { status: 400, .. })
        ));
        // Reasoning without its encrypted content, and a call that keeps its stored id.
        let mut body = build_body("m", "low", &second_request());
        assert_eq!(check_body(&body), Ok(()));
        body["input"][3]
            .as_object_mut()
            .unwrap()
            .remove("encrypted_content");
        assert!(check_body(&body).unwrap_err().contains("encrypted_content"));
        let mut body = build_body("m", "low", &second_request());
        body["input"][4]["id"] = json!("fc_1");
        assert!(check_body(&body).unwrap_err().contains("by id"));
        let mut body = build_body("m", "low", &request());
        body["store"] = json!(true);
        assert!(check_body(&body).is_err());
        // The scripted errors pass through unchanged.
        let timeout = fake_call("m", "low", &request(), |_, _| Err(ProviderError::Timeout));
        assert_eq!(timeout, Err(ProviderError::Timeout));
    }
}
