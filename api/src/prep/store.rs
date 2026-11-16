//! Reading and writing prep sessions and their turns. Deleting a session is a hard delete
//! (turns go with it); worksheets made in it stay and only lose the link.

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::PgPool;

use super::{
    is_worksheet_kind, KIND_REVISE, NEXT_KINDS, STATUS_DONE, STATUS_FAILED, STATUS_RUNNING,
};
use crate::error::{AppError, AppResult};
use crate::worksheet::ai as ws_ai;
use crate::AppState;

pub const MSG_NOT_FOUND: &str = "找不到這個備課對話。";
pub const MSG_BUSY: &str = "Manna 還在處理上一個請求，好了之後再送出。";
pub const MSG_STALE: &str = "這次處理中斷了（可能是系統剛更新）。請按「再試一次」。";

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SessionRow {
    pub id: i64,
    pub teacher_id: i64,
    pub title: String,
    pub material_id: Option<i64>,
    pub topic: Option<String>,
    pub class_id: Option<i64>,
    pub template_id: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

const SESSION_COLS: &str =
    "id, teacher_id, title, material_id, topic, class_id, template_id, created_at, updated_at";

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TurnRow {
    pub id: i64,
    pub session_id: i64,
    pub seq: i32,
    pub kind: String,
    pub request: String,
    pub status: String,
    pub error: Option<String>,
    pub worksheet_id: Option<i64>,
    pub result: Option<Value>,
    pub interaction_id: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

const TURN_COLS: &str = "id, session_id, seq, kind, request, status, error, worksheet_id, result,
    interaction_id, created_at, finished_at";

/// The session if it exists and belongs to `teacher_id`; otherwise 404 (no existence leak).
pub async fn owned(db: &PgPool, id: i64, teacher_id: i64) -> AppResult<SessionRow> {
    sqlx::query_as(&format!(
        "SELECT {SESSION_COLS} FROM prep_sessions WHERE id = $1 AND teacher_id = $2"
    ))
    .bind(id)
    .bind(teacher_id)
    .fetch_optional(db)
    .await?
    .ok_or_else(|| AppError::not_found(MSG_NOT_FOUND))
}

pub struct NewSession<'a> {
    pub teacher_id: i64,
    pub title: &'a str,
    pub material_id: Option<i64>,
    pub topic: Option<&'a str>,
    pub class_id: Option<i64>,
    pub template_id: Option<i64>,
}

pub async fn insert(db: &PgPool, new: NewSession<'_>) -> sqlx::Result<SessionRow> {
    sqlx::query_as(&format!(
        "INSERT INTO prep_sessions (teacher_id, title, material_id, topic, class_id, template_id)
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING {SESSION_COLS}"
    ))
    .bind(new.teacher_id)
    .bind(new.title)
    .bind(new.material_id)
    .bind(new.topic)
    .bind(new.class_id)
    .bind(new.template_id)
    .fetch_one(db)
    .await
}

/// Sets the fields of a PATCH (each `None` is left as it is).
pub async fn update(
    db: &PgPool,
    id: i64,
    title: Option<&str>,
    class_id: Option<Option<i64>>,
    template_id: Option<Option<i64>>,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE prep_sessions
            SET title = COALESCE($2, title),
                class_id = CASE WHEN $3 THEN $4 ELSE class_id END,
                template_id = CASE WHEN $5 THEN $6 ELSE template_id END,
                updated_at = now()
          WHERE id = $1",
    )
    .bind(id)
    .bind(title)
    .bind(class_id.is_some())
    .bind(class_id.flatten())
    .bind(template_id.is_some())
    .bind(template_id.flatten())
    .execute(db)
    .await?;
    Ok(())
}

/// Hard delete: the session and its turns. Worksheets made in it keep everything and lose
/// only `prep_session_id` (`ON DELETE SET NULL`).
pub async fn delete(db: &PgPool, id: i64) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM prep_sessions WHERE id = $1")
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

// ---------------------------------------------------------------- turns

/// Opens a turn (`running`). 409 while another turn of the session runs.
pub async fn start_turn(
    db: &PgPool,
    session_id: i64,
    kind: &str,
    request: &str,
) -> AppResult<TurnRow> {
    let mut tx = db.begin().await?;
    sqlx::query("SELECT id FROM prep_sessions WHERE id = $1 FOR UPDATE")
        .bind(session_id)
        .execute(&mut *tx)
        .await?;
    let running: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM prep_turns WHERE session_id = $1 AND status = 'running'",
    )
    .bind(session_id)
    .fetch_one(&mut *tx)
    .await?;
    if running > 0 {
        return Err(AppError::Conflict(MSG_BUSY.to_string()));
    }
    let row: TurnRow = sqlx::query_as(&format!(
        "INSERT INTO prep_turns (session_id, seq, kind, request, status)
         VALUES ($1, (SELECT COALESCE(MAX(seq), 0) + 1 FROM prep_turns WHERE session_id = $1),
                 $2, $3, '{STATUS_RUNNING}')
         RETURNING {TURN_COLS}"
    ))
    .bind(session_id)
    .bind(kind)
    .bind(request)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query("UPDATE prep_sessions SET updated_at = now() WHERE id = $1")
        .bind(session_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(row)
}

pub async fn set_worksheet(db: &PgPool, turn_id: i64, worksheet_id: i64) -> sqlx::Result<()> {
    sqlx::query("UPDATE prep_turns SET worksheet_id = $2 WHERE id = $1")
        .bind(turn_id)
        .bind(worksheet_id)
        .execute(db)
        .await?;
    Ok(())
}

/// Closes a running turn: `Ok((result, interaction_id))` → `done`, `Err(message)` → `failed`.
/// Nothing happens when the turn is gone (session deleted) or already closed.
pub async fn finish(
    db: &PgPool,
    turn_id: i64,
    outcome: Result<(Option<Value>, Option<i64>), String>,
) -> sqlx::Result<()> {
    let (status, error, result, interaction_id) = match outcome {
        Ok((result, interaction_id)) => (STATUS_DONE, None, result, interaction_id),
        Err(message) => (STATUS_FAILED, Some(message), None, None),
    };
    let session: Option<i64> = sqlx::query_scalar(
        "UPDATE prep_turns
            SET status = $2, error = $3, result = $4, interaction_id = $5, finished_at = now()
          WHERE id = $1 AND status = 'running'
        RETURNING session_id",
    )
    .bind(turn_id)
    .bind(status)
    .bind(error)
    .bind(result)
    .bind(interaction_id)
    .fetch_optional(db)
    .await?;
    if let Some(session_id) = session {
        sqlx::query("UPDATE prep_sessions SET updated_at = now() WHERE id = $1")
            .bind(session_id)
            .execute(db)
            .await?;
    }
    Ok(())
}

pub async fn turns(db: &PgPool, session_id: i64) -> sqlx::Result<Vec<TurnRow>> {
    sqlx::query_as(&format!(
        "SELECT {TURN_COLS} FROM prep_turns WHERE session_id = $1 ORDER BY seq"
    ))
    .bind(session_id)
    .fetch_all(db)
    .await
}

/// Seconds after which a running turn is reported as failed (its calls must have timed out).
pub fn stale_after(state: &AppState) -> f64 {
    (state.config.ai_timeout_secs * 8 + 60) as f64
}

/// Brings the turns of a session up to date: a turn that outlived a restart fails, and a
/// worksheet / assessment turn whose worksheet finished generating is done (even when a block
/// failed: the block's `message` says so).
pub async fn refresh(state: &AppState, session_id: i64) -> AppResult<()> {
    let db = &state.db;
    sqlx::query(&format!(
        "UPDATE prep_turns SET status = '{STATUS_FAILED}', error = $2, finished_at = now()
          WHERE session_id = $1 AND status = '{STATUS_RUNNING}'
            AND created_at < now() - make_interval(secs => $3)"
    ))
    .bind(session_id)
    .bind(MSG_STALE)
    .bind(stale_after(state))
    .execute(db)
    .await?;
    let open: Vec<(i64, i64)> = sqlx::query_as(&format!(
        "SELECT id, worksheet_id FROM prep_turns
          WHERE session_id = $1 AND status = '{STATUS_RUNNING}'
            AND kind IN ('worksheet', 'assessment') AND worksheet_id IS NOT NULL"
    ))
    .bind(session_id)
    .fetch_all(db)
    .await?;
    for (turn_id, worksheet_id) in open {
        let (_, running) = ws_ai::jobs(state, worksheet_id).await?;
        if !running {
            finish(db, turn_id, Ok((None, None))).await?;
        }
    }
    Ok(())
}

/// The worksheet a `revise` turn works on: the one of the latest worksheet / assessment turn,
/// if it still exists.
pub async fn revise_target(db: &PgPool, session_id: i64) -> sqlx::Result<Option<i64>> {
    sqlx::query_scalar(
        "SELECT t.worksheet_id FROM prep_turns t
           JOIN worksheets w ON w.id = t.worksheet_id AND w.deleted_at IS NULL
          WHERE t.session_id = $1 AND t.kind IN ('worksheet', 'assessment')
          ORDER BY t.seq DESC LIMIT 1",
    )
    .bind(session_id)
    .fetch_optional(db)
    .await
}

// ---------------------------------------------------------------- what the API returns

#[derive(Debug, Clone, Serialize)]
pub struct Named {
    pub id: i64,
    pub title: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClassInfo {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub id: i64,
    pub title: String,
    pub material: Option<Named>,
    pub topic: Option<String>,
    pub class: Option<ClassInfo>,
    pub template: Option<Named>,
    pub turn_count: i64,
    pub last_kind: Option<String>,
    pub updated_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct SummaryRow {
    id: i64,
    title: String,
    material_id: Option<i64>,
    material_title: Option<String>,
    topic: Option<String>,
    class_id: Option<i64>,
    class_name: Option<String>,
    template_id: Option<i64>,
    template_title: Option<String>,
    turn_count: i64,
    last_kind: Option<String>,
    updated_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
}

impl From<SummaryRow> for Summary {
    fn from(r: SummaryRow) -> Self {
        Self {
            id: r.id,
            title: r.title,
            material: r
                .material_id
                .zip(r.material_title)
                .map(|(id, title)| Named { id, title }),
            topic: r.topic,
            class: r
                .class_id
                .zip(r.class_name)
                .map(|(id, name)| ClassInfo { id, name }),
            template: r
                .template_id
                .zip(r.template_title)
                .map(|(id, title)| Named { id, title }),
            turn_count: r.turn_count,
            last_kind: r.last_kind,
            updated_at: r.updated_at,
            created_at: r.created_at,
        }
    }
}

const SUMMARY_SELECT: &str = "SELECT s.id, s.title, s.material_id, m.title AS material_title,
        s.topic, s.class_id, c.name AS class_name, s.template_id, t.title AS template_title,
        (SELECT COUNT(*) FROM prep_turns x WHERE x.session_id = s.id) AS turn_count,
        (SELECT x.kind FROM prep_turns x WHERE x.session_id = s.id
          ORDER BY x.seq DESC LIMIT 1) AS last_kind,
        s.updated_at, s.created_at
   FROM prep_sessions s
   LEFT JOIN materials m ON m.id = s.material_id
   LEFT JOIN classes c ON c.id = s.class_id
   LEFT JOIN worksheet_templates t ON t.id = s.template_id";

/// The teacher's sessions, newest `updated_at` first.
pub async fn list(db: &PgPool, teacher_id: i64) -> sqlx::Result<Vec<Summary>> {
    let rows: Vec<SummaryRow> = sqlx::query_as(&format!(
        "{SUMMARY_SELECT} WHERE s.teacher_id = $1 ORDER BY s.updated_at DESC, s.id DESC"
    ))
    .bind(teacher_id)
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(Summary::from).collect())
}

pub async fn summary(db: &PgPool, id: i64) -> AppResult<Summary> {
    let row: Option<SummaryRow> = sqlx::query_as(&format!("{SUMMARY_SELECT} WHERE s.id = $1"))
        .bind(id)
        .fetch_optional(db)
        .await?;
    row.map(Summary::from)
        .ok_or_else(|| AppError::not_found(MSG_NOT_FOUND))
}

#[derive(Debug, Clone, Serialize)]
pub struct TurnWorksheet {
    pub id: i64,
    pub title: String,
    pub version_no: i32,
    pub running: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Turn {
    pub id: i64,
    pub seq: i32,
    pub kind: String,
    pub request: String,
    pub status: String,
    pub error: Option<String>,
    pub worksheet: Option<TurnWorksheet>,
    pub result: Option<Value>,
    pub interaction_id: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Session {
    #[serde(flatten)]
    pub summary: Summary,
    pub turns: Vec<Turn>,
    pub running: bool,
    pub next: Vec<&'static str>,
}

async fn turn_worksheet(state: &AppState, worksheet_id: i64) -> AppResult<Option<TurnWorksheet>> {
    let row: Option<(String, i32)> = sqlx::query_as(
        "SELECT w.title, v.version_no FROM worksheets w
           JOIN LATERAL (SELECT version_no FROM worksheet_versions
                          WHERE worksheet_id = w.id ORDER BY version_no DESC LIMIT 1) v ON TRUE
          WHERE w.id = $1 AND w.deleted_at IS NULL",
    )
    .bind(worksheet_id)
    .fetch_optional(&state.db)
    .await?;
    let Some((title, version_no)) = row else {
        return Ok(None);
    };
    let (_, running) = ws_ai::jobs(state, worksheet_id).await?;
    Ok(Some(TurnWorksheet {
        id: worksheet_id,
        title,
        version_no,
        running,
    }))
}

/// The whole session, brought up to date first.
pub async fn session(state: &AppState, id: i64) -> AppResult<Session> {
    refresh(state, id).await?;
    let summary = summary(&state.db, id).await?;
    let rows = turns(&state.db, id).await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let worksheet = match r.worksheet_id {
            Some(ws) => turn_worksheet(state, ws).await?,
            None => None,
        };
        out.push(Turn {
            id: r.id,
            seq: r.seq,
            kind: r.kind,
            request: r.request,
            status: r.status,
            error: r.error,
            worksheet,
            result: r.result,
            interaction_id: r.interaction_id,
            created_at: r.created_at,
            finished_at: r.finished_at,
        });
    }
    let running = out.iter().any(|t| t.status == STATUS_RUNNING);
    let has_material = summary.material.is_some();
    let has_topic = summary
        .topic
        .as_deref()
        .is_some_and(|t| !t.trim().is_empty());
    let next = NEXT_KINDS
        .into_iter()
        .filter(|k| {
            if *k == super::KIND_RESOURCES {
                has_material || has_topic
            } else {
                has_material
            }
        })
        .filter(|k| !out.iter().any(|t| t.kind == *k && t.status == STATUS_DONE))
        .collect();
    Ok(Session {
        summary,
        turns: out,
        running,
        next,
    })
}

/// Whether the session has a worksheet / assessment turn (the bottom bar then revises).
pub fn has_worksheet_turn(turns: &[TurnRow]) -> bool {
    turns
        .iter()
        .any(|t| is_worksheet_kind(&t.kind) && t.worksheet_id.is_some())
}

/// Kinds that never start a session.
pub fn starts_a_session(kind: &str) -> bool {
    kind != KIND_REVISE
}
