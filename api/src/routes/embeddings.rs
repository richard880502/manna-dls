//! The semantic index of the knowledge base, for knowledge-base managers
//! ([`Capability::ManageKnowledge`]: PI or 知識庫管理員). docs/API.md「Phase 2.4」.

use axum::body::Bytes;
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::{AuthTeacher, Capability};
use crate::error::{AppError, AppResult};
use crate::kb_embeddings;
use crate::routes::log_usage;
use crate::AppState;

/// How many active items have a vector of the current model, and the latest indexing run.
pub async fn status(State(state): State<AppState>, teacher: AuthTeacher) -> AppResult<Json<Value>> {
    teacher.require(Capability::ManageKnowledge)?;
    Ok(Json(kb_embeddings::status(&state).await?))
}

#[derive(Debug, Default, Deserialize)]
pub struct RebuildRequest {
    /// `true`: embed every active item again, not only the ones without a current vector.
    #[serde(default)]
    pub force: bool,
}

/// Starts an indexing run in the background and returns the status to poll (202). When a run
/// is already in progress nothing new starts and `started` is `false`.
pub async fn rebuild(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    body: Bytes,
) -> AppResult<(StatusCode, Json<Value>)> {
    teacher.require(Capability::ManageKnowledge)?;
    // The body is optional: an empty one means `{}`.
    let req: RebuildRequest = if body.iter().all(u8::is_ascii_whitespace) {
        RebuildRequest::default()
    } else {
        serde_json::from_slice(&body)
            .map_err(|e| AppError::bad_request(format!("請求內容格式不正確：{e}")))?
    };
    if !state.embedder.enabled() {
        return Err(AppError::Conflict(
            "尚未設定語意索引模型（EMBEDDING_PROVIDER），目前只用關鍵字檢索。".into(),
        ));
    }
    let started = kb_embeddings::spawn_sync(&state, "manual", Some(teacher.id()), req.force);
    if started {
        log_usage(
            &state.db,
            teacher.id(),
            "knowledge_embeddings_rebuild",
            json!({ "force": req.force, "model": state.embedder.model() }),
        )
        .await?;
    }
    let mut status = kb_embeddings::status(&state).await?;
    status["started"] = json!(started);
    // The job row is written by the background task; until then report the run as running.
    status["running"] = json!(true);
    Ok((StatusCode::ACCEPTED, Json(status)))
}
