//! Private, authenticated and audited per-lease model forwarding.
use super::super::{POLICY_VERSION, SIDECAR_API_VERSION};
use super::client::client;
use crate::ai::pipeline::{self, Finish, InteractionMeta};
use crate::auth::{bearer_token, sha256_hex};
use crate::config::AiProviderKind;
use crate::error::{AppError, AppResult};
use crate::AppState;
use axum::{
    extract::{DefaultBodyLimit, Request, State},
    http::{HeaderMap, StatusCode},
    routing::post,
    Json, Router,
};
use serde_json::{json, Map, Value};
use std::time::Instant;

/// Path prefix of the forwarding endpoints on the internal listener.
pub const INTERNAL_PREFIX: &str = "/api/internal/memory/v1";
/// Model calls one indexing job may forward (10 habits need far fewer).
pub const MAX_CALLS_PER_JOB: i32 = 300;
pub const MIN_CHAT_TOKENS: u64 = 256;
pub const MAX_CHAT_TOKENS: u64 = 8192;
pub const MAX_MESSAGES: usize = 32;
pub const MAX_MESSAGE_CHARS: usize = 100_000;
pub const MAX_EMBEDDING_BATCH: usize = 64;
pub const MAX_EMBEDDING_INPUT_CHARS: usize = 8192;
/// Request body limit of the internal listener.
pub const INTERNAL_BODY_LIMIT: usize = 1024 * 1024;
/// The sidecar sends `GRAPHITI_TOKEN` here on every forwarded call, next to the job token.
pub const SERVICE_TOKEN_HEADER: &str = "x-manna-service-token";
pub const API_VERSION_HEADER: &str = "x-manna-memory-api-version";
pub const GRAPHITI_VERSION_HEADER: &str = "x-manna-graphiti-version";
pub fn internal_router() -> Router<AppState> {
    Router::new()
        .route("/chat/completions", post(chat))
        .route("/embeddings", post(embeddings))
        .layer(DefaultBodyLimit::max(INTERNAL_BODY_LIMIT))
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProxyKind {
    Chat,
    Embeddings,
}

pub async fn chat(
    State(state): State<AppState>,
    request: Request,
) -> AppResult<(StatusCode, Json<Value>)> {
    forward(&state, request, ProxyKind::Chat).await
}
pub async fn embeddings(
    State(state): State<AppState>,
    request: Request,
) -> AppResult<(StatusCode, Json<Value>)> {
    forward(&state, request, ProxyKind::Embeddings).await
}

fn header<'h>(headers: &'h HeaderMap, name: &str) -> Option<&'h str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

/// `GRAPHITI_TOKEN`, compared by digest (no early exit on the secret's bytes).
fn service_authorized(state: &AppState, headers: &HeaderMap) -> bool {
    match (
        state.config.memory.token.as_deref(),
        header(headers, SERVICE_TOKEN_HEADER),
    ) {
        (Some(expected), Some(got)) => sha256_hex(expected) == sha256_hex(got),
        _ => false,
    }
}

fn bad(msg: &str) -> AppError {
    AppError::bad_request(format!("不支援的模型請求：{msg}"))
}

fn tokens(v: &Value) -> Result<u64, AppError> {
    match v {
        Value::Null => Ok(4096),
        v => v
            .as_u64()
            .filter(|n| *n >= 1)
            .map(|n| n.clamp(MIN_CHAT_TOKENS, MAX_CHAT_TOKENS))
            .ok_or_else(|| bad("max_tokens")),
    }
}

/// The chat request rebuilt from allowlisted fields only: model fixed by configuration,
/// one choice, no tools, bounded messages and output.
pub fn sanitize_chat(body: &Value, model: &str) -> Result<(Map<String, Value>, u64), AppError> {
    let obj = body.as_object().ok_or_else(|| bad("JSON object"))?;
    const ALLOWED: [&str; 6] = [
        "model",
        "messages",
        "temperature",
        "max_tokens",
        "response_format",
        "n",
    ];
    if let Some(field) = obj.keys().find(|k| !ALLOWED.contains(&k.as_str())) {
        return Err(bad(field));
    }
    if obj.get("n").is_some_and(|n| n != 1) {
        return Err(bad("n"));
    }
    let messages = obj
        .get("messages")
        .and_then(Value::as_array)
        .filter(|m| !m.is_empty() && m.len() <= MAX_MESSAGES)
        .ok_or_else(|| bad("messages"))?;
    for m in messages {
        let m = m.as_object().ok_or_else(|| bad("messages"))?;
        let role_ok = m
            .get("role")
            .and_then(Value::as_str)
            .is_some_and(|r| matches!(r, "system" | "user" | "assistant"));
        let content_ok = m
            .get("content")
            .and_then(Value::as_str)
            .is_some_and(|c| c.chars().count() <= MAX_MESSAGE_CHARS);
        if !role_ok || !content_ok || m.keys().any(|k| k != "role" && k != "content") {
            return Err(bad("messages"));
        }
    }
    let mut out = Map::new();
    out.insert("model".into(), json!(model));
    out.insert("messages".into(), json!(messages));
    out.insert("n".into(), json!(1));
    if let Some(t) = obj.get("temperature").filter(|t| !t.is_null()) {
        let t = t
            .as_f64()
            .filter(|t| (0.0..=2.0).contains(t))
            .ok_or_else(|| bad("temperature"))?;
        out.insert("temperature".into(), json!(t));
    }
    if let Some(f) = obj.get("response_format").filter(|f| !f.is_null()) {
        let ok = f["type"]
            .as_str()
            .is_some_and(|t| matches!(t, "text" | "json_object" | "json_schema"));
        if !ok {
            return Err(bad("response_format"));
        }
        out.insert("response_format".into(), f.clone());
    }
    let limit = tokens(obj.get("max_tokens").unwrap_or(&Value::Null))?;
    Ok((out, limit))
}

/// The embedding request rebuilt from allowlisted fields only, with a bounded batch.
pub fn sanitize_embeddings(body: &Value, model: &str) -> Result<Map<String, Value>, AppError> {
    let obj = body.as_object().ok_or_else(|| bad("JSON object"))?;
    if let Some(field) = obj
        .keys()
        .find(|k| !matches!(k.as_str(), "model" | "input" | "encoding_format"))
    {
        return Err(bad(field));
    }
    let short = |s: &Value| {
        s.as_str()
            .is_some_and(|s| s.chars().count() <= MAX_EMBEDDING_INPUT_CHARS)
    };
    let input = obj.get("input").ok_or_else(|| bad("input"))?;
    let ok = match input {
        Value::String(_) => short(input),
        Value::Array(items) => {
            !items.is_empty() && items.len() <= MAX_EMBEDDING_BATCH && items.iter().all(short)
        }
        _ => false,
    };
    if !ok {
        return Err(bad("input"));
    }
    let mut out = Map::new();
    out.insert("model".into(), json!(model));
    out.insert("input".into(), input.clone());
    if let Some(f) = obj.get("encoding_format").filter(|f| !f.is_null()) {
        if !matches!(f.as_str(), Some("float" | "base64")) {
            return Err(bad("encoding_format"));
        }
        out.insert("encoding_format".into(), f.clone());
    }
    Ok(out)
}

/// What the research record keeps of an embedding response: per vector its dimension and a
/// SHA-256 of the returned value, never the vector itself.
pub fn summarize_embeddings(output: &Value) -> Value {
    use base64::Engine;
    let mut out = output.clone();
    if let Some(data) = out.get_mut("data").and_then(Value::as_array_mut) {
        for item in data {
            let Some(e) = item.get_mut("embedding") else {
                continue;
            };
            let summary = match &*e {
                Value::Array(v) => json!({"encoding": "float", "dims": v.len(),
                    "sha256": sha256_hex(serde_json::to_vec(v).unwrap_or_default())}),
                Value::String(s) => json!({"encoding": "base64",
                    "dims": base64::engine::general_purpose::STANDARD.decode(s)
                        .map(|b| b.len() / 4).ok(),
                    "sha256": sha256_hex(s)}),
                _ => continue,
            };
            *e = summary;
        }
    }
    out
}

/// No upstream URL/key/model is accepted from the sidecar. Both the service secret and the
/// job token are checked before the body is read; job tokens expire/revoke on completion and
/// are capped per job. Every upstream request and response is audited.
async fn forward(
    state: &AppState,
    request: Request,
    kind: ProxyKind,
) -> AppResult<(StatusCode, Json<Value>)> {
    let (parts, body) = request.into_parts();
    let headers = &parts.headers;
    if !service_authorized(state, headers) {
        return Err(AppError::Unauthorized("需要服務驗證。".into()));
    }
    let token =
        bearer_token(headers).ok_or_else(|| AppError::Unauthorized("需要服務驗證。".into()))?;
    let job: Option<(i64, i64, String)> = sqlx::query_as(
        "SELECT teacher_id,leased_revision,leased_graphiti_version FROM memory_indexes
        WHERE token_hash=$1 AND lease_until>clock_timestamp()",
    )
    .bind(sha256_hex(&token))
    .fetch_optional(&state.db)
    .await?;
    let (teacher_id, revision, leased_graphiti_version) =
        job.ok_or_else(|| AppError::Unauthorized("服務驗證已失效。".into()))?;
    if header(headers, API_VERSION_HEADER) != Some(SIDECAR_API_VERSION.to_string().as_str()) {
        return Err(AppError::Conflict(
            "記憶服務介面版本不相容，已回到 Postgres。".into(),
        ));
    }
    // The version checked when the job was leased is the one recorded; a sidecar that now
    // reports another core (an upgrade mid-job) is refused and the job is retried.
    let reported = header(headers, GRAPHITI_VERSION_HEADER).unwrap_or_default();
    if reported != leased_graphiti_version {
        return Err(AppError::Conflict(
            "Graphiti 核心版本與領取工作時不同，請重新同步。".into(),
        ));
    }
    let bytes = axum::body::to_bytes(body, INTERNAL_BODY_LIMIT)
        .await
        .map_err(|_| bad("body"))?;
    let body: Value = serde_json::from_slice(&bytes).map_err(|_| bad("JSON"))?;
    let embedding = kind == ProxyKind::Embeddings;
    let c = &state.config;
    let ai = state
        .memory
        .job_model(&token)
        .unwrap_or_else(|| state.ai.active());
    let (base, key, model, provider) = if embedding {
        if c.embedding.provider != crate::embedding::EmbeddingProviderKind::OpenAiCompatible {
            return Err(AppError::Unavailable(
                "記憶索引需要設定 embeddings。".into(),
            ));
        }
        (
            c.embedding.base_url.as_deref(),
            c.embedding.api_key.as_deref(),
            c.embedding.model.as_str(),
            "openai_compatible",
        )
    } else {
        match ai.kind {
            AiProviderKind::OpenAi => (
                Some("https://api.openai.com/v1"),
                c.openai_api_key.as_deref(),
                ai.model.as_str(),
                "openai",
            ),
            AiProviderKind::OpenAiCompatible => (
                c.openai_compat_base_url.as_deref(),
                c.openai_compat_api_key.as_deref(),
                ai.model.as_str(),
                "openai_compatible",
            ),
            _ => {
                return Err(AppError::Unavailable(
                    "Graphiti 需要 OpenAI 相容模型。".into(),
                ))
            }
        }
    };
    let base = base.ok_or_else(|| AppError::Unavailable("模型轉發尚未設定。".into()))?;
    let forwarded = if embedding {
        Value::Object(sanitize_embeddings(&body, model)?)
    } else {
        let (mut out, limit) = sanitize_chat(&body, model)?;
        if ai.kind == AiProviderKind::OpenAi {
            out.remove("temperature");
            out.insert(
                "max_completion_tokens".into(),
                json!(limit + u64::from(crate::ai::openai::REASONING_ROOM)),
            );
            out.insert(
                "reasoning_effort".into(),
                json!(crate::ai::openai::reasoning_effort(&ai.effort)),
            );
        } else {
            out.insert("max_tokens".into(), json!(limit));
        }
        Value::Object(out)
    };
    // One call slot, atomically with the lease (a token can not outrun its cap or lease).
    let call_no: Option<i32> = sqlx::query_scalar(
        "UPDATE memory_indexes SET job_calls=job_calls+1
        WHERE token_hash=$1 AND lease_until>clock_timestamp() AND job_calls<$2
        RETURNING job_calls",
    )
    .bind(sha256_hex(&token))
    .bind(MAX_CALLS_PER_JOB)
    .fetch_optional(&state.db)
    .await?;
    let Some(call_no) = call_no else {
        return Ok((
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({"error": {"message": "model call limit for this indexing job reached"}})),
        ));
    };
    let endpoint = if embedding {
        "embeddings"
    } else {
        "chat/completions"
    };
    let meta = InteractionMeta {
        teacher_id,
        module: if embedding {
            "memory_graph_embedding"
        } else {
            "memory_graph_extract"
        },
        unit_id: None,
        lesson_id: None,
        scaffold_id: None,
        prompt_version_id: None,
    };
    let snapshot = json!({
        // Checked by the API at lease time (authoritative); the sidecar's header must match.
        "graphiti_version": leased_graphiti_version,
        "graphiti_version_reported_by_sidecar": reported,
        "sidecar_api_version": SIDECAR_API_VERSION,
        "policy_version": POLICY_VERSION,
        "revision": revision,
        "job_call_no": call_no,
        "endpoint": endpoint,
        "request": forwarded,
    });
    let id =
        pipeline::insert_interaction(&state.db, &meta, provider, model, &snapshot, 1, None).await?;
    let start = Instant::now();
    let mut request = client(ai.timeout.as_secs())
        .post(format!("{}/{endpoint}", base.trim_end_matches('/')))
        .json(&forwarded);
    if let Some(key) = key {
        request = request.bearer_auth(key);
    }
    let response = request.send().await;
    let (status, output, error_detail) = match response {
        Ok(r) => {
            let status = r.status();
            match r.json::<Value>().await {
                Ok(output) => (
                    status,
                    output,
                    (!status.is_success()).then(|| format!("http_error: HTTP {}", status.as_u16())),
                ),
                Err(e) => (
                    if e.is_timeout() {
                        StatusCode::GATEWAY_TIMEOUT
                    } else {
                        StatusCode::BAD_GATEWAY
                    },
                    json!({"error":"invalid upstream response"}),
                    Some(format!(
                        "bad_format: upstream HTTP {} was not JSON",
                        status.as_u16()
                    )),
                ),
            }
        }
        Err(e) => (
            if e.is_timeout() {
                StatusCode::GATEWAY_TIMEOUT
            } else {
                StatusCode::BAD_GATEWAY
            },
            json!({"error":"model forwarding failed"}),
            Some(if e.is_timeout() {
                "timeout: upstream did not answer in time".to_string()
            } else {
                "http_error: network error".to_string()
            }),
        ),
    };
    let recorded = if embedding {
        summarize_embeddings(&output)
    } else {
        output.clone()
    };
    pipeline::finish_interaction(
        &state.db,
        id,
        Finish {
            status: if status.is_success() {
                "shown"
            } else if status == StatusCode::GATEWAY_TIMEOUT {
                "timeout"
            } else {
                "error"
            },
            output: Some(recorded),
            citations: vec![],
            check_results: None,
            model_version: output["model"].as_str().map(str::to_owned),
            error_detail,
            latency_ms: start.elapsed().as_millis().min(i32::MAX as u128) as i32,
        },
    )
    .await?;
    Ok((status, Json(output)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chat_requests_are_rebuilt_from_allowlisted_fields() {
        let body = json!({"model":"other","messages":[{"role":"user","content":"x"}],
            "temperature":0,"max_tokens":100000,"response_format":{"type":"json_object"}});
        let (out, limit) = sanitize_chat(&body, "configured").unwrap();
        assert_eq!(out["model"], "configured");
        assert_eq!(out["n"], 1);
        assert_eq!(limit, MAX_CHAT_TOKENS);
        assert!(!out.contains_key("max_tokens"), "set per provider");
        for bad in [
            json!({"messages":[{"role":"user","content":"x"}],"tools":[]}),
            json!({"messages":[{"role":"user","content":"x"}],"n":2}),
            json!({"messages":[{"role":"user","content":"x"}],"stream":true}),
            json!({"messages":[{"role":"tool","content":"x"}]}),
            json!({"messages":[{"role":"user","content":[{"type":"image_url"}]}]}),
            json!({"messages":[]}),
            json!({"messages":[{"role":"user","content":"x"}],"temperature":5}),
            json!({"messages":[{"role":"user","content":"x"}],"max_tokens":0}),
            json!({"messages":[{"role":"user","content":"x"}],"response_format":{"type":"x"}}),
            json!([]),
        ] {
            assert!(sanitize_chat(&bad, "m").is_err(), "{bad}");
        }
    }

    #[test]
    fn embedding_requests_have_a_bounded_batch() {
        let ok = json!({"model":"x","input":["a","b"],"encoding_format":"base64"});
        let out = sanitize_embeddings(&ok, "configured").unwrap();
        assert_eq!(out["model"], "configured");
        let many: Vec<String> = (0..=MAX_EMBEDDING_BATCH).map(|i| i.to_string()).collect();
        for bad in [
            json!({"input": many}),
            json!({"input": []}),
            json!({"input": "x".repeat(MAX_EMBEDDING_INPUT_CHARS + 1)}),
            json!({"input": "x", "dimensions": 3}),
            json!({"input": "x", "encoding_format": "binary"}),
            json!({"input": 3}),
        ] {
            assert!(sanitize_embeddings(&bad, "m").is_err(), "{bad}");
        }
    }

    #[test]
    fn embedding_outputs_keep_dimension_and_hash_only() {
        use base64::Engine;
        let floats = json!({"model":"e","data":[{"index":0,"embedding":[0.1,0.2,0.3]}]});
        let s = summarize_embeddings(&floats);
        assert_eq!(s["data"][0]["embedding"]["dims"], 3);
        assert_eq!(s["model"], "e");
        assert!(!s.to_string().contains("0.2"));
        let bytes = [0u8; 16];
        let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
        let s = summarize_embeddings(&json!({"data":[{"embedding": b64}]}));
        assert_eq!(s["data"][0]["embedding"]["dims"], 4);
        assert_eq!(s["data"][0]["embedding"]["encoding"], "base64");
    }
}
