//! The PI's switch of the text model (後台管理 → 平台設定 → 文字模型). Contract: docs/API.md
//! 「文字模型設定」.
//!
//! The environment (`AI_PROVIDER`, `AI_MODEL`, `AI_EFFORT`, `AI_TIMEOUT_SECS`) is the default;
//! `app_settings.ai_text_model` overrides it. A change is accepted only after one small test
//! call with the NEW settings succeeds, then it is saved and becomes active in memory at once
//! ([`AiClient::set_active`]); calls in flight finish with the settings they started with.
//! Keys and addresses are never settable here, and neither are `anthropic` (overseas policy),
//! `fake` and `none`.

use std::time::Instant;

use axum::extract::State;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;

use super::pipeline::{self, Finish, InteractionMeta};
use super::{AiClient, AiSettings, AiSource, LlmRequest, ProviderError};
use crate::audit::log_usage;
use crate::auth::AuthTeacher;
use crate::config::{AiProviderKind, VALID_EFFORTS};
use crate::error::{ApiJson, AppError, AppResult};
use crate::settings;
use crate::AppState;

pub const TIMEOUT_MIN_SECS: u64 = 30;
pub const TIMEOUT_MAX_SECS: u64 = 900;
/// The test call never waits longer than this, whatever timeout is being set.
pub const TEST_TIMEOUT_CAP_SECS: u64 = 120;
const MODEL_MAX_CHARS: usize = 100;

/// `ai_interactions.module` of the test call.
pub const TEST_MODULE: &str = "admin_model_test";

/// What the PI may pick, with the label shown on the page.
pub const SELECTABLE: [(AiProviderKind, &str); 2] = [
    (AiProviderKind::OpenAi, "OpenAI（海外）"),
    (AiProviderKind::OpenAiCompatible, "國網中心（NCHC）"),
];

/// Suggested models per provider (the environment's own model is added to the list).
pub fn presets(kind: AiProviderKind) -> &'static [&'static str] {
    match kind {
        AiProviderKind::OpenAi => &["gpt-5.6-sol", "gpt-5.6-terra"],
        AiProviderKind::OpenAiCompatible => &["gemma-4-31B-it"],
        _ => &[],
    }
}

fn provider_label(kind: AiProviderKind) -> &'static str {
    SELECTABLE
        .iter()
        .find(|(k, _)| *k == kind)
        .map_or("", |(_, l)| l)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelRequest {
    pub provider: String,
    pub model: String,
    /// Only meaningful for `openai`; absent keeps the environment's effort.
    pub effort: Option<String>,
    pub timeout_secs: u64,
}

/// Checks a requested setting against what the PI may choose and what the environment offers.
/// The message is shown to the PI as is.
pub fn validate(
    ai: &AiClient,
    provider: &str,
    model: &str,
    effort: Option<&str>,
    timeout_secs: u64,
) -> Result<AiSettings, String> {
    let kind = AiProviderKind::parse(provider)
        .filter(|k| SELECTABLE.iter().any(|(s, _)| s == k))
        .ok_or_else(|| "模型來源只能選「OpenAI」或「國網中心」。".to_string())?;
    if !ai.provider_configured(kind) {
        return Err(format!(
            "這個環境還沒有設定「{}」的金鑰或位址，不能選它。",
            provider_label(kind)
        ));
    }
    let model = model.trim();
    if model.is_empty() || model.chars().count() > MODEL_MAX_CHARS {
        return Err(format!("模型名稱要 1–{MODEL_MAX_CHARS} 個字元。"));
    }
    if !model
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '/'))
    {
        return Err("模型名稱只能用英文字母、數字和 . - _ : /，不能有空白。".to_string());
    }
    let effort = match effort {
        Some(e) if !VALID_EFFORTS.contains(&e) => {
            return Err(format!("推理強度只能是 {}。", VALID_EFFORTS.join("、")));
        }
        Some(e) => e.to_string(),
        None => ai.env_default().effort.clone(),
    };
    if !(TIMEOUT_MIN_SECS..=TIMEOUT_MAX_SECS).contains(&timeout_secs) {
        return Err(format!(
            "等待時間要在 {TIMEOUT_MIN_SECS}–{TIMEOUT_MAX_SECS} 秒之間。"
        ));
    }
    Ok(AiSettings {
        kind,
        model: model.to_string(),
        effort,
        timeout_secs,
    })
}

/// A stored override read back; `None` when it is malformed or no longer allowed (a key was
/// removed from the environment).
fn parse_stored(ai: &AiClient, v: &Value) -> Option<AiSettings> {
    validate(
        ai,
        v["provider"].as_str()?,
        v["model"].as_str()?,
        Some(v["effort"].as_str()?),
        v["timeout_secs"].as_u64()?,
    )
    .ok()
}

/// Startup: applies the PI's override when there is a valid one, and says which source is
/// active. A stored override that is no longer valid is ignored (the environment applies).
pub async fn load_override(ai: &AiClient, db: &PgPool) -> sqlx::Result<()> {
    let stored = settings::get_stored(db, settings::AI_TEXT_MODEL).await?;
    match stored {
        None => {}
        Some(s) => match parse_stored(ai, &s.value) {
            Some(active) => ai.set_active(active, AiSource::Admin),
            None => tracing::warn!(
                "the stored text model setting is not valid any more (provider not configured?); \
                 using the environment's"
            ),
        },
    }
    let (active, source) = ai.active_settings();
    tracing::info!(
        source = source.as_str(),
        provider = active.kind.as_str(),
        model = %active.model,
        effort = %active.effort,
        timeout_secs = active.timeout_secs,
        "text model in use"
    );
    Ok(())
}

/// Why a test call failed, in words for the PI.
fn explain(e: &ProviderError) -> String {
    match e {
        ProviderError::RateLimited { .. } => "用量太高，稍後再試。".into(),
        ProviderError::QuotaExceeded { .. } => "額度用完，請到 OpenAI 帳戶提高上限或加值。".into(),
        ProviderError::Overloaded { .. } => "模型服務忙碌，稍後再試。".into(),
        ProviderError::Timeout => {
            "模型沒有在時間內回應（可能模型名稱不對，或服務太忙）。".to_string()
        }
        ProviderError::Http { status, .. } => match status {
            401 | 403 => "金鑰被拒絕，或這個帳號沒有權限使用這個模型。".to_string(),
            400 | 404 => format!(
                "服務回報找不到這個模型或不接受這些設定（HTTP {status}）。請檢查模型名稱有沒有打錯。"
            ),
            429 => "服務回報用量太高或額度用完（HTTP 429）。".to_string(),
            s => format!("服務回報錯誤（HTTP {s}）。"),
        },
        ProviderError::Network(_) => "連不上模型服務。".to_string(),
        ProviderError::Refusal(_) => "模型拒絕回答測試題目。".to_string(),
        ProviderError::MaxTokens { .. } => "模型的輸出被截斷，測試沒有通過。".to_string(),
        ProviderError::InvalidOutput(_) => {
            "模型的回答不是要求的 JSON 格式，這個模型可能不適合。".to_string()
        }
        ProviderError::Disabled => "AI 沒有開啟。".to_string(),
    }
}

/// One tiny call with `candidate`, recorded as an `ai_interactions` row of module
/// [`TEST_MODULE`] (provider and model are the candidate's, so the log stays truthful). Returns
/// the latency in milliseconds, or the message for the PI.
async fn test_call(
    state: &AppState,
    teacher_id: i64,
    candidate: &AiSettings,
) -> AppResult<Result<i32, String>> {
    let mut ai = state.ai.with_settings(candidate);
    ai.timeout = ai
        .timeout
        .min(std::time::Duration::from_secs(TEST_TIMEOUT_CAP_SECS));
    let req = LlmRequest {
        system: "This is a connection test. Reply only with the JSON object requested.".into(),
        user: "Return {\"ok\": true}.".into(),
        schema_name: "model_test",
        schema: json!({
            "type": "object",
            "properties": { "ok": { "type": "boolean" } },
            "required": ["ok"],
            "additionalProperties": false
        }),
        fake_context: json!({ "task": "model_test" }),
        attempt: 1,
        max_tokens: Some(64),
        temperature: None,
        images: Vec::new(),
    };
    let meta = InteractionMeta {
        teacher_id,
        module: TEST_MODULE,
        unit_id: None,
        lesson_id: None,
        scaffold_id: None,
        prompt_version_id: None,
    };
    let snapshot = json!({
        "purpose": "test call before the text model is switched",
        "system": req.system,
        "user": req.user,
        "settings": candidate.to_json(),
    });
    let id = pipeline::insert_interaction(
        &state.db,
        &meta,
        ai.provider_name(),
        &ai.model,
        &snapshot,
        1,
        None,
    )
    .await?;
    let start = Instant::now();
    let result = ai.complete(&req).await;
    if let Ok(resp) = &result {
        pipeline::record_usage(&state.db, id, resp.usage).await?;
    }
    let latency_ms = i32::try_from(start.elapsed().as_millis()).unwrap_or(i32::MAX);
    let (outcome, finish) = match result {
        Ok(resp) => (
            Ok(latency_ms),
            Finish {
                status: "shown",
                output: Some(resp.json),
                citations: Vec::new(),
                check_results: None,
                model_version: Some(resp.model_version),
                error_detail: None,
                latency_ms,
            },
        ),
        Err(e) => {
            let status = if e == ProviderError::Timeout {
                "timeout"
            } else {
                "error"
            };
            let detail = match &e {
                ProviderError::Http { status, body } => {
                    format!("HTTP {status}: {}", super::clip(body, 500))
                }
                other => super::clip(&other.to_string(), 500),
            };
            (
                Err(explain(&e)),
                Finish {
                    status,
                    output: None,
                    citations: Vec::new(),
                    check_results: None,
                    model_version: None,
                    error_detail: Some(detail),
                    latency_ms,
                },
            )
        }
    };
    pipeline::finish_interaction(&state.db, id, finish).await?;
    Ok(outcome)
}

async fn status(state: &AppState) -> AppResult<Value> {
    let ai = &state.ai;
    let (active, source) = ai.active_settings();
    let env = ai.env_default();
    let (changed_by, changed_at) = if source == AiSource::Admin {
        match settings::get_stored(&state.db, settings::AI_TEXT_MODEL).await? {
            Some(s) => (json!(s.updated_by), json!(s.updated_at)),
            None => (Value::Null, Value::Null),
        }
    } else {
        (Value::Null, Value::Null)
    };
    let providers: Vec<Value> = SELECTABLE
        .iter()
        .map(|(kind, label)| {
            let mut models: Vec<String> = presets(*kind).iter().map(|m| m.to_string()).collect();
            if env.kind == *kind && !models.contains(&env.model) {
                models.push(env.model.clone());
            }
            json!({
                "id": kind.as_str(),
                "label": label,
                "available": ai.provider_configured(*kind),
                "models": models,
                "uses_effort": *kind == AiProviderKind::OpenAi,
            })
        })
        .collect();
    Ok(json!({
        "active": active.to_json(),
        "source": source.as_str(),
        "changed_by": changed_by,
        "changed_at": changed_at,
        "env_default": env.to_json(),
        "providers": providers,
        "efforts": VALID_EFFORTS,
        "timeout_range": { "min": TIMEOUT_MIN_SECS, "max": TIMEOUT_MAX_SECS },
    }))
}

pub async fn get(State(state): State<AppState>, teacher: AuthTeacher) -> AppResult<Json<Value>> {
    teacher.require_pi()?;
    Ok(Json(status(&state).await?))
}

/// Tests the new settings with one call; only when it works are they saved and activated.
pub async fn put(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiJson(req): ApiJson<ModelRequest>,
) -> AppResult<Json<Value>> {
    teacher.require_pi()?;
    let candidate = validate(
        &state.ai,
        &req.provider,
        &req.model,
        req.effort.as_deref(),
        req.timeout_secs,
    )
    .map_err(AppError::BadRequest)?;
    let _changing = state.ai.change_lock.lock().await;
    let latency_ms = match test_call(&state, teacher.id(), &candidate).await? {
        Ok(ms) => ms,
        Err(why) => {
            return Err(AppError::BadRequest(format!(
                "測試呼叫沒有通過，設定沒有儲存，目前仍用原本的模型。原因：{why}"
            )))
        }
    };
    state.ai.clear_quota_cooldown();
    let value = candidate.to_json();
    let mut tx = state.db.begin().await?;
    let previous = settings::set(&mut tx, settings::AI_TEXT_MODEL, &value, teacher.id()).await?;
    log_usage(
        &mut *tx,
        teacher.id(),
        "setting_changed",
        json!({
            "key": settings::AI_TEXT_MODEL,
            "from": previous,
            "to": value,
            "test_latency_ms": latency_ms,
        }),
    )
    .await?;
    tx.commit().await?;
    state.ai.set_active(candidate, AiSource::Admin);
    let mut body = status(&state).await?;
    body["test_latency_ms"] = json!(latency_ms);
    Ok(Json(body))
}

/// Removes the PI's override: the environment's settings apply again.
pub async fn reset(State(state): State<AppState>, teacher: AuthTeacher) -> AppResult<Json<Value>> {
    teacher.require_pi()?;
    let _changing = state.ai.change_lock.lock().await;
    let mut tx = state.db.begin().await?;
    let previous = settings::delete(&mut tx, settings::AI_TEXT_MODEL).await?;
    if previous.is_some() {
        log_usage(
            &mut *tx,
            teacher.id(),
            "setting_changed",
            json!({
                "key": settings::AI_TEXT_MODEL,
                "from": previous,
                "to": null,
                "reset_to_env": state.ai.env_default().to_json(),
            }),
        )
        .await?;
    }
    tx.commit().await?;
    state
        .ai
        .set_active(state.ai.env_default().clone(), AiSource::Env);
    Ok(Json(status(&state).await?))
}
