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
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::PgPool;

use super::checks::{
    evaluate_analysis, evaluate_objectives, merge_analysis, AnalysisEval, ObjectivesEval,
};
use super::schemas::{self, AnalysisSuggestion, ObjectivesSuggestion, RawAnalysis, RawObjectives};
use super::store::{Material, Version};
use super::{numbered_passage, Paragraph};
use crate::ai::pipeline::{
    self, AiContextItem, AiResult, ContextList, Finish, InteractionMeta, Outcome, Source,
};
use crate::ai::{LlmRequest, LlmResponse, ProviderError};
use crate::checks::{self, CheckResult};
use crate::config::AiProviderKind;
use crate::curriculum::{self, CurriculumItem};
use crate::error::{AppError, AppResult};
use crate::prompts;
use crate::routes::{clip_chars, or_blank, scaffold_id, track_label, LessonRef, ResolvedProfile};
use crate::AppState;

pub const KEY_ANALYSIS: &str = "sop_passage_analysis";
pub const KEY_OBJECTIVES: &str = "sop_objectives";
pub const MODULE_ANALYSIS: &str = "sop_analysis";
pub const MODULE_OBJECTIVES: &str = "sop_objectives";
pub const SCAFFOLD_ANALYSIS: &str = "SC-SOP-ANALYSIS";
pub const SCAFFOLD_OBJECTIVES: &str = "SC-SOP-OBJECTIVES";

/// Above this the analysis is not attempted in one call (long arrays end early).
pub const MAX_AI_PARAGRAPHS: usize = 40;
pub const ANALYSIS_MAX_TOKENS: u32 = 6000;
pub const OBJECTIVES_MAX_TOKENS: u32 = 4000;
/// Most curriculum items put in front of the model.
pub const CURRICULUM_LIMIT: usize = 150;
const BASE_TEMPERATURE: f32 = 0.2;
/// Added for the repair call, so it is never the request the gateway has cached.
const REPAIR_TEMPERATURE_STEP: f32 = 0.4;

pub const MSG_STALE: &str = "這次請求中斷了（可能是系統剛更新）。請再按一次。";

const PHASE_ANALYSIS: &str = "正在讀教材，逐段分析寫作特色";
const PHASE_ANALYSIS_REPAIR: &str = "有幾段沒有分析好，正在補上";
const PHASE_OBJECTIVES: &str = "正在依教材與班級特性想目標，並對照課綱";
const PHASE_OBJECTIVES_REPAIR: &str = "正在重新確認目標與課綱連結";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobKind {
    Analysis,
    Objectives,
}

impl JobKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Analysis => "analysis",
            Self::Objectives => "objectives",
        }
    }

    fn first_phase(self) -> &'static str {
        match self {
            Self::Analysis => PHASE_ANALYSIS,
            Self::Objectives => PHASE_OBJECTIVES,
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
    pub phase: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub sop_version_no: Option<i32>,
    pub result: Option<Value>,
    /// `item_index` values the teacher already decided on.
    pub decided: Vec<i32>,
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
}

const JOB_SELECT: &str = "SELECT j.id, j.lesson_id, j.kind, j.status, j.phase, j.created_at,
        j.finished_at, v.version_no AS sop_version_no, j.interaction_id, j.result
   FROM lesson_sop_jobs j LEFT JOIN lesson_sop_versions v ON v.id = j.sop_version_id";

async fn into_job(db: &PgPool, row: JobRow) -> sqlx::Result<Job> {
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
        status: row.status,
        phase: row.phase,
        started_at: row.created_at,
        finished_at: row.finished_at,
        sop_version_no: row.sop_version_no,
        result: row.result,
        decided,
    })
}

fn result_json<T: Serialize>(result: AiResult<T>) -> Value {
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
    (state.config.ai_timeout_secs * 2 + 60) as f64
}

async fn expire_stale(state: &AppState, lesson_id: i64) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE lesson_sop_jobs SET status = 'done', finished_at = now(), result = $2
          WHERE lesson_id = $1 AND status = 'running'
            AND created_at < now() - make_interval(secs => $3)",
    )
    .bind(lesson_id)
    .bind(error_result(MSG_STALE))
    .bind(stale_after(state))
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
    Ok(Some(into_job(&state.db, row).await?))
}

/// The most recent job of a kind for a lesson.
pub async fn latest_job(
    state: &AppState,
    lesson_id: i64,
    kind: JobKind,
) -> sqlx::Result<Option<Job>> {
    expire_stale(state, lesson_id).await?;
    let row: Option<JobRow> = sqlx::query_as(&format!(
        "{JOB_SELECT} WHERE j.lesson_id = $1 AND j.kind = $2 ORDER BY j.id DESC LIMIT 1"
    ))
    .bind(lesson_id)
    .bind(kind.as_str())
    .fetch_optional(&state.db)
    .await?;
    match row {
        Some(row) => Ok(Some(into_job(&state.db, row).await?)),
        None => Ok(None),
    }
}

/// What a job needs, loaded while the request is still there to answer with a 4xx.
pub struct JobInput {
    pub kind: JobKind,
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
               (lesson_id, teacher_id, kind, status, sop_version_id, material_id, result, finished_at)
             VALUES ($1, $2, $3, 'done', $4, $5, $6, now()) RETURNING id",
        )
        .bind(lesson_id)
        .bind(input.teacher_id)
        .bind(kind.as_str())
        .bind(input.version.id)
        .bind(input.material.id)
        .bind(result_json(AiResult::<Value>::disabled()))
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
    let created: Option<i64> = sqlx::query_scalar(
        "INSERT INTO lesson_sop_jobs
           (lesson_id, teacher_id, kind, status, phase, sop_version_id, material_id)
         VALUES ($1, $2, $3, 'running', $4, $5, $6)
         ON CONFLICT (lesson_id, kind) WHERE status = 'running' DO NOTHING
         RETURNING id",
    )
    .bind(lesson_id)
    .bind(input.teacher_id)
    .bind(kind.as_str())
    .bind(kind.first_phase())
    .bind(input.version.id)
    .bind(input.material.id)
    .fetch_optional(&state.db)
    .await?;
    let Some(job_id) = created else {
        let running: i64 = sqlx::query_scalar(
            "SELECT id FROM lesson_sop_jobs WHERE lesson_id = $1 AND kind = $2 AND status = 'running'",
        )
        .bind(lesson_id)
        .bind(kind.as_str())
        .fetch_one(&state.db)
        .await?;
        return Ok(running);
    };

    let state = state.clone();
    let temperature = BASE_TEMPERATURE + 0.1 * earlier.min(4) as f32;
    tokio::spawn(async move {
        let db = state.db.clone();
        // The work runs in its own task so that a panic there still ends the job.
        let work = tokio::spawn(async move {
            match kind {
                JobKind::Analysis => run_analysis(&state, job_id, &input, temperature).await,
                JobKind::Objectives => run_objectives(&state, job_id, &input, temperature).await,
            }
        });
        let (result, interaction_id) = match work.await {
            Ok(Ok(done)) => done,
            Ok(Err(e)) => {
                tracing::error!(job_id, error = %e, "SOP job failed");
                (error_result(pipeline::MSG_ERROR), None)
            }
            Err(e) => {
                tracing::error!(job_id, error = %e, "SOP job panicked");
                (error_result(pipeline::MSG_ERROR), None)
            }
        };
        let saved = sqlx::query(
            "UPDATE lesson_sop_jobs
                SET status = 'done', finished_at = now(), result = $2, interaction_id = $3
              WHERE id = $1 AND status = 'running'",
        )
        .bind(job_id)
        .bind(result)
        .bind(interaction_id)
        .execute(&db)
        .await;
        if let Err(e) = saved {
            tracing::error!(job_id, error = %e, "could not store the SOP job result");
        }
    });
    Ok(job_id)
}

async fn set_phase(db: &PgPool, job_id: i64, phase: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE lesson_sop_jobs SET phase = $2 WHERE id = $1")
        .bind(job_id)
        .bind(phase)
        .execute(db)
        .await?;
    Ok(())
}

// ---------------------------------------------------------------- one model call

pub(crate) struct Attempt {
    pub(crate) id: i64,
    pub(crate) latency_ms: i32,
    pub(crate) response: Result<LlmResponse, ProviderError>,
}

pub(crate) enum Failure {
    Timeout,
    Error,
    /// Refusal, unreadable output or a runaway to `max_tokens`: worth one changed retry.
    Retryable,
}

pub(crate) async fn attempt(
    state: &AppState,
    meta: &InteractionMeta,
    snapshot: &Value,
    req: &LlmRequest,
    retry_of: Option<i64>,
) -> AppResult<Attempt> {
    let id = pipeline::insert_interaction(
        &state.db,
        meta,
        state.ai.provider_name(),
        &state.ai.model,
        snapshot,
        req.attempt,
        retry_of,
    )
    .await?;
    if state.ai.kind == AiProviderKind::Fake && req.user.contains("FAKE_SLOW") {
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
    let start = Instant::now();
    let response = state.ai.complete(req).await;
    let latency_ms = i32::try_from(start.elapsed().as_millis()).unwrap_or(i32::MAX);
    Ok(Attempt {
        id,
        latency_ms,
        response,
    })
}

/// Records a call that returned JSON.
pub(crate) async fn finish_output(
    db: &PgPool,
    a: &Attempt,
    resp: &LlmResponse,
    status: &str,
    report: Value,
    citations: Vec<String>,
) -> sqlx::Result<()> {
    pipeline::finish_interaction(
        db,
        a.id,
        Finish {
            status,
            output: Some(resp.json.clone()),
            citations,
            check_results: Some(report),
            model_version: Some(resp.model_version.clone()),
            error_detail: None,
            latency_ms: a.latency_ms,
        },
    )
    .await
}

/// Records a call that did not return JSON and says what kind of failure it was.
pub(crate) async fn finish_failure(
    db: &PgPool,
    a: &Attempt,
    error: &ProviderError,
) -> sqlx::Result<Failure> {
    let (status, check, detail, kind) = match error {
        ProviderError::Timeout => (
            "timeout",
            None,
            Some("provider timeout".to_string()),
            Failure::Timeout,
        ),
        ProviderError::Refusal(d) => (
            "blocked",
            Some(CheckResult::fail("refusal", d.clone())),
            None,
            Failure::Retryable,
        ),
        ProviderError::InvalidOutput(d) => (
            "blocked",
            Some(CheckResult::fail("schema", d.clone())),
            None,
            Failure::Retryable,
        ),
        ProviderError::MaxTokens => (
            "blocked",
            Some(CheckResult::fail(
                "max_tokens",
                "output ran to max_tokens (runaway repetition)",
            )),
            None,
            Failure::Retryable,
        ),
        e => ("error", None, Some(e.to_string()), Failure::Error),
    };
    if matches!(kind, Failure::Error) {
        tracing::warn!(interaction_id = a.id, error = %error, "AI provider error");
    }
    pipeline::finish_interaction(
        db,
        a.id,
        Finish {
            status,
            output: None,
            citations: Vec::new(),
            check_results: check.map(|c| checks::report(&[c])),
            model_version: None,
            error_detail: detail,
            latency_ms: a.latency_ms,
        },
    )
    .await?;
    Ok(kind)
}

/// Marks the row the shown result is attributed to, with what the code repaired.
pub(crate) async fn set_final(
    db: &PgPool,
    id: i64,
    status: &str,
    report: Value,
) -> sqlx::Result<()> {
    sqlx::query("UPDATE ai_interactions SET status = $2, check_results = $3 WHERE id = $1")
        .bind(id)
        .bind(status)
        .bind(report)
        .execute(db)
        .await?;
    Ok(())
}

pub(crate) fn report_with(results: &[CheckResult], repairs: Value) -> Value {
    let mut report = checks::report(results);
    report["repairs"] = repairs;
    report
}

type Done = (Value, Option<i64>);

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

pub(crate) async fn prompt(db: &PgPool, key: &str) -> AppResult<prompts::PromptVersion> {
    prompts::latest(db, key)
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("no prompt version for {key}")))
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
        "request": { "temperature": temperature, "max_tokens": ANALYSIS_MAX_TOKENS },
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
        max_tokens: Some(ANALYSIS_MAX_TOKENS),
        temperature: Some(temperature),
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
            Failure::Error => {
                return Ok(not_shown::<AnalysisSuggestion>(
                    Outcome::Error {
                        interaction_id: a1.id,
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
            Some((id, Failure::Error)) => Outcome::Error { interaction_id: id },
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

fn analysis_summary(version: &Version, paragraphs: &[Paragraph]) -> String {
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
) -> AppResult<Done> {
    let db = &state.db;
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
    let user = prompts::render_user_with(&prompt, &vars, &required);
    vars.insert("passage_numbered", prompts::redacted("課例教材", &passage));
    // The list is identified by its codes in `retrieval`; no need to log every text again.
    vars.insert(
        "curriculum_items",
        format!("（{} 條，代碼見 retrieval.items）", items.len()),
    );
    let logged_user = prompts::render_user_with(&prompt, &vars, &required);
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
            "paragraphs": paragraphs.len(),
            "items": items.iter().map(|i| json!({
                "item_code": i.item_code, "title": i.title
            })).collect::<Vec<_>>(),
        }),
        attempt: 1,
        max_tokens: Some(OBJECTIVES_MAX_TOKENS),
        temperature: Some(temperature),
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
                let status = if eval.needs_repair() {
                    "blocked"
                } else {
                    "shown"
                };
                let mut results = vec![CheckResult::pass("schema")];
                results.extend(eval.results.clone());
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
            Failure::Error => {
                return Ok(not_shown::<ObjectivesSuggestion>(
                    Outcome::Error {
                        interaction_id: a1.id,
                    },
                    context,
                ))
            }
            Failure::Retryable => {}
        },
    }

    let repair = first.as_ref().is_none_or(|(e, _)| e.needs_repair());
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
            if !e.has_both_types() {
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

    // The repair call's output is used when it has both objective types; otherwise what the
    // first call got right. Either way invented codes and rule-breaking objectives are gone.
    let repairs = |e: &ObjectivesEval| {
        json!({
            "attempts": if repair { 2 } else { 1 },
            "dropped_codes": e.invalid_codes,
            "dropped_objectives": e.dropped,
        })
    };
    let second_usable = second.as_ref().is_some_and(|(_, e, _)| e.has_both_types());
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
            let out = eval.into_suggestion(items.len());
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
        if eval.has_both_types() {
            if repair {
                set_final(db, a1.id, "shown", report_with(&results, repairs(&eval))).await?;
            }
            let out = eval.into_suggestion(items.len());
            let sources = objective_sources(&out);
            return Ok((
                result_json(AiResult::shown(a1.id, out, sources).with_context(context)),
                Some(a1.id),
            ));
        }
    }
    let outcome = match last_failure {
        Some((id, Failure::Timeout)) => Outcome::Timeout { interaction_id: id },
        Some((id, Failure::Error)) => Outcome::Error { interaction_id: id },
        Some((id, Failure::Retryable)) => Outcome::Blocked { interaction_id: id },
        None => Outcome::Blocked {
            interaction_id: a1.id,
        },
    };
    Ok(not_shown::<ObjectivesSuggestion>(outcome, context))
}
