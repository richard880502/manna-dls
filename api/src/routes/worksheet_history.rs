//! 修改歷程 (docs/API.md「修改歷程與採用過往的版本」): the earlier states of one block, and
//! adopting one of them. Every worksheet belongs to one teacher; another teacher's is a 404.

use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::auth::AuthTeacher;
use crate::error::{ApiJson, ApiPath, AppError, AppResult};
use crate::routes::log_usage;
use crate::routes::worksheets::{state_of, WorksheetState};
use crate::worksheet::history::{self, History, Target};
use crate::worksheet::images::actions::PictureOutcome;
use crate::worksheet::revise as rv;
use crate::worksheet::store;
use crate::AppState;

/// `GET /api/worksheets/:id/blocks/:block_id/history`
pub async fn block_history(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath((id, block_id)): ApiPath<(i64, String)>,
) -> AppResult<Json<History>> {
    let db = &state.db;
    store::owned(db, id, teacher.id()).await?;
    rv::expire_stale(db, id, crate::worksheet::ai::revise::stale_after(&state)).await?;
    let latest = store::latest(db, id).await?;
    let current = latest
        .content
        .block(&block_id)
        .ok_or_else(|| AppError::not_found("找不到這個區塊。"))?
        .clone();
    let history = history::list(&state, id, &block_id, &current, latest.version_no).await?;
    log_usage(
        db,
        teacher.id(),
        "worksheet_block_history_viewed",
        json!({ "worksheet_id": id, "block_id": block_id, "steps": history.steps.len() }),
    )
    .await?;
    Ok(Json(history))
}

#[derive(Debug, Deserialize)]
pub struct AdoptRequest {
    /// Put the block back as it was in this version of the worksheet…
    #[serde(default)]
    pub version_no: Option<i32>,
    /// …or adopt this proposal that was not adopted at the time. Exactly one of the two.
    #[serde(default)]
    pub proposal_id: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct AdoptResult {
    /// The version 採用 wrote (`cause = "restore"`).
    pub version_no: i32,
    /// `version` | `proposal`
    pub kind: &'static str,
    /// What happened to the picture actions of a proposal (`[]` otherwise).
    pub pictures: Vec<PictureOutcome>,
    pub worksheet: WorksheetState,
}

/// `POST /api/worksheets/:id/blocks/:block_id/adopt`
pub async fn adopt_version(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath((id, block_id)): ApiPath<(i64, String)>,
    ApiJson(req): ApiJson<AdoptRequest>,
) -> AppResult<Json<AdoptResult>> {
    let ws = store::owned(&state.db, id, teacher.id()).await?;
    let target = match (req.version_no, req.proposal_id) {
        (Some(n), None) => Target::Version(n),
        (None, Some(p)) => Target::Proposal(p),
        _ => {
            return Err(AppError::bad_request(
                "請給 version_no 或 proposal_id 其中一個。",
            ))
        }
    };
    let done = history::adopt(&state, &ws, teacher.id(), &block_id, target).await?;
    Ok(Json(AdoptResult {
        version_no: done.version_no,
        kind: done.kind,
        pictures: done.pictures,
        worksheet: state_of(&state, id).await?,
    }))
}
