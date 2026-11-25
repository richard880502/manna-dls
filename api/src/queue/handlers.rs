//! What each job kind runs, and how the rows a job was for are closed when it cannot finish.
//!
//! A handler takes only ids from `params` and reads everything else from the database.

use futures_util::future::BoxFuture;

use super::Job;
use crate::error::{AppError, AppResult};
use crate::AppState;

pub const KIND_PREP_TURN: &str = "prep_turn";
pub const KIND_WORKSHEET_RUN: &str = "worksheet_run";
pub const KIND_BLOCK_REGENERATE: &str = "block_regenerate";
pub const KIND_BLOCK_REVISE: &str = "block_revise";
pub const KIND_SOP_STEP: &str = "sop_step";
pub const KIND_TEMPLATE_EXTRACT: &str = "template_extract";
pub const KIND_SIOP_CHECK: &str = "siop_check";

pub struct Handler {
    /// Whether a job lost with its server may simply run again from the start.
    pub replayable: bool,
    pub run: fn(AppState, Job) -> BoxFuture<'static, AppResult<()>>,
}

static PREP_TURN: Handler = Handler {
    // A turn writes its result once, at its end (a chat turn's tool edits are one version).
    replayable: true,
    run: |state, job| Box::pin(async move { prep_turn(state, job).await }),
};

static WORKSHEET_RUN: Handler = Handler {
    // The run starts over from its first step (see `worksheet::ai::run_queued`).
    replayable: true,
    run: |state, job| Box::pin(async move { worksheet_run(state, job).await }),
};

static BLOCK_REGENERATE: Handler = Handler {
    // Starts over from the block's first step; the new block is written once, at the end.
    replayable: true,
    run: |state, job| Box::pin(async move { block_regenerate(state, job).await }),
};

static BLOCK_REVISE: Handler = Handler {
    // The result is a proposal row that only an adoption writes into the worksheet.
    replayable: true,
    run: |state, job| Box::pin(async move { block_revise(state, job).await }),
};

static SOP_STEP: Handler = Handler {
    // One stage's result is written once, at its end, into the job's own row.
    replayable: true,
    run: |state, job| Box::pin(async move { sop_step(state, job).await }),
};

static TEMPLATE_EXTRACT: Handler = Handler {
    // Reads the sample again and writes a new template version once, at its end.
    replayable: true,
    run: |state, job| Box::pin(async move { template_extract(state, job).await }),
};

static SIOP_CHECK: Handler = Handler {
    // A check reads the finished output and writes its judgements once.
    replayable: true,
    run: |state, job| Box::pin(async move { siop_check(state, job).await }),
};

pub fn handler(kind: &str) -> Option<&'static Handler> {
    match kind {
        KIND_PREP_TURN => Some(&PREP_TURN),
        KIND_WORKSHEET_RUN => Some(&WORKSHEET_RUN),
        KIND_BLOCK_REGENERATE => Some(&BLOCK_REGENERATE),
        KIND_BLOCK_REVISE => Some(&BLOCK_REVISE),
        KIND_SOP_STEP => Some(&SOP_STEP),
        KIND_TEMPLATE_EXTRACT => Some(&TEMPLATE_EXTRACT),
        KIND_SIOP_CHECK => Some(&SIOP_CHECK),
        _ => None,
    }
}

const ALL_KINDS: [&str; 7] = [
    KIND_PREP_TURN,
    KIND_WORKSHEET_RUN,
    KIND_BLOCK_REGENERATE,
    KIND_BLOCK_REVISE,
    KIND_SOP_STEP,
    KIND_TEMPLATE_EXTRACT,
    KIND_SIOP_CHECK,
];

pub fn replayable_kinds() -> Vec<&'static str> {
    ALL_KINDS
        .into_iter()
        .filter(|k| handler(k).is_some_and(|h| h.replayable))
        .collect()
}

fn id_param(job: &Job, key: &str) -> AppResult<i64> {
    job.params[key]
        .as_i64()
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("job {} has no {key}", job.id)))
}

fn text_param(job: &Job, key: &str) -> String {
    job.params[key].as_str().unwrap_or_default().to_string()
}

async fn prep_turn(state: AppState, job: Job) -> AppResult<()> {
    let turn_id = id_param(&job, "turn_id")?;
    crate::prep::run::run_queued(
        &state,
        crate::prep::run::Queued {
            turn_id,
            track: text_param(&job, "track"),
            worksheet_id: job.params["worksheet_id"].as_i64(),
            worksheet_error: job.params["worksheet_error"].as_str().map(str::to_string),
        },
    )
    .await
}

async fn worksheet_run(state: AppState, job: Job) -> AppResult<()> {
    let worksheet_id = id_param(&job, "worksheet_id")?;
    let run_no = i32::try_from(id_param(&job, "run_no")?).unwrap_or(0);
    crate::worksheet::ai::run_queued(
        &state,
        worksheet_id,
        run_no,
        &text_param(&job, "track"),
        job.attempts > 1,
    )
    .await
}

async fn block_regenerate(state: AppState, job: Job) -> AppResult<()> {
    let worksheet_id = id_param(&job, "worksheet_id")?;
    let run_no = i32::try_from(id_param(&job, "run_no")?).unwrap_or(0);
    crate::worksheet::ai::run_block_queued(
        &state,
        worksheet_id,
        run_no,
        &text_param(&job, "track"),
        job.attempts > 1,
    )
    .await
}

async fn block_revise(state: AppState, job: Job) -> AppResult<()> {
    let proposal_id = id_param(&job, "proposal_id")?;
    crate::worksheet::ai::revise::run_queued(
        &state,
        job.teacher_id,
        proposal_id,
        &text_param(&job, "track"),
    )
    .await
}

async fn sop_step(state: AppState, job: Job) -> AppResult<()> {
    let sop_job_id = id_param(&job, "sop_job_id")?;
    crate::sop::ai::run_queued(
        &state,
        sop_job_id,
        job.teacher_id,
        job.params["class_id"].as_i64(),
        &text_param(&job, "track"),
    )
    .await
}

async fn template_extract(state: AppState, job: Job) -> AppResult<()> {
    let template_id = id_param(&job, "template_id")?;
    let run = i32::try_from(id_param(&job, "run")?).unwrap_or(0);
    crate::worksheet::template::extract::run_queued(&state, template_id, run).await
}

async fn siop_check(state: AppState, job: Job) -> AppResult<()> {
    let check_id = id_param(&job, "check_id")?;
    crate::prep::selfcheck::run_queued(&state, check_id).await
}

/// Closes the rows a job was for when the job cannot finish (lost with its server, an error
/// the handler did not settle, an unknown kind): the page then shows the failure and offers
/// 再試一次 instead of waiting.
pub async fn fail_feature(state: &AppState, job: &Job, message: &str) {
    let result = match job.kind.as_str() {
        KIND_PREP_TURN => match job.ref_id {
            Some(turn_id) => crate::prep::store::finish(&state.db, turn_id, Err(message.into()))
                .await
                .map(|_| ()),
            None => Ok(()),
        },
        KIND_BLOCK_REVISE => match job.ref_id {
            Some(proposal_id) => crate::worksheet::revise::finish(
                &state.db,
                proposal_id,
                Err(message.to_string()),
                None,
            )
            .await
            .map(|_| ()),
            None => Ok(()),
        },
        KIND_SOP_STEP => match job.ref_id {
            Some(id) => crate::sop::ai::close_failed(&state.db, id, message).await,
            None => Ok(()),
        },
        KIND_TEMPLATE_EXTRACT => match (job.ref_id, job.params["run"].as_i64()) {
            (Some(id), Some(run)) => crate::worksheet::template::extract::close_failed(
                state,
                id,
                job.teacher_id,
                i32::try_from(run).unwrap_or(0),
                message,
            )
            .await
            .map_err(|e| sqlx::Error::Protocol(e.to_string())),
            _ => Ok(()),
        },
        KIND_SIOP_CHECK => match job.ref_id {
            Some(id) => crate::prep::selfcheck::close_failed(&state.db, id, message).await,
            None => Ok(()),
        },
        KIND_WORKSHEET_RUN | KIND_BLOCK_REGENERATE => {
            let run_no = job.params["run_no"]
                .as_i64()
                .and_then(|n| i32::try_from(n).ok());
            match (job.ref_id, run_no) {
                (Some(worksheet_id), Some(run_no)) => {
                    crate::worksheet::ai::close_run_failed(&state.db, worksheet_id, run_no, message)
                        .await
                }
                _ => Ok(()),
            }
        }
        _ => Ok(()),
    };
    if let Err(e) = result {
        tracing::error!(job_id = job.id, error = %e, "could not close the rows of an AI job");
    }
}

/// Closes what a cancelled (still queued) job was for.
pub async fn cancelled(state: &AppState, job: &Job) {
    fail_feature(state, job, MSG_CANCELLED).await;
    // A prep turn that was waiting for this worksheet's generation ends with it.
    if job.kind == KIND_WORKSHEET_RUN {
        if let Some(worksheet_id) = job.ref_id {
            let turns: Vec<i64> = sqlx::query_scalar(
                "SELECT id FROM prep_turns WHERE worksheet_id = $1 AND status = 'running'",
            )
            .bind(worksheet_id)
            .fetch_all(&state.db)
            .await
            .unwrap_or_default();
            for turn_id in turns {
                let _ =
                    crate::prep::store::finish(&state.db, turn_id, Err(MSG_CANCELLED.into())).await;
            }
        }
    }
}

pub const MSG_CANCELLED: &str = "已取消排隊。";
