//! The two validated AI stages of the lesson SOP, run as background jobs:
//! (a) passage analysis and (b) SIOP objectives linked to curriculum items.
//!
//! Why a job and not a request: one model call takes 15–50 s and now and then 200 s; with the
//! repair call that is more than a proxy keeps a request open. `POST` creates a
//! `lesson_sop_jobs` row and returns; the work happens in a spawned task and the browser polls.
//!
//! Designed around docs/LLM_VALIDATION.md:
//! * one stage per call, always with `max_tokens`;
//! * the gateway caches identical requests, so the repair call changes the request (higher
//!   temperature, the failed checks appended), and so does a teacher's later retry;
//! * paragraphs are split in code and the output is checked against them: paragraphs the model
//!   left out are asked for again, once;
//! * evidence must be a continuous piece of its paragraph: repaired once, then dropped;
//! * a curriculum code that was not offered is dropped, and curriculum text is only ever taken
//!   from the database.
//!
//! Every model call is its own `ai_interactions` row (attempt 2 has `retry_of`). What the
//! teacher sees is the checked and merged result; what was changed on the way is recorded in the
//! final row's `check_results.repairs`.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::PgPool;

use super::checks::{
    evaluate_analysis, evaluate_objectives, merge_analysis, AnalysisEval, ObjectivesEval,
};
use super::schemas::{self, AnalysisSuggestion, ObjectivesSuggestion, RawAnalysis, RawObjectives};
use super::stages;
use super::store::{Material, Version};
use super::{numbered_passage, ObjectiveType, Paragraph};
use crate::ai::pipeline::{
    self, AiContextItem, AiResult, ContextList, InteractionMeta, Outcome, Source,
};
use crate::ai::LlmRequest;
use crate::application::ai::{
    attempt, finish_failure, finish_output, report_with, set_final, Attempt, Failure,
};
use crate::audit::scaffold_id;
use crate::checks::{self, CheckResult};
use crate::curriculum::{self, CurriculumItem};
use crate::error::{AppError, AppResult};
use crate::prompts::{self, latest_required as prompt};
use crate::teaching_context::{
    owned_lesson, resolved_profile, track_label, LessonRef, ResolvedProfile,
};
use crate::text::{clip_chars, or_blank};
use crate::AppState;

pub const KEY_ANALYSIS: &str = "sop_passage_analysis";
pub const KEY_OBJECTIVES: &str = "sop_objectives";
pub const MODULE_ANALYSIS: &str = "sop_analysis";
pub const MODULE_OBJECTIVES: &str = "sop_objectives";
pub const SCAFFOLD_ANALYSIS: &str = "SC-SOP-ANALYSIS";
pub const SCAFFOLD_OBJECTIVES: &str = "SC-SOP-OBJECTIVES";

/// The analysis budget of a material of up to 30 paragraphs; a longer one gets more
/// ([`analysis_max_tokens`]).
pub const ANALYSIS_MAX_TOKENS: u32 = 6000;
/// The most one analysis call may write.
pub const ANALYSIS_MAX_TOKENS_CAP: u32 = 12000;

/// Output budget of the analysis of `paragraphs` paragraphs (one entry each).
pub fn analysis_max_tokens(paragraphs: usize) -> u32 {
    (1000 + 200 * paragraphs.min(200) as u32).clamp(ANALYSIS_MAX_TOKENS, ANALYSIS_MAX_TOKENS_CAP)
}
pub const OBJECTIVES_MAX_TOKENS: u32 = 4000;
/// Most suggestions of one kind a run for that kind keeps.
pub const KIND_MAX: usize = 3;
/// Most curriculum items put in front of the model.
pub const CURRICULUM_LIMIT: usize = 150;
const BASE_TEMPERATURE: f32 = 0.2;
/// Added for the repair call, so it is never the request the gateway has cached.
const REPAIR_TEMPERATURE_STEP: f32 = 0.4;

pub const MSG_STALE: &str = "這次請求中斷了（可能是系統剛更新）。請再按一次。";

/// What a job that waits in the AI queue is doing.
const PHASE_QUEUED: &str = "排隊中";
const PHASE_ANALYSIS: &str = "正在讀教材，逐段分析寫作特色";
const PHASE_ANALYSIS_REPAIR: &str = "有幾段沒有分析好，正在補上";
const PHASE_OBJECTIVES: &str = "正在依教材與班級特性想目標，並對照課綱";
const PHASE_OBJECTIVES_REPAIR: &str = "正在重新確認目標與課綱連結";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobKind {
    Analysis,
    Objectives,
    Goals,
    Tasks,
    Warmup,
    ExitTicket,
}

impl JobKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Analysis => "analysis",
            Self::Objectives => "objectives",
            Self::Goals => "goals",
            Self::Tasks => "tasks",
            Self::Warmup => "warmup",
            Self::ExitTicket => "exit_ticket",
        }
    }

    fn first_phase(self, scope: &Scope) -> &'static str {
        match (self, scope) {
            (Self::Objectives, Scope::Swap(_)) => stages::PHASE_SWAP,
            (Self::Goals, Scope::Paragraph { .. }) => stages::PHASE_REDO,
            (Self::Analysis, _) => PHASE_ANALYSIS,
            (Self::Objectives, _) => PHASE_OBJECTIVES,
            (Self::Goals, _) => stages::PHASE_GOALS,
            (Self::Tasks, _) => stages::PHASE_TASKS,
            (Self::Warmup, _) => stages::PHASE_WARMUP,
            (Self::ExitTicket, _) => stages::PHASE_EXIT,
        }
    }
}

/// 換一個: the one suggestion of an objectives job to replace.
#[derive(Debug, Clone)]
pub struct Swap {
    pub parent_job_id: i64,
    /// The suggestion's `item_index` in the parent job's result.
    pub index: i32,
    pub kind: ObjectiveType,
    /// The suggestion being set aside (its text).
    pub declined: String,
    /// Objectives the new one must not repeat.
    pub avoid: Vec<String>,
}

/// What part of a stage a job covers: all of it, or one suggestion (換一個, 這一段再建議一次).
#[derive(Debug, Clone)]
pub enum Scope {
    Whole,
    /// The whole objectives stage for one kind: 內容目標 first, then (once the teacher decided
    /// them) 語言目標 derived from them.
    Kind(ObjectiveType),
    Swap(Swap),
    Paragraph {
        parent_job_id: Option<i64>,
        paragraph_id: String,
    },
}

impl Scope {
    /// Part of the uniqueness of a running job: the whole stage is `""`.
    pub fn slot(&self) -> String {
        match self {
            Self::Whole | Self::Kind(_) => String::new(),
            Self::Swap(s) => format!("{}:{}", s.parent_job_id, s.index),
            Self::Paragraph { paragraph_id, .. } => format!("p:{paragraph_id}"),
        }
    }

    pub fn paragraph_id(&self) -> Option<&str> {
        match self {
            Self::Paragraph { paragraph_id, .. } => Some(paragraph_id),
            _ => None,
        }
    }

    fn parent(&self) -> Option<i64> {
        match self {
            Self::Whole | Self::Kind(_) => None,
            Self::Swap(s) => Some(s.parent_job_id),
            Self::Paragraph { parent_job_id, .. } => *parent_job_id,
        }
    }

    fn params(&self) -> Option<Value> {
        match self {
            Self::Whole => None,
            Self::Kind(k) => Some(json!({ "only_type": k.label() })),
            Self::Swap(s) => Some(json!({
                "index": s.index, "type": s.kind.label(),
                // Counts only: the declined text is in the parent job's result.
                "avoid_count": s.avoid.len(),
            })),
            Self::Paragraph { paragraph_id, .. } => Some(json!({ "paragraph_id": paragraph_id })),
        }
    }
}

// ---------------------------------------------------------------- jobs

#[derive(Debug, Serialize)]
pub struct Job {
    pub id: i64,
    pub lesson_id: i64,
    pub kind: String,
    pub status: String,
    /// What a running job is doing, for the teacher; empty once it is done.
    pub phase: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub sop_version_no: Option<i32>,
    pub result: Option<Value>,
    /// `item_index` values the teacher already decided on.
    pub decided: Vec<i32>,
    /// `""` for a whole stage; otherwise the one suggestion this job redoes.
    pub slot: String,
    /// The job whose suggestion this one redoes.
    pub parent_job_id: Option<i64>,
    /// While the job waits in the AI queue: where it stands (docs/API.md「AI 工作佇列」).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue: Option<crate::progress::QueueInfo>,
}

#[derive(sqlx::FromRow)]
struct JobRow {
    id: i64,
    lesson_id: i64,
    kind: String,
    status: String,
    phase: String,
    created_at: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>,
    sop_version_no: Option<i32>,
    interaction_id: Option<i64>,
    result: Option<Value>,
    slot: String,
    parent_job_id: Option<i64>,
}

const JOB_SELECT: &str = "SELECT j.id, j.lesson_id, j.kind, j.status, j.phase, j.created_at,
        j.finished_at, v.version_no AS sop_version_no, j.interaction_id, j.result,
        j.slot, j.parent_job_id
   FROM lesson_sop_jobs j LEFT JOIN lesson_sop_versions v ON v.id = j.sop_version_id";

async fn into_job(state: &AppState, row: JobRow) -> sqlx::Result<Job> {
    let db = &state.db;
    let queue = if row.status == "running" {
        crate::queue::info_for_ref(state, "sop_job", row.id).await?
    } else {
        None
    };
    let decided = match row.interaction_id {
        Some(id) => {
            sqlx::query_scalar(
                "SELECT DISTINCT item_index FROM teacher_decisions
                  WHERE ai_interaction_id = $1 ORDER BY item_index",
            )
            .bind(id)
            .fetch_all(db)
            .await?
        }
        None => Vec::new(),
    };
    Ok(Job {
        id: row.id,
        lesson_id: row.lesson_id,
        kind: row.kind,
        // What a job was doing is over once it is done (rows finished before the phase was
        // cleared still carry their last one).
        phase: if row.status == "done" {
            String::new()
        } else {
            row.phase
        },
        status: row.status,
        started_at: row.created_at,
        finished_at: row.finished_at,
        sop_version_no: row.sop_version_no,
        result: row.result,
        decided,
        slot: row.slot,
        parent_job_id: row.parent_job_id,
        queue,
    })
}

pub(super) fn result_json<T: Serialize>(result: AiResult<T>) -> Value {
    serde_json::to_value(result).unwrap_or_else(|_| json!({ "status": "error" }))
}

fn error_result(message: &str) -> Value {
    json!({
        "interaction_id": null, "status": "error", "message": message, "output": null,
        "sources": [], "context": []
    })
}

/// A job still `running` long after its calls must have timed out died with the server.
fn stale_after(state: &AppState) -> f64 {
    (state.ai.longest_timeout_secs() * 2 + 60) as f64
}

async fn expire_stale(state: &AppState, lesson_id: i64) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE lesson_sop_jobs s SET status = 'done', phase = '', finished_at = now(), result = $2
          WHERE s.lesson_id = $1 AND s.status = 'running'
            AND s.created_at < now() - make_interval(secs => $3)
            -- A job that waits in the AI queue (or runs from it) is not stale: the queue's
            -- leases tell whether its server is alive. (Only while the queue is on.)
            AND NOT ($4 AND EXISTS (SELECT 1 FROM ai_jobs j
                             WHERE j.ref_kind = 'sop_job' AND j.ref_id = s.id
                               AND j.status IN ('queued', 'running')))",
    )
    .bind(lesson_id)
    .bind(error_result(MSG_STALE))
    .bind(stale_after(state))
    .bind(state.config.ai_queue_enabled)
    .execute(&state.db)
    .await?;
    Ok(())
}

/// A job of this teacher, or None.
pub async fn job(state: &AppState, id: i64, teacher_id: i64) -> sqlx::Result<Option<Job>> {
    let lesson_id: Option<i64> = sqlx::query_scalar(
        "SELECT lesson_id FROM lesson_sop_jobs WHERE id = $1 AND teacher_id = $2",
    )
    .bind(id)
    .bind(teacher_id)
    .fetch_optional(&state.db)
    .await?;
    let Some(lesson_id) = lesson_id else {
        return Ok(None);
    };
    expire_stale(state, lesson_id).await?;
    let row: JobRow = sqlx::query_as(&format!("{JOB_SELECT} WHERE j.id = $1"))
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    Ok(Some(into_job(state, row).await?))
}

/// The most recent job of a kind for a lesson.
pub async fn latest_job(
    state: &AppState,
    lesson_id: i64,
    kind: JobKind,
) -> sqlx::Result<Option<Job>> {
    expire_stale(state, lesson_id).await?;
    let row: Option<JobRow> = sqlx::query_as(&format!(
        "{JOB_SELECT} WHERE j.lesson_id = $1 AND j.kind = $2 AND j.slot = '' ORDER BY j.id DESC LIMIT 1"
    ))
    .bind(lesson_id)
    .bind(kind.as_str())
    .fetch_optional(&state.db)
    .await?;
    match row {
        Some(row) => Ok(Some(into_job(state, row).await?)),
        None => Ok(None),
    }
}

/// The jobs that redo one suggestion (換一個, 這一段再建議一次) since the last whole run of
/// their stage, oldest first: the page puts each where it belongs.
pub async fn redo_jobs(state: &AppState, lesson_id: i64) -> sqlx::Result<Vec<Job>> {
    expire_stale(state, lesson_id).await?;
    let rows: Vec<JobRow> = sqlx::query_as(&format!(
        "{JOB_SELECT} WHERE j.lesson_id = $1 AND j.slot <> ''
            AND j.id > COALESCE((SELECT MAX(w.id) FROM lesson_sop_jobs w
                                  WHERE w.lesson_id = j.lesson_id AND w.kind = j.kind
                                    AND w.slot = ''), 0)
          ORDER BY j.id DESC LIMIT 100"
    ))
    .bind(lesson_id)
    .fetch_all(&state.db)
    .await?;
    let mut jobs = Vec::with_capacity(rows.len());
    for row in rows.into_iter().rev() {
        jobs.push(into_job(state, row).await?);
    }
    Ok(jobs)
}

/// What a job needs, loaded while the request is still there to answer with a 4xx.
pub struct JobInput {
    pub kind: JobKind,
    pub scope: Scope,
    pub teacher_id: i64,
    pub track: String,
    pub lesson: LessonRef,
    pub profile: ResolvedProfile,
    pub version: Version,
    pub material: Material,
    /// The plan's 「今天要教什麼？」 line.
    pub today_objective: String,
}

/// Creates the job (or joins the one already running for this lesson and kind) and starts the
/// background work. Returns the job id.
pub async fn start(state: &AppState, input: JobInput) -> AppResult<i64> {
    let lesson_id = input.lesson.id;
    let kind = input.kind;
    expire_stale(state, lesson_id).await?;
    if !state.ai.enabled() {
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO lesson_sop_jobs
               (lesson_id, teacher_id, kind, status, sop_version_id, material_id, result, finished_at,
                slot, parent_job_id, params)
             VALUES ($1, $2, $3, 'done', $4, $5, $6, now(), $7, $8, $9) RETURNING id",
        )
        .bind(lesson_id)
        .bind(input.teacher_id)
        .bind(kind.as_str())
        .bind(input.version.id)
        .bind(input.material.id)
        .bind(result_json(AiResult::<Value>::disabled()))
        .bind(input.scope.slot())
        .bind(input.scope.parent())
        .bind(input.scope.params())
        .fetch_one(&state.db)
        .await?;
        return Ok(id);
    }
    // How often this was asked before: a retry must not be the request the gateway cached.
    let earlier: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM lesson_sop_jobs WHERE lesson_id = $1 AND kind = $2",
    )
    .bind(lesson_id)
    .bind(kind.as_str())
    .fetch_one(&state.db)
    .await?;
    let queued = state.config.ai_queue_enabled;
    let mut tx = state.db.begin().await?;
    let created: Option<i64> = sqlx::query_scalar(
        "INSERT INTO lesson_sop_jobs
           (lesson_id, teacher_id, kind, status, phase, sop_version_id, material_id,
            slot, parent_job_id, params)
         VALUES ($1, $2, $3, 'running', $4, $5, $6, $7, $8, $9)
         ON CONFLICT (lesson_id, kind, slot) WHERE status = 'running' DO NOTHING
         RETURNING id",
    )
    .bind(lesson_id)
    .bind(input.teacher_id)
    .bind(kind.as_str())
    .bind(if queued {
        PHASE_QUEUED
    } else {
        kind.first_phase(&input.scope)
    })
    .bind(input.version.id)
    .bind(input.material.id)
    .bind(input.scope.slot())
    .bind(input.scope.parent())
    .bind(input.scope.params())
    .fetch_optional(&mut *tx)
    .await?;
    let Some(job_id) = created else {
        drop(tx);
        let running: i64 = sqlx::query_scalar(
            "SELECT id FROM lesson_sop_jobs
              WHERE lesson_id = $1 AND kind = $2 AND slot = $3 AND status = 'running'",
        )
        .bind(lesson_id)
        .bind(kind.as_str())
        .bind(input.scope.slot())
        .fetch_one(&state.db)
        .await?;
        return Ok(running);
    };
    if queued {
        // The job row and its queue entry are one commit.
        crate::queue::enqueue_tx(
            &mut tx,
            crate::queue::NewJob {
                kind: crate::queue::handlers::KIND_SOP_STEP,
                teacher_id: input.teacher_id,
                priority: 1,
                params: json!({
                    "sop_job_id": job_id, "class_id": input.profile.class.as_ref().map(|c| c.id),
                    "track": input.track,
                }),
                ref_kind: "sop_job",
                ref_id: job_id,
            },
            state.config.ai_max_queued_per_teacher,
        )
        .await?;
    }
    tx.commit().await?;
    if queued {
        state.queue.wake();
        return Ok(job_id);
    }

    let state = state.clone();
    let temperature = BASE_TEMPERATURE + 0.1 * earlier.min(4) as f32;
    let pin = state.ai.active();
    tokio::spawn(async move {
        // The work runs in its own task so that a panic there still ends the job.
        let work = pin.scope({
            let state = state.clone();
            async move { execute(&state, job_id, &input, temperature).await }
        });
        let (result, interaction_id) = match crate::jobs::run("sop", job_id, work).await {
            Ok(done) => done,
            Err(_) => (error_result(pipeline::MSG_ERROR), None),
        };
        store_result(&state.db, job_id, result, interaction_id).await;
    });
    Ok(job_id)
}

/// The work of a job: the stage `input.kind` names.
async fn execute(
    state: &AppState,
    job_id: i64,
    input: &JobInput,
    temperature: f32,
) -> AppResult<Done> {
    match input.kind {
        JobKind::Analysis => run_analysis(state, job_id, input, temperature).await,
        JobKind::Objectives => {
            let swap = match &input.scope {
                Scope::Swap(s) => Some(s),
                _ => None,
            };
            let kind = match &input.scope {
                Scope::Kind(k) => Some(*k),
                _ => None,
            };
            run_objectives(state, job_id, input, temperature, swap, kind).await
        }
        JobKind::Goals => stages::run_goals(state, job_id, input, temperature).await,
        JobKind::Tasks => stages::run_tasks(state, job_id, input, temperature).await,
        JobKind::Warmup => stages::run_warmup(state, job_id, input, temperature).await,
        JobKind::ExitTicket => stages::run_exit(state, job_id, input, temperature).await,
    }
}

async fn store_result(db: &PgPool, job_id: i64, result: Value, interaction_id: Option<i64>) {
    let saved = sqlx::query(
        "UPDATE lesson_sop_jobs
            SET status = 'done', phase = '', finished_at = now(), result = $2,
                interaction_id = $3
          WHERE id = $1 AND status = 'running'",
    )
    .bind(job_id)
    .bind(result)
    .bind(interaction_id)
    .execute(db)
    .await;
    if let Err(e) = saved {
        tracing::error!(job_id, error = %e, "could not store the SOP job result");
    }
}

/// Closes a job that will not run (cancelled in the queue, lost with its server) as one that
/// ended with `message`: the page shows it with 「再試一次」.
pub async fn close_failed(db: &PgPool, job_id: i64, message: &str) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE lesson_sop_jobs SET status = 'done', phase = '', finished_at = now(), result = $2
          WHERE id = $1 AND status = 'running'",
    )
    .bind(job_id)
    .bind(error_result(message))
    .execute(db)
    .await?;
    Ok(())
}

/// Runs the job the AI queue holds (the `sop_step` job) to its end: what [`start`] spawns,
/// rebuilt from the job row, its version and material (as they were when it was asked for)
/// and the lesson as it is now. `class_id` and `track` are the request's. A job that is no
/// longer running (cancelled, failed meanwhile) is left alone; one that fails stores an error
/// result, as the spawned task does.
pub async fn run_queued(
    state: &AppState,
    job_id: i64,
    teacher_id: i64,
    class_id: Option<i64>,
    track: &str,
) -> AppResult<()> {
    let db = &state.db;
    #[derive(sqlx::FromRow)]
    struct Row {
        lesson_id: i64,
        kind: String,
        slot: String,
        parent_job_id: Option<i64>,
        params: Option<Value>,
        sop_version_id: Option<i64>,
        material_id: Option<i64>,
    }
    let row: Option<Row> = sqlx::query_as(
        "SELECT lesson_id, kind, slot, parent_job_id, params, sop_version_id, material_id
           FROM lesson_sop_jobs WHERE id = $1 AND status = 'running'",
    )
    .bind(job_id)
    .fetch_optional(db)
    .await?;
    let Some(row) = row else { return Ok(()) };
    let kind = [
        JobKind::Analysis,
        JobKind::Objectives,
        JobKind::Goals,
        JobKind::Tasks,
        JobKind::Warmup,
        JobKind::ExitTicket,
    ]
    .into_iter()
    .find(|k| k.as_str() == row.kind)
    .ok_or_else(|| AppError::Internal(anyhow::anyhow!("SOP job {job_id} has kind {}", row.kind)))?;
    let lesson = owned_lesson(db, row.lesson_id, teacher_id).await?;
    let profile = resolved_profile(
        db,
        teacher_id,
        class_id,
        &lesson.class_description,
        &lesson.unit_class_profile,
    )
    .await?;
    let version_id = row
        .sop_version_id
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("SOP job {job_id} has no version")))?;
    let version = super::store::version_by_id(db, version_id)
        .await?
        .ok_or_else(|| AppError::not_found("找不到這個版本。"))?;
    let material = super::store::material(db, row.material_id.or(version.material_id))
        .await?
        .ok_or_else(|| AppError::not_found("教材已經刪除。"))?;
    let params = row.params.unwrap_or(Value::Null);
    let scope = match (kind, row.parent_job_id, row.slot.as_str()) {
        (JobKind::Objectives, Some(parent_id), _) => {
            let index = params["index"]
                .as_i64()
                .and_then(|i| i32::try_from(i).ok())
                .ok_or_else(|| AppError::bad_request("找不到要換掉的那一項建議。"))?;
            Scope::Swap(swap_of(state, teacher_id, lesson.id, parent_id, index, &version).await?)
        }
        (JobKind::Goals, parent, slot) if slot.starts_with("p:") => Scope::Paragraph {
            parent_job_id: parent,
            paragraph_id: params["paragraph_id"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
        },
        (JobKind::Objectives, None, _) => Scope::Kind(
            if params["only_type"].as_str() == Some(ObjectiveType::Language.label()) {
                ObjectiveType::Language
            } else {
                ObjectiveType::Content
            },
        ),
        _ => Scope::Whole,
    };
    let today_objective: String = sqlx::query_scalar(
        "SELECT objective FROM lesson_versions WHERE lesson_id = $1
          ORDER BY version_no DESC LIMIT 1",
    )
    .bind(lesson.id)
    .fetch_optional(db)
    .await?
    .unwrap_or_default();
    // How often this was asked before: a retry must not be the request the gateway cached.
    let earlier: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM lesson_sop_jobs WHERE lesson_id = $1 AND kind = $2 AND id < $3",
    )
    .bind(lesson.id)
    .bind(kind.as_str())
    .bind(job_id)
    .fetch_one(db)
    .await?;
    let temperature = BASE_TEMPERATURE + 0.1 * earlier.min(4) as f32;
    let input = JobInput {
        kind,
        scope,
        teacher_id,
        track: track.to_string(),
        lesson,
        profile,
        version,
        material,
        today_objective,
    };
    set_phase(db, job_id, kind.first_phase(&input.scope)).await?;
    let (result, interaction_id) = match state
        .ai
        .pinned(execute(state, job_id, &input, temperature))
        .await
    {
        Ok(done) => done,
        Err(e) => {
            tracing::error!(job_id, error = %e, "SOP job failed");
            (error_result(pipeline::MSG_ERROR), None)
        }
    };
    store_result(db, job_id, result, interaction_id).await;
    Ok(())
}

/// The suggestion of an objectives job that 換一個 replaces (read from the stored result, never
/// from the request), and the objectives the new one must not repeat.
pub async fn swap_of(
    state: &AppState,
    teacher_id: i64,
    lesson_id: i64,
    parent_id: i64,
    index: i32,
    version: &Version,
) -> AppResult<Swap> {
    let parent = job(state, parent_id, teacher_id)
        .await?
        .filter(|j| j.lesson_id == lesson_id && j.kind == "objectives" && j.status == "done")
        .ok_or_else(|| AppError::not_found("找不到這一批建議。"))?;
    let list = parent
        .result
        .as_ref()
        .and_then(|r| r["output"]["objectives"].as_array())
        .ok_or_else(|| AppError::bad_request("這一批沒有可以換的建議。"))?;
    let chosen = usize::try_from(index)
        .ok()
        .and_then(|i| list.get(i))
        .ok_or_else(|| AppError::bad_request("找不到要換掉的那一項建議。"))?;
    let kind = match chosen["type"].as_str() {
        Some("language") => ObjectiveType::Language,
        Some("content") => ObjectiveType::Content,
        _ => return Err(AppError::bad_request("這一項建議沒有目標類型。")),
    };
    let text_of = |o: &Value| o["text"].as_str().unwrap_or_default().trim().to_string();
    let mut avoid: Vec<String> = version
        .content
        .objectives
        .iter()
        .filter(|o| !o.text.trim().is_empty())
        .map(|o| o.text.trim().to_string())
        .collect();
    avoid.extend(
        list.iter()
            .enumerate()
            .filter(|(i, _)| *i != index as usize)
            .map(|(_, o)| text_of(o))
            .filter(|t| !t.is_empty()),
    );
    Ok(Swap {
        parent_job_id: parent_id,
        index,
        kind,
        declined: text_of(chosen),
        avoid,
    })
}

pub(super) async fn set_phase(db: &PgPool, job_id: i64, phase: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE lesson_sop_jobs SET phase = $2 WHERE id = $1 AND status = 'running'")
        .bind(job_id)
        .bind(phase)
        .execute(db)
        .await?;
    Ok(())
}

pub(super) type Done = (Value, Option<i64>);

fn not_shown<T: Serialize>(outcome: Outcome<T>, context: Vec<AiContextItem>) -> Done {
    let id = outcome.interaction_id();
    (
        result_json(outcome.into_result(|_| Vec::new()).with_context(context)),
        Some(id),
    )
}

fn material_label(material: &Material) -> String {
    // Never the text itself: this label is stored with the job result.
    let name = if material.title.trim().is_empty() {
        "未命名教材".to_string()
    } else {
        material.title.trim().to_string()
    };
    format!(
        "{name}（{} 段，{} 字）",
        material.paragraphs.len(),
        material.word_count
    )
}

fn nos(list: &[i32]) -> String {
    list.iter()
        .map(|n| format!("[P{n}]"))
        .collect::<Vec<_>>()
        .join("、")
}

// ---------------------------------------------------------------- (a) passage analysis

async fn run_analysis(
    state: &AppState,
    job_id: i64,
    input: &JobInput,
    temperature: f32,
) -> AppResult<Done> {
    let db = &state.db;
    let paragraphs = &input.material.paragraphs;
    let all: Vec<i32> = paragraphs.iter().map(|p| p.no).collect();
    let prompt = prompt(db, KEY_ANALYSIS).await?;
    let passage = numbered_passage(paragraphs);
    let class_profile = input.profile.text.clone();
    let mut vars: HashMap<&str, String> = HashMap::from([
        ("school_track", track_label(&input.track).to_string()),
        ("class_profile", or_blank(&class_profile)),
        ("passage_numbered", passage.clone()),
    ]);
    let required = [("class_profile", "班級特性")];
    let system = prompts::render(&prompt.system_text, &vars);
    let user = prompts::render_user_with(&prompt, &vars, &required);
    // The material must stay deletable: the logged copy keeps only its length and a hash.
    vars.insert("passage_numbered", prompts::redacted("課例教材", &passage));
    let logged_user = prompts::render_user_with(&prompt, &vars, &required);
    let snapshot = json!({
        "inputs": vars,
        "class_context": input.profile.snapshot(),
        "sop": {
            "version_no": input.version.version_no,
            "material_id": input.material.id,
            "material_version_no": input.material.version_no,
            "paragraph_ids": paragraphs.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
        },
        "prompt": { "key": KEY_ANALYSIS, "version_no": prompt.version_no,
                    "system": system, "user": logged_user },
        "schema": schemas::ANALYSIS_SCHEMA_NAME,
        "request": { "temperature": temperature, "max_tokens": analysis_max_tokens(paragraphs.len()) },
    });
    let meta = InteractionMeta {
        teacher_id: input.teacher_id,
        module: MODULE_ANALYSIS,
        unit_id: Some(input.lesson.unit_id),
        lesson_id: Some(input.lesson.id),
        scaffold_id: scaffold_id(db, SCAFFOLD_ANALYSIS).await?,
        prompt_version_id: Some(prompt.id),
    };
    let mut context = ContextList::default();
    context
        .add("教材", &material_label(&input.material))
        .add(&input.profile.context_label(), &class_profile);
    let context = context.into_vec();
    let fake_paragraphs: Vec<Value> = paragraphs
        .iter()
        .map(|p| json!({ "no": p.no, "text": p.text }))
        .collect();
    let mut req = LlmRequest {
        system,
        user: user.clone(),
        schema_name: schemas::ANALYSIS_SCHEMA_NAME,
        schema: schemas::analysis_schema(),
        fake_context: json!({ "task": KEY_ANALYSIS, "paragraphs": fake_paragraphs }),
        attempt: 1,
        max_tokens: Some(analysis_max_tokens(paragraphs.len())),
        temperature: Some(temperature),
        images: Vec::new(),
    };

    // ---- first call: the whole passage
    let a1 = attempt(state, &meta, &snapshot, &req, None).await?;
    let mut first: Option<AnalysisEval> = None;
    let mut first_results: Vec<CheckResult> = Vec::new();
    match &a1.response {
        Ok(resp) => match serde_json::from_value::<RawAnalysis>(resp.json.clone()) {
            Ok(raw) => {
                let eval = evaluate_analysis(&raw, paragraphs, &all);
                let status = if eval.needs_repair() {
                    "blocked"
                } else {
                    "shown"
                };
                let mut results = vec![CheckResult::pass("schema")];
                results.extend(eval.results.clone());
                finish_output(db, &a1, resp, status, checks::report(&results), Vec::new()).await?;
                first_results = results;
                first = Some(eval);
            }
            Err(e) => {
                let results = [CheckResult::fail("schema", e.to_string())];
                finish_output(
                    db,
                    &a1,
                    resp,
                    "blocked",
                    checks::report(&results),
                    Vec::new(),
                )
                .await?;
            }
        },
        Err(e) => match finish_failure(db, &a1, e).await? {
            Failure::Timeout => {
                return Ok(not_shown::<AnalysisSuggestion>(
                    Outcome::Timeout {
                        interaction_id: a1.id,
                    },
                    context,
                ))
            }
            Failure::Error(kind) => {
                return Ok(not_shown::<AnalysisSuggestion>(
                    Outcome::Error {
                        interaction_id: a1.id,
                        kind,
                    },
                    context,
                ))
            }
            Failure::Retryable => {}
        },
    }

    // ---- repair call: only what is missing or wrong, with a changed request
    let mut second: Option<(Attempt, AnalysisEval, Vec<CheckResult>)> = None;
    let mut last_failure: Option<(i64, Failure)> = None;
    let targets: Vec<i32> = match &first {
        None => all.clone(),
        Some(e) if e.needs_repair() => {
            let mut t = e.missing.clone();
            t.extend(e.redo());
            t.sort_unstable();
            // Only the overall summary was unusable: one paragraph is enough to get it again.
            if t.is_empty() {
                t = all.iter().copied().take(1).collect();
            }
            t
        }
        Some(_) => Vec::new(),
    };
    if !targets.is_empty() {
        set_phase(db, job_id, PHASE_ANALYSIS_REPAIR).await?;
        let mut note = format!(
            "\n\n【重要：這是第二次請求】上一次的輸出沒有通過平台的檢查。這次 paragraphs 只需要輸出以下段落，每段一筆，不要輸出其他段落：{}。",
            nos(&targets)
        );
        if let Some(e) = &first {
            if !e.missing.is_empty() {
                note.push_str(&format!(
                    "\n- 上一次沒有輸出這些段落：{}。",
                    nos(&e.missing)
                ));
            }
            for (no, p) in &e.paragraphs {
                for bad in &p.bad_evidence {
                    note.push_str(&format!(
                        "\n- [P{no}] 的 evidence「{}」不是該段原文的連續片段。請改成從 [P{no}] 逐字複製的一段連續文字，不可用「...」省略或拼接。",
                        clip_chars(bad, 200)
                    ));
                }
                if !p.zh_ok {
                    note.push_str(&format!(
                        "\n- [P{no}] 的 function_zh、summary_zh 必須用繁體中文。"
                    ));
                }
            }
            if e.overall.is_none() {
                note.push_str("\n- main_idea_zh、structure_zh 必須用繁體中文。");
            }
        }
        note.push_str("\ntext_type、main_idea_zh、structure_zh、language_focus 仍然針對全文填寫。");
        req.user = format!("{user}{note}");
        req.attempt = 2;
        req.temperature = Some(temperature + REPAIR_TEMPERATURE_STEP);
        req.fake_context["only_paragraphs"] = json!(targets);
        let mut snap = snapshot.clone();
        snap["retry_of"] = json!(a1.id);
        snap["repair"] = json!({ "paragraphs": targets, "note": note });
        snap["request"]["temperature"] = json!(temperature + REPAIR_TEMPERATURE_STEP);
        let a2 = attempt(state, &meta, &snap, &req, Some(a1.id)).await?;
        match &a2.response {
            Ok(resp) => match serde_json::from_value::<RawAnalysis>(resp.json.clone()) {
                Ok(raw) => {
                    let eval = evaluate_analysis(&raw, paragraphs, &targets);
                    let mut results = vec![CheckResult::pass("schema")];
                    results.extend(eval.results.clone());
                    second = Some((a2, eval, results));
                }
                Err(e) => {
                    let results = [CheckResult::fail("schema", e.to_string())];
                    finish_output(
                        db,
                        &a2,
                        resp,
                        "blocked",
                        checks::report(&results),
                        Vec::new(),
                    )
                    .await?;
                }
            },
            Err(e) => {
                let failure = finish_failure(db, &a2, e).await?;
                last_failure = Some((a2.id, failure));
            }
        }
    }

    // ---- merge, drop what is still wrong, decide
    let merged = merge_analysis(first.as_ref(), second.as_ref().map(|s| &s.1), paragraphs);
    let usable = !merged.paragraphs.is_empty();
    let repairs = json!({
        "attempts": if targets.is_empty() { 1 } else { 2 },
        "repaired_paragraphs": targets,
        "missing_paragraph_ids": merged.missing_paragraph_ids,
        "dropped_evidence": merged.dropped_evidence,
    });
    let final_id = match &second {
        Some((a2, _, results)) => {
            if let Ok(resp) = &a2.response {
                let status = if usable { "shown" } else { "blocked" };
                finish_output(
                    db,
                    a2,
                    resp,
                    status,
                    report_with(results, repairs),
                    Vec::new(),
                )
                .await?;
            }
            a2.id
        }
        None => {
            if usable && !targets.is_empty() {
                // The repair call failed: what the first call got right is still shown.
                set_final(db, a1.id, "shown", report_with(&first_results, repairs)).await?;
            }
            a1.id
        }
    };
    if !usable {
        let outcome = match last_failure {
            Some((id, Failure::Timeout)) => Outcome::Timeout { interaction_id: id },
            Some((id, Failure::Error(kind))) => Outcome::Error {
                interaction_id: id,
                kind,
            },
            Some((id, Failure::Retryable)) => Outcome::Blocked { interaction_id: id },
            None => Outcome::Blocked {
                interaction_id: final_id,
            },
        };
        return Ok(not_shown::<AnalysisSuggestion>(outcome, context));
    }
    Ok((
        result_json(AiResult::shown(final_id, merged, Vec::new()).with_context(context)),
        Some(final_id),
    ))
}

// ---------------------------------------------------------------- (b) objectives

pub(super) fn analysis_summary(version: &Version, paragraphs: &[Paragraph]) -> String {
    let a = &version.content.analysis;
    let mut lines = Vec::new();
    if !a.text_type.trim().is_empty() {
        lines.push(format!("文體：{}", a.text_type.trim()));
    }
    if !a.main_idea.trim().is_empty() {
        lines.push(format!("主旨：{}", a.main_idea.trim()));
    }
    if !a.structure.trim().is_empty() {
        lines.push(format!("結構：{}", a.structure.trim()));
    }
    for p in paragraphs {
        let Some(pa) = a.paragraphs.iter().find(|x| x.paragraph_id == p.id) else {
            continue;
        };
        let features = pa
            .features
            .iter()
            .filter(|f| !f.feature.trim().is_empty())
            .map(|f| f.feature.trim())
            .collect::<Vec<_>>()
            .join("、");
        let mut line = format!("[P{}]", p.no);
        for (label, text) in [
            ("功能", pa.function.trim()),
            ("摘要", pa.summary.trim()),
            ("寫作特色", features.as_str()),
        ] {
            if !text.is_empty() {
                line.push_str(&format!(" {label}：{text}；"));
            }
        }
        if line.len() > format!("[P{}]", p.no).len() {
            lines.push(line);
        }
    }
    for f in &a.language_focus {
        if !f.item.trim().is_empty() {
            lines.push(format!("語言重點（{}）：{}", f.kind.trim(), f.item.trim()));
        }
    }
    lines.join("\n")
}

fn teacher_objectives(version: &Version, today: &str) -> String {
    let mut lines: Vec<String> = version
        .content
        .objectives
        .iter()
        .filter(|o| !o.text.trim().is_empty())
        .map(|o| format!("・{}：{}", o.kind.label(), o.text.trim()))
        .collect();
    if !today.trim().is_empty() {
        lines.push(format!("・教師寫的「今天要教什麼」：{}", today.trim()));
    }
    lines.join("\n")
}

fn format_curriculum(items: &[CurriculumItem]) -> String {
    items
        .iter()
        .map(|i| {
            format!(
                "{}｜{}｜{}",
                i.item_code,
                i.title,
                clip_chars(&i.content, 600)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn objective_sources(out: &ObjectivesSuggestion) -> Vec<Source> {
    let mut seen = std::collections::HashSet::new();
    out.objectives
        .iter()
        .flat_map(|o| o.links.iter())
        .filter(|l| seen.insert(l.item_code.clone()))
        .map(|l| Source {
            item_code: l.item_code.clone(),
            title: l.title.clone(),
            category: l.category.clone(),
            source_doc: l.source_doc.clone(),
            source_page: l.source_page.clone(),
        })
        .collect()
}

async fn run_objectives(
    state: &AppState,
    job_id: i64,
    input: &JobInput,
    temperature: f32,
    swap: Option<&Swap>,
    kind: Option<ObjectiveType>,
) -> AppResult<Done> {
    let db = &state.db;
    let only: Option<ObjectiveType> = swap.map(|s| s.kind).or(kind);
    let decided_content: Vec<String> = input
        .version
        .content
        .objectives
        .iter()
        .filter(|o| o.kind == ObjectiveType::Content)
        .map(|o| {
            let both = [o.text.trim(), o.student_facing_en.trim()]
                .into_iter()
                .filter(|t| !t.is_empty())
                .collect::<Vec<_>>();
            both.join("｜")
        })
        .filter(|t| !t.is_empty())
        .collect();
    // 換一個 keeps one; a whole run for one kind keeps the first few.
    let keep_max = if swap.is_some() { 1 } else { KIND_MAX };
    let swap_note = match (swap, kind) {
        (Some(s), _) => stages::swap_note(s),
        (None, Some(k)) => stages::kind_note(k, &decided_content),
        (None, None) => String::new(),
    };
    let paragraphs = &input.material.paragraphs;
    let prompt = prompt(db, KEY_OBJECTIVES).await?;
    let passage = numbered_passage(paragraphs);
    let class_profile = input.profile.text.clone();
    let analysis = analysis_summary(&input.version, paragraphs);
    let own = teacher_objectives(&input.version, &input.today_objective);
    let query = [
        input.lesson.title.as_str(),
        input.material.title.as_str(),
        own.as_str(),
        analysis.as_str(),
        clip_chars(&input.material.text, 4000).as_str(),
    ]
    .join("\n");
    let (items, trace) = curriculum::items_for_track_hybrid(
        db,
        &state.embedder,
        &input.track,
        &query,
        CURRICULUM_LIMIT,
    )
    .await?;
    // The query contains material text, which must stay deletable: the log keeps the teacher's
    // own lines and, for the material, its length and a hash.
    let logged_query = clip_chars(
        &[
            input.lesson.title.as_str(),
            own.as_str(),
            analysis.as_str(),
            prompts::redacted(
                "課例教材",
                &format!(
                    "{}\n{}",
                    input.material.title,
                    clip_chars(&input.material.text, 4000)
                ),
            )
            .as_str(),
        ]
        .join("\n"),
        2000,
    );
    let allowed: HashMap<String, CurriculumItem> = items
        .iter()
        .cloned()
        .map(|i| (i.item_code.clone(), i))
        .collect();
    let codes: Vec<String> = items.iter().map(|i| i.item_code.clone()).collect();

    let mut vars: HashMap<&str, String> = HashMap::from([
        ("school_track", track_label(&input.track).to_string()),
        ("class_profile", or_blank(&class_profile)),
        (
            "teacher_objectives",
            if own.is_empty() {
                "（教師還沒有寫目標）".to_string()
            } else {
                own.clone()
            },
        ),
        ("passage_numbered", passage.clone()),
        (
            "analysis_summary",
            if analysis.is_empty() {
                "（教師還沒有做教材分析）".to_string()
            } else {
                analysis.clone()
            },
        ),
        (
            "curriculum_items",
            if items.is_empty() {
                "（知識庫目前沒有適用的課綱條目；所有目標請填 unlinked）".to_string()
            } else {
                format_curriculum(&items)
            },
        ),
    ]);
    let required = [
        ("class_profile", "班級特性"),
        ("curriculum_items", "可連結的 108 課綱條目（只能從這裡選）"),
    ];
    let system = prompts::render(&prompt.system_text, &vars);
    let user = format!(
        "{}{swap_note}",
        prompts::render_user_with(&prompt, &vars, &required)
    );
    vars.insert("passage_numbered", prompts::redacted("課例教材", &passage));
    // The list is identified by its codes in `retrieval`; no need to log every text again.
    vars.insert(
        "curriculum_items",
        format!("（{} 條，代碼見 retrieval.items）", items.len()),
    );
    let logged_user = format!(
        "{}{swap_note}",
        prompts::render_user_with(&prompt, &vars, &required)
    );
    let snapshot = json!({
        "inputs": vars,
        "class_context": input.profile.snapshot(),
        "sop": {
            "version_no": input.version.version_no,
            "material_id": input.material.id,
            "material_version_no": input.material.version_no,
        },
        "retrieval": {
            "query": logged_query,
            "limit": CURRICULUM_LIMIT,
            "method": trace.method,
            "fallback_reason": trace.fallback_reason,
            "items": items.iter().map(|i| json!({
                "item_code": i.item_code, "version": i.version
            })).collect::<Vec<_>>(),
        },
        "prompt": { "key": KEY_OBJECTIVES, "version_no": prompt.version_no,
                    "system": system, "user": logged_user },
        "schema": schemas::OBJECTIVES_SCHEMA_NAME,
        "request": { "temperature": temperature, "max_tokens": OBJECTIVES_MAX_TOKENS },
    });
    let meta = InteractionMeta {
        teacher_id: input.teacher_id,
        module: MODULE_OBJECTIVES,
        unit_id: Some(input.lesson.unit_id),
        lesson_id: Some(input.lesson.id),
        scaffold_id: scaffold_id(db, SCAFFOLD_OBJECTIVES).await?,
        prompt_version_id: Some(prompt.id),
    };
    let mut context = ContextList::default();
    context
        .add("教材", &material_label(&input.material))
        .add(&input.profile.context_label(), &class_profile)
        .add("你已經寫的目標", &own)
        .add("你的教材分析", &analysis)
        .add(
            "可連結的課綱條目",
            &if items.is_empty() {
                "知識庫目前沒有適用的條目".to_string()
            } else {
                format!("{} 條（條文由知識庫帶出）", items.len())
            },
        );
    let context = context.into_vec();
    let mut req = LlmRequest {
        system,
        user: user.clone(),
        schema_name: schemas::OBJECTIVES_SCHEMA_NAME,
        schema: schemas::objectives_schema(),
        fake_context: json!({
            "task": KEY_OBJECTIVES,
            "only_type": only.map(ObjectiveType::label),
            "swap_index": swap.map(|s| s.index),
            "content_objectives": decided_content,
            "paragraphs": paragraphs.len(),
            "items": items.iter().map(|i| json!({
                "item_code": i.item_code, "title": i.title
            })).collect::<Vec<_>>(),
        }),
        attempt: 1,
        max_tokens: Some(OBJECTIVES_MAX_TOKENS),
        temperature: Some(temperature),
        images: Vec::new(),
    };

    let a1 = attempt(state, &meta, &snapshot, &req, None).await?;
    crate::retrieval::log(db, Some(a1.id), &logged_query, &input.track, &codes, &trace).await?;

    let cited = |e: &ObjectivesEval| -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        e.kept
            .iter()
            .flat_map(|o| o.links.iter())
            .filter(|l| seen.insert(l.item_code.clone()))
            .map(|l| l.item_code.clone())
            .collect()
    };

    let mut first: Option<(ObjectivesEval, Vec<CheckResult>)> = None;
    match &a1.response {
        Ok(resp) => match serde_json::from_value::<RawObjectives>(resp.json.clone()) {
            Ok(raw) => {
                let eval = evaluate_objectives(&raw, &allowed, paragraphs);
                let status = if eval.needs_repair_for(only) {
                    "blocked"
                } else {
                    "shown"
                };
                let mut results = vec![CheckResult::pass("schema")];
                results.extend(eval.results_for(only));
                finish_output(
                    db,
                    &a1,
                    resp,
                    status,
                    checks::report(&results),
                    cited(&eval),
                )
                .await?;
                first = Some((eval, results));
            }
            Err(e) => {
                let results = [CheckResult::fail("schema", e.to_string())];
                finish_output(
                    db,
                    &a1,
                    resp,
                    "blocked",
                    checks::report(&results),
                    Vec::new(),
                )
                .await?;
            }
        },
        Err(e) => match finish_failure(db, &a1, e).await? {
            Failure::Timeout => {
                return Ok(not_shown::<ObjectivesSuggestion>(
                    Outcome::Timeout {
                        interaction_id: a1.id,
                    },
                    context,
                ))
            }
            Failure::Error(kind) => {
                return Ok(not_shown::<ObjectivesSuggestion>(
                    Outcome::Error {
                        interaction_id: a1.id,
                        kind,
                    },
                    context,
                ))
            }
            Failure::Retryable => {}
        },
    }

    let repair = first.as_ref().is_none_or(|(e, _)| e.needs_repair_for(only));
    let mut second: Option<(Attempt, ObjectivesEval, Vec<CheckResult>)> = None;
    let mut last_failure: Option<(i64, Failure)> = None;
    if repair {
        set_phase(db, job_id, PHASE_OBJECTIVES_REPAIR).await?;
        let mut note = String::from(
            "\n\n【重要：這是第二次請求】上一次的輸出沒有通過平台的檢查，請重新產生完整的 objectives。",
        );
        if let Some((e, _)) = &first {
            if !e.invalid_codes.is_empty() {
                note.push_str(&format!(
                    "\n- 這些代碼不在「可連結的 108 課綱條目」清單裡，不可以使用：{}。只能逐字複製清單中的代碼；沒有合適的條目就填 unlinked、curriculum_links 給空陣列。",
                    e.invalid_codes.join("、")
                ));
            }
            for d in &e.dropped {
                note.push_str(&format!("\n- 這個目標不符合規則：{d}。"));
            }
            if let Some(t) = only {
                if !e.usable_for(only) {
                    note.push_str(&format!("\n- objectives 必須有 1 項可用的{}。", t.label()));
                }
            } else if !e.has_both_types() {
                note.push_str("\n- 內容目標與語言目標都必須至少各有 1 項。");
            }
        }
        req.user = format!("{user}{note}");
        req.attempt = 2;
        req.temperature = Some(temperature + REPAIR_TEMPERATURE_STEP);
        let mut snap = snapshot.clone();
        snap["retry_of"] = json!(a1.id);
        snap["repair"] = json!({ "note": note });
        snap["request"]["temperature"] = json!(temperature + REPAIR_TEMPERATURE_STEP);
        let a2 = attempt(state, &meta, &snap, &req, Some(a1.id)).await?;
        match &a2.response {
            Ok(resp) => match serde_json::from_value::<RawObjectives>(resp.json.clone()) {
                Ok(raw) => {
                    let eval = evaluate_objectives(&raw, &allowed, paragraphs);
                    let mut results = vec![CheckResult::pass("schema")];
                    results.extend(eval.results_for(only));
                    second = Some((a2, eval, results));
                }
                Err(e) => {
                    let results = [CheckResult::fail("schema", e.to_string())];
                    finish_output(
                        db,
                        &a2,
                        resp,
                        "blocked",
                        checks::report(&results),
                        Vec::new(),
                    )
                    .await?;
                }
            },
            Err(e) => {
                let failure = finish_failure(db, &a2, e).await?;
                last_failure = Some((a2.id, failure));
            }
        }
    }

    // The repair call's output is used when it has both objective types; otherwise what the
    // first call got right. Either way invented codes and rule-breaking objectives are gone.
    let repairs = |e: &ObjectivesEval| {
        json!({
            "attempts": if repair { 2 } else { 1 },
            "dropped_codes": e.invalid_codes,
            "dropped_objectives": e.dropped,
        })
    };
    let second_usable = second.as_ref().is_some_and(|(_, e, _)| e.usable_for(only));
    if let Some((a2, eval, results)) = second {
        if let Ok(resp) = &a2.response {
            let status = if second_usable { "shown" } else { "blocked" };
            finish_output(
                db,
                &a2,
                resp,
                status,
                report_with(&results, repairs(&eval)),
                cited(&eval),
            )
            .await?;
        }
        if second_usable {
            let out = eval.restrict(only, keep_max).into_suggestion(items.len());
            let sources = objective_sources(&out);
            return Ok((
                result_json(AiResult::shown(a2.id, out, sources).with_context(context)),
                Some(a2.id),
            ));
        }
        if last_failure.is_none() {
            last_failure = Some((a2.id, Failure::Retryable));
        }
    }
    if let Some((eval, results)) = first {
        if eval.usable_for(only) {
            if repair {
                set_final(db, a1.id, "shown", report_with(&results, repairs(&eval))).await?;
            }
            let out = eval.restrict(only, keep_max).into_suggestion(items.len());
            let sources = objective_sources(&out);
            return Ok((
                result_json(AiResult::shown(a1.id, out, sources).with_context(context)),
                Some(a1.id),
            ));
        }
    }
    let outcome = match last_failure {
        Some((id, Failure::Timeout)) => Outcome::Timeout { interaction_id: id },
        Some((id, Failure::Error(kind))) => Outcome::Error {
            interaction_id: id,
            kind,
        },
        Some((id, Failure::Retryable)) => Outcome::Blocked { interaction_id: id },
        None => Outcome::Blocked {
            interaction_id: a1.id,
        },
    };
    Ok(not_shown::<ObjectivesSuggestion>(outcome, context))
}
