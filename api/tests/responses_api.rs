//! The prep chat's calls on OpenAI's Responses API (`POST /v1/responses`, docs/PREP_CHAT.md
//! section 6), against a local stand-in for OpenAI: the request body (tools, reasoning effort,
//! `store: false`, `include`), a two-step tool loop (function call → its output → the answer)
//! with the reasoning sent back, the tokens kept, 400 and 429 handled like every other call, and
//! the PI's test of an OpenAI model that also tries the chat's call. No network.

#[macro_use]
mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use manna_api::ai::{AiSettings, AiSource};
use manna_api::config::{AiProviderKind, Config};
use serde_json::{json, Value};

const P: [&str; 4] = [
    "Mrs. Lin put an umbrella stand outside her shop on a rainy Monday. A sign said, “Take one if you need it.”",
    "Her neighbors laughed at the idea. They were sure that every umbrella would be gone within a week.",
    "In the first month, twelve of the thirty umbrellas did disappear. However, something unexpected followed.",
    "Today the stand holds more umbrellas than it did at the start. Mrs. Lin says trust is like an umbrella.",
];

const ANSWER: &str =
    "{\"paragraphs\":[\"第一題問的是林太太為什麼把傘架放在外面。\"],\"follow_ups\":[]}";

/// A stand-in for OpenAI. What it answers depends on the model name:
/// - `*tools*`: a reasoning item and a `read_block` call first, the answer once the call's output
///   came back;
/// - `gpt-400` / `gpt-no-tools`: HTTP 400 on `/responses` (the second still answers the plain
///   chat completions call);
/// - `gpt-quota`: 429 `insufficient_quota`;
/// - `gpt-busy`: 429 `rate_limit_exceeded` (with `retry-after-ms`) once, then the answer;
/// - anything else: the answer at once.
#[derive(Clone, Default)]
struct Stub {
    /// `(path, body, authorization)` of every request, in order.
    seen: Arc<Mutex<Vec<(String, Value, String)>>>,
    /// The block the scripted call reads.
    block_id: Arc<Mutex<String>>,
    busy_once: Arc<std::sync::atomic::AtomicBool>,
}

impl Stub {
    fn bodies(&self, path: &str) -> Vec<Value> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _, _)| p == path)
            .map(|(_, b, _)| b.clone())
            .collect()
    }

    fn record(&self, path: &str, headers: &HeaderMap, body: &Value) {
        let auth = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        self.seen
            .lock()
            .unwrap()
            .push((path.to_string(), body.clone(), auth));
    }
}

fn error(status: StatusCode, kind: &str, code: &str, message: &str) -> axum::response::Response {
    let body = json!({"error": {"message": message, "type": kind, "param": null, "code": code}});
    (status, Json(body)).into_response()
}

async fn responses(
    State(stub): State<Stub>,
    headers: HeaderMap,
    Json(v): Json<Value>,
) -> axum::response::Response {
    stub.record("/responses", &headers, &v);
    let model = v["model"].as_str().unwrap_or_default().to_string();
    match model.as_str() {
        "gpt-400" | "gpt-no-tools" => {
            return error(
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                "unsupported_parameter",
                "Function tools with reasoning are not supported for this model.",
            )
        }
        "gpt-quota" => {
            return error(
                StatusCode::TOO_MANY_REQUESTS,
                "insufficient_quota",
                "insufficient_quota",
                "You exceeded your current quota.",
            )
        }
        "gpt-busy"
            if !stub
                .busy_once
                .swap(true, std::sync::atomic::Ordering::SeqCst) =>
        {
            let mut r = error(
                StatusCode::TOO_MANY_REQUESTS,
                "tokens",
                "rate_limit_exceeded",
                "Rate limit reached on tokens per min (TPM).",
            );
            r.headers_mut()
                .insert("retry-after-ms", "5".parse().unwrap());
            return r;
        }
        _ => {}
    }
    let input = v["input"].as_array().cloned().unwrap_or_default();
    let answered = input.iter().any(|i| i["type"] == "function_call_output");
    let (output, usage) = if model.contains("tools") && !answered {
        let block = stub.block_id.lock().unwrap().clone();
        (
            json!([
                {"type": "reasoning", "id": "rs_1", "summary": [], "encrypted_content": "enc-1"},
                {"type": "function_call", "id": "fc_1", "status": "completed", "call_id": "call_1",
                 "name": "read_block", "arguments": json!({"block_id": block}).to_string()}
            ]),
            json!({"input_tokens": 3000, "input_tokens_details": {"cached_tokens": 0},
                   "output_tokens": 180, "output_tokens_details": {"reasoning_tokens": 150},
                   "total_tokens": 3180}),
        )
    } else {
        (
            json!([
                {"type": "reasoning", "id": "rs_2", "summary": [], "encrypted_content": "enc-2"},
                {"type": "message", "id": "msg_2", "status": "completed", "role": "assistant",
                 "content": [{"type": "output_text", "text": ANSWER, "annotations": []}]}
            ]),
            json!({"input_tokens": 3400, "input_tokens_details": {"cached_tokens": 2944},
                   "output_tokens": 60, "output_tokens_details": {"reasoning_tokens": 32},
                   "total_tokens": 3460}),
        )
    };
    Json(
        json!({"id": "resp_1", "object": "response", "status": "completed",
                "model": format!("{model}-2026-11-01"), "output": output, "usage": usage}),
    )
    .into_response()
}

/// The plain call (chat completions) the admin test makes first.
async fn chat_completions(
    State(stub): State<Stub>,
    headers: HeaderMap,
    Json(v): Json<Value>,
) -> axum::response::Response {
    stub.record("/chat/completions", &headers, &v);
    let model = v["model"].as_str().unwrap_or_default();
    Json(json!({"model": format!("{model}-2026-11-01"),
        "choices": [{"message": {"content": "{\"ok\":true}"}, "finish_reason": "stop"}],
        "usage": {"prompt_tokens": 30, "completion_tokens": 5}}))
    .into_response()
}

async fn start_stub() -> (Stub, String) {
    let stub = Stub::default();
    let router = Router::new()
        .route("/v1/responses", post(responses))
        .route("/v1/chat/completions", post(chat_completions))
        .with_state(stub.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (stub, url)
}

/// The fake provider stays the default (it makes the worksheet); OpenAI is configured with the
/// stand-in's address and a placeholder key, and switched on per test.
fn openai(url: &str) -> impl FnOnce(&mut Config) + '_ {
    move |c: &mut Config| {
        c.openai_api_key = Some("stub-key".into());
        c.openai_base_url = url.to_string();
        c.ai_rate_limit_retries = 1;
    }
}

fn use_model(app: &common::TestApp, model: &str, effort: &str) {
    app.state.ai.set_active(
        AiSettings {
            kind: AiProviderKind::OpenAi,
            model: model.into(),
            effort: effort.into(),
            timeout_secs: 60,
        },
        AiSource::Admin,
    );
}

async fn wait(app: &common::TestApp, t: &str, id: i64) -> Value {
    for _ in 0..800 {
        let (s, v) = app.get(&format!("/api/prep-sessions/{id}"), Some(t)).await;
        assert_eq!(s, StatusCode::OK, "{v}");
        if v["running"] == false {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("prep session {id} is still running");
}

/// A session whose first turn made a worksheet (with the fake provider): (session, first block).
async fn session(app: &common::TestApp, t: &str) -> (i64, String) {
    let text = P.join("\n\n");
    let (s, v) = app
        .post(
            "/api/prep-sessions",
            Some(t),
            json!({ "kind": "worksheet",
                    "material": { "title": "The Umbrella Stand", "text": text } }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    let id = v["id"].as_i64().unwrap();
    let done = wait(app, t, id).await;
    assert_eq!(done["turns"][0]["status"], "done", "{done}");
    let ws = done["turns"][0]["worksheet"]["id"].as_i64().unwrap();
    let (_, w) = app.get(&format!("/api/worksheets/{ws}"), Some(t)).await;
    let block = w["content"]["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| !b["items"].as_array().unwrap().is_empty())
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    (id, block)
}

async fn chat(app: &common::TestApp, t: &str, id: i64, request: &str) -> Value {
    let (s, v) = app
        .post(
            &format!("/api/prep-sessions/{id}/turns"),
            Some(t),
            json!({ "kind": "chat", "request": request }),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    let done = wait(app, t, id).await;
    done["turns"].as_array().unwrap().last().unwrap().clone()
}

/// `(provider, model, status, model_version, error_detail, input_snapshot, usage)` of the rows
/// of `module`, oldest first.
type Row = (
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Value,
    Option<Value>,
);

async fn rows(app: &common::TestApp, module: &str) -> Vec<Row> {
    sqlx::query_as(
        "SELECT provider, model, status, model_version, error_detail, input_snapshot, usage
           FROM ai_interactions WHERE module = $1 ORDER BY id",
    )
    .bind(module)
    .fetch_all(&app.db)
    .await
    .unwrap()
}

#[tokio::test]
async fn a_tool_loop_runs_on_the_responses_api_with_reasoning_sent_back() {
    let (stub, url) = start_stub().await;
    let app = test_app!(openai(&url));
    let t = app.teacher("T-RESP-1", "general").await;
    let (id, block) = session(&app, &t).await;
    *stub.block_id.lock().unwrap() = block.clone();
    // The PI's setting, not the environment's (`medium`): the effort follows the active model.
    use_model(&app, "gpt-tools", "low");

    let turn = chat(&app, &t, id, "第一題在問什麼？").await;
    assert_eq!(turn["status"], "done", "{turn}");
    let r = &turn["result"];
    assert!(r["answer"].as_str().unwrap().contains("林太太"), "{r}");
    assert_eq!(r["tool_calls"], 1);
    assert_eq!(r["version_no"], Value::Null, "reading changes nothing");
    assert!(stub.bodies("/chat/completions").is_empty());

    let bodies = stub.bodies("/responses");
    assert_eq!(bodies.len(), 2);
    for (k, b) in bodies.iter().enumerate() {
        assert_eq!(b["model"], "gpt-tools");
        assert_eq!(b["store"], false, "the teacher's material is not kept");
        assert_eq!(b["include"], json!(["reasoning.encrypted_content"]));
        assert_eq!(b["reasoning"], json!({"effort": "low"}));
        assert_eq!(b["tool_choice"], "auto");
        assert_eq!(b["parallel_tool_calls"], false);
        assert_eq!(b["prompt_cache_key"], format!("prep-chat-{id}"));
        assert_eq!(b["text"]["format"]["type"], "json_schema");
        assert_eq!(b["text"]["format"]["name"], "prep_chat");
        assert_eq!(b["text"]["format"]["strict"], true);
        assert!(b["max_output_tokens"].as_u64().unwrap() > 6000);
        let tools = b["tools"].as_array().unwrap();
        assert!(tools.iter().any(|t| t["name"] == "read_block"));
        assert!(tools.iter().all(|t| t["type"] == "function"
            && t["strict"] == true
            && t.get("function").is_none()));
        // Nothing of chat completions.
        for key in [
            "messages",
            "response_format",
            "reasoning_effort",
            "max_completion_tokens",
        ] {
            assert!(b.get(key).is_none(), "call {k} sends {key}");
        }
        assert_eq!(b["input"][0]["role"], "developer");
    }
    let seen = stub.seen.lock().unwrap().clone();
    assert!(seen.iter().all(|(_, _, auth)| auth == "Bearer stub-key"));

    // The second request is the first plus the reasoning item (whole, with its encrypted
    // content), the call (without the stored item's id) and the call's output.
    let first = bodies[0]["input"].as_array().unwrap();
    let second = bodies[1]["input"].as_array().unwrap();
    assert_eq!(second.len(), first.len() + 3);
    assert_eq!(second[..first.len()], first[..], "the cached prefix stays");
    let n = first.len();
    assert_eq!(
        second[n],
        json!({"type": "reasoning", "id": "rs_1", "summary": [], "encrypted_content": "enc-1"})
    );
    assert_eq!(
        second[n + 1],
        json!({"type": "function_call", "call_id": "call_1", "name": "read_block",
               "arguments": json!({"block_id": block}).to_string()})
    );
    assert_eq!(second[n + 2]["type"], "function_call_output");
    assert_eq!(second[n + 2]["call_id"], "call_1");
    assert!(!second[n + 2]["output"].as_str().unwrap().is_empty());

    // One row per call: what answered it, the API and effort it was sent with, the tokens.
    let calls = rows(&app, "prep_chat").await;
    assert_eq!(calls.len(), 2);
    for (k, (provider, model, status, version, detail, snapshot, _)) in calls.iter().enumerate() {
        assert_eq!(
            (provider.as_str(), model.as_str(), status.as_str()),
            ("openai", "gpt-tools", "shown")
        );
        assert_eq!(version.as_deref(), Some("gpt-tools-2026-11-01"));
        assert!(detail.is_none());
        let req = &snapshot["request"];
        assert_eq!(req["api"], "responses");
        assert_eq!(req["reasoning_effort"], "low");
        assert_eq!(req["store"], false);
        assert_eq!(req["model_call"], k + 1);
        assert_eq!(req["tool_choice"], "auto");
    }
    assert_eq!(
        calls[0].6,
        Some(json!({"prompt_tokens": 3000, "completion_tokens": 180,
                    "cached_tokens": 0, "reasoning_tokens": 150}))
    );
    assert_eq!(
        calls[1].6,
        Some(json!({"prompt_tokens": 3400, "completion_tokens": 60,
                    "cached_tokens": 2944, "reasoning_tokens": 32}))
    );
}

#[tokio::test]
async fn a_400_a_rate_limit_and_a_used_up_quota_are_handled_like_every_other_call() {
    let (stub, url) = start_stub().await;
    let app = test_app!(openai(&url));
    let t = app.teacher("T-RESP-2", "general").await;
    let (id, _) = session(&app, &t).await;

    // 400: the turn fails with the general message, the row keeps what OpenAI said.
    use_model(&app, "gpt-400", "medium");
    let turn = chat(&app, &t, id, "第一題在問什麼？").await;
    assert_eq!(turn["status"], "failed", "{turn}");
    assert!(turn["error"].as_str().unwrap().contains("AI 暫時無法使用"));
    let calls = rows(&app, "prep_chat").await;
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].2, "error");
    let detail = calls[0].4.clone().unwrap();
    assert!(detail.starts_with("HTTP 400:"), "{detail}");
    assert!(detail.contains("invalid_request_error"), "{detail}");
    assert_eq!(calls[0].6, None, "no tokens, no retries");

    // 429 rate limit: waited for (`retry-after-ms`) and repeated once; the repeat is recorded.
    use_model(&app, "gpt-busy", "medium");
    let turn = chat(&app, &t, id, "第二題在問什麼？").await;
    assert_eq!(turn["status"], "done", "{turn}");
    assert_eq!(stub.bodies("/responses").len(), 1 + 2);
    let calls = rows(&app, "prep_chat").await;
    assert_eq!(calls.len(), 2, "a repeat is the same row");
    assert_eq!(calls[1].2, "shown");
    assert_eq!(calls[1].6.as_ref().unwrap()["transport_retries"], 1);

    // 429 quota: the turn fails with the quota message, calls stop for the cooldown.
    use_model(&app, "gpt-quota", "medium");
    let turn = chat(&app, &t, id, "第三題在問什麼？").await;
    assert_eq!(turn["status"], "failed", "{turn}");
    assert_eq!(turn["error"], "今天的 AI 用量已達上限，請通知計畫主持人。");
    let calls = rows(&app, "prep_chat").await;
    assert!(calls[2].4.as_deref().unwrap().starts_with("quota:"));
    assert!(app.state.ai.quota_blocked_secs().is_some());
    let sent = stub.bodies("/responses").len();
    let turn = chat(&app, &t, id, "再問一次").await;
    assert_eq!(turn["status"], "failed");
    assert_eq!(turn["error"], "今天的 AI 用量已達上限，請通知計畫主持人。");
    assert_eq!(stub.bodies("/responses").len(), sent, "nothing sent");
    app.state.ai.clear_quota_cooldown();
}

#[tokio::test]
async fn the_pi_test_of_an_openai_model_also_tries_the_chats_tools_and_reasoning() {
    let (stub, url) = start_stub().await;
    let app = test_app!(openai(&url));
    let pi = app.pi_token().await;
    let put = |model: &str| json!({"provider": "openai", "model": model, "effort": "high", "timeout_secs": 120});

    // A model that takes both calls (here it even calls a tool, which also passes).
    let (s, v) = app
        .put("/api/admin/ai-model", Some(&pi), put("gpt-tools"))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["active"]["model"], "gpt-tools");
    let plain = stub.bodies("/chat/completions");
    assert_eq!(plain.len(), 1);
    assert_eq!(plain[0]["reasoning_effort"], "high");
    let chat = stub.bodies("/responses");
    assert_eq!(chat.len(), 1);
    assert_eq!(chat[0]["reasoning"], json!({"effort": "high"}));
    assert_eq!(chat[0]["store"], false);
    assert_eq!(chat[0]["tool_choice"], "auto");
    assert_eq!(chat[0]["text"]["format"]["name"], "prep_chat");
    assert!(
        chat[0]["tools"].as_array().unwrap().len() >= 5,
        "the chat's own tools"
    );
    let tests = rows(&app, "admin_model_test").await;
    assert_eq!(tests.len(), 2);
    assert!(tests.iter().all(|r| r.2 == "shown" && r.0 == "openai"));
    assert!(tests[0].5.get("request").is_none(), "the plain call");
    assert_eq!(tests[1].5["request"]["api"], "responses");
    assert_eq!(tests[1].5["request"]["reasoning_effort"], "high");

    // A model that takes the plain call but not tools with a reasoning effort is not switched on.
    let (s, v) = app
        .put("/api/admin/ai-model", Some(&pi), put("gpt-no-tools"))
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    let why = v["error"]["message"].as_str().unwrap();
    assert!(why.contains("設定沒有儲存"), "{why}");
    assert!(why.contains("工具＋推理強度"), "{why}");
    assert_eq!(app.state.ai.active_settings().0.model, "gpt-tools");
    let tests = rows(&app, "admin_model_test").await;
    assert_eq!(tests.len(), 4);
    assert_eq!(tests[2].2, "shown", "the plain call passed");
    assert_eq!(tests[3].2, "error");
    assert!(tests[3].4.as_deref().unwrap().starts_with("HTTP 400:"));
    let changes: Vec<Value> = sqlx::query_scalar(
        "SELECT payload FROM usage_logs WHERE event = 'setting_changed' ORDER BY id",
    )
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert_eq!(changes.len(), 1, "only the change that passed");
}
