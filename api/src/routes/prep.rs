//! 備課對話 endpoints (docs/API.md「Phase 2.6：備課對話」): sessions, their turns (all run in the
//! background; the page polls `GET /api/prep-sessions/:id`), the context PATCH and the hard
//! delete. Every session belongs to one teacher; anyone else's id is a 404.

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::auth::AuthTeacher;
use crate::error::{ApiJson, ApiPath, AppError, AppResult};
use crate::materials::store as material_store;
use crate::prep::run::{self, TurnInput};
use crate::prep::store::{self, Session, SessionRow, Summary};
use crate::prep::{
    chip_label, is_worksheet_kind, AUTO_TITLE_CHARS, KINDS, KIND_ACTIVITY, KIND_ASK,
    KIND_RESOURCES, KIND_REVISE, MAX_REQUEST_CHARS, MAX_TITLE_CHARS, MAX_TOPIC_CHARS,
};
use crate::routes::{clip_chars, double_option, log_usage, owned_class};
use crate::worksheet::ai as ws_ai;
use crate::worksheet::template::extract::{MSG_MATERIAL_NOT_FOUND, MSG_NO_TEXT};
use crate::worksheet::template::store as tpl_store;
use crate::AppState;

const MSG_KIND: &str = "kind 需為 worksheet、assessment、activity、resources、ask 或 revise。";
const MSG_NEEDS_MATERIAL: &str = "這個動作需要教材：請上傳檔案、從教材庫選，或貼上課文。";
const MSG_NEEDS_MATERIAL_OR_TOPIC: &str = "找教學資源需要教材或主題。";
const MSG_NEEDS_REQUEST: &str = "請寫下你想問或想改的內容。";
const MSG_MATERIAL_DELETED: &str = "這個備課對話的教材已經刪除了，請開一個新的備課對話。";
const MSG_NO_WORKSHEET: &str = "這個備課對話還沒有學習單可以修改。";
const MSG_WORKSHEET_RUNNING: &str = "Manna 正在產生這份學習單，好了之後再改。";

#[derive(Debug, Serialize)]
pub struct SessionList {
    pub sessions: Vec<Summary>,
}

pub async fn list(
    State(state): State<AppState>,
    teacher: AuthTeacher,
) -> AppResult<Json<SessionList>> {
    Ok(Json(SessionList {
        sessions: store::list(&state.db, teacher.id()).await?,
    }))
}

fn check_kind(kind: &str) -> AppResult<()> {
    if KINDS.contains(&kind) {
        Ok(())
    } else {
        Err(AppError::bad_request(MSG_KIND))
    }
}

fn check_request(request: &str) -> AppResult<String> {
    let request = request.trim();
    if request.chars().count() > MAX_REQUEST_CHARS {
        return Err(AppError::bad_request(format!(
            "內容最多 {MAX_REQUEST_CHARS} 字。"
        )));
    }
    Ok(request.to_string())
}

/// The request a turn records: the teacher's words, or the chip's label.
fn request_or_label(kind: &str, request: String) -> String {
    if request.is_empty() {
        chip_label(kind).to_string()
    } else {
        request
    }
}

#[derive(Debug, Default, Deserialize)]
pub struct PastedMaterial {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub text: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateSession {
    pub kind: String,
    #[serde(default)]
    pub request: Option<String>,
    #[serde(default)]
    pub material_id: Option<i64>,
    #[serde(default)]
    pub material: Option<PastedMaterial>,
    #[serde(default)]
    pub topic: Option<String>,
    #[serde(default)]
    pub class_id: Option<i64>,
    #[serde(default)]
    pub template_id: Option<i64>,
}

/// A library material the session can work with: the teacher's own, with extracted text.
async fn usable_material(db: &sqlx::PgPool, teacher_id: i64, id: i64) -> AppResult<(i64, String)> {
    let row = material_store::owned(db, id, teacher_id)
        .await
        .map_err(|_| AppError::bad_request(MSG_MATERIAL_NOT_FOUND))?;
    if row.text_status != "ready" || row.paragraphs.0.is_empty() {
        return Err(AppError::bad_request(MSG_NO_TEXT));
    }
    Ok((row.id, row.title))
}

async fn spawn_turn(
    state: &AppState,
    teacher: &AuthTeacher,
    session: SessionRow,
    kind: &str,
    request: &str,
    worksheet_id: Option<i64>,
) -> AppResult<()> {
    let turn = store::start_turn(&state.db, session.id, kind, request).await?;
    log_usage(
        &state.db,
        teacher.id(),
        "prep_turn_created",
        json!({ "session_id": session.id, "turn_id": turn.id, "kind": kind }),
    )
    .await?;
    run::start(
        state,
        TurnInput {
            turn,
            session,
            track: teacher.track().to_string(),
            worksheet_id,
        },
    )
    .await
}

pub async fn create(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiJson(req): ApiJson<CreateSession>,
) -> AppResult<(StatusCode, Json<Session>)> {
    let db = &state.db;
    let kind = req.kind.trim();
    check_kind(kind)?;
    if kind == KIND_REVISE {
        return Err(AppError::bad_request(
            "新的備課對話不能從「修改」開始；請先做學習單或設計評量。",
        ));
    }
    let request = check_request(req.request.as_deref().unwrap_or_default())?;
    let topic = req
        .topic
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string);
    if topic
        .as_deref()
        .is_some_and(|t| t.chars().count() > MAX_TOPIC_CHARS)
    {
        return Err(AppError::bad_request(format!(
            "主題最多 {MAX_TOPIC_CHARS} 字。"
        )));
    }
    let pasted = req
        .material
        .as_ref()
        .filter(|m| !m.text.trim().is_empty() || m.title.is_some());
    let has_material = req.material_id.is_some() || pasted.is_some();
    match kind {
        KIND_ASK if request.is_empty() => {
            return Err(AppError::bad_request("請寫下你想問 Manna 的問題。"))
        }
        KIND_RESOURCES if !has_material && topic.is_none() => {
            return Err(AppError::bad_request(MSG_NEEDS_MATERIAL_OR_TOPIC))
        }
        KIND_ASK | KIND_RESOURCES => {}
        _ if !has_material => return Err(AppError::bad_request(MSG_NEEDS_MATERIAL)),
        _ => {}
    }
    if let Some(class_id) = req.class_id {
        owned_class(db, class_id, teacher.id()).await?;
    }
    if let Some(template_id) = req.template_id {
        tpl_store::ready_for_use(db, teacher.id(), template_id).await?;
    }
    // A library material wins over pasted text (which is then not stored), as for worksheets.
    let material: Option<(i64, String)> = match (req.material_id, pasted) {
        (Some(id), _) => Some(usable_material(db, teacher.id(), id).await?),
        (None, Some(p)) => {
            let title = p.title.as_deref().unwrap_or_default().trim();
            let row = material_store::create_text(db, teacher.id(), title, "", &p.text).await?;
            if row.text_status != "ready" || row.paragraphs.0.is_empty() {
                return Err(AppError::bad_request(MSG_NO_TEXT));
            }
            Some((row.id, row.title))
        }
        (None, None) => None,
    };
    let request = request_or_label(kind, request);
    let title = match (&material, &topic) {
        (Some((_, t)), _) if !t.trim().is_empty() => clip_chars(t.trim(), MAX_TITLE_CHARS - 1),
        (_, Some(t)) => clip_chars(t, MAX_TITLE_CHARS - 1),
        _ => clip_chars(&request, AUTO_TITLE_CHARS),
    };
    let session = store::insert(
        db,
        store::NewSession {
            teacher_id: teacher.id(),
            title: &title,
            material_id: material.as_ref().map(|(id, _)| *id),
            topic: topic.as_deref(),
            class_id: req.class_id,
            template_id: req.template_id,
        },
    )
    .await?;
    log_usage(
        db,
        teacher.id(),
        "prep_session_created",
        json!({
            "session_id": session.id, "kind": kind, "material_id": session.material_id,
            "class_id": session.class_id, "template_id": session.template_id,
        }),
    )
    .await?;
    let id = session.id;
    spawn_turn(&state, &teacher, session, kind, &request, None).await?;
    Ok((StatusCode::CREATED, Json(store::session(&state, id).await?)))
}

pub async fn get(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<Json<Session>> {
    store::owned(&state.db, id, teacher.id()).await?;
    Ok(Json(store::session(&state, id).await?))
}

#[derive(Debug, Deserialize)]
pub struct PatchSession {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub class_id: Option<Option<i64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub template_id: Option<Option<i64>>,
}

pub async fn patch(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    ApiJson(req): ApiJson<PatchSession>,
) -> AppResult<Json<Session>> {
    let db = &state.db;
    store::owned(db, id, teacher.id()).await?;
    let title = match req.title.as_deref().map(str::trim) {
        Some("") => return Err(AppError::bad_request("標題不能是空的。")),
        Some(t) if t.chars().count() > MAX_TITLE_CHARS => {
            return Err(AppError::bad_request(format!(
                "標題最多 {MAX_TITLE_CHARS} 字。"
            )))
        }
        other => other,
    };
    if let Some(Some(class_id)) = req.class_id {
        owned_class(db, class_id, teacher.id()).await?;
    }
    if let Some(Some(template_id)) = req.template_id {
        tpl_store::ready_for_use(db, teacher.id(), template_id).await?;
    }
    store::update(db, id, title, req.class_id, req.template_id).await?;
    Ok(Json(store::session(&state, id).await?))
}

pub async fn delete(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<Json<serde_json::Value>> {
    store::owned(&state.db, id, teacher.id()).await?;
    store::delete(&state.db, id).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Debug, Deserialize)]
pub struct CreateTurn {
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub request: Option<String>,
}

pub async fn create_turn(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    ApiJson(req): ApiJson<CreateTurn>,
) -> AppResult<(StatusCode, Json<Session>)> {
    let db = &state.db;
    let session = store::owned(db, id, teacher.id()).await?;
    let request = check_request(req.request.as_deref().unwrap_or_default())?;
    store::refresh(&state, id).await?;
    let turns = store::turns(db, id).await?;
    let kind = match req.kind.as_deref().map(str::trim).filter(|k| !k.is_empty()) {
        Some(k) => {
            check_kind(k)?;
            k.to_string()
        }
        // The bottom bar: a revision once there is a worksheet, otherwise a question.
        None if store::has_worksheet_turn(&turns) => KIND_REVISE.to_string(),
        None => KIND_ASK.to_string(),
    };
    let kind = kind.as_str();
    let has_material = session.material_id.is_some();
    let has_topic = session
        .topic
        .as_deref()
        .is_some_and(|t| !t.trim().is_empty());
    let mut worksheet_id = None;
    match kind {
        KIND_ASK | KIND_REVISE if request.is_empty() => {
            return Err(AppError::bad_request(MSG_NEEDS_REQUEST))
        }
        KIND_RESOURCES if !has_material && !has_topic => {
            return Err(AppError::bad_request(MSG_NEEDS_MATERIAL_OR_TOPIC))
        }
        k if (is_worksheet_kind(k) || k == KIND_ACTIVITY) && !has_material => {
            let had = turns
                .iter()
                .any(|t| t.kind != KIND_ASK && t.kind != KIND_RESOURCES);
            return Err(AppError::bad_request(if had {
                MSG_MATERIAL_DELETED
            } else {
                MSG_NEEDS_MATERIAL
            }));
        }
        KIND_REVISE => {
            let Some(target) = store::revise_target(db, id).await? else {
                return Err(AppError::Conflict(MSG_NO_WORKSHEET.to_string()));
            };
            if ws_ai::jobs(&state, target).await?.1 {
                return Err(AppError::Conflict(MSG_WORKSHEET_RUNNING.to_string()));
            }
            worksheet_id = Some(target);
        }
        _ => {}
    }
    if turns
        .iter()
        .any(|t| t.status == crate::prep::STATUS_RUNNING)
    {
        return Err(AppError::Conflict(store::MSG_BUSY.to_string()));
    }
    let request = request_or_label(kind, request);
    spawn_turn(&state, &teacher, session, kind, &request, worksheet_id).await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(store::session(&state, id).await?),
    ))
}
