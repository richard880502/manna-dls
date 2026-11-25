//! Durable dispatch metadata for text-model work.
//!
//! Feature handlers keep ownership of their result rows.  This module only decides which
//! durable job may start, tracks a lease while it runs, and makes a job visible to the UI.
//!
//! - this file: the table's data layer (enqueue, claim, lease, finish, cancel, recover, ETA);
//! - [`dispatcher`]: the per-process task that claims and runs jobs, and shuts down gracefully;
//! - [`handlers`]: what each job `kind` runs, and how its feature rows are failed.

pub mod dispatcher;
pub mod handlers;
pub mod routes;

use std::time::Duration;

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{FromRow, PgPool, Postgres, Transaction};

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, FromRow)]
pub struct Job {
    pub id: i64,
    pub kind: String,
    pub teacher_id: i64,
    pub priority: i16,
    pub status: String,
    pub params: Value,
    pub ref_kind: Option<String>,
    pub ref_id: Option<i64>,
    pub attempts: i32,
    pub lease_until: Option<DateTime<Utc>>,
    pub queued_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
}

pub struct NewJob<'a> {
    pub kind: &'a str,
    pub teacher_id: i64,
    pub priority: u8,
    pub params: Value,
    pub ref_kind: &'a str,
    pub ref_id: i64,
}

/// How long a job's lease is renewed for, as configured.
pub fn lease_of(config: &crate::config::Config) -> Duration {
    Duration::from_secs(config.ai_job_lease_secs.max(3))
}

/// What a job's owner is told when it was lost with its server.
pub const MSG_LOST: &str = "伺服器重新啟動，這次沒有完成。請按再試一次。";

/// Adds a job in the caller's transaction so an accepted request cannot lose its queue entry.
/// The per-teacher queued count is checked under an advisory lock keyed on the teacher, so two
/// requests of the same teacher cannot both pass the check.
pub async fn enqueue_tx(
    tx: &mut Transaction<'_, Postgres>,
    job: NewJob<'_>,
    max_queued: usize,
) -> AppResult<i64> {
    if job.priority > 2 {
        return Err(AppError::bad_request("AI 工作優先級不正確。"));
    }
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('ai_jobs.enqueue'), $1::int)")
        .bind(job.teacher_id as i32)
        .execute(&mut **tx)
        .await?;
    let queued: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ai_jobs WHERE teacher_id = $1 AND status = 'queued'",
    )
    .bind(job.teacher_id)
    .fetch_one(&mut **tx)
    .await?;
    if queued >= max_queued as i64 {
        return Err(AppError::Conflict(MSG_TOO_MANY.into()));
    }
    let id = sqlx::query_scalar(
        "INSERT INTO ai_jobs (kind, teacher_id, priority, params, ref_kind, ref_id)
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
    )
    .bind(job.kind)
    .bind(job.teacher_id)
    .bind(i16::from(job.priority))
    .bind(job.params)
    .bind(job.ref_kind)
    .bind(job.ref_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| {
        if e.as_database_error()
            .is_some_and(|db| db.code().as_deref() == Some("23505"))
        {
            AppError::Conflict("這個工作已在排隊或執行中。".into())
        } else {
            e.into()
        }
    })?;
    Ok(id)
}

pub const MSG_TOO_MANY: &str = "你排隊中的工作太多了，等前面的完成再送。";

/// Enqueues in its own transaction (for callers that have none).
pub async fn enqueue(db: &PgPool, job: NewJob<'_>, max_queued: usize) -> AppResult<i64> {
    let mut tx = db.begin().await?;
    let id = enqueue_tx(&mut tx, job, max_queued).await?;
    tx.commit().await?;
    Ok(id)
}

const JOB_COLS: &str = "id, kind, teacher_id, priority, status, params, ref_kind, ref_id,
    attempts, lease_until, queued_at, started_at";

/// Atomically claims the next fair job. `SKIP LOCKED` lets API instances share a database.
pub async fn claim_next(
    db: &PgPool,
    max_per_teacher: usize,
    lease: Duration,
) -> sqlx::Result<Option<Job>> {
    let seconds = i64::try_from(lease.as_secs()).unwrap_or(i64::MAX);
    sqlx::query_as::<_, Job>(
        "WITH running AS (
             SELECT teacher_id, count(*) AS n FROM ai_jobs WHERE status = 'running' GROUP BY teacher_id
         ), candidate AS (
             SELECT j.id FROM ai_jobs j
             JOIN teachers t ON t.id = j.teacher_id
             LEFT JOIN running r ON r.teacher_id = j.teacher_id
             WHERE j.status = 'queued'
               AND (t.role = 'pi' OR coalesce(r.n, 0) < $1)
             ORDER BY j.priority, coalesce(r.n, 0), j.queued_at, j.id
             LIMIT 1 FOR UPDATE OF j SKIP LOCKED
         )
         UPDATE ai_jobs j SET status = 'running', started_at = coalesce(j.started_at, now()),
             attempts = j.attempts + 1, lease_until = now() + ($2 * interval '1 second')
         FROM candidate WHERE j.id = candidate.id
         RETURNING j.id, j.kind, j.teacher_id, j.priority, j.status, j.params, j.ref_kind,
             j.ref_id, j.attempts, j.lease_until, j.queued_at, j.started_at",
    )
    .bind(max_per_teacher as i64)
    .bind(seconds)
    .fetch_optional(db)
    .await
}

/// Extends the lease of run number `attempts` of a job. False when that run is no longer the
/// job's (the lease expired and the job was reclaimed, finished or cancelled): its worker must
/// stop treating the job as its own.
pub async fn renew_lease(
    db: &PgPool,
    id: i64,
    attempts: i32,
    lease: Duration,
) -> sqlx::Result<bool> {
    let seconds = i64::try_from(lease.as_secs()).unwrap_or(i64::MAX);
    Ok(sqlx::query(
        "UPDATE ai_jobs SET lease_until = now() + ($3 * interval '1 second')
          WHERE id = $1 AND status = 'running' AND attempts = $2",
    )
    .bind(id)
    .bind(attempts)
    .bind(seconds)
    .execute(db)
    .await?
    .rows_affected()
        == 1)
}

/// Closes run number `attempts` of a job. False when that run is no longer the job's (a newer
/// run owns it), in which case nothing is written.
pub async fn finish(
    db: &PgPool,
    id: i64,
    attempts: i32,
    result: Result<(), &str>,
) -> sqlx::Result<bool> {
    let (status, error) = match result {
        Ok(()) => ("done", None),
        Err(message) => ("failed", Some(message)),
    };
    Ok(sqlx::query(
        "UPDATE ai_jobs SET status = $3, error = $4, lease_until = NULL, finished_at = now()
          WHERE id = $1 AND status = 'running' AND attempts = $2",
    )
    .bind(id)
    .bind(attempts)
    .bind(status)
    .bind(error)
    .execute(db)
    .await?
    .rows_affected()
        == 1)
}

/// Lets the lease of a run end now (a process that stops with work still running), so the next
/// process recovers the job at once instead of after the lease.
pub async fn release_lease(db: &PgPool, id: i64, attempts: i32) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE ai_jobs SET lease_until = now() - interval '1 second'
          WHERE id = $1 AND status = 'running' AND attempts = $2",
    )
    .bind(id)
    .bind(attempts)
    .execute(db)
    .await?;
    Ok(())
}

/// Cancels a queued job of `teacher_id` and returns it (so the caller can close what the job
/// was for). 404 when the job is not that teacher's, 409 once it has started or ended.
pub async fn cancel(db: &PgPool, id: i64, teacher_id: i64) -> AppResult<Job> {
    let job = sqlx::query_as::<_, Job>(&format!(
        "UPDATE ai_jobs SET status = 'cancelled', finished_at = now()
          WHERE id = $1 AND teacher_id = $2 AND status = 'queued' RETURNING {JOB_COLS}"
    ))
    .bind(id)
    .bind(teacher_id)
    .fetch_optional(db)
    .await?;
    match job {
        Some(job) => Ok(job),
        None => {
            let mine: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM ai_jobs WHERE id = $1 AND teacher_id = $2)",
            )
            .bind(id)
            .bind(teacher_id)
            .fetch_one(db)
            .await?;
            if mine {
                Err(AppError::Conflict(
                    "這個工作已經開始或結束了，沒辦法取消排隊。".into(),
                ))
            } else {
                Err(AppError::not_found("找不到這個工作。"))
            }
        }
    }
}

/// What recovery did.
#[derive(Debug, Default)]
pub struct Recovery {
    /// Jobs put back in the queue.
    pub requeued: u64,
    /// Jobs that ended `failed` (their feature rows must be failed too).
    pub failed: Vec<Job>,
}

/// Requeue work whose server died after its lease. Handlers decide whether a kind is replayable.
pub async fn recover_expired(
    db: &PgPool,
    max_attempts: i32,
    replayable: &[&str],
) -> sqlx::Result<Recovery> {
    let requeued = sqlx::query(
        "UPDATE ai_jobs SET status = 'queued', lease_until = NULL, started_at = NULL
        WHERE status = 'running' AND lease_until < now() AND attempts < $1 AND kind = ANY($2)",
    )
    .bind(max_attempts)
    .bind(replayable)
    .execute(db)
    .await?
    .rows_affected();
    let failed = sqlx::query_as::<_, Job>(&format!(
        "UPDATE ai_jobs SET status = 'failed', error = $1, lease_until = NULL, finished_at = now()
          WHERE status = 'running' AND lease_until < now() RETURNING {JOB_COLS}"
    ))
    .bind(MSG_LOST)
    .fetch_all(db)
    .await?;
    Ok(Recovery { requeued, failed })
}

/// How many queued jobs will be claimed before this one (by priority, then arrival; an
/// estimate, as fairness between teachers may reorder them). `None` unless the job is queued.
pub async fn ahead(db: &PgPool, id: i64) -> sqlx::Result<Option<i64>> {
    sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM ai_jobs q
                  WHERE q.status = 'queued' AND q.id <> j.id
                    AND (q.priority < j.priority
                         OR (q.priority = j.priority AND (q.queued_at, q.id) < (j.queued_at, j.id))))
           FROM ai_jobs j WHERE j.id = $1 AND j.status = 'queued'",
    )
    .bind(id)
    .fetch_optional(db)
    .await
}

/// Average seconds a job of `kind` ran in the last hour; `None` without data.
pub async fn avg_run_secs(db: &PgPool, kind: &str) -> sqlx::Result<Option<f64>> {
    sqlx::query_scalar(
        "SELECT avg(extract(epoch FROM finished_at - started_at))::float8 FROM ai_jobs
          WHERE kind = $1 AND status = 'done' AND finished_at > now() - interval '1 hour'
            AND started_at IS NOT NULL",
    )
    .bind(kind)
    .fetch_one(db)
    .await
}

/// `eta_secs` of a job with `ahead` jobs before it: `ahead / max_running * average run`.
pub fn eta_secs(ahead: i64, max_running: usize, avg_run_secs: Option<f64>) -> Option<i64> {
    let avg = avg_run_secs.filter(|a| a.is_finite() && *a >= 0.0)?;
    Some((ahead as f64 / max_running.max(1) as f64 * avg).ceil() as i64)
}

/// The queue state of the job of `(ref_kind, ref_id)` while it waits; `None` when it is not
/// queued (not enqueued, running or over).
pub async fn info_for_ref(
    state: &crate::AppState,
    ref_kind: &str,
    ref_id: i64,
) -> sqlx::Result<Option<crate::progress::QueueInfo>> {
    let job: Option<(i64, String)> = sqlx::query_as(
        "SELECT id, kind FROM ai_jobs WHERE ref_kind = $1 AND ref_id = $2 AND status = 'queued'",
    )
    .bind(ref_kind)
    .bind(ref_id)
    .fetch_optional(&state.db)
    .await?;
    let Some((job_id, kind)) = job else {
        return Ok(None);
    };
    let Some(ahead) = ahead(&state.db, job_id).await? else {
        return Ok(None);
    };
    let avg = avg_run_secs(&state.db, &kind).await?;
    Ok(Some(crate::progress::QueueInfo {
        job_id,
        ahead,
        eta_secs: eta_secs(ahead, state.config.ai_max_running_jobs, avg),
    }))
}
