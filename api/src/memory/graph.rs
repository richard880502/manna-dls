//! Durable Graphiti synchronization and per-job, audited OpenAI-compatible forwarding.
//!
//! The forwarding endpoints are served only by the internal listener (`INTERNAL_PORT`,
//! [`crate::build_internal_app`]): the public router and the reverse proxy never route to them.
use std::time::{Duration, Instant};

use axum::{
    extract::{DefaultBodyLimit, Request, State},
    http::{HeaderMap, StatusCode},
    routing::post,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use super::{Memory, POLICY_VERSION, SIDECAR_API_VERSION};
use crate::ai::pipeline::{self, Finish, InteractionMeta};
use crate::auth::{bearer_token, new_token, sha256_hex};
use crate::config::AiProviderKind;
use crate::error::{AppError, AppResult};
use crate::AppState;

/// Path prefix of the forwarding endpoints on the internal listener.
pub const INTERNAL_PREFIX: &str = "/api/internal/memory/v1";
/// How long a `/health` answer (compatible or not) is reused before asking again.
pub const HEALTH_TTL: Duration = Duration::from_secs(45);
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
/// Grace period before a replaced group is deleted, so searches already under way finish.
const REPLACED_GROUP_GRACE: &str = "2 minutes";
/// How often the worker lists the sidecar's groups to collect ones nothing refers to.
const SWEEP_EVERY: Duration = Duration::from_secs(600);

#[derive(Clone)]
pub struct MemoryConfig {
    pub url: Option<String>,
    pub token: Option<String>,
    pub proxy_url: Option<String>,
    pub health_ttl: Duration,
    /// Why the index stays off although it was configured (said once at startup).
    pub warning: Option<String>,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            url: None,
            token: None,
            proxy_url: None,
            health_ttl: HEALTH_TTL,
            warning: None,
        }
    }
}

// Hand-written: the internal service secret never reaches logs.
impl std::fmt::Debug for MemoryConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryConfig")
            .field("url", &self.url)
            .field("token_set", &self.token.is_some())
            .field("proxy_url", &self.proxy_url)
            .field("warning", &self.warning)
            .finish()
    }
}

impl MemoryConfig {
    /// `providers`: whether the configured chat model and embeddings can be forwarded
    /// (OpenAI or OpenAI-compatible). Otherwise the index stays off: Phase 1 only.
    pub fn from_lookup(
        get: impl Fn(&str) -> Option<String>,
        providers: Result<(), &'static str>,
    ) -> Self {
        let nonempty = |k| get(k).filter(|v| !v.trim().is_empty());
        let mut c = Self {
            url: nonempty("GRAPHITI_URL"),
            token: nonempty("GRAPHITI_TOKEN"),
            proxy_url: nonempty("MEMORY_PROXY_URL"),
            ..Self::default()
        };
        if c.url.is_some() != c.token.is_some() {
            c.warning = Some("GRAPHITI_URL and GRAPHITI_TOKEN must both be set".into());
            c.url = None;
        } else if c.enabled() {
            if let Err(why) = providers {
                c.warning = Some(format!("the Graphiti index needs {why}"));
                c.url = None;
            }
        } else if c.url.is_some() {
            c.warning = Some("MEMORY_PROXY_URL is not set".into());
            c.url = None;
        }
        c
    }
    pub fn enabled(&self) -> bool {
        self.url.is_some() && self.token.is_some() && self.proxy_url.is_some()
    }
    fn base(&self) -> &str {
        self.url
            .as_deref()
            .unwrap_or_default()
            .trim_end_matches('/')
    }
    fn secret(&self) -> &str {
        self.token.as_deref().unwrap_or_default()
    }
}

/// Process-wide state of the index client (cached health, last orphan sweep).
#[derive(Default)]
pub struct Runtime {
    health: tokio::sync::Mutex<Option<(Instant, Option<SidecarInfo>)>>,
    last_sweep: std::sync::Mutex<Option<Instant>>,
}

fn client(timeout: u64) -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout))
        .build()
        .expect("TLS client")
}

/// The HTTP contract is versioned independently of the Python engine. A core
/// upgrade with the same adapter contract never requires a Manna redeployment.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SidecarInfo {
    pub ok: bool,
    pub api_version: u32,
    pub graphiti_version: String,
    pub policy_version: String,
}
impl SidecarInfo {
    fn compatible(&self) -> bool {
        self.ok
            && self.api_version == SIDECAR_API_VERSION
            && self.policy_version == POLICY_VERSION
            && !self.graphiti_version.trim().is_empty()
            && self.graphiti_version.len() <= 64
    }
}

async fn fetch_health(cfg: &MemoryConfig) -> Option<SidecarInfo> {
    let response = client(2)
        .get(format!("{}/health", cfg.base()))
        .bearer_auth(cfg.secret())
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    response
        .json::<SidecarInfo>()
        .await
        .ok()
        .filter(SidecarInfo::compatible)
}

/// The sidecar's health, reused for [`MemoryConfig::health_ttl`]: searches and the worker do
/// not ask before every call. `None` when offline or incompatible (Postgres only).
pub async fn compatible_sidecar(state: &AppState) -> Option<SidecarInfo> {
    let cfg = &state.config.memory;
    if !cfg.enabled() {
        return None;
    }
    let mut cache = state.memory.health.lock().await;
    if let Some((at, info)) = cache.as_ref() {
        if at.elapsed() < cfg.health_ttl {
            return info.clone();
        }
    }
    let info = fetch_health(cfg).await;
    *cache = Some((Instant::now(), info.clone()));
    info
}

/// Ask again on the next call (after a version mismatch or a failed index call).
pub async fn forget_health(state: &AppState) {
    *state.memory.health.lock().await = None;
}

pub fn spawn_worker(state: AppState) {
    if !state.config.memory.enabled() {
        return;
    }
    tokio::spawn(async move {
        match reconcile(&state).await {
            Ok(0) => {}
            Ok(n) => tracing::info!(teachers = n, "queued memory index rebuilds after restart"),
            Err(e) => tracing::warn!(error = %e, "could not reconcile the memory index"),
        }
        loop {
            match sync_one(&state).await {
                Ok(true) => continue,
                Ok(false) => {}
                Err(e) => tracing::warn!(error = %e, "memory index worker failed; retrying"),
            }
            let due = {
                let mut last = state.memory.last_sweep.lock().expect("sweep clock");
                let due = last.is_none_or(|t| t.elapsed() >= SWEEP_EVERY);
                if due {
                    *last = Some(Instant::now());
                }
                due
            };
            if due && compatible_sidecar(&state).await.is_some() {
                if let Err(e) = sweep_orphans(&state).await {
                    tracing::warn!(error = %e, "could not list memory index groups");
                }
            }
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    });
}

/// Queue every teacher whose habits changed after the published snapshot was read (changes
/// made while the index was not configured are not queued then) or who has none yet.
pub async fn reconcile(state: &AppState) -> sqlx::Result<u64> {
    if !state.config.memory.enabled() {
        return Ok(0);
    }
    Ok(sqlx::query(
        "INSERT INTO memory_indexes (teacher_id)
        SELECT teacher_id FROM teacher_memories GROUP BY teacher_id
        ON CONFLICT (teacher_id) DO UPDATE SET revision = memory_indexes.revision + 1,
            attempts = 0, next_attempt_at = now(), updated_at = now()
        WHERE memory_indexes.revision = memory_indexes.indexed_revision
          AND (memory_indexes.indexed_snapshot_at IS NULL OR memory_indexes.indexed_snapshot_at <
              (SELECT max(greatest(t.valid_at, coalesce(t.invalid_at, t.valid_at)))
                 FROM teacher_memories t WHERE t.teacher_id = memory_indexes.teacher_id))",
    )
    .execute(&state.db)
    .await?
    .rows_affected())
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
    // Incompatible/offline sidecars leave the durable queue untouched. The next
    // poll recovers automatically when a compatible deployment returns.
    let Some(sidecar) = compatible_sidecar(state).await else {
        return Ok(false);
    };
    collect_garbage(state).await?;
    let token = new_token();
    let work: Option<Work> = sqlx::query_as(
        "UPDATE memory_indexes SET lease_until=clock_timestamp()+interval '10 minutes',
        leased_at=clock_timestamp(),token_hash=$1,leased_revision=revision,
        leased_graphiti_version=$2,job_calls=0
        WHERE teacher_id=(SELECT teacher_id FROM memory_indexes
            WHERE revision>indexed_revision AND next_attempt_at<=now()
            AND (lease_until IS NULL OR lease_until<clock_timestamp())
            ORDER BY next_attempt_at,teacher_id FOR UPDATE SKIP LOCKED LIMIT 1)
        RETURNING teacher_id,revision,group_id",
    )
    .bind(sha256_hex(&token))
    .bind(&sidecar.graphiti_version)
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
    // Tracked before the sidecar creates it: an attempt that is never published is deleted
    // after its lease (10 min) and the HTTP limit (8 min) have certainly passed.
    sqlx::query(
        "INSERT INTO memory_index_garbage(group_id,not_before) VALUES($1,now()+interval '15 minutes')",
    )
    .bind(&group)
    .execute(&state.db)
    .await?;
    let active = super::list(&state.db, work.teacher_id, false).await?;
    let cfg = &state.config.memory;
    let result = client(480)
        .post(format!("{}/index", cfg.base()))
        .bearer_auth(cfg.secret())
        .json(&json!({
            "group_id":group,"memories":active,"proxy_url":cfg.proxy_url,
            "proxy_key":token,"model":state.ai.active().model,
            "embedding_model":state.config.embedding.model,
            "embedding_dims":state.config.embedding.dims,
            "policy_version":POLICY_VERSION,"api_version":SIDECAR_API_VERSION,
        }))
        .send()
        .await;
    let success = match result {
        Ok(r) if r.status().is_success() => r.json::<Value>().await.ok().is_some_and(|v| {
            v["ok"] == true
                && v["api_version"] == SIDECAR_API_VERSION
                && v["graphiti_version"] == sidecar.graphiti_version
                && v["policy_version"] == POLICY_VERSION
        }),
        _ => false,
    };
    if success {
        let mut tx = state.db.begin().await?;
        // Compare-and-set: only the revision this job read, by this job, inside its lease.
        let published = sqlx::query(
            "UPDATE memory_indexes SET indexed_revision=$2,group_id=$3,
            indexed_graphiti_version=$5,indexed_snapshot_at=leased_at,attempts=0,
            lease_until=NULL,token_hash=NULL,last_error=NULL,updated_at=now()
            WHERE teacher_id=$1 AND revision=$2 AND token_hash=$4
              AND lease_until>clock_timestamp()",
        )
        .bind(work.teacher_id)
        .bind(work.revision)
        .bind(&group)
        .bind(sha256_hex(&token))
        .bind(&sidecar.graphiti_version)
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
                sqlx::query(&format!(
                    "INSERT INTO memory_index_garbage(group_id,not_before)
                    VALUES($1,now()+interval '{REPLACED_GROUP_GRACE}') ON CONFLICT DO NOTHING"
                ))
                .bind(old)
                .execute(&mut *tx)
                .await?;
            }
        }
        tx.commit().await?;
        if !published {
            // A newer change arrived meanwhile: the attempt group stays queued for deletion.
            release(state, work.teacher_id, &token, false).await?;
        }
    } else {
        forget_health(state).await;
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

/// Deletes queued groups whose time has come. The published group of any teacher is never
/// deleted, even if it was queued by mistake.
pub async fn collect_garbage(state: &AppState) -> AppResult<()> {
    let groups: Vec<String> = sqlx::query_scalar("SELECT group_id FROM memory_index_garbage g
        WHERE not_before<=now() AND NOT EXISTS(SELECT 1 FROM memory_indexes i WHERE i.group_id=g.group_id)
        ORDER BY not_before LIMIT 5").fetch_all(&state.db).await?;
    let cfg = &state.config.memory;
    for group in groups {
        let response = client(5)
            .delete(format!("{}/groups/{}", cfg.base(), group))
            .bearer_auth(cfg.secret())
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

fn is_group_name(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("manna_t") else {
        return false;
    };
    let mut parts = rest.splitn(2, "_r");
    let (Some(teacher), Some(rest)) = (parts.next(), parts.next()) else {
        return false;
    };
    let Some((revision, suffix)) = rest.split_once('_') else {
        return false;
    };
    !teacher.is_empty()
        && teacher.bytes().all(|b| b.is_ascii_digit())
        && !revision.is_empty()
        && revision.bytes().all(|b| b.is_ascii_digit())
        && suffix.len() == 12
        && suffix
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Queues groups that exist in FalkorDB but that nothing refers to (e.g. an empty graph left
/// by an older sidecar's search). In-flight attempts are already queued with their delay, the
/// published groups are skipped. Returns how many were newly queued.
pub async fn sweep_orphans(state: &AppState) -> AppResult<usize> {
    let cfg = &state.config.memory;
    if !cfg.enabled() {
        return Ok(0);
    }
    let response = client(5)
        .get(format!("{}/groups", cfg.base()))
        .bearer_auth(cfg.secret())
        .send()
        .await
        .map_err(|e| AppError::Unavailable(format!("memory groups: {e}")))?;
    if !response.status().is_success() {
        return Err(AppError::Unavailable("memory groups unavailable".into()));
    }
    let listed: Value = response
        .json()
        .await
        .map_err(|e| AppError::Unavailable(format!("memory groups: {e}")))?;
    let mut queued = 0;
    for group in listed["groups"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|g| is_group_name(g))
    {
        queued += sqlx::query(
            "INSERT INTO memory_index_garbage(group_id,not_before)
            SELECT $1, now()+interval '15 minutes'
            WHERE NOT EXISTS(SELECT 1 FROM memory_indexes WHERE group_id=$1)
            ON CONFLICT DO NOTHING",
        )
        .bind(group)
        .execute(&state.db)
        .await?
        .rows_affected() as usize;
    }
    Ok(queued)
}

/// What the index said about one generation step. Metadata only: the prompt always carries
/// every active habit in confirmation order, whatever the index answers.
#[derive(Debug, Clone, Serialize, Default)]
pub struct GraphObservation {
    /// `not_configured`, `no_habits`, `unavailable`, `not_indexed`, `core_changed`,
    /// `missing_group`, `empty_group`, `error` or `ranked`.
    pub status: &'static str,
    /// Graphiti's ranking of the active habit ids (BM25), when it answered.
    pub ranked_ids: Vec<i64>,
    pub group_id: Option<String>,
    pub revision: Option<i64>,
    /// The core that built the searched group.
    pub graphiti_version: Option<String>,
    /// The teacher was queued for a rebuild because the group cannot be trusted.
    pub reindex_requested: bool,
}

fn observed(status: &'static str) -> GraphObservation {
    GraphObservation {
        status,
        ..GraphObservation::default()
    }
}

/// Queue a rebuild of the published group `group` (only if it is still the published one).
async fn request_reindex(state: &AppState, teacher_id: i64, group: &str) -> bool {
    sqlx::query(
        "UPDATE memory_indexes SET revision=revision+1,attempts=0,next_attempt_at=now(),
        updated_at=now() WHERE teacher_id=$1 AND group_id=$2 AND revision=indexed_revision",
    )
    .bind(teacher_id)
    .bind(group)
    .execute(&state.db)
    .await
    .is_ok_and(|r| r.rows_affected() > 0)
}

/// Search the published group with the canonical habits only (never raw conversation or
/// material). Graph results can never introduce a fact or another teacher's habit: ids are
/// intersected with the teacher's active ids. A group built by another core, or one that is
/// missing or empty while Postgres has habits, is never searched and the teacher is rebuilt.
pub async fn observe(state: &AppState, teacher_id: i64, active: &[Memory]) -> GraphObservation {
    if !state.config.memory.enabled() {
        return observed("not_configured");
    }
    if active.is_empty() {
        return observed("no_habits");
    }
    let Some(sidecar) = compatible_sidecar(state).await else {
        return observed("unavailable");
    };
    let published: Option<(String, i64, Option<String>)> = sqlx::query_as(
        "SELECT group_id, revision, indexed_graphiti_version FROM memory_indexes
        WHERE teacher_id=$1 AND revision=indexed_revision AND group_id IS NOT NULL",
    )
    .bind(teacher_id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();
    let Some((group, revision, built_by)) = published else {
        return observed("not_indexed");
    };
    let mut out = GraphObservation {
        status: "error",
        group_id: Some(group.clone()),
        revision: Some(revision),
        graphiti_version: built_by.clone(),
        ..GraphObservation::default()
    };
    if built_by.as_deref() != Some(sidecar.graphiti_version.as_str()) {
        out.status = "core_changed";
        out.reindex_requested = request_reindex(state, teacher_id, &group).await;
        return out;
    }
    let cfg = &state.config.memory;
    let response = client(2)
        .post(format!("{}/search", cfg.base()))
        .bearer_auth(cfg.secret())
        .json(&json!({"group_id":group,
            "query":active.iter().map(|m|m.statement.as_str()).collect::<Vec<_>>().join(" "),
            "memory_ids":active.iter().map(|m|m.id).collect::<Vec<_>>()}))
        .send()
        .await;
    let Ok(response) = response else {
        return out;
    };
    if !response.status().is_success() {
        return out;
    }
    let Ok(v) = response.json::<Value>().await else {
        return out;
    };
    match v["group"].as_str() {
        Some("missing") | Some("empty") => {
            out.status = if v["group"] == "missing" {
                "missing_group"
            } else {
                "empty_group"
            };
            out.reindex_requested = request_reindex(state, teacher_id, &group).await;
            return out;
        }
        Some("ok") => {}
        _ => return out,
    }
    for id in v["memory_ids"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_i64)
    {
        if active.iter().any(|m| m.id == id) && !out.ranked_ids.contains(&id) {
            out.ranked_ids.push(id);
        }
    }
    out.status = "ranked";
    out
}

// ------------------------------------------------------------------ model forwarding

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
    let ai = state.ai.active();
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

#[derive(Serialize)]
pub struct IndexStatus {
    pub pending: i64,
    pub failed: i64,
    pub graph_enabled: bool,
    pub compatible: bool,
    pub sidecar: Option<SidecarInfo>,
}

/// PI view. Without a configured index there is nothing to synchronize: both counters are 0.
pub async fn status(
    State(state): State<AppState>,
    teacher: crate::auth::AuthTeacher,
) -> AppResult<Json<IndexStatus>> {
    teacher.require_pi()?;
    if !state.config.memory.enabled() {
        return Ok(Json(IndexStatus {
            pending: 0,
            failed: 0,
            graph_enabled: false,
            compatible: false,
            sidecar: None,
        }));
    }
    let (pending, failed): (i64, i64) = sqlx::query_as(
        "SELECT count(*) FILTER(WHERE revision>indexed_revision),
        count(*) FILTER(WHERE last_error IS NOT NULL) FROM memory_indexes",
    )
    .fetch_one(&state.db)
    .await?;
    let sidecar = compatible_sidecar(&state).await;
    Ok(Json(IndexStatus {
        pending,
        failed,
        graph_enabled: true,
        compatible: sidecar.is_some(),
        sidecar,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_names_are_recognised_exactly() {
        assert!(is_group_name("manna_t12_r3_0123456789ab"));
        for bad in [
            "manna_t12_r3_0123456789AB",
            "manna_t_r3_0123456789ab",
            "manna_t12_r3_0123456789a",
            "manna_memory",
            "other_t1_r1_0123456789ab",
        ] {
            assert!(!is_group_name(bad), "{bad}");
        }
    }

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

    #[test]
    fn index_stays_off_without_forwardable_providers_or_with_partial_settings() {
        let all = |k: &str| match k {
            "GRAPHITI_URL" => Some("http://manna-memory:8000".to_string()),
            "GRAPHITI_TOKEN" => Some("secret".to_string()),
            "MEMORY_PROXY_URL" => Some("http://manna-api:8081/api/internal/memory/v1".to_string()),
            _ => None,
        };
        let on = MemoryConfig::from_lookup(all, Ok(()));
        assert!(on.enabled() && on.warning.is_none());
        assert!(!format!("{on:?}").contains("secret"));
        let off = MemoryConfig::from_lookup(all, Err("an OpenAI-compatible chat model"));
        assert!(!off.enabled());
        assert!(off.warning.unwrap().contains("OpenAI-compatible"));
        let partial = MemoryConfig::from_lookup(
            |k| (k == "GRAPHITI_URL").then(|| "http://x".to_string()),
            Ok(()),
        );
        assert!(!partial.enabled() && partial.warning.is_some());
        let unset = MemoryConfig::from_lookup(
            |k| (k == "MEMORY_PROXY_URL").then(|| "http://x".to_string()),
            Ok(()),
        );
        assert!(!unset.enabled() && unset.warning.is_none());
    }
}
