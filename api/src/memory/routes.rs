use axum::extract::State;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use super::candidates::{self, Dismissal, Filter};
use super::{dirty, extract, list, IndexChange, MAX_ACTIVE};
use crate::auth::AuthTeacher;
use crate::error::{ApiJson, ApiPath, ApiQuery, AppError, AppResult};
use crate::AppState;

pub async fn get(State(state): State<AppState>, teacher: AuthTeacher) -> AppResult<Json<Value>> {
    let history = list(&state.db, teacher.id(), true).await?;
    let index: Option<(i64, i64, Option<String>)> = sqlx::query_as(
        "SELECT revision, indexed_revision, last_error FROM memory_indexes WHERE teacher_id=$1",
    )
    .bind(teacher.id())
    .fetch_optional(&state.db)
    .await?;
    let graph_enabled = state.config.memory.enabled();
    let pending = candidates::pending(&state.db, teacher.id(), &Filter::default()).await?;
    Ok(Json(json!({"memories": history,
        "enabled":crate::settings::teacher_memory_enabled(&state.db).await?,
        "max_active": MAX_ACTIVE,
        "scopes": super::scope::SCOPES.iter()
            .map(|(code, name, _)| json!({"code": code, "name": name})).collect::<Vec<_>>(),
        "pending": pending,
        "graph_enabled":graph_enabled,
        "index":index.filter(|_| graph_enabled).map(|(revision,indexed_revision,error)| json!({"revision":revision,
            "indexed_revision":indexed_revision,"error":error}))})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preview {
    pub text: String,
}

pub async fn preview(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiJson(req): ApiJson<Preview>,
) -> AppResult<Json<Value>> {
    let (p, interaction_id) = extract(&state, teacher.id(), &req.text).await?;
    let id = candidates::create(
        &state.db,
        teacher.id(),
        &p,
        interaction_id,
        &candidates::Source::Manual,
        None,
    )
    .await?;
    let replaces_statement: Option<String> = match p.replaces_id {
        Some(old) => {
            sqlx::query_scalar(
                "SELECT statement FROM teacher_memories WHERE id=$1 AND teacher_id=$2",
            )
            .bind(old)
            .bind(teacher.id())
            .fetch_optional(&state.db)
            .await?
        }
        None => None,
    };
    Ok(Json(json!({"id":id,"statement":p.statement,"kind":p.kind,
        "scope":p.scope,"replaces_id":p.replaces_id,"replaces_statement":replaces_statement})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Confirm {
    pub candidate_id: i64,
    /// `false` cancels the preview: recorded as the teacher's rejection of the suggestion.
    pub confirmed: bool,
    /// With `confirmed: false`: the teacher closes the suggestion to edit it (修改後記住).
    #[serde(default)]
    pub edit: bool,
}

pub async fn confirm(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiJson(req): ApiJson<Confirm>,
) -> AppResult<Json<Value>> {
    if !req.confirmed {
        let how = if req.edit {
            Dismissal::Edit
        } else {
            Dismissal::Cancel
        };
        candidates::dismiss(&state, teacher.id(), req.candidate_id, how).await?;
        return Ok(Json(json!({"ok": true, "cancelled": true})));
    }
    let done = candidates::confirm(&state, teacher.id(), req.candidate_id).await?;
    if done.unchanged {
        Ok(Json(json!({"id": done.id, "unchanged": true})))
    } else {
        Ok(Json(json!({"id": done.id})))
    }
}

#[derive(Deserialize, Default)]
pub struct CandidateQuery {
    #[serde(default)]
    pub prep_session_id: Option<i64>,
    #[serde(default)]
    pub worksheet_id: Option<i64>,
}

/// `GET /api/me/memories/candidates`: the suggestions waiting for an answer, optionally only the
/// ones from one 備課對話 or one worksheet.
pub async fn candidates_list(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiQuery(q): ApiQuery<CandidateQuery>,
) -> AppResult<Json<Value>> {
    let filter = Filter {
        prep_session_id: q.prep_session_id,
        worksheet_id: q.worksheet_id,
    };
    let list = candidates::pending(&state.db, teacher.id(), &filter).await?;
    Ok(Json(json!({"candidates": list})))
}

/// `POST /api/me/memories/candidates/{id}/shown`: the page showed the suggestion (logged once).
pub async fn candidate_shown(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<Json<Value>> {
    candidates::mark_shown(&state.db, teacher.id(), id).await?;
    Ok(Json(json!({"ok": true})))
}

pub async fn remove(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<Json<Value>> {
    let mut tx = state.db.begin().await?;
    sqlx::query("SELECT id FROM teachers WHERE id=$1 FOR UPDATE")
        .bind(teacher.id())
        .execute(&mut *tx)
        .await?;
    let kind: Option<String> = sqlx::query_scalar(
        "UPDATE teacher_memories SET invalid_at=clock_timestamp(),invalidation_reason='removed'
        WHERE id=$1 AND teacher_id=$2 AND invalid_at IS NULL RETURNING kind",
    )
    .bind(id)
    .bind(teacher.id())
    .fetch_optional(&mut *tx)
    .await?;
    let Some(kind) = kind else {
        return Err(AppError::not_found("找不到目前有效的習慣。"));
    };
    crate::audit::log_usage(
        &mut *tx,
        teacher.id(),
        "memory_removed",
        json!({"memory_id": id, "kind": kind}),
    )
    .await?;
    dirty(
        &mut tx,
        &state.config.memory,
        teacher.id(),
        &[IndexChange::Remove(id)],
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}

/// The full rebuild: every teacher's whole active snapshot into a new group. Normal changes are
/// incremental; this is for a restore, a suspect index, or a PI who wants a clean start.
pub async fn rebuild(
    State(state): State<AppState>,
    teacher: AuthTeacher,
) -> AppResult<Json<Value>> {
    teacher.require_pi()?;
    if !state.config.memory.enabled() {
        return Err(AppError::Unavailable("Graphiti 尚未設定。".into()));
    }
    let mut tx = state.db.begin().await?;
    // Also recreates entries when restoring a backup taken before the outbox existed.
    sqlx::query(
        "INSERT INTO memory_indexes(teacher_id, full_rebuild)
        SELECT DISTINCT teacher_id, true FROM teacher_memories
        ON CONFLICT(teacher_id) DO UPDATE SET revision=memory_indexes.revision+1,
        full_rebuild=true,attempts=0,next_attempt_at=now(),updated_at=now()",
    )
    .execute(&mut *tx)
    .await?;
    crate::audit::log_usage(&mut *tx, teacher.id(), "memory_index_rebuild", json!({})).await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub enabled: bool,
}

pub async fn settings_get(
    State(state): State<AppState>,
    teacher: AuthTeacher,
) -> AppResult<Json<Value>> {
    teacher.require_pi()?;
    Ok(Json(
        json!({"enabled":crate::settings::teacher_memory_enabled(&state.db).await?,
        "graph_enabled":state.config.memory.enabled()}),
    ))
}

pub async fn settings_put(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiJson(req): ApiJson<Settings>,
) -> AppResult<Json<Value>> {
    teacher.require_pi()?;
    let mut tx = state.db.begin().await?;
    let value = json!(req.enabled);
    let previous = crate::settings::set(
        &mut tx,
        crate::settings::TEACHER_MEMORY_ENABLED,
        &value,
        teacher.id(),
    )
    .await?;
    crate::audit::log_usage(
        &mut *tx,
        teacher.id(),
        "setting_changed",
        json!({
        "key":crate::settings::TEACHER_MEMORY_ENABLED,"from":previous,"to":value}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(
        json!({"enabled":req.enabled,"graph_enabled":state.config.memory.enabled()}),
    ))
}

/// PI-only transport adapter for the queue/status application query.
pub async fn index_status(
    State(state): State<AppState>,
    teacher: AuthTeacher,
) -> AppResult<Json<super::graph::IndexStatus>> {
    teacher.require_pi()?;
    Ok(Json(super::graph::index_status(&state).await?))
}
