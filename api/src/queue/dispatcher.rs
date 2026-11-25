//! The dispatcher: one tokio task per process that claims queued jobs while there is room and
//! runs them through [`handlers`], keeps their leases alive, recovers jobs whose server died,
//! and drains on shutdown.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use futures_util::future::{AbortHandle, Abortable};
use serde_json::json;
use tokio::sync::Notify;

use super::{handlers, Job};
use crate::error::AppError;
use crate::AppState;

/// How often the dispatcher looks for work when nobody woke it.
const TICK: Duration = Duration::from_secs(2);
/// How often expired leases are looked for.
const RECOVER_EVERY: Duration = Duration::from_secs(60);

/// The process-local half of the queue: wake-ups, the count of jobs running here and the stop
/// flag. The jobs themselves are rows of `ai_jobs`.
#[derive(Default)]
pub struct Queue {
    wake: Notify,
    stopped: Notify,
    stopping: AtomicBool,
    running: AtomicUsize,
    /// `job id → attempt` of the jobs running in this process.
    local: Mutex<HashMap<i64, i32>>,
}

impl Queue {
    /// Tells the dispatcher a job was added or finished.
    pub fn wake(&self) {
        self.wake.notify_one();
    }

    pub fn running(&self) -> usize {
        self.running.load(Ordering::SeqCst)
    }

    pub fn is_stopping(&self) -> bool {
        self.stopping.load(Ordering::SeqCst)
    }

    /// Stops claiming (shutdown). Running jobs go on.
    pub fn stop(&self) {
        self.stopping.store(true, Ordering::SeqCst);
        self.wake.notify_one();
        self.stopped.notify_waiters();
    }

    fn started(&self, job: &Job) {
        self.local
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(job.id, job.attempts);
        self.running.fetch_add(1, Ordering::SeqCst);
    }

    fn ended(&self, job: &Job) {
        self.local
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&job.id);
        self.running.fetch_sub(1, Ordering::SeqCst);
        self.wake();
    }

    fn local_jobs(&self) -> Vec<(i64, i32)> {
        self.local
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|(id, attempts)| (*id, *attempts))
            .collect()
    }
}

/// Recovers expired leases once: jobs of replayable kinds are queued again, the rest fail and
/// take their feature rows with them. Returns `(requeued, failed)`.
pub async fn recover(state: &AppState) -> sqlx::Result<(u64, usize)> {
    let recovery = super::recover_expired(
        &state.db,
        state.config.ai_job_max_attempts,
        &handlers::replayable_kinds(),
    )
    .await?;
    for job in &recovery.failed {
        handlers::fail_feature(state, job, super::MSG_LOST).await;
        log_finished(state, job, "failed").await;
    }
    if recovery.requeued > 0 || !recovery.failed.is_empty() {
        tracing::warn!(
            requeued = recovery.requeued,
            failed = recovery.failed.len(),
            "recovered AI jobs whose server stopped"
        );
        state.queue.wake();
    }
    Ok((recovery.requeued, recovery.failed.len()))
}

/// The dispatcher loop. Returns once [`Queue::stop`] was called.
pub async fn run(state: AppState) {
    let queue = state.queue.clone();
    let mut last_recovery = tokio::time::Instant::now() - RECOVER_EVERY;
    while !queue.is_stopping() {
        if last_recovery.elapsed() >= RECOVER_EVERY {
            last_recovery = tokio::time::Instant::now();
            if let Err(e) = recover(&state).await {
                tracing::error!(error = %e, "AI job recovery failed");
            }
        }
        dispatch_ready(&state).await;
        tokio::select! {
            _ = queue.wake.notified() => {}
            _ = tokio::time::sleep(TICK) => {}
        }
    }
}

/// Claims and starts jobs while this process has room.
pub async fn dispatch_ready(state: &AppState) {
    let queue = &state.queue;
    let cfg = &state.config;
    while !queue.is_stopping() && queue.running() < cfg.ai_max_running_jobs {
        let claimed = super::claim_next(
            &state.db,
            cfg.ai_max_running_per_teacher,
            super::lease_of(cfg),
        )
        .await;
        match claimed {
            Ok(Some(job)) => {
                queue.started(&job);
                let state = state.clone();
                tokio::spawn(async move {
                    work(&state, job.clone()).await;
                    state.queue.ended(&job);
                });
            }
            Ok(None) => break,
            Err(e) => {
                tracing::error!(error = %e, "could not claim an AI job");
                break;
            }
        }
    }
}

/// Runs one claimed job to its end, with a heartbeat on its lease.
async fn work(state: &AppState, job: Job) {
    let lease = super::lease_of(&state.config);
    let Some(handler) = handlers::handler(&job.kind) else {
        let message = "這種工作不認得，沒有執行。";
        tracing::error!(job_id = job.id, kind = %job.kind, "unknown AI job kind");
        let _ = super::finish(&state.db, job.id, job.attempts, Err(message)).await;
        handlers::fail_feature(state, &job, message).await;
        log_finished(state, &job, "failed").await;
        return;
    };
    let (abort, registration) = AbortHandle::new_pair();
    let heartbeat = {
        let db = state.db.clone();
        let (id, attempts) = (job.id, job.attempts);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(lease / 3);
            tick.tick().await;
            loop {
                tick.tick().await;
                match super::renew_lease(&db, id, attempts, lease).await {
                    Ok(true) => {}
                    Ok(false) => {
                        // Someone else owns the job now: this run must stop.
                        tracing::warn!(
                            job_id = id,
                            attempts,
                            "AI job lease lost; stopping its work"
                        );
                        abort.abort();
                        return;
                    }
                    Err(e) => {
                        tracing::warn!(job_id = id, error = %e, "could not renew an AI job lease")
                    }
                }
            }
        })
    };
    let work = Abortable::new((handler.run)(state.clone(), job.clone()), registration);
    let result = crate::jobs::run("ai_job", job.id, async move {
        match work.await {
            Ok(result) => result,
            Err(_) => Err(AppError::Internal(anyhow::anyhow!("lease lost"))),
        }
    })
    .await;
    heartbeat.abort();
    let (status, message) = match &result {
        Ok(()) => ("done", None),
        Err(AppError::Internal(e)) if e.to_string() == "lease lost" => {
            // The job belongs to a newer run (or to recovery): write nothing.
            return;
        }
        Err(e) => ("failed", Some(failure_message(e))),
    };
    let owned = match super::finish(
        &state.db,
        job.id,
        job.attempts,
        message.as_deref().map_or(Ok(()), Err),
    )
    .await
    {
        Ok(owned) => owned,
        Err(e) => {
            tracing::error!(job_id = job.id, error = %e, "could not close an AI job");
            return;
        }
    };
    if !owned {
        return;
    }
    if let Some(message) = &message {
        handlers::fail_feature(state, &job, message).await;
    }
    log_finished(state, &job, status).await;
}

fn failure_message(error: &AppError) -> String {
    match error {
        AppError::BadRequest(m)
        | AppError::NotFound(m)
        | AppError::Conflict(m)
        | AppError::Forbidden(m)
        | AppError::Unauthorized(m)
        | AppError::Unavailable(m) => m.clone(),
        AppError::Internal(_) => "處理這個請求時發生錯誤。請按「再試一次」。".to_string(),
    }
}

/// `usage_logs` `ai_job_finished`: when it waited and how long it ran (no content).
async fn log_finished(state: &AppState, job: &Job, status: &str) {
    let now = chrono::Utc::now();
    let wait_ms = job
        .started_at
        .map_or(0, |s| (s - job.queued_at).num_milliseconds().max(0));
    let run_ms = job
        .started_at
        .map_or(0, |s| (now - s).num_milliseconds().max(0));
    let logged = crate::audit::log_usage(
        &state.db,
        job.teacher_id,
        "ai_job_finished",
        json!({
            "job_id": job.id, "kind": job.kind, "wait_ms": wait_ms, "run_ms": run_ms,
            "status": status, "attempts": job.attempts,
        }),
    )
    .await;
    if let Err(e) = logged {
        tracing::warn!(job_id = job.id, error = %e, "could not log ai_job_finished");
    }
}

/// Graceful shutdown: stop claiming, wait up to `grace` for the jobs running here, then let
/// their leases end so the next process recovers what is left at once.
pub async fn drain(state: &AppState, grace: Duration) {
    let queue = &state.queue;
    queue.stop();
    let deadline = tokio::time::Instant::now() + grace;
    while queue.running() > 0 && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let left = queue.local_jobs();
    if !left.is_empty() {
        tracing::warn!(
            left = left.len(),
            "AI jobs still running at shutdown; releasing their leases"
        );
    }
    for (id, attempts) in left {
        let _ = super::release_lease(&state.db, id, attempts).await;
    }
}
