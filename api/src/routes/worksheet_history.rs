//! 修改歷程 (docs/API.md「修改歷程與採用過往的版本」): the earlier states of one block, and
//! adopting one of them. Every worksheet belongs to one teacher; another teacher's is a 404.

use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::audit::log_usage;
use crate::auth::AuthTeacher;
use crate::error::{ApiJson, ApiPath, ApiQuery, AppError, AppResult};
use crate::routes::worksheets::{sheet_of_version, state_of, SheetQuery, WorksheetState};
use crate::worksheet::assemble::Sheet;
use crate::worksheet::history::{self, History, Target};
use crate::worksheet::images::actions::PictureOutcome;
use crate::worksheet::restore;
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
    rv::expire_stale(
        db,
        id,
        crate::worksheet::ai::revise::stale_after(&state),
        state.config.ai_queue_enabled,
    )
    .await?;
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

// ---------------------------------------------------------------- 版本紀錄 (whole worksheet)

/// `GET /api/worksheets/:id/versions/:version_no/sheet?level&audience`: the sheet as that
/// version had it, with the pictures it showed.
pub async fn version_sheet(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath((id, version_no)): ApiPath<(i64, i32)>,
    ApiQuery(q): ApiQuery<SheetQuery>,
) -> AppResult<Json<Sheet>> {
    let ws = store::owned(&state.db, id, teacher.id()).await?;
    let (version, pictures) = store::version_at(&state.db, id, version_no)
        .await?
        .ok_or_else(|| AppError::not_found("找不到這一版。"))?;
    Ok(Json(
        sheet_of_version(&state, &ws, &version.content, pictures.as_ref(), &q).await?,
    ))
}

#[derive(Debug, Deserialize)]
pub struct RestoreRequest {
    /// The latest version the client has seen; a newer one is a 409.
    pub base_version_no: i32,
}

#[derive(Debug, Serialize)]
pub struct RestoreResult {
    /// The version the restore wrote (`cause = "restore"`).
    pub version_no: i32,
    /// The version whose content it put back.
    pub from_version_no: i32,
    pub worksheet: WorksheetState,
}

/// `POST /api/worksheets/:id/versions/:version_no/restore`
pub async fn restore_version(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath((id, version_no)): ApiPath<(i64, i32)>,
    ApiJson(req): ApiJson<RestoreRequest>,
) -> AppResult<Json<RestoreResult>> {
    let ws = store::owned(&state.db, id, teacher.id()).await?;
    let done = restore::restore(&state, &ws, teacher.id(), version_no, req.base_version_no).await?;
    Ok(Json(RestoreResult {
        version_no: done.version_no,
        from_version_no: done.from_version_no,
        worksheet: state_of(&state, id).await?,
    }))
}
