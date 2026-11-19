//! Durable Graphiti synchronization and per-job, audited OpenAI-compatible forwarding.
use std::time::{Duration, Instant};

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{Memory, GRAPHITI_VERSION, POLICY_VERSION};
use crate::ai::pipeline::{self, Finish, InteractionMeta};
use crate::auth::{bearer_token, new_token, sha256_hex};
use crate::config::AiProviderKind;
use crate::error::{ApiJson, AppError, AppResult};
use crate::AppState;

// Debug deliberately omits the internal service secret.
#[derive(Clone, Default)]
pub struct MemoryConfig {
    pub url: Option<String>,
    pub token: Option<String>,
    pub proxy_url: Option<String>,
}
impl MemoryConfig {
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Self {
        let nonempty = |k| get(k).filter(|v| !v.trim().is_empty());
        Self {
            url: nonempty("GRAPHITI_URL"),
            token: nonempty("GRAPHITI_TOKEN"),
            proxy_url: nonempty("MEMORY_PROXY_URL"),
        }
    }
    pub fn enabled(&self) -> bool {
        self.url.is_some() && self.token.is_some() && self.proxy_url.is_some()
    }
}

fn client(timeout: u64) -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout))
        .build()
        .expect("TLS client")
}

pub fn spawn_worker(state: AppState) {
    if !state.config.memory.enabled() {
        return;
    }
    tokio::spawn(async move {
        loop {
            match sync_one(&state).await {
                Ok(true) => continue,
                Ok(false) => {}
                Err(_) => tracing::warn!("memory index worker failed; retrying"),
            }
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    });
}

#[derive(sqlx::FromRow)]
struct Work {
    teacher_id: i64,
    revision: i64,
    group_id: Option<String>,
}

/// Claim a renewable durable job. Each attempt uses a distinct group, so a timed-out
/// sidecar can never write into the next attempt's graph or publish a stale revision.
pub async fn sync_one(state: &AppState) -> AppResult<bool> {
    if !state.config.memory.enabled() {
        return Ok(false);
    }
    collect_garbage(state).await?;
    let token = new_token();
    let work: Option<Work> = sqlx::query_as(
        "UPDATE memory_indexes SET
        lease_until=clock_timestamp()+interval '10 minutes',token_hash=$1,leased_revision=revision
        WHERE teacher_id=(SELECT teacher_id FROM memory_indexes
            WHERE revision>indexed_revision AND next_attempt_at<=now()
            AND (lease_until IS NULL OR lease_until<clock_timestamp())
            ORDER BY next_attempt_at,teacher_id FOR UPDATE SKIP LOCKED LIMIT 1)
        RETURNING teacher_id,revision,group_id",
    )
    .bind(sha256_hex(&token))
    .fetch_optional(&state.db)
    .await?;
    let Some(work) = work else {
        return Ok(false);
    };
    let group = format!(
        "manna_t{}_r{}_{}",
        work.teacher_id,
        work.revision,
        &token[..12]
    );
    sqlx::query("INSERT INTO memory_index_garbage(group_id,not_before) VALUES($1,now()+interval '15 minutes')")
        .bind(&group).execute(&state.db).await?;
    let active = super::list(&state.db, work.teacher_id, false).await?;
    let cfg = &state.config.memory;
    let result = client(480)
        .post(format!(
            "{}/index",
            cfg.url.as_deref().unwrap().trim_end_matches('/')
        ))
        .bearer_auth(cfg.token.as_deref().unwrap())
        .json(&json!({
            "group_id":group,"memories":active,"proxy_url":cfg.proxy_url,
            "proxy_key":token,"model":state.config.ai_model,
            "embedding_model":state.config.embedding.model,
            "embedding_dims":state.config.embedding.dims,
            "policy_version":POLICY_VERSION,
        }))
        .send()
        .await;
    let success = match result {
        Ok(r) if r.status().is_success() => r.json::<Value>().await.ok().is_some_and(|v| {
            v["ok"] == true
                && v["graphiti_version"] == GRAPHITI_VERSION
                && v["policy_version"] == POLICY_VERSION
        }),
        _ => false,
    };
    if success {
        let mut tx = state.db.begin().await?;
        let published = sqlx::query(
            "UPDATE memory_indexes SET indexed_revision=$2,group_id=$3,
            lease_until=NULL,token_hash=NULL,last_error=NULL,updated_at=now()
            WHERE teacher_id=$1 AND revision=$2 AND token_hash=$4",
        )
        .bind(work.teacher_id)
        .bind(work.revision)
        .bind(&group)
        .bind(sha256_hex(&token))
        .execute(&mut *tx)
        .await?
        .rows_affected()
            > 0;
        if published {
            sqlx::query("DELETE FROM memory_index_garbage WHERE group_id=$1")
                .bind(&group)
                .execute(&mut *tx)
                .await?;
            if let Some(old) = work.group_id {
                sqlx::query(
                    "INSERT INTO memory_index_garbage(group_id) VALUES($1) ON CONFLICT DO NOTHING",
                )
                .bind(old)
                .execute(&mut *tx)
                .await?;
            }
        }
        tx.commit().await?;
        if !published {
            release(state, work.teacher_id, &token, false).await?;
        }
    } else {
        release(state, work.teacher_id, &token, true).await?;
    }
    Ok(true)
}

async fn release(state: &AppState, teacher_id: i64, token: &str, failed: bool) -> sqlx::Result<()> {
    sqlx::query("UPDATE memory_indexes SET lease_until=NULL,token_hash=NULL,
        attempts=attempts+$3::int, last_error=CASE WHEN $3 THEN 'Graphiti 同步未完成，稍後重試。' ELSE NULL END,
        next_attempt_at=CASE WHEN $3 THEN now()+make_interval(secs=>LEAST(3600,10*power(2,LEAST(attempts,8)))::int) ELSE now() END
        WHERE teacher_id=$1 AND token_hash=$2")
        .bind(teacher_id).bind(sha256_hex(token)).bind(failed).execute(&state.db).await?;
    Ok(())
}

async fn collect_garbage(state: &AppState) -> AppResult<()> {
    let groups: Vec<String> = sqlx::query_scalar("SELECT group_id FROM memory_index_garbage g
        WHERE not_before<=now() AND NOT EXISTS(SELECT 1 FROM memory_indexes i WHERE i.group_id=g.group_id)
        ORDER BY not_before LIMIT 5").fetch_all(&state.db).await?;
    let cfg = &state.config.memory;
    for group in groups {
        let response = client(5)
            .delete(format!(
                "{}/groups/{}",
                cfg.url.as_deref().unwrap().trim_end_matches('/'),
                group
            ))
            .bearer_auth(cfg.token.as_deref().unwrap())
            .send()
            .await;
        if response.is_ok_and(|r| r.status().is_success()) {
            sqlx::query("DELETE FROM memory_index_garbage WHERE group_id=$1")
                .bind(group)
                .execute(&state.db)
                .await?;
        }
    }
    Ok(())
}

/// The search query contains only canonical habits, never raw conversation/material text.
/// Graph results may reorder approved IDs, but can never introduce a fact or another teacher.
pub async fn rank(state: &AppState, teacher_id: i64, _query: &str, active: &[Memory]) -> Vec<i64> {
    if !state.config.memory.enabled() || active.is_empty() {
        return vec![];
    }
    let group: Option<String> = sqlx::query_scalar(
        "SELECT group_id FROM memory_indexes
        WHERE teacher_id=$1 AND revision=indexed_revision AND group_id IS NOT NULL",
    )
    .bind(teacher_id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();
    let Some(group) = group else {
        return vec![];
    };
    let cfg = &state.config.memory;
    let response = client(2)
        .post(format!(
            "{}/search",
            cfg.url.as_deref().unwrap().trim_end_matches('/')
        ))
        .bearer_auth(cfg.token.as_deref().unwrap())
        .json(&json!({"group_id":group,
            "query":active.iter().map(|m|m.statement.as_str()).collect::<Vec<_>>().join(" "),
            "memory_ids":active.iter().map(|m|m.id).collect::<Vec<_>>()}))
        .send()
        .await;
    let Ok(response) = response else {
        return vec![];
    };
    if !response.status().is_success() {
        return vec![];
    }
    let Ok(v) = response.json::<Value>().await else {
        return vec![];
    };
    let mut ids = vec![];
    for id in v["memory_ids"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_i64)
    {
        if active.iter().any(|m| m.id == id) && !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyKind {
    Chat,
    Embeddings,
}

pub async fn chat(
    State(state): State<AppState>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> AppResult<(StatusCode, Json<Value>)> {
    forward(&state, &headers, body, ProxyKind::Chat).await
}
pub async fn embeddings(
    State(state): State<AppState>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> AppResult<(StatusCode, Json<Value>)> {
    forward(&state, &headers, body, ProxyKind::Embeddings).await
}

/// No upstream URL/key/model is accepted from the sidecar. Job tokens expire/revoke on
/// completion; every upstream request and response (including embedding calls) is audited.
async fn forward(
    state: &AppState,
    headers: &HeaderMap,
    mut body: Value,
    kind: ProxyKind,
) -> AppResult<(StatusCode, Json<Value>)> {
    let token =
        bearer_token(headers).ok_or_else(|| AppError::Unauthorized("需要服務驗證。".into()))?;
    let job: Option<(i64, i64)> = sqlx::query_as(
        "SELECT teacher_id,leased_revision FROM memory_indexes
        WHERE token_hash=$1 AND lease_until>clock_timestamp()",
    )
    .bind(sha256_hex(token))
    .fetch_optional(&state.db)
    .await?;
    let (teacher_id, revision) =
        job.ok_or_else(|| AppError::Unauthorized("服務驗證已失效。".into()))?;
    if !body.is_object() || body["stream"] == true {
        return Err(AppError::bad_request("不支援的模型請求。"));
    }
    let embedding = matches!(kind, ProxyKind::Embeddings);
    let c = &state.config;
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
        match c.ai_provider {
            AiProviderKind::OpenAi => (
                Some("https://api.openai.com/v1"),
                c.openai_api_key.as_deref(),
                c.ai_model.as_str(),
                "openai",
            ),
            AiProviderKind::OpenAiCompatible => (
                c.openai_compat_base_url.as_deref(),
                c.openai_compat_api_key.as_deref(),
                c.ai_model.as_str(),
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
    body["model"] = json!(model);
    let endpoint = if embedding {
        "embeddings"
    } else {
        "chat/completions"
    };
    if !embedding {
        let limit = body["max_tokens"]
            .as_u64()
            .unwrap_or(4096)
            .clamp(256, 16384);
        if c.ai_provider == AiProviderKind::OpenAi {
            body.as_object_mut().unwrap().remove("temperature");
            body.as_object_mut().unwrap().remove("max_tokens");
            body["max_completion_tokens"] =
                json!(limit + u64::from(crate::ai::openai::REASONING_ROOM));
            body["reasoning_effort"] = json!(crate::ai::openai::reasoning_effort(&c.ai_effort));
        } else {
            body["max_tokens"] = json!(limit);
        }
    }
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
    let snapshot = json!({"graphiti_version":GRAPHITI_VERSION,"policy_version":POLICY_VERSION,
        "revision":revision,"endpoint":endpoint,"request":body});
    let id =
        pipeline::insert_interaction(&state.db, &meta, provider, model, &snapshot, 1, None).await?;
    let start = Instant::now();
    let mut request = client(c.ai_timeout_secs)
        .post(format!("{}/{endpoint}", base.trim_end_matches('/')))
        .json(&body);
    if let Some(key) = key {
        request = request.bearer_auth(key);
    }
    let response = request.send().await;
    let (status, output) = match response {
        Ok(r) => {
            let status = r.status();
            match r.json::<Value>().await {
                Ok(output) => (status, output),
                Err(e) => (
                    if e.is_timeout() {
                        StatusCode::GATEWAY_TIMEOUT
                    } else {
                        StatusCode::BAD_GATEWAY
                    },
                    json!({"error":"invalid upstream response"}),
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
        ),
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
            output: Some(output.clone()),
            citations: vec![],
            check_results: None,
            model_version: output["model"].as_str().map(str::to_owned),
            error_detail: None,
            latency_ms: start.elapsed().as_millis().min(i32::MAX as u128) as i32,
        },
    )
    .await?;
    Ok((status, Json(output)))
}

#[derive(Serialize)]
pub struct IndexStatus {
    pub pending: i64,
    pub failed: i64,
}
pub async fn status(
    State(state): State<AppState>,
    teacher: crate::auth::AuthTeacher,
) -> AppResult<Json<IndexStatus>> {
    teacher.require_pi()?;
    let (pending, failed): (i64, i64) = sqlx::query_as(
        "SELECT count(*) FILTER(WHERE revision>indexed_revision),
        count(*) FILTER(WHERE last_error IS NOT NULL) FROM memory_indexes",
    )
    .fetch_one(&state.db)
    .await?;
    Ok(Json(IndexStatus { pending, failed }))
}
