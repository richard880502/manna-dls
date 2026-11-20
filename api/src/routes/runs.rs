//! Teach & Record: lesson runs (開始／結束本節課), bookmarks synced from the device-side queue,
//! post-lesson review (課後補寫) and QR pairing for the phone remote (spec §4.3, §6.4;
//! contract: docs/API.md「Phase 1.2」). The phone remote's own endpoints are in `routes::remote`.

use axum::body::Bytes;
use axum::extract::State;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{PgExecutor, PgPool, Postgres, Transaction};

use crate::auth::{new_token, sha256_hex, AuthTeacher};
use crate::error::{ApiJson, ApiPath, ApiQuery, AppError, AppResult};
use crate::record::{
    self, normalize_behaviors, remote_create_allowed, valid_handling, Actor, RecordOp, Rejection,
    MAX_FOLLOWUP_CHARS, MAX_OPS_PER_SYNC, PAIRING_MINUTES,
};
use crate::routes::classes::MSG_CLASS_ARCHIVED;
use crate::teaching_context::{owned_class, owned_lesson};
use crate::AppState;

// ---------------------------------------------------------------- shapes

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct LessonRun {
    pub id: i64,
    pub lesson_id: i64,
    pub lesson_title: String,
    pub unit_id: i64,
    pub unit_title: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub review_completed_at: Option<DateTime<Utc>>,
    pub good_count: i64,
    pub stuck_count: i64,
    /// The class this lesson was taught in; NULL for a run without a class.
    pub class_id: Option<i64>,
    pub class_name: Option<String>,
}

/// Runs whose lesson and unit still exist; callers add the ownership / id filters.
const RUN_SELECT: &str = "
    SELECT r.id, r.lesson_id, l.title AS lesson_title, l.unit_id, u.title AS unit_title,
           r.started_at, r.ended_at, r.review_completed_at,
           (SELECT COUNT(*) FROM record_events e
             WHERE e.lesson_run_id = r.id AND e.deleted_at IS NULL AND e.polarity = 'good') AS good_count,
           (SELECT COUNT(*) FROM record_events e
             WHERE e.lesson_run_id = r.id AND e.deleted_at IS NULL AND e.polarity = 'stuck') AS stuck_count,
           r.class_id, c.name AS class_name
      FROM lesson_runs r
      JOIN lessons l ON l.id = r.lesson_id
      JOIN units u ON u.id = l.unit_id
      LEFT JOIN classes c ON c.id = r.class_id
     WHERE l.deleted_at IS NULL AND u.deleted_at IS NULL";

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct RecordEvent {
    pub id: i64,
    pub device_event_id: String,
    pub device_ts: DateTime<Utc>,
    pub received_at: DateTime<Utc>,
    pub polarity: String,
    pub note: String,
    pub device_type: String,
    pub via: String,
    pub activity_step_id: Option<i64>,
    pub behaviors: Vec<String>,
    pub handling: Option<String>,
    pub followup_text: String,
    pub followup_at: Option<DateTime<Utc>>,
    pub revision_count: i64,
}

const EVENT_COLS: &str = "
    e.id, e.device_event_id, e.device_ts, e.received_at, e.polarity, e.note, e.device_type, e.via,
    e.activity_step_id, e.icap_cues AS behaviors, e.handling,
    COALESCE(e.followup_text, '') AS followup_text, e.followup_at,
    (SELECT COUNT(*) FROM record_event_revisions rv WHERE rv.record_event_id = e.id) AS revision_count";

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct PlanStepRef {
    pub id: i64,
    pub position: i32,
    pub activity_type: String,
    pub icap_level: Option<String>,
    pub content: String,
    pub duration_min: Option<i32>,
    pub support_note: String,
    pub extension_note: String,
}

#[derive(Debug, Serialize)]
pub struct RunPlan {
    pub lesson_version_id: i64,
    pub version_no: i32,
    pub objective: String,
    pub steps: Vec<PlanStepRef>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct RemoteDevice {
    pub id: i64,
    pub device_type: String,
    pub paired_at: DateTime<Utc>,
    pub last_seen_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
pub struct RunDetail {
    #[serde(flatten)]
    pub run: LessonRun,
    pub events: Vec<RecordEvent>,
    pub plan: Option<RunPlan>,
    pub remotes: Vec<RemoteDevice>,
}

#[derive(Debug, Serialize)]
pub struct SyncOpResult {
    pub op_id: String,
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SyncResponse {
    pub results: Vec<SyncOpResult>,
    pub events: Vec<RecordEvent>,
    pub ended_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct SyncRequest {
    pub ops: Vec<Value>,
}

// ---------------------------------------------------------------- lookups

/// The run if it belongs to one of the teacher's (non-deleted) lessons; otherwise 404.
pub async fn owned_run(db: &PgPool, run_id: i64, teacher_id: i64) -> AppResult<LessonRun> {
    sqlx::query_as(&format!("{RUN_SELECT} AND r.id = $1 AND u.teacher_id = $2"))
        .bind(run_id)
        .bind(teacher_id)
        .fetch_optional(db)
        .await?
        .ok_or_else(|| AppError::not_found("找不到這節課的紀錄。"))
}

async fn load_run(db: &PgPool, run_id: i64) -> AppResult<LessonRun> {
    Ok(sqlx::query_as(&format!("{RUN_SELECT} AND r.id = $1"))
        .bind(run_id)
        .fetch_one(db)
        .await?)
}

/// Latest state of the run's bookmarks (deleted ones hidden), in device-time order. With
/// `remote_id`, only the bookmarks that remote created.
pub async fn run_events<'e, E: PgExecutor<'e>>(
    executor: E,
    run_id: i64,
    remote_id: Option<i64>,
) -> sqlx::Result<Vec<RecordEvent>> {
    sqlx::query_as(&format!(
        "SELECT {EVENT_COLS} FROM record_events e
          WHERE e.lesson_run_id = $1 AND e.deleted_at IS NULL
            AND ($2::bigint IS NULL OR e.remote_id = $2)
          ORDER BY e.device_ts, e.id"
    ))
    .bind(run_id)
    .bind(remote_id)
    .fetch_all(executor)
    .await
}

async fn latest_plan(db: &PgPool, lesson_id: i64) -> sqlx::Result<Option<RunPlan>> {
    let version: Option<(i64, i32, String)> = sqlx::query_as(
        "SELECT id, version_no, objective FROM lesson_versions
          WHERE lesson_id = $1 ORDER BY version_no DESC LIMIT 1",
    )
    .bind(lesson_id)
    .fetch_optional(db)
    .await?;
    let Some((id, version_no, objective)) = version else {
        return Ok(None);
    };
    let steps = sqlx::query_as(
        "SELECT id, position, activity_type, icap_level, content, duration_min, support_note,
                extension_note
           FROM lesson_steps WHERE lesson_version_id = $1 ORDER BY position",
    )
    .bind(id)
    .fetch_all(db)
    .await?;
    Ok(Some(RunPlan {
        lesson_version_id: id,
        version_no,
        objective,
        steps,
    }))
}

// ---------------------------------------------------------------- runs

#[derive(Debug, Default, Deserialize)]
pub struct RunsQuery {
    /// Only the runs taught in this class (must be the teacher's own, else 404).
    #[serde(default)]
    pub class_id: Option<i64>,
}

pub async fn list_mine(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiQuery(q): ApiQuery<RunsQuery>,
) -> AppResult<Json<Vec<LessonRun>>> {
    if let Some(class_id) = q.class_id {
        owned_class(&state.db, class_id, teacher.id()).await?;
    }
    let runs = sqlx::query_as(&format!(
        "{RUN_SELECT} AND u.teacher_id = $1 AND ($2::bigint IS NULL OR r.class_id = $2)
          ORDER BY (r.ended_at IS NULL) DESC, r.started_at DESC, r.id DESC LIMIT 100"
    ))
    .bind(teacher.id())
    .bind(q.class_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(runs))
}

pub async fn list_for_lesson(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(lesson_id): ApiPath<i64>,
    ApiQuery(q): ApiQuery<RunsQuery>,
) -> AppResult<Json<Vec<LessonRun>>> {
    owned_lesson(&state.db, lesson_id, teacher.id()).await?;
    if let Some(class_id) = q.class_id {
        owned_class(&state.db, class_id, teacher.id()).await?;
    }
    let runs = sqlx::query_as(&format!(
        "{RUN_SELECT} AND r.lesson_id = $1 AND ($2::bigint IS NULL OR r.class_id = $2)
          ORDER BY r.started_at DESC, r.id DESC"
    ))
    .bind(lesson_id)
    .bind(q.class_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(runs))
}

#[derive(Debug, Default, Deserialize)]
pub struct StartRequest {
    /// The class this lesson is taught in. Omitted or null: see [`start`].
    #[serde(default)]
    pub class_id: Option<i64>,
}

pub const MSG_CLASS_REQUIRED: &str = "這個單元有好幾個班級在用，請先選擇這節課是哪一班。";
pub const MSG_UNIT_NOT_IN_CLASS: &str = "這個班級還沒有取用這個單元，請先在班級頁加入。";

/// 開始本節課. At most one open run per lesson **and class** (partial unique index).
///
/// The class (docs/API.md Phase 1.4): with `class_id`, that class (it must be the teacher's,
/// not archived, and use this lesson's unit). Without it: the unit's only class when exactly one
/// class uses it; a run without a class when none does (never blocked); 409 when several do,
/// because the client must say which one.
pub async fn start(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(lesson_id): ApiPath<i64>,
    body: Bytes,
) -> AppResult<Json<LessonRun>> {
    let req: StartRequest = if body.iter().all(u8::is_ascii_whitespace) {
        StartRequest::default()
    } else {
        serde_json::from_slice(&body)
            .map_err(|e| AppError::bad_request(format!("請求內容格式不正確：{e}")))?
    };
    let lesson = owned_lesson(&state.db, lesson_id, teacher.id()).await?;
    let using: Vec<i64> = sqlx::query_scalar(
        "SELECT c.id FROM class_units cu JOIN classes c ON c.id = cu.class_id
          WHERE cu.unit_id = $1 AND cu.removed_at IS NULL AND c.archived_at IS NULL
          ORDER BY c.position, c.id",
    )
    .bind(lesson.unit_id)
    .fetch_all(&state.db)
    .await?;
    let class_id = match req.class_id {
        Some(class_id) => {
            if owned_class(&state.db, class_id, teacher.id())
                .await?
                .archived
            {
                return Err(AppError::Conflict(MSG_CLASS_ARCHIVED.into()));
            }
            if !using.contains(&class_id) {
                return Err(AppError::Conflict(MSG_UNIT_NOT_IN_CLASS.into()));
            }
            Some(class_id)
        }
        None => match using.as_slice() {
            [] => None,
            [only] => Some(*only),
            _ => return Err(AppError::Conflict(MSG_CLASS_REQUIRED.into())),
        },
    };
    let id: Option<i64> = sqlx::query_scalar(
        "INSERT INTO lesson_runs (lesson_id, class_id, started_at, lesson_version_id,
                                  lesson_sop_version_id)
         VALUES ($1, $2, now(),
                 (SELECT id FROM lesson_versions WHERE lesson_id = $1
                   ORDER BY version_no DESC LIMIT 1),
                 (SELECT id FROM lesson_sop_versions WHERE lesson_id = $1
                   ORDER BY version_no DESC LIMIT 1))
         ON CONFLICT (lesson_id, (COALESCE(class_id, 0))) WHERE ended_at IS NULL DO NOTHING
         RETURNING id",
    )
    .bind(lesson_id)
    .bind(class_id)
    .fetch_optional(&state.db)
    .await?;
    let id =
        id.ok_or_else(|| AppError::Conflict("這節課已經開始了，請回到進行中的課堂紀錄。".into()))?;
    Ok(Json(load_run(&state.db, id).await?))
}

pub async fn detail(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(run_id): ApiPath<i64>,
) -> AppResult<Json<RunDetail>> {
    let run = owned_run(&state.db, run_id, teacher.id()).await?;
    let events = run_events(&state.db, run_id, None).await?;
    let plan = latest_plan(&state.db, run.lesson_id).await?;
    let remotes = sqlx::query_as(
        "SELECT id, device_type, created_at AS paired_at, last_seen_at FROM run_remotes
          WHERE lesson_run_id = $1 AND revoked_at IS NULL ORDER BY id",
    )
    .bind(run_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(RunDetail {
        run,
        events,
        plan,
        remotes,
    }))
}

/// 結束本節課並回顧. Idempotent: a retry keeps the first end time.
pub async fn end(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(run_id): ApiPath<i64>,
) -> AppResult<Json<LessonRun>> {
    owned_run(&state.db, run_id, teacher.id()).await?;
    sqlx::query("UPDATE lesson_runs SET ended_at = COALESCE(ended_at, now()) WHERE id = $1")
        .bind(run_id)
        .execute(&state.db)
        .await?;
    Ok(Json(load_run(&state.db, run_id).await?))
}

/// 完成課後補寫: records that the teacher pressed it; nothing has to be filled in (§6.4).
pub async fn complete_review(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(run_id): ApiPath<i64>,
) -> AppResult<Json<LessonRun>> {
    let run = owned_run(&state.db, run_id, teacher.id()).await?;
    if run.ended_at.is_none() {
        return Err(AppError::Conflict("請先結束本節課。".into()));
    }
    sqlx::query(
        "UPDATE lesson_runs SET review_completed_at = COALESCE(review_completed_at, now())
          WHERE id = $1",
    )
    .bind(run_id)
    .execute(&state.db)
    .await?;
    Ok(Json(load_run(&state.db, run_id).await?))
}

// ---------------------------------------------------------------- bookmark sync

enum Outcome {
    Applied,
    Duplicate,
    Rejected(Rejection),
}

fn op_result(op_id: String, outcome: Outcome) -> SyncOpResult {
    match outcome {
        Outcome::Applied => SyncOpResult {
            op_id,
            status: "applied",
            code: None,
            message: None,
        },
        Outcome::Duplicate => SyncOpResult {
            op_id,
            status: "duplicate",
            code: None,
            message: None,
        },
        Outcome::Rejected(r) => SyncOpResult {
            op_id,
            status: "rejected",
            code: Some(r.code),
            message: Some(r.message),
        },
    }
}

/// Applies queued device operations in order, in one transaction. Every op is idempotent:
/// creates by `(run, device_event_id)`, changes by `op_id`. Invalid ops are rejected one by one
/// so they never block the rest of the queue.
pub async fn sync_ops(
    db: &PgPool,
    run_id: i64,
    actor: Actor,
    ops: &[Value],
) -> AppResult<(Vec<SyncOpResult>, Option<DateTime<Utc>>)> {
    if ops.len() > MAX_OPS_PER_SYNC {
        return Err(AppError::bad_request(format!(
            "一次最多同步 {MAX_OPS_PER_SYNC} 個操作。"
        )));
    }
    let mut tx = db.begin().await?;
    // Holds off a concurrent 結束本節課 until this batch is in.
    let ended_at: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT ended_at FROM lesson_runs WHERE id = $1 FOR SHARE")
            .bind(run_id)
            .fetch_one(&mut *tx)
            .await?;
    let mut results = Vec::with_capacity(ops.len());
    for raw in ops {
        let op = match record::parse_op(raw) {
            Ok(op) => op,
            Err((op_id, rejection)) => {
                results.push(op_result(op_id, Outcome::Rejected(rejection)));
                continue;
            }
        };
        let outcome = match &op {
            RecordOp::Create { .. } => apply_create(&mut tx, run_id, ended_at, actor, &op).await?,
            _ => apply_change(&mut tx, run_id, ended_at, actor, &op).await?,
        };
        results.push(op_result(op.op_id().to_string(), outcome));
    }
    tx.commit().await?;
    Ok((results, ended_at))
}

async fn apply_create(
    tx: &mut Transaction<'_, Postgres>,
    run_id: i64,
    ended_at: Option<DateTime<Utc>>,
    actor: Actor,
    op: &RecordOp,
) -> AppResult<Outcome> {
    let RecordOp::Create {
        device_event_id,
        device_ts,
        polarity,
        note,
        device_type,
        via,
        ..
    } = op
    else {
        unreachable!("apply_create called with {op:?}");
    };
    let (via, remote_id) = match actor {
        Actor::Teacher => (via.as_deref().unwrap_or("console"), None),
        Actor::Remote { remote_id } => {
            if !remote_create_allowed(ended_at, *device_ts, Utc::now()) {
                return Ok(Outcome::Rejected(Rejection::new(
                    "run_ended",
                    "本節課已結束，結束後按的書籤不會記錄。",
                )));
            }
            ("remote", Some(remote_id))
        }
    };
    let inserted: Option<i64> = sqlx::query_scalar(
        "INSERT INTO record_events (lesson_run_id, device_event_id, device_ts, polarity, note,
                                    device_type, via, remote_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         ON CONFLICT (lesson_run_id, device_event_id) DO NOTHING
         RETURNING id",
    )
    .bind(run_id)
    .bind(device_event_id)
    .bind(device_ts)
    .bind(polarity)
    .bind(note.trim())
    .bind(device_type)
    .bind(via)
    .bind(remote_id)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(if inserted.is_some() {
        Outcome::Applied
    } else {
        // Already stored by an earlier attempt: the original is kept as-is.
        Outcome::Duplicate
    })
}

#[derive(sqlx::FromRow)]
struct EventState {
    id: i64,
    polarity: String,
    note: String,
    deleted: bool,
    remote_id: Option<i64>,
}

async fn apply_change(
    tx: &mut Transaction<'_, Postgres>,
    run_id: i64,
    ended_at: Option<DateTime<Utc>>,
    actor: Actor,
    op: &RecordOp,
) -> AppResult<Outcome> {
    if matches!(actor, Actor::Remote { .. })
        && matches!(op, RecordOp::Edit { .. } | RecordOp::Delete { .. })
    {
        return Ok(Outcome::Rejected(Rejection::new(
            "forbidden",
            "遙控器只能新增書籤與復原自己按的書籤，其他修改請在電腦上進行。",
        )));
    }
    let ev: Option<EventState> = sqlx::query_as(
        "SELECT id, polarity, note, deleted_at IS NOT NULL AS deleted, remote_id
           FROM record_events WHERE lesson_run_id = $1 AND device_event_id = $2 FOR UPDATE",
    )
    .bind(run_id)
    .bind(op.device_event_id())
    .fetch_optional(&mut **tx)
    .await?;
    let Some(ev) = ev else {
        return Ok(Outcome::Rejected(Rejection::new(
            "not_found",
            "找不到這個書籤。",
        )));
    };
    let seen: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM record_event_revisions
                         WHERE record_event_id = $1 AND client_op_id = $2)",
    )
    .bind(ev.id)
    .bind(op.op_id())
    .fetch_one(&mut **tx)
    .await?;
    if seen {
        return Ok(Outcome::Duplicate);
    }
    if let Actor::Remote { remote_id } = actor {
        if ev.remote_id != Some(remote_id) {
            return Ok(Outcome::Rejected(Rejection::new(
                "forbidden",
                "遙控器只能復原自己按的書籤。",
            )));
        }
        if ended_at.is_some() {
            return Ok(Outcome::Rejected(Rejection::new(
                "run_ended",
                "本節課已結束，請在電腦上修改書籤。",
            )));
        }
    }

    let before = json!({ "polarity": ev.polarity, "note": ev.note, "deleted": ev.deleted });
    let (change_type, at) = match op {
        RecordOp::Edit {
            polarity, note, at, ..
        } => {
            if ev.deleted {
                return Ok(Outcome::Rejected(Rejection::new(
                    "deleted",
                    "這個書籤已刪除，不能再修改。",
                )));
            }
            let polarity = polarity.clone().unwrap_or_else(|| ev.polarity.clone());
            let note = note
                .as_deref()
                .map(|n| n.trim().to_string())
                .unwrap_or_else(|| ev.note.clone());
            if polarity == ev.polarity && note == ev.note {
                return Ok(Outcome::Applied);
            }
            sqlx::query("UPDATE record_events SET polarity = $2, note = $3 WHERE id = $1")
                .bind(ev.id)
                .bind(&polarity)
                .bind(&note)
                .execute(&mut **tx)
                .await?;
            insert_revision(
                tx,
                ev.id,
                "edit",
                &before,
                &json!({ "polarity": polarity, "note": note, "deleted": false }),
                actor,
                op.op_id(),
                *at,
            )
            .await?;
            return Ok(Outcome::Applied);
        }
        RecordOp::Undo { at, .. } => ("undo", *at),
        RecordOp::Delete { at, .. } => ("delete", *at),
        RecordOp::Create { .. } => unreachable!("creates are handled by apply_create"),
    };
    if ev.deleted {
        // Already gone: nothing changes, nothing to record.
        return Ok(Outcome::Applied);
    }
    sqlx::query("UPDATE record_events SET deleted_at = now() WHERE id = $1")
        .bind(ev.id)
        .execute(&mut **tx)
        .await?;
    insert_revision(
        tx,
        ev.id,
        change_type,
        &before,
        &json!({ "polarity": ev.polarity, "note": ev.note, "deleted": true }),
        actor,
        op.op_id(),
        at,
    )
    .await?;
    Ok(Outcome::Applied)
}

#[allow(clippy::too_many_arguments)]
async fn insert_revision(
    tx: &mut Transaction<'_, Postgres>,
    event_id: i64,
    change_type: &str,
    before: &Value,
    after: &Value,
    actor: Actor,
    op_id: &str,
    device_ts: Option<DateTime<Utc>>,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO record_event_revisions
           (record_event_id, change_type, before_value, after_value, actor, client_op_id, device_ts)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(event_id)
    .bind(change_type)
    .bind(before)
    .bind(after)
    .bind(actor.as_str())
    .bind(op_id)
    .bind(device_ts)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub async fn sync(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(run_id): ApiPath<i64>,
    ApiJson(req): ApiJson<SyncRequest>,
) -> AppResult<Json<SyncResponse>> {
    owned_run(&state.db, run_id, teacher.id()).await?;
    let (results, ended_at) = sync_ops(&state.db, run_id, Actor::Teacher, &req.ops).await?;
    Ok(Json(SyncResponse {
        results,
        events: run_events(&state.db, run_id, None).await?,
        ended_at,
    }))
}

// ---------------------------------------------------------------- 課後補寫

#[derive(Debug, Deserialize)]
pub struct ReviewRequest {
    #[serde(default)]
    pub activity_step_id: Option<i64>,
    #[serde(default)]
    pub behaviors: Vec<String>,
    #[serde(default)]
    pub handling: Option<String>,
    #[serde(default)]
    pub followup_text: String,
}

#[derive(sqlx::FromRow)]
struct ReviewState {
    id: i64,
    deleted: bool,
    activity_step_id: Option<i64>,
    behaviors: Vec<String>,
    handling: Option<String>,
    followup_text: String,
}

pub async fn review(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath((run_id, device_event_id)): ApiPath<(i64, String)>,
    ApiJson(req): ApiJson<ReviewRequest>,
) -> AppResult<Json<RecordEvent>> {
    let run = owned_run(&state.db, run_id, teacher.id()).await?;
    let behaviors = normalize_behaviors(&req.behaviors)
        .map_err(|bad| AppError::bad_request(format!("學生可觀察行為「{bad}」不正確。")))?;
    let handling = req.handling.filter(|h| !h.trim().is_empty());
    if let Some(h) = &handling {
        if !valid_handling(h) {
            return Err(AppError::bad_request(format!("處理方式「{h}」不正確。")));
        }
    }
    let followup_text = req.followup_text.trim().to_string();
    if followup_text.chars().count() > MAX_FOLLOWUP_CHARS {
        return Err(AppError::bad_request(format!(
            "補寫最多 {MAX_FOLLOWUP_CHARS} 字。"
        )));
    }
    if let Some(step_id) = req.activity_step_id {
        let ok: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM lesson_steps s
                              JOIN lesson_versions v ON v.id = s.lesson_version_id
                             WHERE s.id = $1 AND v.lesson_id = $2)",
        )
        .bind(step_id)
        .bind(run.lesson_id)
        .fetch_one(&state.db)
        .await?;
        if !ok {
            return Err(AppError::bad_request("活動歸屬只能選這個課例的教案步驟。"));
        }
    }

    let mut tx = state.db.begin().await?;
    let ev: ReviewState = sqlx::query_as(
        "SELECT id, deleted_at IS NOT NULL AS deleted, activity_step_id, icap_cues AS behaviors,
                handling, COALESCE(followup_text, '') AS followup_text
           FROM record_events WHERE lesson_run_id = $1 AND device_event_id = $2 FOR UPDATE",
    )
    .bind(run_id)
    .bind(&device_event_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::not_found("找不到這個書籤。"))?;
    if ev.deleted {
        return Err(AppError::Conflict("這個書籤已刪除。".into()));
    }
    let before = json!({
        "activity_step_id": ev.activity_step_id, "behaviors": ev.behaviors,
        "handling": ev.handling, "followup_text": ev.followup_text,
    });
    let after = json!({
        "activity_step_id": req.activity_step_id, "behaviors": behaviors,
        "handling": handling, "followup_text": followup_text,
    });
    if before != after {
        sqlx::query(
            "UPDATE record_events
                SET activity_step_id = $2, icap_cues = $3, handling = $4, followup_text = $5,
                    followup_at = now()
              WHERE id = $1",
        )
        .bind(ev.id)
        .bind(req.activity_step_id)
        .bind(&behaviors)
        .bind(&handling)
        .bind(&followup_text)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO record_event_revisions (record_event_id, change_type, before_value, after_value, actor)
             VALUES ($1, 'review', $2, $3, 'teacher')",
        )
        .bind(ev.id)
        .bind(&before)
        .bind(&after)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    let event = sqlx::query_as(&format!(
        "SELECT {EVENT_COLS} FROM record_events e WHERE e.id = $1"
    ))
    .bind(ev.id)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(event))
}

// ---------------------------------------------------------------- QR pairing

#[derive(Debug, Serialize)]
pub struct Pairing {
    pub pairing_id: i64,
    /// Shown once (inside the QR code); only its SHA-256 is stored.
    pub code: String,
    pub expires_at: DateTime<Utc>,
}

pub async fn create_pairing(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(run_id): ApiPath<i64>,
) -> AppResult<Json<Pairing>> {
    let run = owned_run(&state.db, run_id, teacher.id()).await?;
    if run.ended_at.is_some() {
        return Err(AppError::Conflict(
            "本節課已結束，不能再配對遙控器。".into(),
        ));
    }
    let code = new_token();
    let mut tx = state.db.begin().await?;
    // Only the newest QR code works: showing a new one retires the old ones.
    sqlx::query(
        "UPDATE run_pairings SET revoked_at = now()
          WHERE lesson_run_id = $1 AND used_at IS NULL AND revoked_at IS NULL",
    )
    .bind(run_id)
    .execute(&mut *tx)
    .await?;
    let (pairing_id, expires_at): (i64, DateTime<Utc>) = sqlx::query_as(
        "INSERT INTO run_pairings (lesson_run_id, code_hash, expires_at)
         VALUES ($1, $2, now() + make_interval(mins => $3))
         RETURNING id, expires_at",
    )
    .bind(run_id)
    .bind(sha256_hex(&code))
    .bind(PAIRING_MINUTES as i32)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(Pairing {
        pairing_id,
        code,
        expires_at,
    }))
}

/// Revokes unused QR codes and every paired remote of the run.
pub async fn revoke_pairing(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(run_id): ApiPath<i64>,
) -> AppResult<Json<Value>> {
    owned_run(&state.db, run_id, teacher.id()).await?;
    let mut tx = state.db.begin().await?;
    let pairings = sqlx::query(
        "UPDATE run_pairings SET revoked_at = now()
          WHERE lesson_run_id = $1 AND used_at IS NULL AND revoked_at IS NULL",
    )
    .bind(run_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    let remotes = sqlx::query(
        "UPDATE run_remotes SET revoked_at = now() WHERE lesson_run_id = $1 AND revoked_at IS NULL",
    )
    .bind(run_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    tx.commit().await?;
    Ok(Json(json!({
        "ok": true,
        "revoked_pairings": pairings,
        "revoked_remotes": remotes,
    })))
}
