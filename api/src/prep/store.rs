//! Reading and writing prep sessions and their turns. Deleting a session is a hard delete
//! (turns go with it); worksheets made in it stay and only lose the link.

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::types::Json;
use sqlx::PgPool;

use super::selfcheck::{self, SiopCheck};
use super::{
    is_worksheet_kind, kit_error, SessionOptions, KIND_REVISE, NEXT_KINDS, STATUS_DONE,
    STATUS_FAILED, STATUS_RUNNING,
};
use crate::error::{AppError, AppResult};
use crate::worksheet::ai as ws_ai;
use crate::worksheet::store as ws_store;
use crate::worksheet::template::extract as tpl_extract;
use crate::worksheet::template::store as tpl_store;
use crate::AppState;

pub const MSG_NOT_FOUND: &str = "找不到這個備課對話。";
pub const MSG_BUSY: &str = "Manna 還在處理上一個請求，好了之後再送出。";
pub const MSG_STALE: &str = "這次處理中斷了（可能是系統剛更新）。請按「再試一次」。";
const MSG_KIT_NO_WORKSHEET: &str = "分層學習單沒有建立起來。請按「再試一次」。";
const MSG_KIT_NO_ITEMS: &str =
    "分層學習單沒有產生任何題目。可以打開學習單按「產生」再試一次，或按「再試一次」。";

/// Kinds whose turn is `running` exactly while its worksheet generates.
const WORKSHEET_ONLY_KINDS: &str = "('worksheet', 'assessment', 'comprehension', 'tiered')";

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SessionRow {
    pub id: i64,
    pub teacher_id: i64,
    pub title: String,
    pub material_id: Option<i64>,
    pub topic: Option<String>,
    pub class_id: Option<i64>,
    pub template_id: Option<i64>,
    pub options: Json<SessionOptions>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

const SESSION_COLS: &str = "id, teacher_id, title, material_id, topic, class_id, template_id, \
     options, created_at, updated_at";

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
    /// `worksheet` turns: the template the turn follows (the session's when it started).
    pub template_id: Option<i64>,
}

const TURN_COLS: &str = "id, session_id, seq, kind, request, status, error, worksheet_id, result,
    interaction_id, created_at, finished_at, template_id";

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

/// A session by id (no owner check: for background work on a turn already checked).
pub async fn session_row(db: &PgPool, id: i64) -> AppResult<SessionRow> {
    sqlx::query_as(&format!(
        "SELECT {SESSION_COLS} FROM prep_sessions WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(db)
    .await?
    .ok_or_else(|| AppError::not_found(MSG_NOT_FOUND))
}

/// A turn by id (no owner check: for background work on a turn already checked).
pub async fn turn(db: &PgPool, id: i64) -> AppResult<TurnRow> {
    sqlx::query_as(&format!("SELECT {TURN_COLS} FROM prep_turns WHERE id = $1"))
        .bind(id)
        .fetch_optional(db)
        .await?
        .ok_or_else(|| AppError::not_found("找不到這一輪。"))
}

pub struct NewSession<'a> {
    pub teacher_id: i64,
    pub title: &'a str,
    pub material_id: Option<i64>,
    pub topic: Option<&'a str>,
    pub class_id: Option<i64>,
    pub template_id: Option<i64>,
    pub options: &'a SessionOptions,
}

pub async fn insert(db: &PgPool, new: NewSession<'_>) -> sqlx::Result<SessionRow> {
    sqlx::query_as(&format!(
        "INSERT INTO prep_sessions
             (teacher_id, title, material_id, topic, class_id, template_id, options)
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING {SESSION_COLS}"
    ))
    .bind(new.teacher_id)
    .bind(new.title)
    .bind(new.material_id)
    .bind(new.topic)
    .bind(new.class_id)
    .bind(new.template_id)
    .bind(Json(new.options))
    .fetch_one(db)
    .await
}

/// Sets the fields of a PATCH (each `None` is left as it is; `options` replaces them all).
pub async fn update(
    db: &PgPool,
    id: i64,
    title: Option<&str>,
    class_id: Option<Option<i64>>,
    template_id: Option<Option<i64>>,
    options: Option<&SessionOptions>,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE prep_sessions
            SET title = COALESCE($2, title),
                class_id = CASE WHEN $3 THEN $4 ELSE class_id END,
                template_id = CASE WHEN $5 THEN $6 ELSE template_id END,
                options = COALESCE($7, options),
                updated_at = now()
          WHERE id = $1",
    )
    .bind(id)
    .bind(title)
    .bind(class_id.is_some())
    .bind(class_id.flatten())
    .bind(template_id.is_some())
    .bind(template_id.flatten())
    .bind(options.map(Json))
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

/// Opens a turn (`running`). 409 while another turn of the session runs. `template_id`: the
/// template a `worksheet` turn follows (ignored when it was deleted meanwhile).
pub async fn start_turn(
    db: &PgPool,
    session_id: i64,
    kind: &str,
    request: &str,
    template_id: Option<i64>,
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
        "INSERT INTO prep_turns (session_id, seq, kind, request, status, template_id)
         VALUES ($1, (SELECT COALESCE(MAX(seq), 0) + 1 FROM prep_turns WHERE session_id = $1),
                 $2, $3, '{STATUS_RUNNING}',
                 (SELECT id FROM worksheet_templates WHERE id = $4))
         RETURNING {TURN_COLS}"
    ))
    .bind(session_id)
    .bind(kind)
    .bind(request)
    .bind(template_id)
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
///
/// A turn of a kind with a SIOP 自我檢核 that closes `done` opens its first check in the same
/// transaction (so the page never sees the output without it): its id is returned, and the
/// caller starts it with [`selfcheck::spawn`].
pub async fn finish(
    db: &PgPool,
    turn_id: i64,
    outcome: Result<(Option<Value>, Option<i64>), String>,
) -> sqlx::Result<Option<i64>> {
    let (status, error, result, interaction_id) = match outcome {
        Ok((result, interaction_id)) => (STATUS_DONE, None, result, interaction_id),
        Err(message) => (STATUS_FAILED, Some(message), None, None),
    };
    let mut tx = db.begin().await?;
    let closed: Option<(i64, String)> = sqlx::query_as(
        "UPDATE prep_turns
            SET status = $2, error = $3, result = $4, interaction_id = $5, finished_at = now()
          WHERE id = $1 AND status = 'running'
        RETURNING session_id, kind",
    )
    .bind(turn_id)
    .bind(status)
    .bind(error)
    .bind(result)
    .bind(interaction_id)
    .fetch_optional(&mut *tx)
    .await?;
    let check = after_close(&mut tx, turn_id, closed, status).await?;
    tx.commit().await?;
    Ok(check)
}

/// What closing a turn also does, in its transaction: the session is touched, and a `done`
/// turn of a checked kind opens its first SIOP check (its id returned).
async fn after_close(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    turn_id: i64,
    closed: Option<(i64, String)>,
    status: &str,
) -> sqlx::Result<Option<i64>> {
    let Some((session_id, kind)) = closed else {
        return Ok(None);
    };
    sqlx::query("UPDATE prep_sessions SET updated_at = now() WHERE id = $1")
        .bind(session_id)
        .execute(&mut **tx)
        .await?;
    if status == STATUS_DONE && selfcheck::is_checked(&kind) {
        selfcheck::open(tx, turn_id).await
    } else {
        Ok(None)
    }
}

// ---------------------------------------------------------------- 教學套件
//
// A kit turn has two parts: the activity plan (one model call, in a spawned task) and the tiered
// worksheet (its generation, like a worksheet turn). The plan's outcome is recorded on the
// running turn as soon as it is known (`result` and `interaction_id`, or its failure in
// `error`, which the API only shows once the turn is closed); the turn is closed by whichever
// of the two finishes last: the plan's task, or [`refresh`] once the generation is over.

/// Records the activity plan's outcome on a running kit turn: `Ok((result, interaction_id))`,
/// or `Err(message)`.
pub async fn record_plan(
    db: &PgPool,
    turn_id: i64,
    outcome: &Result<(Option<Value>, Option<i64>), String>,
) -> sqlx::Result<()> {
    let (result, interaction_id, error) = match outcome {
        Ok((result, interaction_id)) => (result.clone(), *interaction_id, None),
        Err(message) => (None, None, Some(message.clone())),
    };
    sqlx::query(
        "UPDATE prep_turns SET result = $2, interaction_id = $3, error = $4
          WHERE id = $1 AND status = 'running'",
    )
    .bind(turn_id)
    .bind(result)
    .bind(interaction_id)
    .bind(error)
    .execute(db)
    .await?;
    Ok(())
}

/// Closes a running kit turn with the plan's outcome when its worksheet could not be made or
/// started (`worksheet_error`): `failed`, the plan (when there is one) kept.
pub async fn close_kit_without_worksheet(
    db: &PgPool,
    turn_id: i64,
    outcome: &Result<(Option<Value>, Option<i64>), String>,
    worksheet_error: &str,
) -> sqlx::Result<()> {
    let (result, interaction_id, plan_error) = match outcome {
        Ok((result, interaction_id)) => (result.clone(), *interaction_id, None),
        Err(message) => (None, None, Some(message.as_str())),
    };
    // Always `failed`: no SIOP check is opened.
    close_kit_turn(
        db,
        turn_id,
        Some((result, interaction_id)),
        kit_error(plan_error, Some(worksheet_error)),
    )
    .await
    .map(|_| ())
}

/// Closes the turn: `done` without an error, `failed` with one. `plan` replaces the recorded
/// plan; `None` keeps it. A kit closed `done` opens its first SIOP check (see [`finish`]).
async fn close_kit_turn(
    db: &PgPool,
    turn_id: i64,
    plan: Option<(Option<Value>, Option<i64>)>,
    error: Option<String>,
) -> sqlx::Result<Option<i64>> {
    let replace = plan.is_some();
    let (result, interaction_id) = plan.unwrap_or_default();
    let status = if error.is_none() {
        STATUS_DONE
    } else {
        STATUS_FAILED
    };
    let mut tx = db.begin().await?;
    let closed: Option<(i64, String)> = sqlx::query_as(&format!(
        "UPDATE prep_turns
            SET status = $6,
                error = $2,
                result = CASE WHEN $3 THEN $4 ELSE result END,
                interaction_id = CASE WHEN $3 THEN $5 ELSE interaction_id END,
                finished_at = now()
          WHERE id = $1 AND status = '{STATUS_RUNNING}'
        RETURNING session_id, kind"
    ))
    .bind(turn_id)
    .bind(error)
    .bind(replace)
    .bind(result)
    .bind(interaction_id)
    .bind(status)
    .fetch_optional(&mut *tx)
    .await?;
    let check = after_close(&mut tx, turn_id, closed, status).await?;
    tx.commit().await?;
    Ok(check)
}

/// Closes a running kit turn once both parts are over: the plan's outcome is recorded and the
/// worksheet's generation has ended. `done` when the plan was made and the worksheet has
/// questions; otherwise `failed` with what went wrong, whatever succeeded staying visible.
/// Nothing happens while a part is still running (or when the turn is already closed).
pub async fn close_kit(state: &AppState, turn_id: i64) -> AppResult<()> {
    let db = &state.db;
    let row: Option<(Option<Value>, Option<String>, Option<i64>)> = sqlx::query_as(&format!(
        "SELECT result, error, worksheet_id FROM prep_turns
          WHERE id = $1 AND kind = 'kit' AND status = '{STATUS_RUNNING}'"
    ))
    .bind(turn_id)
    .fetch_optional(db)
    .await?;
    let Some((result, plan_error, worksheet_id)) = row else {
        return Ok(());
    };
    if result.is_none() && plan_error.is_none() {
        return Ok(());
    }
    let worksheet_error = match worksheet_id {
        None => Some(MSG_KIT_NO_WORKSHEET),
        Some(ws) => {
            if ws_ai::jobs(state, ws).await?.1 {
                return Ok(());
            }
            let content = ws_store::latest(db, ws).await?.content;
            let items: usize = content.blocks.iter().map(|b| b.items.len()).sum();
            (items == 0).then_some(MSG_KIT_NO_ITEMS)
        }
    };
    if let Some(check) = close_kit_turn(
        db,
        turn_id,
        None,
        kit_error(plan_error.as_deref(), worksheet_error),
    )
    .await?
    {
        selfcheck::spawn(state, check);
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

/// Brings the turns of a session up to date: a turn that outlived a restart fails, a turn that
/// only makes a worksheet is done once its worksheet's generation started and finished (even
/// when a block failed: the block's `message` says so), and a kit is closed once both its parts
/// are over ([`close_kit`]).
pub async fn refresh(state: &AppState, session_id: i64) -> AppResult<()> {
    let db = &state.db;
    // An extraction that died with the server is reported as failed (its turn then fails too).
    let teacher_id: Option<i64> =
        sqlx::query_scalar("SELECT teacher_id FROM prep_sessions WHERE id = $1")
            .bind(session_id)
            .fetch_optional(db)
            .await?;
    if let Some(teacher_id) = teacher_id {
        tpl_store::expire_stale(db, teacher_id, tpl_extract::stale_after(state)).await?;
    }
    // A turn that waited for its template's sample to be read by OCR counts from when the
    // extraction proper started (the waiting template keeps `extract_started_at` current).
    sqlx::query(&format!(
        "UPDATE prep_turns p SET status = '{STATUS_FAILED}', error = $2, finished_at = now()
          WHERE p.session_id = $1 AND p.status = '{STATUS_RUNNING}'
            AND GREATEST(p.created_at,
                         COALESCE((SELECT t.extract_started_at FROM worksheet_templates t
                                    WHERE t.id = p.template_id), p.created_at))
                < now() - make_interval(secs => $3)"
    ))
    .bind(session_id)
    .bind(MSG_STALE)
    .bind(stale_after(state))
    .execute(db)
    .await?;
    let open: Vec<(i64, i64)> = sqlx::query_as(&format!(
        "SELECT id, worksheet_id FROM prep_turns
          WHERE session_id = $1 AND status = '{STATUS_RUNNING}'
            AND kind IN {WORKSHEET_ONLY_KINDS} AND worksheet_id IS NOT NULL"
    ))
    .bind(session_id)
    .fetch_all(db)
    .await?;
    for (turn_id, worksheet_id) in open {
        let (jobs, running) = ws_ai::jobs(state, worksheet_id).await?;
        // No job yet: the worksheet was made but its generation is still being started (a
        // start that fails closes the turn itself).
        if !jobs.is_empty() && !running {
            if let Some(check) = finish(db, turn_id, Ok((None, None))).await? {
                selfcheck::spawn(state, check);
            }
        }
    }
    let kits: Vec<i64> = sqlx::query_scalar(&format!(
        "SELECT id FROM prep_turns
          WHERE session_id = $1 AND status = '{STATUS_RUNNING}' AND kind = 'kit'
            AND worksheet_id IS NOT NULL"
    ))
    .bind(session_id)
    .fetch_all(db)
    .await?;
    for turn_id in kits {
        close_kit(state, turn_id).await?;
    }
    Ok(())
}

/// The worksheet a `revise` turn works on: the one of the latest turn that made a worksheet
/// (any kind: worksheet, assessment, comprehension, tiered or kit), if it still exists.
pub async fn revise_target(db: &PgPool, session_id: i64) -> sqlx::Result<Option<i64>> {
    sqlx::query_scalar(
        "SELECT t.worksheet_id FROM prep_turns t
           JOIN worksheets w ON w.id = t.worksheet_id AND w.deleted_at IS NULL
          WHERE t.session_id = $1
            AND t.kind IN ('worksheet', 'assessment', 'comprehension', 'tiered', 'kit')
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

/// The template a `worksheet` turn follows, as it is now: while it is `extracting` the turn
/// waits for it and has no worksheet yet.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct TurnTemplate {
    pub id: i64,
    pub title: String,
    /// `extracting` | `ready` | `failed`
    pub status: String,
}

/// OCR progress of the sample a waiting turn's template is made from.
#[derive(Debug, Clone, Copy, Serialize, sqlx::FromRow)]
pub struct TurnOcr {
    pub done_pages: i32,
    pub total_pages: i32,
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
    /// `worksheet` turns that follow a template; `null` otherwise (or once it is deleted).
    pub template: Option<TurnTemplate>,
    pub result: Option<Value>,
    pub interaction_id: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    /// `"ocr"` while the turn waits for its worksheet sample to be read by OCR
    /// (「Manna 正在辨識掃描檔（3／8 頁）」); absent otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ocr: Option<TurnOcr>,
    /// The latest SIOP 自我檢核 of a turn whose output is checked (`null` while it has none:
    /// the turn is running or failed, or was made before the check existed); absent for the
    /// other kinds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub siop_check: Option<Option<SiopCheck>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Session {
    #[serde(flatten)]
    pub summary: Summary,
    pub turns: Vec<Turn>,
    pub running: bool,
    pub next: Vec<&'static str>,
    /// 出題選項, without the keys that are not set (`{}` when none is).
    pub options: SessionOptions,
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

/// The OCR progress of the sample of an extracting template, while that OCR runs.
async fn turn_ocr(db: &PgPool, template_id: i64) -> sqlx::Result<Option<TurnOcr>> {
    sqlx::query_as(&format!(
        "SELECT m.ocr_done_pages AS done_pages, m.ocr_total_pages AS total_pages
           FROM worksheet_templates t JOIN materials m ON m.id = t.source_material_id
          WHERE t.id = $1 AND t.status = '{}' AND m.ocr_status IN ('queued', 'running')",
        tpl_store::STATUS_EXTRACTING
    ))
    .bind(template_id)
    .fetch_optional(db)
    .await
}

async fn turn_template(db: &PgPool, template_id: i64) -> sqlx::Result<Option<TurnTemplate>> {
    sqlx::query_as("SELECT id, title, status FROM worksheet_templates WHERE id = $1")
        .bind(template_id)
        .fetch_optional(db)
        .await
}

/// The whole session, brought up to date first.
pub async fn session(state: &AppState, id: i64) -> AppResult<Session> {
    refresh(state, id).await?;
    selfcheck::expire_stale(state, id).await?;
    let summary = summary(&state.db, id).await?;
    let rows = turns(&state.db, id).await?;
    let mut checks = selfcheck::latest_of_session(&state.db, id).await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let worksheet = match r.worksheet_id {
            Some(ws) => turn_worksheet(state, ws).await?,
            None => None,
        };
        let template = match r.template_id {
            Some(t) => turn_template(&state.db, t).await?,
            None => None,
        };
        let ocr = match (r.template_id, r.status == STATUS_RUNNING, r.worksheet_id) {
            (Some(t), true, None) => turn_ocr(&state.db, t).await?,
            _ => None,
        };
        let siop_check = selfcheck::is_checked(&r.kind).then(|| checks.remove(&r.id));
        out.push(Turn {
            id: r.id,
            seq: r.seq,
            kind: r.kind,
            request: r.request,
            // A kit's failed plan is only reported once the turn is closed.
            error: if r.status == STATUS_RUNNING {
                None
            } else {
                r.error
            },
            status: r.status,
            worksheet,
            template,
            result: r.result,
            interaction_id: r.interaction_id,
            created_at: r.created_at,
            finished_at: r.finished_at,
            phase: ocr.map(|_| "ocr"),
            ocr,
            siop_check,
        });
    }
    let running = out.iter().any(|t| t.status == STATUS_RUNNING);
    // Every suggested kind needs the material.
    let next = if summary.material.is_some() {
        NEXT_KINDS
            .into_iter()
            .filter(|k| !out.iter().any(|t| t.kind == *k && t.status == STATUS_DONE))
            .collect()
    } else {
        Vec::new()
    };
    let options: Option<Json<SessionOptions>> =
        sqlx::query_scalar("SELECT options FROM prep_sessions WHERE id = $1")
            .bind(id)
            .fetch_optional(&state.db)
            .await?;
    Ok(Session {
        summary,
        turns: out,
        running,
        next,
        options: options.map(|o| o.0).unwrap_or_default(),
    })
}

/// Whether the session has a turn that made a worksheet (the bottom bar then revises).
pub fn has_worksheet_turn(turns: &[TurnRow]) -> bool {
    turns
        .iter()
        .any(|t| is_worksheet_kind(&t.kind) && t.worksheet_id.is_some())
}

/// Kinds that never start a session.
pub fn starts_a_session(kind: &str) -> bool {
    kind != KIND_REVISE
}
