//! The generation pipeline of the worksheet generator, run as background jobs.
//!
//! One job is one model step (docs/LLM_VALIDATION.md: one step per call, each with its own
//! checks and one repair): tasks per paragraph → warm-up → scaffolds → exit ticket. A generation
//! is those four jobs run in order by one background task; the worksheet itself is then
//! assembled in code ([`super::assemble`]). Regenerating a single block runs only the steps that
//! block needs.
//!
//! Every model call is its own `ai_interactions` row, made through the same helpers as the
//! lesson SOP stages: `max_tokens` is always set, and the repair call changes the request
//! (higher temperature, the failed checks appended) because the gateway caches identical ones.
//! A step that fails never stops the rest: its block is left empty with an honest message, and
//! everything stays editable by hand.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::PgPool;

use super::assemble::{self, EXIT_ID, WARMUP_ID};
use super::checks::{
    self, evaluate_exit, evaluate_scaffolds, evaluate_tasks, evaluate_warmup, Candidate,
    CleanWarmup, Eval, ExitCtx, ScaffoldsCtx, TasksCtx, WarmupCtx,
};
use super::schemas::{
    self, RawBlock, RawExit, RawExitItem, RawScaffoldSet, RawScaffolds, RawTasks, RawWarmup,
    TARGET_WARMUP, TIER_CORE,
};
use super::store::{self, Worksheet};
use super::{
    exit_count, info_function_label, resolve, student_tasks, vocab_count, Block, BlockKind, Chosen,
    Content, Level, Objective, Practice, ANSWER_FORMATS, GEN_OBJECTIVES, GEN_WARMUP,
    INSTRUCTIONAL_KINDS, PROCEDURAL_KINDS, VERBAL_KINDS,
};
use crate::ai::pipeline::{self, AiContextItem, ContextList, InteractionMeta};
use crate::ai::LlmRequest;
use crate::checks::{self as base_checks, CheckResult};
use crate::error::{AppError, AppResult};
use crate::materials::detect;
use crate::prompts;
use crate::routes::{clip_chars, log_usage, or_blank, scaffold_id, track_label};
use crate::sop::ai::{
    attempt, finish_failure, finish_output, prompt, report_with, set_final, Failure,
};
use crate::vocabulary;
use crate::AppState;

pub const STEP_TASKS: &str = "tasks";
pub const STEP_WARMUP: &str = "warmup";
pub const STEP_SCAFFOLDS: &str = "scaffolds";
pub const STEP_EXIT: &str = "exit_ticket";
/// A generation from a template: one job, one model call per block.
pub const STEP_TEMPLATE: &str = "template";
/// 閱讀理解題／差異化學習單: one job for the calls of all Bloom levels …
pub const STEP_BLOOM: &str = "bloom";
/// … and one for the judge (and the regeneration of mismatched questions).
pub const STEP_BLOOM_CHECK: &str = "bloom_check";

pub mod bloom;
pub mod revise;
pub mod template;

// Output budgets: about twice the longest output measured with gemma-4-31B-it
// (docs/llm-eval/worksheet-real/README.md: tasks ≈ 500 tokens per paragraph + 500, warm-up
// ≤ 810, scaffolds ≈ 220 per target, exit ticket ≤ 434). A budget only matters when the model
// runs away, and then a smaller one ends the wasted call sooner (≈ 170 tokens per second).
/// The tasks budget for the whole material; see [`tasks_max_tokens`].
pub const TASKS_MAX_TOKENS: u32 = 6000;
pub const WARMUP_MAX_TOKENS: u32 = 2000;
/// The scaffolds budget for many blocks; see [`scaffolds_max_tokens`].
pub const SCAFFOLDS_MAX_TOKENS: u32 = 5000;
pub const EXIT_MAX_TOKENS: u32 = 1200;

/// Output budget of the tasks step for `paragraphs` paragraphs.
pub fn tasks_max_tokens(paragraphs: usize) -> u32 {
    (1500 + 600 * paragraphs.min(50) as u32).clamp(3000, TASKS_MAX_TOKENS)
}

/// Output budget of the scaffolds step for `targets` blocks.
pub fn scaffolds_max_tokens(targets: usize) -> u32 {
    (800 + 350 * targets.min(50) as u32).clamp(2000, SCAFFOLDS_MAX_TOKENS)
}
const BASE_TEMPERATURE: f32 = 0.2;
/// Added for the repair call, so it is never the request the gateway has cached.
const REPAIR_TEMPERATURE_STEP: f32 = 0.4;
/// Most key-word candidates put in front of the model.
const MAX_CANDIDATES: usize = 30;

pub const MSG_STALE: &str = "這次產生中斷了（可能是系統剛更新）。請再按一次。";
const MSG_BLOCKED: &str = "Manna 交出的內容沒有通過檢查";
/// How the teacher's objective and request are named in every prompt.
pub const OBJECTIVE_HEADING: &str = "學習目標（教師寫的）";
pub const REQUEST_HEADING: &str = "教師的要求";
const MSG_NEED_MATERIAL: &str = "請先貼上教材，Manna 才有東西可以設計。";

struct StepInfo {
    key: &'static str,
    module: &'static str,
    scaffold: &'static str,
    schema_name: &'static str,
    phase: &'static str,
    repair_phase: &'static str,
}

const TASKS: StepInfo = StepInfo {
    key: "worksheet_tasks",
    module: "worksheet_tasks",
    scaffold: "SC-WS-TASKS",
    schema_name: schemas::TASKS_SCHEMA_NAME,
    phase: "正在依段落設計問題與任務",
    repair_phase: "有幾題沒有通過檢查，正在修正",
};
const WARMUP: StepInfo = StepInfo {
    key: "worksheet_warmup",
    module: "worksheet_warmup",
    scaffold: "SC-WS-WARMUP",
    schema_name: schemas::WARMUP_SCHEMA_NAME,
    phase: "正在設計暖身活動與關鍵字彙",
    repair_phase: "正在修正暖身活動",
};
const SCAFFOLDS: StepInfo = StepInfo {
    key: "worksheet_scaffolds",
    module: "worksheet_scaffolds",
    scaffold: "SC-WS-SCAFFOLDS",
    schema_name: schemas::SCAFFOLDS_SCHEMA_NAME,
    phase: "正在為每個區塊安排鷹架",
    repair_phase: "正在修正鷹架",
};
const EXIT: StepInfo = StepInfo {
    key: "worksheet_exit_ticket",
    module: "worksheet_exit_ticket",
    scaffold: "SC-WS-EXIT",
    schema_name: schemas::EXIT_SCHEMA_NAME,
    phase: "正在設計 Exit ticket",
    repair_phase: "正在修正 Exit ticket",
};

// ---------------------------------------------------------------- jobs

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Job {
    pub id: i64,
    pub worksheet_id: i64,
    pub run_no: i32,
    pub step: String,
    pub block_id: Option<String>,
    pub status: String,
    pub outcome: Option<String>,
    pub phase: String,
    pub message: Option<String>,
    pub interaction_id: Option<i64>,
    pub context: Value,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

/// Jobs still open long after their calls must have timed out died with the server.
async fn expire_stale(state: &AppState, worksheet_id: i64) -> sqlx::Result<()> {
    let stale_after = (state.config.ai_timeout_secs * 8 + 60) as f64;
    sqlx::query(
        "UPDATE worksheet_jobs
            SET status = 'done', outcome = 'error', message = $2, finished_at = now()
          WHERE worksheet_id = $1 AND status <> 'done'
            AND created_at < now() - make_interval(secs => $3)",
    )
    .bind(worksheet_id)
    .bind(MSG_STALE)
    .bind(stale_after)
    .execute(&state.db)
    .await?;
    Ok(())
}

/// The jobs of the most recent run, in order, and whether any is still open.
pub async fn jobs(state: &AppState, worksheet_id: i64) -> sqlx::Result<(Vec<Job>, bool)> {
    expire_stale(state, worksheet_id).await?;
    let jobs: Vec<Job> = sqlx::query_as(
        "SELECT id, worksheet_id, run_no, step, block_id, status, outcome, phase, message,
                interaction_id, context, started_at, finished_at
           FROM worksheet_jobs
          WHERE worksheet_id = $1
            AND run_no = (SELECT MAX(run_no) FROM worksheet_jobs WHERE worksheet_id = $1)
          ORDER BY id",
    )
    .bind(worksheet_id)
    .fetch_all(&state.db)
    .await?;
    let running = jobs.iter().any(|j| j.status != "done");
    Ok((jobs, running))
}

/// Opens a run: one queued job per step. `None` when a run is already in progress (the caller
/// joins it). Returns the run number and the job id of each step.
async fn open_run(
    db: &PgPool,
    worksheet_id: i64,
    teacher_id: i64,
    steps: &[&'static str],
    block_id: Option<&str>,
) -> AppResult<Option<(i32, HashMap<&'static str, i64>)>> {
    let mut tx = db.begin().await?;
    sqlx::query("SELECT id FROM worksheets WHERE id = $1 FOR UPDATE")
        .bind(worksheet_id)
        .execute(&mut *tx)
        .await?;
    let open: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM worksheet_jobs WHERE worksheet_id = $1 AND status <> 'done'",
    )
    .bind(worksheet_id)
    .fetch_one(&mut *tx)
    .await?;
    if open > 0 {
        return Ok(None);
    }
    let run_no: i32 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(run_no), 0) + 1 FROM worksheet_jobs WHERE worksheet_id = $1",
    )
    .bind(worksheet_id)
    .fetch_one(&mut *tx)
    .await?;
    let mut ids = HashMap::new();
    for step in steps {
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO worksheet_jobs (worksheet_id, teacher_id, run_no, step, block_id, status, phase)
             VALUES ($1, $2, $3, $4, $5, 'queued', '排隊中') RETURNING id",
        )
        .bind(worksheet_id)
        .bind(teacher_id)
        .bind(run_no)
        .bind(step)
        .bind(block_id)
        .fetch_one(&mut *tx)
        .await?;
        ids.insert(*step, id);
    }
    tx.commit().await?;
    Ok(Some((run_no, ids)))
}

async fn mark_running(db: &PgPool, job_id: i64, phase: &str) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE worksheet_jobs SET status = 'running', phase = $2,
                started_at = COALESCE(started_at, now())
          WHERE id = $1",
    )
    .bind(job_id)
    .bind(phase)
    .execute(db)
    .await?;
    Ok(())
}

async fn mark_done(
    db: &PgPool,
    job_id: i64,
    outcome: &str,
    message: Option<&str>,
    interaction_id: Option<i64>,
    context: &[AiContextItem],
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE worksheet_jobs
            SET status = 'done', outcome = $2, message = $3, interaction_id = $4, context = $5,
                phase = '', finished_at = now()
          WHERE id = $1 AND status <> 'done'",
    )
    .bind(job_id)
    .bind(outcome)
    .bind(message)
    .bind(interaction_id)
    .bind(json!(context))
    .execute(db)
    .await?;
    Ok(())
}

/// Records the outcome of the last step of a run but keeps it open: the run is only over once
/// the worksheet has been assembled and saved, so a page that sees nothing running anymore
/// always finds the new version.
async fn mark_held(
    db: &PgPool,
    job_id: i64,
    outcome: &str,
    message: Option<&str>,
    interaction_id: Option<i64>,
    context: &[AiContextItem],
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE worksheet_jobs
            SET outcome = $2, message = $3, interaction_id = $4, context = $5,
                phase = '正在把結果組合成學習單'
          WHERE id = $1 AND status <> 'done'",
    )
    .bind(job_id)
    .bind(outcome)
    .bind(message)
    .bind(interaction_id)
    .bind(json!(context))
    .execute(db)
    .await?;
    Ok(())
}

/// Starts the background task of a run. Whatever is still open when the work returns, fails or
/// panics is closed as an error, so the page never waits for ever.
fn spawn_run<F>(state: &AppState, worksheet_id: i64, run_no: i32, work: F)
where
    F: std::future::Future<Output = AppResult<()>> + Send + 'static,
{
    let db = state.db.clone();
    tokio::spawn(async move {
        match tokio::spawn(work).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => tracing::error!(worksheet_id, run_no, error = %e, "worksheet run failed"),
            Err(e) => tracing::error!(worksheet_id, run_no, error = %e, "worksheet run panicked"),
        }
        // A step that has its outcome is finished now; one that never got one failed.
        let held = sqlx::query(
            "UPDATE worksheet_jobs SET status = 'done', phase = '', finished_at = now()
              WHERE worksheet_id = $1 AND run_no = $2 AND status <> 'done' AND outcome IS NOT NULL",
        )
        .bind(worksheet_id)
        .bind(run_no)
        .execute(&db)
        .await;
        if let Err(e) = held {
            tracing::error!(worksheet_id, error = %e, "could not close the worksheet run");
        }
        let closed = sqlx::query(
            "UPDATE worksheet_jobs
                SET status = 'done', outcome = 'error', message = $3, finished_at = now()
              WHERE worksheet_id = $1 AND run_no = $2 AND status <> 'done'",
        )
        .bind(worksheet_id)
        .bind(run_no)
        .bind(pipeline::MSG_ERROR)
        .execute(&db)
        .await;
        if let Err(e) = closed {
            tracing::error!(worksheet_id, error = %e, "could not close the worksheet run");
        }
    });
}

// ---------------------------------------------------------------- what a run works with

/// Loaded while the request is still there to answer with a 4xx.
pub struct RunInput {
    pub worksheet: Worksheet,
    pub content: Content,
    pub track: String,
    /// The class profile sent to the model, and what to call it in 「Manna 已帶入的資訊」.
    pub class_profile: String,
    pub class_profile_label: String,
    pub class_snapshot: Value,
    pub unit_id: Option<i64>,
}

struct Run {
    state: AppState,
    input: RunInput,
    run_no: i32,
    temperature: f32,
    paragraphs: Vec<String>,
    /// (code sent to the model, objective)
    objectives: Vec<(String, Objective)>,
    candidates: Vec<Candidate>,
    hard_share: Option<f64>,
    /// The word list, for the levels of the task words (SIOP V2).
    lexicon: Option<vocabulary::Lexicon>,
    /// Annotations of the material the teacher marked 「提供給 AI」, as prompt text ("" if none).
    ai_notes: String,
    ai_notes_count: usize,
    /// The worksheet's 內容目標 and 語言目標 (the two fields and a lesson's), for the prompts
    /// that take them as text.
    teacher: crate::objectives::TeacherObjectives,
    /// 「教師的要求」, trimmed ("" if none).
    request: String,
    /// The last step's job: kept open until the result is saved.
    hold: Option<i64>,
    /// The picture-slot rules appended to the steps that write blocks ("" while pictures are
    /// off: those steps then ask for no `image_slots`, as before).
    slot_rules: String,
}

impl Run {
    async fn new(
        state: &AppState,
        input: RunInput,
        run_no: i32,
        hold: Option<i64>,
    ) -> AppResult<Self> {
        let earlier: i64 = sqlx::query_scalar(
            "SELECT COUNT(DISTINCT run_no) FROM worksheet_jobs WHERE worksheet_id = $1 AND run_no < $2",
        )
        .bind(input.worksheet.id)
        .bind(run_no)
        .fetch_one(&state.db)
        .await?;
        let paragraphs: Vec<String> = input
            .worksheet
            .material
            .paragraphs
            .iter()
            .map(|p| p.text.clone())
            .collect();
        // The two fields are the worksheet's objectives too (G1): a worksheet stored before they
        // went into `content.objectives` gets them now.
        let current =
            crate::objectives::ensured(&input.content.objectives, &input.worksheet.settings);
        let teacher = crate::objectives::TeacherObjectives::from_worksheet(&current);
        let objectives = current
            .iter()
            .filter(|o| !o.text.trim().is_empty() || !o.student_facing_en.trim().is_empty())
            .enumerate()
            .map(|(i, o)| (format!("O{}", i + 1), o.clone()))
            .collect();
        let lexicon = vocabulary::load_lexicon(&state.db, None)
            .await?
            .filter(|l| !l.is_empty());
        let (candidates, hard_share) = key_word_candidates(
            &paragraphs,
            lexicon.as_ref(),
            &input.worksheet.brief.vocabulary,
        );
        let notes = match input.worksheet.material.source.material_id {
            Some(material_id) => {
                crate::materials::annotations::shared_with_ai(
                    &state.db,
                    input.worksheet.teacher_id,
                    material_id,
                )
                .await?
            }
            None => Vec::new(),
        };
        let slot_rules = if state.images.enabled() {
            crate::prompts::latest(&state.db, super::images::SLOTS_RULES_KEY)
                .await?
                .map(|p| p.system_text.trim().to_string())
                .unwrap_or_default()
        } else {
            String::new()
        };
        // Notes the teacher left on this worksheet for Manna (逐區修改的註記): a new generation
        // reads the open ones too, under their own heading.
        let ws_notes = super::comments::open_for_ai(
            &state.db,
            input.worksheet.id,
            input.worksheet.teacher_id,
            &input.content,
        )
        .await?;
        let ai_notes = [
            crate::materials::annotations::ai_context_block(&notes),
            super::comments::generation_block(&ws_notes, &input.content),
        ]
        .into_iter()
        .filter(|b| !b.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
        Ok(Self {
            slot_rules,
            ai_notes,
            ai_notes_count: notes.len() + ws_notes.len(),
            teacher,
            request: input.worksheet.request.trim().to_string(),
            state: state.clone(),
            temperature: BASE_TEMPERATURE + 0.1 * earlier.min(6) as f32,
            input,
            run_no,
            paragraphs,
            objectives,
            candidates,
            hard_share,
            lexicon,
            hold,
        })
    }

    fn db(&self) -> &PgPool {
        &self.state.db
    }

    /// The block of shared context for prompts that predate `{{teacher_objective}}` and
    /// `{{teacher_request}}`: the shared annotations, then the teacher's objectives (when
    /// `with_objectives`: the four steps list them in `{{objectives_summary}}` instead) and
    /// request, each under its own heading ("" when there is nothing).
    fn notes_for_ai(&self, with_objectives: bool) -> String {
        let mut parts: Vec<String> = Vec::new();
        if !self.ai_notes.is_empty() {
            parts.push(self.ai_notes.clone());
        }
        let objectives = self.teacher.combined();
        if with_objectives && !objectives.is_empty() {
            parts.push(format!("{OBJECTIVE_HEADING}：{objectives}"));
        }
        if !self.request.is_empty() {
            parts.push(format!("{REQUEST_HEADING}：{}", self.request));
        }
        parts.join("\n\n")
    }

    /// The objectives a generated version keeps: what the worksheet has now, with the two
    /// fields added when it does not have them yet.
    fn objectives_for(&self, stored: &[Objective]) -> Vec<Objective> {
        crate::objectives::ensured(stored, &self.input.worksheet.settings)
    }

    fn objective_codes(&self) -> Vec<String> {
        self.objectives.iter().map(|(c, _)| c.clone()).collect()
    }

    /// SIOP E1: what the teacher is told when the exit ticket still leaves objectives unchecked
    /// after the repair call (nothing when it checks them all).
    fn uncovered_notice(&self, items: &[RawExitItem]) -> Option<String> {
        let missing = checks::uncovered_objectives(items, &self.objective_codes());
        if missing.is_empty() {
            return None;
        }
        let names: Vec<String> = self
            .objectives
            .iter()
            .filter(|(code, _)| missing.contains(code))
            .map(|(_, o)| {
                let line = if o.student_facing_en.trim().is_empty() {
                    o.text.trim()
                } else {
                    o.student_facing_en.trim()
                };
                format!("{}「{}」", o.kind.label(), clip_chars(line, 40))
            })
            .collect();
        Some(format!(
            "Exit ticket 沒有檢核到{}。可以在 Exit ticket 加一題，或按它的「重新產生」。",
            names.join("、")
        ))
    }

    /// code → objective id in the worksheet
    fn objective_ids(&self) -> HashMap<String, String> {
        self.objectives
            .iter()
            .map(|(c, o)| (c.clone(), o.id.clone()))
            .collect()
    }

    fn objectives_summary(&self) -> String {
        if self.objectives.is_empty() {
            return "（教師沒有提供目標；objective_refs 請給空陣列）".to_string();
        }
        self.objectives
            .iter()
            .map(|(code, o)| {
                let mut both: Vec<&str> = [o.text.trim(), o.student_facing_en.trim()]
                    .into_iter()
                    .filter(|s| !s.is_empty())
                    .collect();
                both.dedup();
                let both = both.join("｜");
                format!("{code}（{}）：{both}", o.kind.label())
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn common_vars(&self) -> HashMap<&'static str, String> {
        HashMap::from([
            ("school_track", track_label(&self.input.track).to_string()),
            ("class_profile", or_blank(&self.input.class_profile)),
            ("objectives_summary", self.objectives_summary()),
            ("passage_numbered", self.input.worksheet.material.numbered()),
            ("student_task_labels", student_tasks().join("、")),
            ("answer_format_labels", ANSWER_FORMATS.join("、")),
            ("teacher_notes_for_ai", self.notes_for_ai(false)),
        ])
    }

    fn context(&self, extra: &[(&str, String)]) -> Vec<AiContextItem> {
        let mut list = ContextList::default();
        list.add("教材", &self.input.worksheet.material.label())
            .add(&self.input.class_profile_label, &self.input.class_profile);
        if !self.objectives.is_empty() {
            list.add("學習目標", &format!("{} 項", self.objectives.len()));
        }
        if self.ai_notes_count > 0 {
            list.add(
                "教師註記（提供給 AI）",
                &format!("{} 則", self.ai_notes_count),
            );
        }
        list.add(OBJECTIVE_HEADING, &clip_chars(&self.teacher.combined(), 80))
            .add(REQUEST_HEADING, &clip_chars(&self.request, 80));
        for (label, text) in extra {
            list.add(label, text);
        }
        list.into_vec()
    }
}

fn lines_or(lines: Vec<String>, empty: &str) -> String {
    if lines.is_empty() {
        empty.to_string()
    } else {
        lines.join("\n")
    }
}

fn labels_or(labels: &[String], empty: &str) -> String {
    if labels.is_empty() {
        empty.to_string()
    } else {
        labels.join("、")
    }
}

fn nos_label(nos: &[i64]) -> String {
    nos.iter()
        .map(|n| format!("[P{n}]"))
        .collect::<Vec<_>>()
        .join("")
}

// ---------------------------------------------------------------- key-word candidates (from data)

fn sentences(paragraph: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let bytes = paragraph.as_bytes();
    for (i, c) in paragraph.char_indices() {
        if matches!(c, '.' | '!' | '?') {
            let next = i + c.len_utf8();
            // A sentence ends at a mark followed by a space (or a closing quote and a space).
            let mut end = next;
            while end < bytes.len() && matches!(bytes[end], b'"' | b'\'' | b')')
                || paragraph[end..].starts_with('”')
                || paragraph[end..].starts_with('’')
            {
                end += paragraph[end..].chars().next().map_or(1, char::len_utf8);
            }
            if end >= bytes.len() || bytes[end] == b' ' {
                let s = paragraph[start..end].trim();
                if !s.is_empty() {
                    out.push(s);
                }
                start = end;
            }
        }
    }
    let rest = paragraph[start..].trim();
    if !rest.is_empty() {
        out.push(rest);
    }
    out
}

fn words_of(text: &str) -> Vec<&str> {
    text.split(|c: char| !(c.is_ascii_alphabetic() || c == '\'' || c == '’'))
        .map(|w| w.trim_matches(|c| c == '\'' || c == '’'))
        .filter(|w| w.len() >= 2)
        .collect()
}

/// Words the model may pick as key vocabulary, found by the code: with a word list, the listed
/// words of level 3 and above (highest first); without one, the longer words, with no level.
/// The teacher's own key words come first. Also returns the share of listed words above level 4.
pub fn key_word_candidates(
    paragraphs: &[String],
    lexicon: Option<&vocabulary::Lexicon>,
    teacher_words: &[String],
) -> (Vec<Candidate>, Option<f64>) {
    let mut seen: HashSet<String> = HashSet::new();
    let mut found: Vec<Candidate> = Vec::new();
    let mut listed = 0usize;
    let mut hard = 0usize;
    for (pi, paragraph) in paragraphs.iter().enumerate() {
        for sentence in sentences(paragraph) {
            for (wi, surface) in words_of(sentence).into_iter().enumerate() {
                let (word, level) = match lexicon {
                    Some(lex) => {
                        let hit = vocabulary::word_level(lex, &surface.to_lowercase());
                        match (hit.base, hit.level) {
                            (Some(base), Some(level)) => (base, Some(level)),
                            _ => continue,
                        }
                    }
                    None => {
                        // No list: longer words, skipping names (capitalised inside a sentence).
                        let name = wi > 0 && surface.chars().next().is_some_and(char::is_uppercase);
                        if surface.chars().count() < 7 || name {
                            continue;
                        }
                        (surface.to_lowercase(), None)
                    }
                };
                if !seen.insert(word.clone()) {
                    continue;
                }
                if let Some(l) = level {
                    listed += 1;
                    if l > vocabulary::DEFAULT_TARGET_LEVEL {
                        hard += 1;
                    }
                    if l < 3 {
                        continue;
                    }
                }
                found.push(Candidate {
                    word,
                    level,
                    paragraph_no: pi as i64 + 1,
                    sentence: sentence.to_string(),
                });
            }
        }
    }
    // Stable: highest level first, then the order of appearance.
    found.sort_by_key(|c| std::cmp::Reverse(c.level.unwrap_or(0)));
    let mut out: Vec<Candidate> = Vec::new();
    for raw in teacher_words {
        let word = raw.trim();
        if word.is_empty() || out.iter().any(|c| c.word.eq_ignore_ascii_case(word)) {
            continue;
        }
        let level = lexicon.and_then(|lex| vocabulary::word_level(lex, word).level);
        let stem: String = word
            .to_lowercase()
            .chars()
            .take(word.chars().count().saturating_sub(2).max(3))
            .collect();
        let place = paragraphs.iter().enumerate().find_map(|(pi, p)| {
            sentences(p)
                .into_iter()
                .find(|s| s.to_lowercase().contains(&stem))
                .map(|s| (pi as i64 + 1, s.to_string()))
        });
        if let Some((paragraph_no, sentence)) = place {
            out.push(Candidate {
                word: word.to_string(),
                level,
                paragraph_no,
                sentence,
            });
        }
    }
    for c in found {
        if out.len() >= MAX_CANDIDATES {
            break;
        }
        if !out.iter().any(|o| o.word.eq_ignore_ascii_case(&c.word)) {
            out.push(c);
        }
    }
    let share = (lexicon.is_some() && listed > 0).then(|| hard as f64 / listed as f64);
    (out, share)
}

// ---------------------------------------------------------------- one step: call, check, repair once

struct StepCall {
    vars: HashMap<&'static str, String>,
    schema: Value,
    fake_context: Value,
    /// Always set: the gateway hangs on a runaway output without one.
    max_tokens: u32,
}

enum StepResult<T> {
    Shown {
        interaction_id: i64,
        output: T,
    },
    Failed {
        /// `blocked` | `timeout` | `error` | `disabled`
        outcome: &'static str,
        interaction_id: Option<i64>,
        /// Said to the teacher, e.g. 「AI 目前沒有回應」.
        reason: String,
    },
}

fn failed<T>(outcome: &'static str, interaction_id: Option<i64>) -> StepResult<T> {
    let reason = match outcome {
        "timeout" => "AI 目前沒有回應",
        "error" => "AI 暫時無法使用",
        "disabled" => "這個環境沒有開啟 AI 協助",
        _ => MSG_BLOCKED,
    };
    StepResult::Failed {
        outcome,
        interaction_id,
        reason: reason.to_string(),
    }
}

async fn run_step<R, T>(
    run: &Run,
    info: &StepInfo,
    job_id: i64,
    call: StepCall,
    eval: impl Fn(&R) -> Eval<T>,
) -> AppResult<StepResult<T>>
where
    R: DeserializeOwned,
{
    let state = &run.state;
    let db = run.db();
    mark_running(db, job_id, info.phase).await?;
    if !state.ai.enabled() {
        return Ok(failed("disabled", None));
    }
    let prompt = prompt(db, info.key).await?;
    let mut vars = call.vars;
    let mut required = vec![("class_profile", "班級特性")];
    // Shared annotations reach the model even with prompt versions that do not mention them;
    // the block carries its own heading saying they are the teacher's notes.
    if !vars
        .get("teacher_notes_for_ai")
        .is_none_or(|n| n.is_empty())
    {
        required.push(("teacher_notes_for_ai", "參考資料"));
    }
    // A step that asks for picture slots carries their rules (pictures on only).
    if !vars.get("image_slot_rules").is_none_or(|n| n.is_empty()) {
        required.push(("image_slot_rules", "圖片位置"));
    }
    // A call that has the objective variables gets them under their headings whatever its
    // version mentions (the four steps list the objectives in `{{objectives_summary}}`).
    required.extend(crate::objectives::required(&prompt));
    let system = prompts::render(&prompt.system_text, &vars);
    let user = prompts::render_user_with(&prompt, &vars, &required);
    // The material must stay deletable: the logged copy keeps only its length and a hash.
    let passage = vars.get("passage_numbered").cloned().unwrap_or_default();
    vars.insert(
        "passage_numbered",
        prompts::redacted("學習單教材", &passage),
    );
    for key in [
        "blocks_detail",
        "tasks_summary",
        "vocab_candidates",
        "teacher_tasks",
        "done_titles",
        "earlier_questions",
        "questions",
    ] {
        if let Some(v) = vars.get(key).cloned() {
            vars.insert(key, prompts::redacted("由教材衍生的內容", &v));
        }
    }
    let logged_user = prompts::render_user_with(&prompt, &vars, &required);
    let snapshot = json!({
        "inputs": vars,
        "class_context": run.input.class_snapshot,
        "worksheet": {
            "id": run.input.worksheet.id,
            "run_no": run.run_no,
            "paragraph_ids": run.input.worksheet.material.paragraphs.iter()
                .map(|p| p.id.as_str()).collect::<Vec<_>>(),
            "material_source": run.input.worksheet.material.source,
            "settings": run.input.worksheet.settings,
        },
        "prompt": { "key": info.key, "version_no": prompt.version_no,
                    "system": system, "user": logged_user },
        "schema": info.schema_name,
        "request": { "temperature": run.temperature, "max_tokens": call.max_tokens },
    });
    let meta = InteractionMeta {
        teacher_id: run.input.worksheet.teacher_id,
        module: info.module,
        unit_id: run.input.unit_id,
        lesson_id: run.input.worksheet.lesson_id,
        scaffold_id: scaffold_id(db, info.scaffold).await?,
        prompt_version_id: Some(prompt.id),
    };
    let mut req = LlmRequest {
        system,
        user: user.clone(),
        schema_name: info.schema_name,
        schema: call.schema,
        fake_context: call.fake_context,
        attempt: 1,
        max_tokens: Some(call.max_tokens),
        temperature: Some(run.temperature),
    };

    // ---- first call
    let a1 = attempt(state, &meta, &snapshot, &req, None).await?;
    // The first call's cleaned output and report, kept in case the repair call does worse.
    let mut first: Option<(T, Vec<CheckResult>, Vec<String>)> = None;
    let problems: Vec<String>;
    match &a1.response {
        Ok(resp) => match serde_json::from_value::<R>(resp.json.clone()) {
            Ok(raw) => {
                let e = eval(&raw);
                let mut results = vec![CheckResult::pass("schema")];
                results.extend(e.results.clone());
                let passed = !e.needs_repair();
                match (passed, e.output) {
                    (true, Some(output)) => {
                        let report = report_with(&results, json!(e.repairs));
                        finish_output(db, &a1, resp, "shown", report, Vec::new()).await?;
                        return Ok(StepResult::Shown {
                            interaction_id: a1.id,
                            output,
                        });
                    }
                    (_, output) => {
                        let report = base_checks::report(&results);
                        finish_output(db, &a1, resp, "blocked", report, Vec::new()).await?;
                        problems = if e.problems.is_empty() {
                            vec!["上一次沒有交出可用的內容。".to_string()]
                        } else {
                            e.problems
                        };
                        first = output.map(|o| (o, results, e.repairs));
                    }
                }
            }
            Err(e) => {
                let results = [CheckResult::fail("schema", e.to_string())];
                let report = base_checks::report(&results);
                finish_output(db, &a1, resp, "blocked", report, Vec::new()).await?;
                problems = vec!["上一次的輸出不符合指定的 JSON 格式。".to_string()];
            }
        },
        Err(e) => match finish_failure(db, &a1, e).await? {
            Failure::Timeout => return Ok(failed("timeout", Some(a1.id))),
            Failure::Error => return Ok(failed("error", Some(a1.id))),
            Failure::Retryable => {
                problems = vec!["上一次沒有交出可用的輸出。".to_string()];
            }
        },
    }

    // ---- repair call: the same task with what failed, and a changed request
    mark_running(db, job_id, info.repair_phase).await?;
    let note = format!(
        "\n\n【重要：這是第二次請求】上一次的輸出沒有通過平台的檢查。請修正下列問題後，重新輸出完整的 JSON：\n{}",
        problems
            .iter()
            .take(12)
            .map(|p| format!("- {}", clip_chars(p, 300)))
            .collect::<Vec<_>>()
            .join("\n")
    );
    let repair_temperature = run.temperature + REPAIR_TEMPERATURE_STEP;
    req.user = format!("{user}{note}");
    req.attempt = 2;
    req.temperature = Some(repair_temperature);
    let mut snapshot2 = snapshot.clone();
    snapshot2["retry_of"] = json!(a1.id);
    snapshot2["repair_note"] = json!(note);
    snapshot2["request"]["temperature"] = json!(repair_temperature);
    let a2 = attempt(state, &meta, &snapshot2, &req, Some(a1.id)).await?;
    let mut outcome = "blocked";
    match &a2.response {
        Ok(resp) => match serde_json::from_value::<R>(resp.json.clone()) {
            Ok(raw) => {
                let e = eval(&raw);
                let mut results = vec![CheckResult::pass("schema")];
                results.extend(e.results.clone());
                if let Some(output) = e.output {
                    // What still failed has been taken out by the code; that is recorded.
                    let report = report_with(&results, json!(e.repairs));
                    finish_output(db, &a2, resp, "shown", report, Vec::new()).await?;
                    return Ok(StepResult::Shown {
                        interaction_id: a2.id,
                        output,
                    });
                }
                let report = base_checks::report(&results);
                finish_output(db, &a2, resp, "blocked", report, Vec::new()).await?;
            }
            Err(e) => {
                let results = [CheckResult::fail("schema", e.to_string())];
                let report = base_checks::report(&results);
                finish_output(db, &a2, resp, "blocked", report, Vec::new()).await?;
            }
        },
        Err(e) => match finish_failure(db, &a2, e).await? {
            Failure::Timeout => outcome = "timeout",
            Failure::Error => outcome = "error",
            Failure::Retryable => {}
        },
    }
    // The repair call gave nothing usable: fall back on what was left of the first one.
    if let Some((output, results, repairs)) = first {
        set_final(db, a1.id, "shown", report_with(&results, json!(repairs))).await?;
        return Ok(StepResult::Shown {
            interaction_id: a1.id,
            output,
        });
    }
    Ok(failed(outcome, Some(a2.id)))
}

/// Closes the job of a step and hands back its output (or why there is none).
async fn close<T>(
    run: &Run,
    job_id: i64,
    result: StepResult<T>,
    context: &[AiContextItem],
) -> AppResult<Result<(T, i64), String>> {
    match result {
        StepResult::Shown {
            interaction_id,
            output,
        } => {
            if run.hold == Some(job_id) {
                mark_held(
                    run.db(),
                    job_id,
                    "shown",
                    None,
                    Some(interaction_id),
                    context,
                )
                .await?;
            } else {
                mark_done(
                    run.db(),
                    job_id,
                    "shown",
                    None,
                    Some(interaction_id),
                    context,
                )
                .await?;
            }
            Ok(Ok((output, interaction_id)))
        }
        StepResult::Failed {
            outcome,
            interaction_id,
            reason,
        } => {
            let message = format!("{reason}，這一步沒有產生成功；你可以繼續手動完成。");
            if run.hold == Some(job_id) {
                mark_held(
                    run.db(),
                    job_id,
                    outcome,
                    Some(&message),
                    interaction_id,
                    context,
                )
                .await?;
            } else {
                mark_done(
                    run.db(),
                    job_id,
                    outcome,
                    Some(&message),
                    interaction_id,
                    context,
                )
                .await?;
            }
            Ok(Err(reason))
        }
    }
}

// ---------------------------------------------------------------- the four steps

struct TasksRequest<'a> {
    chosen: &'a Chosen,
    /// A single-block regeneration: its paragraph numbers, its item budget, and the prompts of
    /// the other blocks (not to be repeated).
    only: Option<(&'a [i64], usize, Vec<String>)>,
    /// A single-block regeneration: the block's Q4 stage, which it keeps.
    practice: Option<Practice>,
}

async fn step_tasks(
    run: &Run,
    job_id: i64,
    request: TasksRequest<'_>,
) -> AppResult<Result<(Vec<RawBlock>, i64), String>> {
    let material = &run.input.worksheet.material;
    let s = &request.chosen.settings;
    let budget = match &request.only {
        Some((_, budget, _)) => *budget,
        None => request.chosen.item_budget,
    };
    let seeds: Vec<(Vec<i64>, String, Option<crate::sop::InfoFunction>)> = run
        .input
        .worksheet
        .brief
        .tasks
        .iter()
        .filter(|t| !t.text.trim().is_empty())
        .map(|t| {
            let nos: Vec<i64> = t
                .paragraph_ids
                .iter()
                .filter_map(|id| material.no_of(id))
                .map(|n| n as i64)
                .collect();
            (nos, t.text.trim().to_string(), t.function)
        })
        .filter(|(nos, _, _)| match &request.only {
            Some((only, _, _)) => nos.is_empty() || nos.iter().any(|n| only.contains(n)),
            None => true,
        })
        .collect();
    let only_note = match &request.only {
        None => String::new(),
        Some((nos, budget, others)) => format!(
            "\n\n【只需要一個區塊】這次只重新設計涵蓋 {} 的那一個區塊：blocks 只輸出 1 筆，paragraph_nos 為 {:?}，核心題約 {budget} 題。{}請換一種問法，不要與下列其他區塊的題目重複：\n{}",
            nos_label(nos),
            nos,
            request
                .practice
                .map(|p| format!("這個區塊的 practice 是「{}」，作答指示照這個階段寫。", p.label()))
                .unwrap_or_default(),
            lines_or(
                others.iter().map(|p| format!("- {}", clip_chars(p, 200))).collect(),
                "（沒有其他區塊）"
            )
        ),
    };
    let mut vars = run.common_vars();
    vars.extend([
        ("material_types", labels_or(&s.material_types, "（未指定）")),
        (
            "minutes",
            match s.minutes {
                Some(m) => format!("{m} 分鐘"),
                None => "未指定（依教材份量判斷）".to_string(),
            },
        ),
        ("item_budget", budget.to_string()),
        (
            "selected_student_tasks",
            labels_or(
                &run.input.worksheet.settings.student_tasks,
                "（沒有勾選，請依教材判斷）",
            ),
        ),
        (
            "selected_answer_formats",
            labels_or(
                &run.input.worksheet.settings.answer_formats,
                "（沒有勾選，請依教材判斷）",
            ),
        ),
        (
            "teacher_tasks",
            lines_or(
                seeds
                    .iter()
                    .map(|(nos, text, function)| {
                        format!(
                            "- {}{}{}",
                            nos_label(nos),
                            function
                                .map(|f| format!("（{}）", info_function_label(f)))
                                .unwrap_or_default(),
                            text
                        )
                    })
                    .collect(),
                "（沒有）",
            ),
        ),
        ("only_block", only_note),
    ]);
    let only_nos: Option<Vec<i64>> = request.only.as_ref().map(|(nos, _, _)| nos.to_vec());
    let mut schema = schemas::tasks_schema();
    if !run.slot_rules.is_empty() {
        vars.insert("image_slot_rules", run.slot_rules.clone());
        if let Some(block) = schema["properties"]["blocks"].get_mut("items") {
            super::images::add_slots_property(
                block,
                Some("items"),
                super::images::MAX_SLOTS_PER_TASK_BLOCK,
            );
        }
    }
    let fake_context = json!({
        "task": TASKS.key,
        "image_slots": if run.slot_rules.is_empty() { 0 } else { super::images::MAX_SLOTS_PER_TASK_BLOCK },
        "paragraphs": run.paragraphs.iter().enumerate()
            .map(|(i, text)| json!({ "no": i + 1, "text": text })).collect::<Vec<_>>(),
        "item_budget": budget,
        "selected_student_tasks": run.input.worksheet.settings.student_tasks,
        "selected_answer_formats": run.input.worksheet.settings.answer_formats,
        "objective_ids": run.objective_codes(),
        "only": only_nos,
        "run_no": run.run_no,
    });
    let context = run.context(&[
        (
            "學生任務",
            labels_or(&run.input.worksheet.settings.student_tasks, ""),
        ),
        (
            "作答方式",
            labels_or(&run.input.worksheet.settings.answer_formats, ""),
        ),
        (
            "預計完成時間",
            s.minutes.map(|m| format!("{m} 分鐘")).unwrap_or_default(),
        ),
        (
            "課例裡已寫的任務",
            if seeds.is_empty() {
                String::new()
            } else {
                format!("{} 項", seeds.len())
            },
        ),
    ]);
    let codes = run.objective_codes();
    let result = run_step(
        run,
        &TASKS,
        job_id,
        StepCall {
            vars,
            schema,
            fake_context,
            max_tokens: tasks_max_tokens(only_nos.as_ref().map_or(run.paragraphs.len(), Vec::len)),
        },
        |raw: &RawTasks| {
            evaluate_tasks(
                raw,
                &TasksCtx {
                    paragraphs: &run.paragraphs,
                    objective_ids: &codes,
                    item_budget: budget,
                    only: only_nos.as_deref(),
                },
            )
        },
    )
    .await?;
    close(run, job_id, result, &context).await
}

fn tasks_summary(blocks: &[RawBlock]) -> String {
    lines_or(
        blocks
            .iter()
            .map(|b| {
                format!(
                    "{} {}（{}）：{}",
                    b.block_id,
                    nos_label(&b.paragraph_nos),
                    b.info_function,
                    b.items
                        .iter()
                        .map(|i| clip_chars(&i.prompt_en, 160))
                        .collect::<Vec<_>>()
                        .join("；")
                )
            })
            .collect(),
        "（主要任務還沒有產生）",
    )
}

async fn step_warmup(
    run: &Run,
    job_id: i64,
    minutes: Option<i32>,
    tasks: &[RawBlock],
) -> AppResult<Result<(CleanWarmup, i64), String>> {
    let brief = &run.input.worksheet.brief;
    let count = vocab_count(minutes);
    let mut vars = run.common_vars();
    vars.extend([
        (
            "vocab_count",
            format!("{}–{count}", count.saturating_sub(2).max(2)),
        ),
        (
            "teacher_warmup",
            lines_or(
                brief
                    .warmup
                    .iter()
                    .filter(|w| !w.trim().is_empty())
                    .map(|w| format!("- {}", w.trim()))
                    .collect(),
                "（沒有）",
            ),
        ),
        (
            "teacher_vocabulary",
            labels_or(
                &brief
                    .vocabulary
                    .iter()
                    .filter(|w| !w.trim().is_empty())
                    .map(|w| w.trim().to_string())
                    .collect::<Vec<_>>(),
                "（沒有）",
            ),
        ),
        ("tasks_summary", tasks_summary(tasks)),
        (
            "vocab_candidates",
            lines_or(
                run.candidates
                    .iter()
                    .map(|c| {
                        format!(
                            "{}｜{}｜P{}",
                            c.word,
                            c.level
                                .map(|l| format!("第 {l} 級"))
                                .unwrap_or_else(|| "未對應到詞彙表".to_string()),
                            c.paragraph_no
                        )
                    })
                    .collect(),
                "（沒有候選字彙；key_vocabulary 請給空陣列）",
            ),
        ),
    ]);
    let fake_context = json!({
        "task": WARMUP.key,
        "title": run.input.worksheet.material.title,
        "vocab_count": count,
        "candidates": run.candidates.iter()
            .map(|c| json!({ "word": c.word, "paragraph_no": c.paragraph_no, "sentence": c.sentence }))
            .collect::<Vec<_>>(),
    });
    let context = run.context(&[(
        "候選關鍵字（由詞彙表比對）",
        if run.candidates.is_empty() {
            String::new()
        } else {
            format!("{} 個", run.candidates.len())
        },
    )]);
    let result = run_step(
        run,
        &WARMUP,
        job_id,
        StepCall {
            vars,
            schema: schemas::warmup_schema(),
            fake_context,
            max_tokens: WARMUP_MAX_TOKENS,
        },
        |raw: &RawWarmup| {
            evaluate_warmup(
                raw,
                &WarmupCtx {
                    paragraphs: &run.paragraphs,
                    candidates: &run.candidates,
                    vocab_count: count,
                },
            )
        },
    )
    .await?;
    close(run, job_id, result, &context).await
}

/// One block as the scaffolds step sees it.
struct ScaffoldTarget {
    target: String,
    detail: String,
    answers: Vec<String>,
}

fn target_of_raw(b: &RawBlock) -> ScaffoldTarget {
    ScaffoldTarget {
        target: b.block_id.clone(),
        detail: format!(
            "[{}] {}（{}；{}；學生任務：{}）\n{}",
            b.block_id,
            b.title_en,
            b.info_function,
            nos_label(&b.paragraph_nos),
            labels_or(&b.student_tasks, "未標示"),
            b.items
                .iter()
                .enumerate()
                .map(|(i, it)| format!(
                    "  {}. ({}) {}\n     參考答案：{}",
                    i + 1,
                    it.answer_format,
                    clip_chars(&it.prompt_en, 300),
                    clip_chars(&it.answer_en, 300)
                ))
                .collect::<Vec<_>>()
                .join("\n")
        ),
        answers: b.items.iter().map(|i| i.answer_en.clone()).collect(),
    }
}

fn target_of_warmup(w: &CleanWarmup) -> ScaffoldTarget {
    ScaffoldTarget {
        target: TARGET_WARMUP.to_string(),
        detail: format!(
            "[{TARGET_WARMUP}] Warm-up\n{}\n  Key words: {}",
            w.activities
                .iter()
                .flat_map(|a| a.prompts.iter())
                .map(|p| format!("  - {}", clip_chars(p, 300)))
                .collect::<Vec<_>>()
                .join("\n"),
            w.words
                .iter()
                .map(|w| w.word.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        answers: Vec::new(),
    }
}

async fn step_scaffolds(
    run: &Run,
    job_id: i64,
    chosen: &Chosen,
    targets: &[ScaffoldTarget],
    all_answers: &[String],
) -> AppResult<Result<(Vec<RawScaffoldSet>, i64), String>> {
    let ticked = assemble::selected_kinds(&chosen.settings);
    let mut vars = run.common_vars();
    vars.extend([
        ("verbal_kinds", VERBAL_KINDS.join("、")),
        ("instructional_kinds", INSTRUCTIONAL_KINDS.join("、")),
        ("procedural_kinds", PROCEDURAL_KINDS.join("、")),
        (
            "selected_supports",
            if ticked.is_empty() {
                "（沒有勾選，由你依任務判斷）".to_string()
            } else {
                ticked.join("、")
            },
        ),
        (
            "blocks_detail",
            targets
                .iter()
                .map(|t| t.detail.clone())
                .collect::<Vec<_>>()
                .join("\n\n"),
        ),
    ]);
    let names: Vec<String> = targets.iter().map(|t| t.target.clone()).collect();
    let fake_context = json!({
        "task": SCAFFOLDS.key,
        "targets": targets.iter()
            .map(|t| json!({ "target": t.target, "answers": t.answers })).collect::<Vec<_>>(),
        "selected_kinds": ticked,
    });
    let context = run.context(&[
        ("產生設定", ticked.join("、")),
        ("要安排鷹架的區塊", format!("{} 個", targets.len())),
    ]);
    let result = run_step(
        run,
        &SCAFFOLDS,
        job_id,
        StepCall {
            vars,
            schema: schemas::scaffolds_schema(),
            fake_context,
            max_tokens: scaffolds_max_tokens(targets.len()),
        },
        |raw: &RawScaffolds| {
            evaluate_scaffolds(
                raw,
                &ScaffoldsCtx {
                    targets: &names,
                    answers: all_answers,
                },
            )
        },
    )
    .await?;
    close(run, job_id, result, &context).await
}

async fn step_exit(
    run: &Run,
    job_id: i64,
    minutes: Option<i32>,
    tasks: &[RawBlock],
    task_prompts: &[String],
) -> AppResult<Result<(Vec<RawExitItem>, i64), String>> {
    let count = exit_count(minutes);
    let mut vars = run.common_vars();
    vars.extend([
        ("exit_count", count.to_string()),
        (
            "exit_minutes",
            if count <= 2 { "3" } else { "5" }.to_string(),
        ),
        (
            "teacher_exit",
            lines_or(
                run.input
                    .worksheet
                    .brief
                    .exit_ticket
                    .iter()
                    .filter(|e| !e.text.trim().is_empty())
                    .map(|e| format!("- {}", e.text.trim()))
                    .collect(),
                "（沒有）",
            ),
        ),
        ("tasks_summary", tasks_summary(tasks)),
    ]);
    let codes = run.objective_codes();
    let fake_context = json!({
        "task": EXIT.key,
        "exit_count": count,
        "objective_ids": codes,
    });
    let context = run.context(&[]);
    let result = run_step(
        run,
        &EXIT,
        job_id,
        StepCall {
            vars,
            schema: schemas::exit_schema(),
            fake_context,
            max_tokens: EXIT_MAX_TOKENS,
        },
        |raw: &RawExit| {
            evaluate_exit(
                raw,
                &ExitCtx {
                    objective_ids: &codes,
                    task_prompts,
                },
            )
        },
    )
    .await?;
    close(run, job_id, result, &context).await
}

// ---------------------------------------------------------------- a whole generation

fn empty_message(what: &str, reason: &str) -> String {
    format!("{what}沒有產生成功（{reason}）。你可以自己寫，或按「重新產生」。")
}

/// Added to a refusal when the material looks like a worksheet: the teacher probably put her
/// worksheet sample where the lesson text goes.
pub const MSG_LOOKS_LIKE_WORKSHEET: &str = "這份看起來是學習單範本：要照它的樣式做新學習單，請用「照我的學習單做」，再另外放這節課的課文。";

/// Whether the worksheet's material looks like a worksheet: the library material's flag when
/// the snapshot came from the library (and the material still exists), otherwise the same rule
/// on the snapshot's paragraphs.
async fn material_looks_like_worksheet(db: &PgPool, worksheet: &Worksheet) -> sqlx::Result<bool> {
    if let Some(id) = worksheet.material.source.material_id {
        let row: Option<(Option<bool>, String)> = sqlx::query_as(&format!(
            "SELECT looks_like_worksheet, text FROM materials WHERE id = $1 AND {}",
            crate::materials::store::READABLE_SQL
        ))
        .bind(id)
        .bind(worksheet.teacher_id)
        .fetch_optional(db)
        .await?;
        if let Some((flag, text)) = row {
            return Ok(flag.unwrap_or_else(|| detect::looks_like_worksheet(&text)));
        }
    }
    let text: Vec<&str> = worksheet
        .material
        .paragraphs
        .iter()
        .map(|p| p.text.as_str())
        .collect();
    Ok(detect::looks_like_worksheet(&text.join("\n")))
}

/// The material must have paragraphs, and at most [`super::MAX_AI_PARAGRAPHS`]. A refused
/// material that looks like a worksheet gets [`MSG_LOOKS_LIKE_WORKSHEET`] appended.
async fn check_material(db: &PgPool, worksheet: &Worksheet) -> AppResult<()> {
    let n = worksheet.material.paragraphs.len();
    let mut message = if n == 0 {
        MSG_NEED_MATERIAL.to_string()
    } else if n > super::MAX_AI_PARAGRAPHS {
        format!(
            "這份教材有 {n} 段，超過 Manna 一次能設計的 {} 段。請先貼上這節課要用的部分。",
            super::MAX_AI_PARAGRAPHS
        )
    } else {
        return Ok(());
    };
    if material_looks_like_worksheet(db, worksheet).await? {
        message.push_str(MSG_LOOKS_LIKE_WORKSHEET);
    }
    Err(AppError::bad_request(message))
}

/// Creates the four jobs of a generation (or joins the run in progress) and starts the work.
pub async fn start_generation(state: &AppState, input: RunInput) -> AppResult<()> {
    check_material(&state.db, &input.worksheet).await?;
    // Reading comprehension and tiered practice are made per Bloom level.
    if input.worksheet.settings.output.by_bloom() {
        return bloom::start_generation(state, input).await;
    }
    // A worksheet made from a template follows it instead of the four steps.
    if let Some(tpl) = template::for_generation(&state.db, &input.worksheet).await? {
        return template::start_generation(state, input, tpl).await;
    }
    let worksheet_id = input.worksheet.id;
    expire_stale(state, worksheet_id).await?;
    let steps = [STEP_TASKS, STEP_WARMUP, STEP_SCAFFOLDS, STEP_EXIT];
    let Some((run_no, ids)) = open_run(
        &state.db,
        worksheet_id,
        input.worksheet.teacher_id,
        &steps,
        None,
    )
    .await?
    else {
        return Ok(());
    };
    let state2 = state.clone();
    spawn_run(state, worksheet_id, run_no, async move {
        let hold = ids.get(STEP_EXIT).copied();
        let run = Run::new(&state2, input, run_no, hold).await?;
        generate(&run, &ids).await
    });
    Ok(())
}

async fn generate(run: &Run, jobs: &HashMap<&'static str, i64>) -> AppResult<()> {
    let db = run.db();
    let ws = &run.input.worksheet;
    let material = &ws.material;
    let mut chosen = resolve(
        &ws.settings,
        material,
        !run.objectives.is_empty(),
        &run.input.class_profile,
        run.hard_share,
    );
    let levels = chosen.settings.levels.clone();
    let ticked = assemble::selected_kinds(&chosen.settings);
    let objective_ids = run.objective_ids();
    let mut notices: Vec<String> = Vec::new();

    // ---- (c) tasks
    let tasks = step_tasks(
        run,
        jobs[STEP_TASKS],
        TasksRequest {
            chosen: &chosen,
            only: None,
            practice: None,
        },
    )
    .await?;
    let raw_blocks: Vec<RawBlock> = tasks.as_ref().map(|(b, _)| b.clone()).unwrap_or_default();
    if !raw_blocks.is_empty() {
        // What Manna decided where the teacher left a block empty is reported back (spec §4.6).
        if chosen.auto.iter().any(|a| a == "student_tasks") {
            chosen.settings.student_tasks = student_tasks()
                .into_iter()
                .filter(|l| {
                    raw_blocks
                        .iter()
                        .any(|b| b.student_tasks.iter().any(|t| t == l))
                })
                .map(str::to_string)
                .collect();
        }
        if chosen.auto.iter().any(|a| a == "answer_formats") {
            chosen.settings.answer_formats = ANSWER_FORMATS
                .into_iter()
                .filter(|l| {
                    raw_blocks
                        .iter()
                        .flat_map(|b| &b.items)
                        .any(|i| i.answer_format == *l)
                })
                .map(str::to_string)
                .collect();
        }
        let missing = checks::uncovered_paragraphs(&raw_blocks, run.paragraphs.len());
        if !missing.is_empty() {
            notices.push(format!(
                "第 {} 段沒有對應的任務，可以自己加一個區塊。",
                missing
                    .iter()
                    .map(i64::to_string)
                    .collect::<Vec<_>>()
                    .join("、")
            ));
        }
    }
    let task_prompts: Vec<String> = raw_blocks
        .iter()
        .flat_map(|b| b.items.iter().map(|i| i.prompt_en.clone()))
        .collect();
    let all_answers: Vec<String> = raw_blocks
        .iter()
        .flat_map(|b| b.items.iter().map(|i| i.answer_en.clone()))
        .collect();

    // ---- (d) warm-up
    let warmup = if chosen.settings.wants(GEN_WARMUP) {
        Some(step_warmup(run, jobs[STEP_WARMUP], chosen.settings.minutes, &raw_blocks).await?)
    } else {
        mark_done(
            db,
            jobs[STEP_WARMUP],
            "skipped",
            Some("沒有勾選「加入暖身活動」。"),
            None,
            &[],
        )
        .await?;
        None
    };

    // ---- (e) scaffolds
    let mut targets: Vec<ScaffoldTarget> = Vec::new();
    if let Some(Ok((w, _))) = &warmup {
        targets.push(target_of_warmup(w));
    }
    targets.extend(raw_blocks.iter().map(target_of_raw));
    let scaffold_sets: Vec<RawScaffoldSet> = if targets.is_empty() {
        mark_done(
            db,
            jobs[STEP_SCAFFOLDS],
            "skipped",
            Some("沒有可以安排鷹架的區塊。"),
            None,
            &[],
        )
        .await?;
        Vec::new()
    } else {
        match step_scaffolds(run, jobs[STEP_SCAFFOLDS], &chosen, &targets, &all_answers).await? {
            Ok((sets, _)) => sets,
            Err(reason) => {
                notices.push(format!(
                    "鷹架沒有產生成功（{reason}）；目前每個區塊只有程式自動加入的分步指示，可以自己補上或重新產生區塊。"
                ));
                Vec::new()
            }
        }
    };
    let set_for = |target: &str| scaffold_sets.iter().find(|s| s.target == target);

    // ---- (f) exit ticket
    let exit = step_exit(
        run,
        jobs[STEP_EXIT],
        chosen.settings.minutes,
        &raw_blocks,
        &task_prompts,
    )
    .await?;

    if let Ok((items, _)) = &exit {
        notices.extend(run.uncovered_notice(items));
    }

    // ---- assembly, in code: warm-up first, activity blocks in paragraph order, exit ticket last
    let mut blocks: Vec<Block> = Vec::new();
    match &warmup {
        Some(Ok((w, _))) => {
            let mut block = assemble::warmup_block(w, material, &levels);
            assemble::attach_scaffolds(&mut block, set_for(TARGET_WARMUP), &levels, &ticked);
            blocks.push(block);
        }
        Some(Err(reason)) => blocks.push(assemble::placeholder(
            BlockKind::Warmup,
            WARMUP_ID,
            &empty_message("暖身", reason),
            material,
        )),
        None => {}
    }
    match &tasks {
        Ok(_) => {
            let mut used_seed_blocks: HashSet<String> = HashSet::new();
            for (i, raw) in raw_blocks.iter().enumerate() {
                let id = format!("r{}b{}", run.run_no, i + 1);
                let mut block =
                    assemble::activity_block(&id, raw, material, &levels, &objective_ids);
                block.sop_block_id = sop_block_for(ws, &block.paragraph_ids, &mut used_seed_blocks);
                assemble::attach_scaffolds(&mut block, set_for(&raw.block_id), &levels, &ticked);
                blocks.push(block);
            }
        }
        Err(reason) => blocks.push(assemble::placeholder(
            BlockKind::Activity,
            &format!("r{}b1", run.run_no),
            &empty_message("主要任務", reason),
            material,
        )),
    }
    match &exit {
        Ok((items, _)) => {
            let mut block = assemble::exit_block(items, &levels, &objective_ids);
            assemble::attach_scaffolds(&mut block, None, &levels, &ticked);
            blocks.push(block);
        }
        Err(reason) => blocks.push(assemble::placeholder(
            BlockKind::ExitTicket,
            EXIT_ID,
            &empty_message("Exit ticket", reason),
            material,
        )),
    }

    // Objectives are the teacher's: take them as they are now, not as they were at the start.
    let latest = store::latest(db, ws.id).await?;
    let mut content = Content {
        objectives: run.objectives_for(&latest.content.objectives),
        show_objectives: chosen.settings.wants(GEN_OBJECTIVES),
        blocks,
        levels,
        chosen: Some(chosen),
        notices,
    };
    // V2: the words of the instructions, with Chinese hints, in the vocabulary section.
    super::task_words::refresh(&mut content, run.lexicon.as_ref());
    content.normalise().map_err(|e| {
        AppError::Internal(anyhow::anyhow!(
            "assembled worksheet failed validation: {e}"
        ))
    })?;
    let saved = store::save_content(db, ws.id, &content, "generate").await?;
    store::set_generated(db, ws.id).await?;
    // The blocks passed their checks and are saved: their pictures are made in the background.
    super::images::enqueue_logged(&run.state, ws.id, ws.teacher_id, &content).await;
    log_usage(
        db,
        ws.teacher_id,
        "worksheet_generated",
        json!({
            "worksheet_id": ws.id, "run_no": run.run_no, "version_no": saved.version_no,
            "blocks": content.blocks.len(), "levels": content.levels,
            "auto": content.chosen.as_ref().map(|c| c.auto.clone()),
            "steps": {
                "tasks": tasks.is_ok(), "warmup": warmup.as_ref().map(|w| w.is_ok()),
                "scaffolds": !scaffold_sets.is_empty(), "exit_ticket": exit.is_ok(),
            },
        }),
    )
    .await?;
    Ok(())
}

/// The SOP task block a generated block corresponds to: the first seed block with the same set
/// of paragraphs that is not taken yet.
fn sop_block_for(
    worksheet: &Worksheet,
    paragraph_ids: &[String],
    used: &mut HashSet<String>,
) -> Option<String> {
    let wanted: HashSet<&str> = paragraph_ids.iter().map(String::as_str).collect();
    for seed in &worksheet.brief.tasks {
        let Some(sop_block_id) = &seed.sop_block_id else {
            continue;
        };
        let own: HashSet<&str> = seed.paragraph_ids.iter().map(String::as_str).collect();
        if own == wanted && !used.contains(sop_block_id) {
            used.insert(sop_block_id.clone());
            return Some(sop_block_id.clone());
        }
    }
    None
}

// ---------------------------------------------------------------- one block again

/// Creates the jobs for regenerating one block (or joins the run in progress) and starts the
/// work. The block must exist in the latest version.
pub async fn start_block(state: &AppState, input: RunInput, block_id: &str) -> AppResult<()> {
    check_material(&state.db, &input.worksheet).await?;
    let block = input
        .content
        .block(block_id)
        .cloned()
        .ok_or_else(|| AppError::not_found("找不到這個區塊。"))?;
    // A block generated from a template section re-runs that section's call.
    if let Some((tpl, section)) = template::for_block(&state.db, &input.worksheet, &block).await? {
        return template::start_block(state, input, block, tpl, section).await;
    }
    // A block made for one Bloom level re-runs that level.
    if let Some(level) = bloom::level_of_block(&input, &block) {
        return bloom::start_block(state, input, block, level).await;
    }
    let worksheet_id = input.worksheet.id;
    expire_stale(state, worksheet_id).await?;
    let steps: &[&'static str] = match block.kind {
        BlockKind::Activity => &[STEP_TASKS, STEP_SCAFFOLDS],
        BlockKind::Warmup => &[STEP_WARMUP, STEP_SCAFFOLDS],
        BlockKind::ExitTicket => &[STEP_EXIT],
    };
    let Some((run_no, ids)) = open_run(
        &state.db,
        worksheet_id,
        input.worksheet.teacher_id,
        steps,
        Some(block_id),
    )
    .await?
    else {
        return Ok(());
    };
    let last_step = steps.last().copied();
    let state2 = state.clone();
    spawn_run(state, worksheet_id, run_no, async move {
        let hold = last_step.and_then(|s| ids.get(s)).copied();
        let run = Run::new(&state2, input, run_no, hold).await?;
        regenerate(&run, &ids, block).await
    });
    Ok(())
}

async fn skip(db: &PgPool, job_id: Option<&i64>, message: &str) -> sqlx::Result<()> {
    match job_id {
        Some(id) => mark_done(db, *id, "skipped", Some(message), None, &[]).await,
        None => Ok(()),
    }
}

async fn regenerate(run: &Run, jobs: &HashMap<&'static str, i64>, old: Block) -> AppResult<()> {
    let db = run.db();
    let ws = &run.input.worksheet;
    let material = &ws.material;
    let content = &run.input.content;
    // The settings of the generation this block belongs to; before any, what is ticked now.
    let chosen = content.chosen.clone().unwrap_or_else(|| {
        resolve(
            &ws.settings,
            material,
            !run.objectives.is_empty(),
            &run.input.class_profile,
            run.hard_share,
        )
    });
    let levels: Vec<Level> = if content.levels.is_empty() {
        chosen.settings.levels.clone()
    } else {
        content.levels.clone()
    };
    let ticked = assemble::selected_kinds(&chosen.settings);
    let objective_ids = run.objective_ids();
    let others: Vec<&Block> = content
        .blocks
        .iter()
        .filter(|b| b.id != old.id && b.kind == BlockKind::Activity)
        .collect();
    let other_prompts: Vec<String> = others
        .iter()
        .flat_map(|b| b.items.iter().map(|i| i.prompt.clone()))
        .collect();
    const SKIPPED: &str = "前一步沒有成功，所以沒有執行。";

    let (block, interaction_id) = match old.kind {
        BlockKind::Activity => {
            let mut nos: Vec<i64> = old
                .paragraph_ids
                .iter()
                .filter_map(|id| material.no_of(id))
                .map(|n| n as i64)
                .collect();
            if nos.is_empty() {
                nos = (1..=run.paragraphs.len() as i64).collect();
            }
            let core = old
                .items
                .iter()
                .filter(|i| i.levels.len() == levels.len())
                .count();
            let budget = core
                .clamp(1, 6)
                .max(if old.items.is_empty() { 2 } else { 1 });
            let tasks = step_tasks(
                run,
                jobs[STEP_TASKS],
                TasksRequest {
                    chosen: &chosen,
                    only: Some((&nos, budget, other_prompts.clone())),
                    practice: old.practice,
                },
            )
            .await?;
            let Ok((raw, interaction_id)) = tasks else {
                skip(db, jobs.get(STEP_SCAFFOLDS), SKIPPED).await?;
                return Ok(());
            };
            let Some(mut raw) = raw.into_iter().next() else {
                skip(db, jobs.get(STEP_SCAFFOLDS), SKIPPED).await?;
                return Ok(());
            };
            // Q4: the block keeps its place in 示範 → 獨立應用.
            if let Some(p) = old.practice {
                raw.practice = Some(p.label().to_string());
            }
            let mut block =
                assemble::activity_block(&old.id, &raw, material, &levels, &objective_ids);
            if old.paragraph_ids.is_empty() {
                block.paragraph_ids = Vec::new();
            }
            let answers: Vec<String> = raw.items.iter().map(|i| i.answer_en.clone()).collect();
            let sets = step_scaffolds(
                run,
                jobs[STEP_SCAFFOLDS],
                &chosen,
                &[target_of_raw(&raw)],
                &answers,
            )
            .await?
            .map(|(sets, _)| sets)
            .unwrap_or_default();
            assemble::attach_scaffolds(&mut block, sets.first(), &levels, &ticked);
            (block, interaction_id)
        }
        BlockKind::Warmup => {
            let summary_blocks: Vec<RawBlock> = Vec::new();
            let warmup = step_warmup(
                run,
                jobs[STEP_WARMUP],
                chosen.settings.minutes,
                &summary_blocks,
            )
            .await?;
            let Ok((w, interaction_id)) = warmup else {
                skip(db, jobs.get(STEP_SCAFFOLDS), SKIPPED).await?;
                return Ok(());
            };
            let mut block = assemble::warmup_block(&w, material, &levels);
            let sets = step_scaffolds(
                run,
                jobs[STEP_SCAFFOLDS],
                &chosen,
                &[target_of_warmup(&w)],
                &[],
            )
            .await?
            .map(|(sets, _)| sets)
            .unwrap_or_default();
            assemble::attach_scaffolds(&mut block, sets.first(), &levels, &ticked);
            (block, interaction_id)
        }
        BlockKind::ExitTicket => {
            let exit = step_exit(
                run,
                jobs[STEP_EXIT],
                chosen.settings.minutes,
                &[],
                &other_prompts,
            )
            .await?;
            let Ok((items, interaction_id)) = exit else {
                return Ok(());
            };
            let mut block = assemble::exit_block(&items, &levels, &objective_ids);
            assemble::attach_scaffolds(&mut block, None, &levels, &ticked);
            let notices: Vec<String> = run.uncovered_notice(&items).into_iter().collect();
            replace_block(run, &old, block, interaction_id, notices).await?;
            return Ok(());
        }
    };
    replace_block(run, &old, block, interaction_id, Vec::new()).await
}

/// Puts a regenerated block in place of `old` in the worksheet as it is now, keeping `old` for
/// undo. `notices` are added to the worksheet's.
async fn replace_block(
    run: &Run,
    old: &Block,
    mut block: Block,
    interaction_id: i64,
    notices: Vec<String>,
) -> AppResult<()> {
    let db = run.db();
    let ws = &run.input.worksheet;
    block.id = old.id.clone();
    block.enabled = old.enabled;
    block.sop_block_id = old.sop_block_id.clone();
    // Item and scaffold ids must not collide with the ones of other blocks.
    for (i, item) in block.items.iter_mut().enumerate() {
        item.id = format!("{}-r{}i{}", old.id, run.run_no, i + 1);
    }
    for (i, s) in block.scaffolds.iter_mut().enumerate() {
        s.id = format!("{}-r{}s{}", old.id, run.run_no, i + 1);
    }
    // Picture slots: the new block's (new ids, so they get new pictures) when this run asked
    // for slots; otherwise the old block's stay, with their pictures.
    if run.slot_rules.is_empty() {
        block.image_slots = old.image_slots.clone();
    } else {
        for (i, s) in block.image_slots.iter_mut().enumerate() {
            s.id = format!("{}-r{}p{}", old.id, run.run_no, i + 1);
        }
    }

    // Replace it in the worksheet as it is now; keep what it was, for undo.
    let latest = store::latest(db, ws.id).await?;
    let mut next = latest.content;
    let Some(slot) = next.blocks.iter_mut().find(|b| b.id == old.id) else {
        return Ok(());
    };
    let previous = std::mem::replace(slot, block);
    next.notices.extend(notices);
    // V2: the task words follow the instructions as they are now (the 完整學習單 only).
    super::task_words::refresh(&mut next, run.lexicon.as_ref());
    next.normalise().map_err(|e| {
        AppError::Internal(anyhow::anyhow!("regenerated block failed validation: {e}"))
    })?;
    store::push_history(db, ws.id, &previous, Some(interaction_id)).await?;
    let saved = store::save_content(db, ws.id, &next, "regenerate").await?;
    super::images::enqueue_logged(&run.state, ws.id, ws.teacher_id, &next).await;
    log_usage(
        db,
        ws.teacher_id,
        "worksheet_block_regenerated",
        json!({
            "worksheet_id": ws.id, "block_id": old.id, "kind": old.kind,
            "run_no": run.run_no, "version_no": saved.version_no,
        }),
    )
    .await?;
    Ok(())
}

/// Number of core items in a list of raw blocks (used by tests and logging).
pub fn core_items(blocks: &[RawBlock]) -> usize {
    blocks
        .iter()
        .flat_map(|b| &b.items)
        .filter(|i| i.tier == TIER_CORE)
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budgets_follow_the_size_of_the_request() {
        // Measured: 4 paragraphs ≈ 2,100 tokens, 6 paragraphs ≈ 2,650.
        assert_eq!(tasks_max_tokens(1), 3000);
        assert_eq!(tasks_max_tokens(4), 3900);
        assert_eq!(tasks_max_tokens(6), 5100);
        assert_eq!(tasks_max_tokens(40), TASKS_MAX_TOKENS);
        // Measured: 7 targets ≈ 1,550 tokens.
        assert_eq!(scaffolds_max_tokens(1), 2000);
        assert_eq!(scaffolds_max_tokens(7), 3250);
        assert_eq!(scaffolds_max_tokens(30), SCAFFOLDS_MAX_TOKENS);
    }

    #[test]
    fn sentences_are_split_at_closing_marks() {
        let p =
            "Mrs. Lin put a stand outside. A sign said, “Take one if you need it.” Nobody laughed!";
        let s = sentences(p);
        assert_eq!(s.last().copied(), Some("Nobody laughed!"));
        assert!(s.iter().any(|x| x.ends_with("need it.”")));
        // Every sentence is a verbatim piece of the paragraph.
        assert!(s.iter().all(|x| p.contains(x)));
    }

    #[test]
    fn candidates_without_a_word_list_have_no_level() {
        let paragraphs = vec![
            "Her neighbors laughed at the idea. Something unexpected followed in Taipei."
                .to_string(),
        ];
        let (c, share) = key_word_candidates(&paragraphs, None, &["laughed".to_string()]);
        assert_eq!(share, None);
        assert_eq!(c[0].word, "laughed", "the teacher's word comes first");
        assert!(c.iter().all(|x| x.level.is_none()));
        assert!(c.iter().any(|x| x.word == "unexpected"));
        assert!(c.iter().any(|x| x.word == "neighbors"));
        assert_eq!(
            c.iter().find(|x| x.word == "unexpected").unwrap().sentence,
            "Something unexpected followed in Taipei."
        );
    }
}
