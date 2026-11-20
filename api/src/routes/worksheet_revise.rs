//! 學習單：逐區修改與註記 (docs/API.md「學習單：逐區修改與註記」): notes on a worksheet, a block
//! revised by Manna as a proposal, and 採用／放棄. Every worksheet belongs to one teacher; another
//! teacher's worksheet, note or proposal is a 404.

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::application::worksheets::WorksheetService;
use crate::audit::log_usage;
use crate::auth::AuthTeacher;
use crate::error::{ApiJson, ApiPath, AppError, AppResult};
use crate::routes::worksheets::{state_of, WorksheetState};
use crate::worksheet::ai::revise::{self as block_revise, ReviseRequest};
use crate::worksheet::comments::{
    self, AnchorInput, Comment, Patch, MAX_NOTE_CHARS, STATUS_DISMISSED, STATUS_OPEN,
};
use crate::worksheet::images;
use crate::worksheet::images::actions::{self, PictureOutcome};
use crate::worksheet::revise::{self as rv, Accepted, Proposal, MSG_CHANGED_MEANWHILE};
use crate::worksheet::store;
use crate::AppState;

// ---------------------------------------------------------------- 註記

pub async fn list_comments(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<Json<Vec<Comment>>> {
    let db = &state.db;
    store::owned(db, id, teacher.id()).await?;
    let content = store::latest(db, id).await?.content;
    Ok(Json(comments::list(db, id, teacher.id(), &content).await?))
}

#[derive(Debug, Deserialize)]
pub struct CreateComment {
    #[serde(flatten)]
    pub anchor: AnchorInput,
    #[serde(default)]
    pub note: String,
    /// 「提供給 AI」; on by default for a note on a worksheet.
    #[serde(default)]
    pub share_with_ai: Option<bool>,
}

fn check_note(note: &str) -> AppResult<()> {
    if note.chars().count() > MAX_NOTE_CHARS {
        return Err(AppError::bad_request(format!(
            "註記最多 {MAX_NOTE_CHARS} 字。"
        )));
    }
    Ok(())
}

pub async fn create_comment(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    ApiJson(req): ApiJson<CreateComment>,
) -> AppResult<Json<Comment>> {
    let db = &state.db;
    store::owned(db, id, teacher.id()).await?;
    let note = req.note.trim();
    check_note(note)?;
    let content = store::latest(db, id).await?.content;
    let anchor = comments::make_anchor(&req.anchor, &content)?;
    let created = comments::create(
        db,
        id,
        teacher.id(),
        &anchor,
        note,
        req.share_with_ai.unwrap_or(true),
        &content,
    )
    .await?;
    log_usage(
        db,
        teacher.id(),
        "worksheet_comment_created",
        json!({
            "worksheet_id": id, "comment_id": created.id, "field": created.field,
            "selection": anchor.range.is_some(), "share_with_ai": created.share_with_ai,
        }),
    )
    .await?;
    Ok(Json(created))
}

#[derive(Debug, Deserialize)]
pub struct UpdateComment {
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub share_with_ai: Option<bool>,
    /// `open`（待處理）or `dismissed`（略過）.
    #[serde(default)]
    pub status: Option<String>,
}

pub async fn update_comment(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath((id, comment_id)): ApiPath<(i64, i64)>,
    ApiJson(req): ApiJson<UpdateComment>,
) -> AppResult<Json<Comment>> {
    let db = &state.db;
    store::owned(db, id, teacher.id()).await?;
    comments::owned(db, id, comment_id, teacher.id()).await?;
    let note = req.note.as_deref().map(str::trim);
    if let Some(n) = note {
        check_note(n)?;
    }
    if let Some(s) = req.status.as_deref() {
        if s != STATUS_OPEN && s != STATUS_DISMISSED {
            return Err(AppError::bad_request("status 需為 open 或 dismissed。"));
        }
    }
    let content = store::latest(db, id).await?.content;
    let updated = comments::update(
        db,
        comment_id,
        Patch {
            note,
            share_with_ai: req.share_with_ai,
            status: req.status.as_deref(),
        },
        &content,
    )
    .await?;
    Ok(Json(updated))
}

pub async fn delete_comment(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath((id, comment_id)): ApiPath<(i64, i64)>,
) -> AppResult<Json<Value>> {
    let db = &state.db;
    store::owned(db, id, teacher.id()).await?;
    comments::owned(db, id, comment_id, teacher.id()).await?;
    comments::delete(db, comment_id).await?;
    Ok(Json(json!({ "deleted": true })))
}

// ---------------------------------------------------------------- 逐區修改

#[derive(Debug, Default, Deserialize)]
pub struct ReviseBlock {
    /// What to change, in the teacher's words (≤2000).
    #[serde(default)]
    pub instruction: String,
    /// The notes to revise by (open, 「提供給 AI」, in this block).
    #[serde(default)]
    pub comment_ids: Vec<i64>,
    /// 「可以動到這一區的其他題」
    #[serde(default)]
    pub whole_block: bool,
    /// Pictures uploaded to the worksheet's prep session, to send with the request (≤ 2).
    #[serde(default)]
    pub image_ids: Vec<i64>,
}

pub async fn revise_block(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath((id, block_id)): ApiPath<(i64, String)>,
    ApiJson(req): ApiJson<ReviseBlock>,
) -> AppResult<(StatusCode, Json<Proposal>)> {
    let ws = store::owned(&state.db, id, teacher.id()).await?;
    let input = WorksheetService::new(&state.db)
        .prepare_generation(teacher.id(), teacher.track(), ws)
        .await?;
    let proposal = block_revise::start(
        &state,
        input,
        &block_id,
        ReviseRequest {
            instruction: req.instruction,
            comment_ids: req.comment_ids,
            whole_block: req.whole_block,
            image_ids: req.image_ids,
        },
    )
    .await?;
    Ok((StatusCode::ACCEPTED, Json(proposal)))
}

pub async fn list_proposals(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<Json<Vec<Proposal>>> {
    let db = &state.db;
    store::owned(db, id, teacher.id()).await?;
    rv::expire_stale(db, id, block_revise::stale_after(&state)).await?;
    Ok(Json(rv::open(db, id).await?))
}

pub async fn get_proposal(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath((id, proposal_id)): ApiPath<(i64, i64)>,
) -> AppResult<Json<Proposal>> {
    let db = &state.db;
    store::owned(db, id, teacher.id()).await?;
    rv::expire_stale(db, id, block_revise::stale_after(&state)).await?;
    Ok(Json(rv::get(db, id, proposal_id).await?))
}

/// 採用: the proposal and the worksheet as it is now.
#[derive(Debug, Serialize)]
pub struct Decided {
    pub proposal: Proposal,
    /// What happened to the proposal's picture actions (`[]` without any): queued pictures
    /// appear in `worksheet.images` as they are drawn.
    pub pictures: Vec<PictureOutcome>,
    pub worksheet: WorksheetState,
}

pub async fn accept_proposal(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath((id, proposal_id)): ApiPath<(i64, i64)>,
) -> AppResult<Json<Decided>> {
    let db = &state.db;
    let ws = store::owned(db, id, teacher.id()).await?;
    if crate::worksheet::ai::jobs(&state, id).await?.1 {
        return Err(AppError::Conflict(
            "Manna 正在產生這份學習單，好了之後再採用。".to_string(),
        ));
    }
    let before = images::undo::snapshot(db, id).await?;
    let (proposal, added, saved, done, attempt, round_ids) =
        match rv::accept(db, id, proposal_id, teacher.id()).await? {
            Accepted::Done {
                proposal,
                added,
                content,
                changed,
                history_id,
                version_no,
                attempt,
                round_ids,
            } => (
                *proposal,
                added,
                *content,
                (changed, history_id, version_no),
                attempt,
                round_ids,
            ),
            Accepted::Stale(_) => {
                return Err(AppError::Conflict(MSG_CHANGED_MEANWHILE.to_string()))
            }
        };
    // 採用 is the teacher's yes to the pictures too: queue the new ones, redraw, remove.
    let pictures = actions::run(&state, &ws, &proposal.picture_actions, &added, &saved).await;
    // What the pictures did is kept with the step, so 復原 can give them back.
    let after = images::undo::snapshot(db, id).await?;
    let added_slots: std::collections::HashSet<String> =
        added.iter().filter_map(|(_, r)| r.clone().ok()).collect();
    let undo = images::undo::diff(&proposal.block_id, &before, &after, &added_slots);
    let proposal_version = rv::record_picture_step(
        db,
        &proposal,
        &rv::PicturesDone {
            changed: done.0,
            history_id: done.1,
            version_no: done.2,
            undo,
            after,
        },
    )
    .await?;
    let mut proposal = proposal;
    proposal.accepted_version_no = Some(proposal_version);
    if !pictures.is_empty() {
        actions::log_applied(
            &state,
            teacher.id(),
            id,
            "block_revise",
            json!({ "proposal_id": proposal.id, "block_id": proposal.block_id }),
            &pictures,
        )
        .await;
    }
    log_usage(
        db,
        teacher.id(),
        "worksheet_block_revise_accepted",
        json!({
            "worksheet_id": id, "block_id": proposal.block_id, "proposal_id": proposal.id,
            "round": proposal.round, "version_no": proposal.accepted_version_no,
            "attempt_no": attempt.0, "attempts": attempt.1, "attempt_proposal_ids": round_ids,
        }),
    )
    .await?;
    Ok(Json(Decided {
        proposal,
        pictures,
        worksheet: state_of(&state, id).await?,
    }))
}

pub async fn reject_proposal(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath((id, proposal_id)): ApiPath<(i64, i64)>,
) -> AppResult<Json<Proposal>> {
    let db = &state.db;
    store::owned(db, id, teacher.id()).await?;
    let proposal = rv::reject(db, id, proposal_id, teacher.id()).await?;
    log_usage(
        db,
        teacher.id(),
        "worksheet_block_revise_rejected",
        json!({
            "worksheet_id": id, "block_id": proposal.block_id, "proposal_id": proposal.id,
            "round": proposal.round, "status": proposal.status,
        }),
    )
    .await?;
    Ok(Json(proposal))
}
