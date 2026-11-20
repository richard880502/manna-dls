use axum::extract::State;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use super::{canonical, dirty, extract, list, MAX_ACTIVE};
use crate::auth::AuthTeacher;
use crate::error::{ApiJson, ApiPath, AppError, AppResult};
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
    Ok(Json(json!({"memories": history,
        "enabled":crate::settings::teacher_memory_enabled(&state.db).await?,
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
    let (out, interaction_id) = extract(&state, teacher.id(), &req.text).await?;
    let statement = canonical(&out.kind, &out.value).expect("validated extraction");
    let id: i64 = sqlx::query_scalar("INSERT INTO memory_candidates
        (teacher_id, kind, value, statement, replaces_id, interaction_id) VALUES ($1,$2,$3,$4,$5,$6) RETURNING id")
        .bind(teacher.id()).bind(&out.kind).bind(&out.value).bind(&statement)
        .bind(out.replaces_id).bind(interaction_id).fetch_one(&state.db).await?;
    Ok(Json(
        json!({"id":id,"statement":statement,"replaces_id":out.replaces_id}),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Confirm {
    pub candidate_id: i64,
    /// `false` cancels the preview: recorded as the teacher's rejection of the suggestion.
    pub confirmed: bool,
}

#[derive(sqlx::FromRow)]
struct Candidate {
    kind: String,
    value: String,
    statement: String,
    replaces_id: Option<i64>,
    interaction_id: i64,
}

/// The teacher's decision on the habit-sorting suggestion, like other AI suggestions.
async fn decide(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    interaction_id: i64,
    teacher_id: i64,
    decision: &str,
    note: &str,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO teacher_decisions (ai_interaction_id, teacher_id, item_index, decision, reason_note)
         VALUES ($1, $2, 0, $3, $4)",
    )
    .bind(interaction_id)
    .bind(teacher_id)
    .bind(decision)
    .bind(note)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub async fn confirm(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiJson(req): ApiJson<Confirm>,
) -> AppResult<Json<Value>> {
    if !req.confirmed {
        return cancel(&state, &teacher, req.candidate_id).await;
    }
    let mut tx = state.db.begin().await?;
    // Serialize all confirms/removals for this teacher, including the limit and replacements.
    sqlx::query("SELECT id FROM teachers WHERE id=$1 FOR UPDATE")
        .bind(teacher.id())
        .execute(&mut *tx)
        .await?;
    let c: Candidate = sqlx::query_as(
        "SELECT kind,value,statement,replaces_id,interaction_id
        FROM memory_candidates WHERE id=$1 AND teacher_id=$2 AND confirmed_at IS NULL
        AND cancelled_at IS NULL AND created_at > clock_timestamp() - interval '15 minutes'
        FOR UPDATE",
    )
    .bind(req.candidate_id)
    .bind(teacher.id())
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::Conflict("預覽已過期或已確認，請重新整理習慣。".into()))?;
    let current: Option<(i64, String)> = sqlx::query_as(
        "SELECT id, value FROM teacher_memories
        WHERE teacher_id=$1 AND kind=$2 AND invalid_at IS NULL",
    )
    .bind(teacher.id())
    .bind(&c.kind)
    .fetch_optional(&mut *tx)
    .await?;
    let at: chrono::DateTime<chrono::Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&mut *tx)
        .await?;
    // Confirming exactly what is already active changes nothing: no new row, no re-index.
    if let Some((same, _)) = current.as_ref().filter(|(_, value)| *value == c.value) {
        sqlx::query("UPDATE memory_candidates SET confirmed_at=$2 WHERE id=$1")
            .bind(req.candidate_id)
            .bind(at)
            .execute(&mut *tx)
            .await?;
        decide(
            &mut tx,
            c.interaction_id,
            teacher.id(),
            "adopt",
            "already_active",
        )
        .await?;
        crate::audit::log_usage(
            &mut *tx,
            teacher.id(),
            "memory_confirmed",
            json!({"candidate_id": req.candidate_id, "memory_id": same, "kind": c.kind,
                "value": c.value, "unchanged": true}),
        )
        .await?;
        tx.commit().await?;
        return Ok(Json(json!({"id": same, "unchanged": true})));
    }
    let current = current.map(|(id, _)| id);
    if current != c.replaces_id {
        return Err(AppError::Conflict(
            "習慣已變更，請重新整理並確認取代的項目。".into(),
        ));
    }
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM teacher_memories WHERE teacher_id=$1 AND invalid_at IS NULL",
    )
    .bind(teacher.id())
    .fetch_one(&mut *tx)
    .await?;
    if current.is_none() && count >= MAX_ACTIVE {
        return Err(AppError::Conflict(
            "最多記住 10 項習慣，請先移除一項。".into(),
        ));
    }
    let id: i64 = sqlx::query_scalar("SELECT nextval('teacher_memories_id_seq')")
        .fetch_one(&mut *tx)
        .await?;
    if let Some(old) = current {
        sqlx::query("UPDATE teacher_memories SET invalid_at=$2,superseded_by=$3,invalidation_reason='replaced' WHERE id=$1")
            .bind(old).bind(at).bind(id).execute(&mut *tx).await?;
    }
    sqlx::query(
        "INSERT INTO teacher_memories (id,teacher_id,kind,value,statement,valid_at,interaction_id)
        VALUES ($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(id)
    .bind(teacher.id())
    .bind(&c.kind)
    .bind(&c.value)
    .bind(c.statement)
    .bind(at)
    .bind(c.interaction_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE memory_candidates SET confirmed_at=$2 WHERE id=$1")
        .bind(req.candidate_id)
        .bind(at)
        .execute(&mut *tx)
        .await?;
    decide(
        &mut tx,
        c.interaction_id,
        teacher.id(),
        "adopt",
        "confirmed",
    )
    .await?;
    crate::audit::log_usage(
        &mut *tx,
        teacher.id(),
        "memory_confirmed",
        json!({"candidate_id": req.candidate_id, "memory_id": id, "replaces_id": current,
            "kind": c.kind, "value": c.value, "unchanged": false}),
    )
    .await?;
    dirty(&mut tx, &state.config.memory, teacher.id()).await?;
    tx.commit().await?;
    Ok(Json(json!({"id":id})))
}

/// 取消: the preview can no longer be confirmed; the rejection is recorded.
async fn cancel(
    state: &AppState,
    teacher: &AuthTeacher,
    candidate_id: i64,
) -> AppResult<Json<Value>> {
    let mut tx = state.db.begin().await?;
    let interaction_id: Option<i64> = sqlx::query_scalar(
        "UPDATE memory_candidates SET cancelled_at=clock_timestamp()
        WHERE id=$1 AND teacher_id=$2 AND confirmed_at IS NULL AND cancelled_at IS NULL
        RETURNING interaction_id",
    )
    .bind(candidate_id)
    .bind(teacher.id())
    .fetch_optional(&mut *tx)
    .await?;
    let interaction_id =
        interaction_id.ok_or_else(|| AppError::Conflict("這個預覽已確認或已取消。".into()))?;
    decide(&mut tx, interaction_id, teacher.id(), "reject", "cancelled").await?;
    crate::audit::log_usage(
        &mut *tx,
        teacher.id(),
        "memory_cancelled",
        json!({"candidate_id": candidate_id}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"ok": true, "cancelled": true})))
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
    dirty(&mut tx, &state.config.memory, teacher.id()).await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}

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
        "INSERT INTO memory_indexes(teacher_id) SELECT DISTINCT teacher_id FROM teacher_memories
        ON CONFLICT(teacher_id) DO UPDATE SET revision=memory_indexes.revision+1,
        attempts=0,next_attempt_at=now(),updated_at=now()",
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
