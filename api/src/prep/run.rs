//! The turns of a prep session, run in the background.
//!
//! `worksheet` / `assessment` / `comprehension` / `tiered` create an ordinary worksheet (the
//! same code as `POST /api/worksheets`, with the session's options and request) and start its
//! generation while the request is still there; the turn then stays `running` until the
//! worksheet's jobs are over ([`super::store::refresh`]). A `kit` does the same with a tiered
//! worksheet and also spawns the activity plan; it is closed when both are over
//! ([`super::store::close_kit`]). Every other kind is one spawned task that ends with
//! [`super::store::finish`].
//!
//! The model calls follow the template extraction: `max_tokens` always set, one retry with the
//! temperature raised by 0.4 (the gateway caches identical requests) when the output ran away,
//! could not be read or failed the checks, each attempt its own `ai_interactions` row, and the
//! material only ever logged as its length and a hash.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use super::images;
use super::store::{self, SessionRow, TurnRow};
use super::{
    activity_schema, ask_schema, chip_label, clean_answer, clean_plan, clean_text, is_bare_label,
    is_worksheet_kind, output_of, revise_schema, warmup_ideas, ActivityPlan, Answer, CurriculumRef,
    MaterialRef, RawAnswer, RawPlan, RawRevision, ResourceItem, Resources, Revision,
    TeacherObjectives, ACTIVITY_SCHEMA_NAME, ASK_CURRICULUM_TOP, ASK_SCHEMA_NAME, CHANGES_MAX,
    CHANGE_ITEMS_MAX, CURRICULUM_TOP, KEY_ACTIVITY, KEY_ASK, KEY_REVISE, KIND_ACTIVITY, KIND_ASK,
    KIND_ASSESSMENT, KIND_KIT, KIND_RESOURCES, KIND_REVISE, KIND_WORKSHEET, MATERIALS_TOP,
    REPLY_CHARS, REVISE_SCHEMA_NAME,
};
use crate::ai::pipeline::InteractionMeta;
use crate::ai::LlmRequest;
use crate::checks::{self as base_checks, CheckResult};
use crate::curriculum::{self, CurriculumItem};
use crate::error::{AppError, AppResult};
use crate::materials::store as material_store;
use crate::progress::{self, Sink, Step, Tracker};
use crate::prompts;
use crate::retrieval::{self, CurriculumFilter, KbItem};
use crate::routes::worksheets::{create_worksheet, run_input, CreateOverrides, CreateWorksheet};
use crate::routes::{clip_chars, or_blank, resolved_profile, scaffold_id};
use crate::sop::ai::{attempt, finish_failure, finish_output, prompt, set_final, Failure};
use crate::sop::store as sop_store;
use crate::worksheet::ai::template::marked_passage;
use crate::worksheet::ai::{self as ws_ai};
use crate::worksheet::assemble::project;
use crate::worksheet::bloom::Output;
use crate::worksheet::checks::{evaluate_section, CleanSection, SectionCtx};
use crate::worksheet::images::actions::{self, PictureAction};
use crate::worksheet::material::SOURCE_LIBRARY;
use crate::worksheet::store::{self as ws_store, Worksheet};
use crate::worksheet::template::extract as tpl_extract;
use crate::worksheet::template::store as tpl_store;
use crate::worksheet::template::RawSectionOut;
use crate::worksheet::{
    Audience, Block, Content, Item, Level, Material, Origin, Settings, ANSWER_FORMATS, FMT_BLANK,
    FMT_CHOICE, FMT_MATCHING, FMT_SHORT, FMT_TRUE_FALSE, GEN_OBJECTIVES, MAX_AI_PARAGRAPHS,
    MAX_ID_CHARS,
};
use crate::AppState;

pub const SCAFFOLD_ACTIVITY: &str = "SC-PREP-ACTIVITY";
pub const SCAFFOLD_ASK: &str = "SC-PREP-ASK";
pub const SCAFFOLD_REVISE: &str = "SC-PREP-REVISE";

/// Output budgets. A budget only matters on a runaway; the schema caps keep a normal output
/// well below them.
pub const ACTIVITY_MAX_TOKENS: u32 = 8000;
pub const ASK_MAX_TOKENS: u32 = 3000;
pub const REVISE_MAX_TOKENS: u32 = 6000;
pub(super) const BASE_TEMPERATURE: f32 = 0.2;
const RETRY_TEMPERATURE_STEP: f32 = 0.4;

pub const MSG_NO_KB: &str = "知識庫裡沒有找到相關條目。";
pub const MSG_NO_EXTERNAL: &str = "外部資源（影片、網站）搜尋還沒有開放。";
const MSG_UNREADABLE: &str =
    "Manna 這次沒有交出可以用的內容（輸出不完整或沒有通過檢查）。請按「再試一次」。";
const MSG_TIMEOUT: &str = "AI 目前沒有回應，這次沒有完成。請稍後按「再試一次」。";
const MSG_ERROR: &str = "AI 暫時無法使用，這次沒有完成。請稍後按「再試一次」。";
const MSG_DISABLED: &str = "這個環境沒有開啟 AI 協助。";
const MSG_INTERNAL: &str = "處理這個請求時發生錯誤。請按「再試一次」。";
const MSG_MATERIAL_GONE: &str = "這個備課對話的教材已經刪除了，請開一個新的備課對話。";
const MSG_WORKSHEET_GONE: &str = "要修改的學習單已經刪除了。";
const MSG_WORKSHEET_BUSY: &str =
    "Manna 正在產生這份學習單，這次的修改沒有套用。請等它完成後再說一次。";
const MSG_TEMPLATE_GONE: &str =
    "這次要照著做的版型已經刪除了，所以沒有做學習單。請重新放上你的學習單範本，或選其他版型。";
const MSG_TEMPLATE_FAILED: &str = "Manna 沒有讀出這份學習單範本的設計。請按「再試一次」。";

/// What a turn works with, loaded while the request is still there.
#[derive(Debug, Clone)]
pub struct TurnInput {
    pub turn: TurnRow,
    pub session: SessionRow,
    pub track: String,
    /// `revise`: the worksheet to change.
    pub worksheet_id: Option<i64>,
}

/// Starts a turn already opened with [`store::start_turn`]. A turn that makes a worksheet makes
/// it now (a failure closes the turn as `failed`); a `worksheet` turn whose template is still
/// being extracted waits for it in the background first ([`spawn_after_template`]); a kit also
/// spawns its activity plan (which runs even when the worksheet could not start); the other
/// kinds are spawned.
pub async fn start(state: &AppState, input: TurnInput) -> AppResult<()> {
    let kind = input.turn.kind.as_str();
    if let (KIND_WORKSHEET, Some(template_id)) = (kind, input.turn.template_id) {
        let teacher_id = input.session.teacher_id;
        match tpl_store::find(&state.db, template_id, teacher_id).await? {
            Some(t) if t.status == tpl_store::STATUS_EXTRACTING => {
                spawn_after_template(state, input, template_id);
                return Ok(());
            }
            Some(t) if t.status == tpl_store::STATUS_FAILED => {
                let message = t.error.unwrap_or_else(|| MSG_TEMPLATE_FAILED.to_string());
                store::finish(&state.db, input.turn.id, Err(message)).await?;
                return Ok(());
            }
            _ => {}
        }
    }
    // The steps the turn runs itself are shown from the start (docs/API.md「產生進度」).
    let tracker = open_progress(state, &input).await;
    if kind == KIND_KIT {
        let worksheet_error = start_worksheet(state, &input)
            .await
            .err()
            .map(|e| worksheet_failure(&input, &e));
        spawn_kit(state, input, worksheet_error, tracker);
        return Ok(());
    }
    if is_worksheet_kind(kind) {
        if let Err(e) = start_worksheet(state, &input).await {
            let message = worksheet_failure(&input, &e);
            store::finish(&state.db, input.turn.id, Err(message)).await?;
        }
        return Ok(());
    }
    spawn(state, input, tracker);
    Ok(())
}

// ---------------------------------------------------------------- progress (docs/API.md「產生進度」)

pub const L_READ_WORKSHEET: &str = "讀學習單";
pub const L_REVISE: &str = "照你的指示修改";
pub const L_SAVE: &str = "存成新版本";
const REVISE: &str = "revise";
const SAVE: &str = "save";

/// The steps a turn of `kind` runs itself. `None` for the kinds whose work is their worksheet's
/// generation (the worksheet keeps its own steps) and for the short ones (找教學資源、直接問 AI).
pub fn own_steps(kind: &str) -> Option<Vec<Step>> {
    match kind {
        KIND_ACTIVITY | KIND_KIT => Some(vec![
            Step::new(progress::READ, progress::L_READ),
            Step::new(progress::CURRICULUM, progress::L_CURRICULUM),
            Step::new(progress::PLAN, progress::L_PLAN),
        ]),
        KIND_REVISE => Some(vec![
            Step::new(progress::READ, L_READ_WORKSHEET),
            Step::new(REVISE, L_REVISE),
            Step::new(SAVE, L_SAVE),
        ]),
        _ => None,
    }
}

/// Starts the progress of a turn's own steps (a tracker that writes nothing for the others).
async fn open_progress(state: &AppState, input: &TurnInput) -> Arc<Tracker> {
    match own_steps(&input.turn.kind) {
        Some(steps) => Tracker::open(Sink::Turn(state.db.clone(), input.turn.id), 0, steps).await,
        None => Tracker::off(),
    }
}

/// What the teacher is told when a turn's worksheet could not be made or started.
fn worksheet_failure(input: &TurnInput, e: &AppError) -> String {
    if matches!(e, AppError::Internal(_)) {
        tracing::error!(turn_id = input.turn.id, error = %e, "prep worksheet turn failed");
    }
    teacher_message(e)
}

fn teacher_message(e: &AppError) -> String {
    match e {
        AppError::BadRequest(m)
        | AppError::NotFound(m)
        | AppError::Conflict(m)
        | AppError::Forbidden(m)
        | AppError::Unauthorized(m)
        | AppError::Unavailable(m) => m.clone(),
        AppError::Internal(_) => MSG_INTERNAL.to_string(),
    }
}

type Outcome = Result<(Option<Value>, Option<i64>), String>;

/// The outcome of a spawned piece of work: an error or a panic is a failure the teacher can
/// read.
fn settle(turn_id: i64, joined: Result<AppResult<Outcome>, tokio::task::JoinError>) -> Outcome {
    match joined {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(e)) => {
            tracing::error!(turn_id, error = %e, "prep turn failed");
            Err(teacher_message(&e))
        }
        Err(e) => {
            tracing::error!(turn_id, error = %e, "prep turn panicked");
            Err(MSG_INTERNAL.to_string())
        }
    }
}

fn spawn(state: &AppState, input: TurnInput, tracker: Arc<Tracker>) {
    let state = state.clone();
    let turn_id = input.turn.id;
    tokio::spawn(async move {
        // The work runs in its own task so that a panic there still closes the turn.
        let work = {
            let state = state.clone();
            let tracker = tracker.clone();
            tokio::spawn(async move { run(&state, &input, &tracker).await })
        };
        let outcome = settle(turn_id, work.await);
        tracker
            .finish(outcome.as_ref().err().map(String::as_str))
            .await;
        match store::finish(&state.db, turn_id, outcome).await {
            Ok(Some(check)) => super::selfcheck::spawn(&state, check),
            Ok(None) => {}
            Err(e) => tracing::error!(turn_id, error = %e, "could not close the prep turn"),
        }
    });
}

/// The activity plan of a kit. Its outcome is recorded on the turn, which is closed now when
/// the worksheet could not start (`worksheet_error`) or has already finished generating, and
/// otherwise by the next [`store::refresh`] after the generation.
fn spawn_kit(
    state: &AppState,
    input: TurnInput,
    worksheet_error: Option<String>,
    tracker: Arc<Tracker>,
) {
    let state = state.clone();
    let turn_id = input.turn.id;
    tokio::spawn(async move {
        let work = {
            let state = state.clone();
            let tracker = tracker.clone();
            tokio::spawn(async move { activity(&state, &input, &tracker).await })
        };
        let outcome = settle(turn_id, work.await);
        tracker
            .finish(outcome.as_ref().err().map(String::as_str))
            .await;
        let closed = match worksheet_error {
            Some(w) => store::close_kit_without_worksheet(&state.db, turn_id, &outcome, &w)
                .await
                .map_err(AppError::from),
            None => match store::record_plan(&state.db, turn_id, &outcome).await {
                Ok(()) => store::close_kit(&state, turn_id).await,
                Err(e) => Err(e.into()),
            },
        };
        if let Err(e) = closed {
            tracing::error!(turn_id, error = %e, "could not record the kit's activity plan");
        }
    });
}

/// How long a turn waits between two looks at the template it waits for.
const TEMPLATE_POLL: std::time::Duration = std::time::Duration::from_millis(250);

enum Waited {
    Ready,
    /// The extraction failed (or the template is gone): why, in Chinese.
    Failed(String),
    /// The turn is no longer running (its session was deleted, or it went stale).
    Abandoned,
}

/// Waits until the template is no longer `extracting`. An extraction that died with the server
/// is reported as failed by `expire_stale`, so the wait always ends.
async fn wait_for_template(
    state: &AppState,
    turn_id: i64,
    template_id: i64,
    teacher_id: i64,
) -> AppResult<Waited> {
    let db = &state.db;
    loop {
        let status: Option<String> =
            sqlx::query_scalar("SELECT status FROM prep_turns WHERE id = $1")
                .bind(turn_id)
                .fetch_optional(db)
                .await?;
        if status.as_deref() != Some(super::STATUS_RUNNING) {
            return Ok(Waited::Abandoned);
        }
        tpl_store::expire_stale(db, teacher_id, tpl_extract::stale_after(state)).await?;
        let Some(t) = tpl_store::find(db, template_id, teacher_id).await? else {
            return Ok(Waited::Failed(MSG_TEMPLATE_GONE.to_string()));
        };
        match t.status.as_str() {
            tpl_store::STATUS_READY => return Ok(Waited::Ready),
            tpl_store::STATUS_FAILED => {
                return Ok(Waited::Failed(
                    t.error.unwrap_or_else(|| MSG_TEMPLATE_FAILED.to_string()),
                ))
            }
            _ => tokio::time::sleep(TEMPLATE_POLL).await,
        }
    }
}

/// A `worksheet` turn whose template is being extracted: it stays `running` (without a
/// worksheet) until the template is ready, then makes and starts the worksheet as usual; when
/// the extraction fails, the turn fails with the template's error.
fn spawn_after_template(state: &AppState, input: TurnInput, template_id: i64) {
    let state = state.clone();
    let turn_id = input.turn.id;
    tokio::spawn(async move {
        let work = {
            let state = state.clone();
            tokio::spawn(async move {
                let teacher_id = input.session.teacher_id;
                Ok::<_, AppError>(
                    match wait_for_template(&state, turn_id, template_id, teacher_id).await? {
                        Waited::Abandoned => None,
                        Waited::Failed(message) => Some(message),
                        Waited::Ready => start_worksheet(&state, &input)
                            .await
                            .err()
                            .map(|e| worksheet_failure(&input, &e)),
                    },
                )
            })
        };
        let failure = match work.await {
            Ok(Ok(failure)) => failure,
            Ok(Err(e)) => {
                tracing::error!(turn_id, error = %e, "waiting for the template failed");
                Some(teacher_message(&e))
            }
            Err(e) => {
                tracing::error!(turn_id, error = %e, "waiting for the template panicked");
                Some(MSG_INTERNAL.to_string())
            }
        };
        // Otherwise the turn runs on with its worksheet (closed by `store::refresh`).
        if let Some(message) = failure {
            if let Err(e) = store::finish(&state.db, turn_id, Err(message)).await {
                tracing::error!(turn_id, error = %e, "could not close the prep turn");
            }
        }
    });
}

async fn run(state: &AppState, input: &TurnInput, progress: &Arc<Tracker>) -> AppResult<Outcome> {
    match input.turn.kind.as_str() {
        KIND_ACTIVITY => activity(state, input, progress).await,
        KIND_RESOURCES => resources(state, input).await,
        KIND_ASK => ask(state, input).await,
        KIND_REVISE => revise(state, input, progress).await,
        other => Err(AppError::Internal(anyhow::anyhow!(
            "prep turn kind {other} is not run in the background"
        ))),
    }
}

// ---------------------------------------------------------------- shared context

/// The session's material with its paragraphs, if it still exists (the teacher's own or a
/// platform one) and has text.
pub(super) async fn material(
    db: &sqlx::PgPool,
    session: &SessionRow,
) -> AppResult<Option<Material>> {
    let Some(id) = session.material_id else {
        return Ok(None);
    };
    let Ok(row) = material_store::readable(db, id, session.teacher_id).await else {
        return Ok(None);
    };
    if row.text_status != "ready" || row.paragraphs.0.is_empty() {
        return Ok(None);
    }
    let stored = sop_store::Material::from(row);
    Ok(Some(
        Material::from_library(SOURCE_LIBRARY, None, &stored).0,
    ))
}

fn material_title(m: &Material) -> String {
    if m.title.trim().is_empty() {
        "未命名教材".to_string()
    } else {
        m.title.trim().to_string()
    }
}

/// `[第 n 段]` paragraphs, at most [`MAX_AI_PARAGRAPHS`] of them.
pub(super) fn passage(m: &Material) -> String {
    let n = m.paragraphs.len();
    if n <= MAX_AI_PARAGRAPHS {
        return marked_passage(m);
    }
    let mut shown = m.clone();
    shown.paragraphs.truncate(MAX_AI_PARAGRAPHS);
    format!(
        "{}\n\n（教材共 {n} 段，這裡只列出前 {MAX_AI_PARAGRAPHS} 段。）",
        marked_passage(&shown)
    )
}

pub(super) async fn class_profile(
    state: &AppState,
    session: &SessionRow,
) -> AppResult<(String, Value)> {
    let profile =
        match resolved_profile(&state.db, session.teacher_id, session.class_id, "", "").await {
            Ok(p) => p,
            // A class deleted meanwhile: no class context.
            Err(AppError::NotFound(_)) => {
                resolved_profile(&state.db, session.teacher_id, None, "", "").await?
            }
            Err(e) => return Err(e),
        };
    Ok((profile.text.clone(), profile.snapshot()))
}

fn curriculum_ref(i: &CurriculumItem) -> CurriculumRef {
    let text = if i.content.trim().is_empty() {
        i.title.trim()
    } else {
        i.content.trim()
    };
    CurriculumRef {
        code: i.item_code.clone(),
        text: clip_chars(text, 300),
    }
}

fn format_curriculum(items: &[CurriculumItem], empty: &str) -> String {
    if items.is_empty() {
        return empty.to_string();
    }
    items
        .iter()
        .map(|i| {
            format!(
                "{}｜{}｜{}",
                i.item_code,
                i.title,
                clip_chars(&i.content, 400)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Material title and its first two paragraphs: the query for curriculum retrieval.
fn material_query(m: &Material) -> String {
    let mut parts = vec![m.title.clone()];
    parts.extend(m.paragraphs.iter().take(2).map(|p| p.text.clone()));
    clip_chars(&parts.join("\n"), 4000)
}

/// What is logged of a query that contains material text: its length and a hash.
fn logged_material_query(query: &str) -> String {
    clip_chars(&prompts::redacted("備課對話教材", query), 2000)
}

// ---------------------------------------------------------------- one model call, retried once

pub(crate) struct Call {
    pub(crate) key: &'static str,
    pub(crate) scaffold: &'static str,
    pub(crate) schema_name: &'static str,
    pub(crate) schema: Value,
    pub(crate) vars: HashMap<&'static str, String>,
    /// Placeholders that must reach the model even with a prompt version that lacks them.
    pub(crate) required: Vec<(&'static str, &'static str)>,
    /// Variables logged only as their length and a hash.
    pub(crate) redact: Vec<(&'static str, &'static str)>,
    pub(crate) fake_context: Value,
    pub(crate) max_tokens: u32,
    /// The first attempt's temperature (the retry adds 0.4).
    pub(crate) temperature: f32,
    /// Logged with every attempt (ids, never text).
    pub(crate) trace: Value,
    /// The step of a run's progress the call is (its retry is said there as 「重試中」).
    pub(crate) progress: Option<(Arc<Tracker>, &'static str)>,
    /// Pictures the teacher attached; sent after the user message's text.
    pub(crate) images: Vec<crate::ai::LlmImage>,
}

/// The output of an attempt after the server's checks: what can be used, and what to tell the
/// model in the retry.
pub(crate) struct Checked<T> {
    pub(crate) output: Option<T>,
    pub(crate) problems: Vec<String>,
}

/// One call and, when the output ran away, could not be read or has problems, one retry with
/// a higher temperature and the problems listed. The retry's usable output wins; when it has
/// none, the first attempt's usable output (if any) is used. `Ok(Err(message))`: nothing usable.
pub(crate) async fn call<R, T>(
    state: &AppState,
    teacher_id: i64,
    c: Call,
    check: impl Fn(&R) -> Checked<T>,
    on_first: impl AsyncFnOnce(i64) -> AppResult<()>,
) -> AppResult<Result<(T, i64), String>>
where
    R: DeserializeOwned,
{
    let db = &state.db;
    if !state.ai.enabled() {
        return Ok(Err(MSG_DISABLED.to_string()));
    }
    let prompt = prompt(db, c.key).await?;
    // The teacher's objectives reach every version: the two fields, or one combined line for a
    // version from before them (nothing for a call without them).
    let mut required = c.required.clone();
    required.extend(crate::objectives::required(&prompt));
    let system = prompts::render(&prompt.system_text, &c.vars);
    let user = prompts::render_user_with(&prompt, &c.vars, &required);
    let mut logged = c.vars.clone();
    for (name, label) in &c.redact {
        if let Some(v) = logged.get(name).cloned() {
            logged.insert(name, prompts::redacted(label, &v));
        }
    }
    let logged_system = prompts::render(&prompt.system_text, &logged);
    let logged_user = prompts::render_user_with(&prompt, &logged, &required);
    let meta = InteractionMeta {
        teacher_id,
        module: c.key,
        unit_id: None,
        lesson_id: None,
        scaffold_id: scaffold_id(db, c.scaffold).await?,
        prompt_version_id: Some(prompt.id),
    };
    let mut retry_of: Option<i64> = None;
    let mut note = String::new();
    let mut first: Option<(T, i64, Value)> = None;
    let mut on_first = Some(on_first);
    for attempt_no in 1..=2 {
        if attempt_no == 2 {
            if let Some((tracker, key)) = &c.progress {
                tracker.note(key, Some(progress::NOTE_RETRY)).await;
            }
        }
        let temperature = c.temperature + RETRY_TEMPERATURE_STEP * (attempt_no - 1) as f32;
        let snapshot = json!({
            "inputs": logged,
            "prep": c.trace,
            "prompt": { "key": c.key, "version_no": prompt.version_no,
                        "system": logged_system, "user": format!("{logged_user}{note}") },
            "schema": c.schema_name,
            "request": { "temperature": temperature, "max_tokens": c.max_tokens },
            "retry_of": retry_of,
        });
        let req = LlmRequest {
            system: system.clone(),
            user: format!("{user}{note}"),
            schema_name: c.schema_name,
            schema: c.schema.clone(),
            fake_context: c.fake_context.clone(),
            attempt: attempt_no,
            max_tokens: Some(c.max_tokens),
            temperature: Some(temperature),
            images: c.images.clone(),
        };
        let a = attempt(state, &meta, &snapshot, &req, retry_of).await?;
        if let Some(f) = on_first.take() {
            f(a.id).await?;
        }
        retry_of = Some(a.id);
        let problems: Vec<String> = match &a.response {
            Ok(resp) => match serde_json::from_value::<R>(resp.json.clone()) {
                Ok(raw) => {
                    let checked = check(&raw);
                    let passed = checked.problems.is_empty();
                    let results = if passed {
                        vec![CheckResult::pass("schema"), CheckResult::pass("checks")]
                    } else {
                        vec![
                            CheckResult::pass("schema"),
                            CheckResult::fail("checks", checked.problems.join("；")),
                        ]
                    };
                    let report = base_checks::report(&results);
                    match checked.output {
                        // A usable output from the retry is what the teacher gets, problems and
                        // all (the parts that failed were taken out by the check).
                        Some(output) if passed || attempt_no == 2 => {
                            finish_output(db, &a, resp, "shown", report, Vec::new()).await?;
                            return Ok(Ok((output, a.id)));
                        }
                        Some(output) => {
                            finish_output(db, &a, resp, "blocked", report.clone(), Vec::new())
                                .await?;
                            first = Some((output, a.id, report));
                        }
                        None => {
                            finish_output(db, &a, resp, "blocked", report, Vec::new()).await?;
                        }
                    }
                    if checked.problems.is_empty() {
                        vec!["上一次沒有交出可用的內容。".to_string()]
                    } else {
                        checked.problems
                    }
                }
                Err(e) => {
                    let report = base_checks::report(&[CheckResult::fail("schema", e.to_string())]);
                    finish_output(db, &a, resp, "blocked", report, Vec::new()).await?;
                    vec!["上一次的輸出不符合指定的 JSON 格式。".to_string()]
                }
            },
            Err(e) => match finish_failure(db, &a, e).await? {
                // The retry did not answer: what was usable in the first attempt is kept.
                Failure::Timeout | Failure::Error if first.is_some() => break,
                Failure::Timeout => return Ok(Err(MSG_TIMEOUT.to_string())),
                Failure::Error => {
                    let message = if matches!(e, crate::ai::ProviderError::Disabled) {
                        MSG_DISABLED
                    } else {
                        MSG_ERROR
                    };
                    return Ok(Err(message.to_string()));
                }
                Failure::Retryable => vec!["上一次沒有交出完整的輸出。".to_string()],
            },
        };
        note = format!(
            "\n\n【重要：這是第二次請求】上一次的輸出沒有通過平台的檢查。請修正下列問題後，重新輸出完整的 JSON：\n{}",
            problems
                .iter()
                .take(12)
                .map(|p| format!("- {}", clip_chars(p, 300)))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
    if let Some((output, id, report)) = first {
        set_final(db, id, "shown", report).await?;
        return Ok(Ok((output, id)));
    }
    Ok(Err(MSG_UNREADABLE.to_string()))
}

pub(crate) async fn nothing(_: i64) -> AppResult<()> {
    Ok(())
}

// ---------------------------------------------------------------- 規劃教學活動

async fn activity(
    state: &AppState,
    input: &TurnInput,
    progress: &Arc<Tracker>,
) -> AppResult<Outcome> {
    let db = &state.db;
    let session = &input.session;
    let Some(m) = material(db, session).await? else {
        return Ok(Err(MSG_MATERIAL_GONE.to_string()));
    };
    let (profile, class_snapshot) = class_profile(state, session).await?;
    progress.done(progress::READ).await;
    // The same retrieval as the SOP objectives, over the material.
    progress.start(progress::CURRICULUM).await;
    let query = clip_chars(&format!("{}\n{}", m.title, m.text()), 4000);
    let (items, trace) = curriculum::items_for_track_hybrid(
        db,
        &state.embedder,
        &input.track,
        &query,
        CURRICULUM_TOP,
    )
    .await?;
    progress.done(progress::CURRICULUM).await;
    let candidates: Vec<CurriculumRef> = items.iter().map(curriculum_ref).collect();
    let codes: Vec<String> = candidates.iter().map(|c| c.code.clone()).collect();
    let teacher = TeacherObjectives::of(&session.options.0);
    let request = teacher_request(db, input).await?;
    let mut vars: HashMap<&'static str, String> = HashMap::from([
        ("class_profile", or_blank(&profile)),
        ("teacher_request", or_none(&request)),
        (
            "curriculum_items",
            format_curriculum(
                &items,
                "（知識庫目前沒有適用的課綱條目；curriculum_codes 一律給空陣列）",
            ),
        ),
        ("material_title", material_title(&m)),
        ("paragraph_count", m.paragraphs.len().to_string()),
        ("passage_numbered", passage(&m)),
    ]);
    // 內容目標 and 語言目標 (their headings are added by `call` for the version in use).
    vars.extend(session.options.0.teacher_objectives().vars());
    let logged_query = logged_material_query(&query);
    let c = Call {
        key: KEY_ACTIVITY,
        scaffold: SCAFFOLD_ACTIVITY,
        schema_name: ACTIVITY_SCHEMA_NAME,
        schema: activity_schema(&codes, &teacher),
        vars,
        required: vec![
            ("class_profile", "班級特性"),
            ("teacher_request", "教師的要求"),
            ("curriculum_items", "可對應的課綱條目（只能從這裡選代碼）"),
        ],
        redact: vec![("passage_numbered", "備課對話教材")],
        fake_context: json!({
            "task": KEY_ACTIVITY,
            "material_title": m.title,
            "paragraphs": m.paragraphs.iter().map(|p| p.text.clone()).collect::<Vec<_>>(),
            "candidates": codes,
            "teacher_content": teacher.content,
            "teacher_language": teacher.language,
        }),
        max_tokens: ACTIVITY_MAX_TOKENS,
        temperature: BASE_TEMPERATURE,
        trace: trace_of(input, Some(&class_snapshot), Some(&codes)),
        progress: Some((progress.clone(), progress::PLAN)),
        images: Vec::new(),
    };
    let track = input.track.clone();
    progress.start(progress::PLAN).await;
    let result = call(
        state,
        session.teacher_id,
        c,
        // A goal group without a usable goal, or a step rule broken, is retried, but the plan
        // stays usable.
        |raw: &RawPlan| match clean_plan(raw, &candidates, &teacher) {
            Ok((plan, problems)) => Checked {
                output: Some(plan),
                problems,
            },
            Err(problem) => Checked {
                output: None,
                problems: vec![problem],
            },
        },
        async |interaction_id| {
            retrieval::log(
                db,
                Some(interaction_id),
                &logged_query,
                &track,
                &codes,
                &trace,
            )
            .await?;
            Ok(())
        },
    )
    .await?;
    match &result {
        Ok(_) => progress.done(progress::PLAN).await,
        Err(message) => progress.fail(progress::PLAN, message).await,
    }
    Ok(result.map(|(plan, id): (ActivityPlan, i64)| (Some(json!(plan)), Some(id))))
}

/// Text shown to the model for an optional teacher field that may be empty.
fn or_none(s: &str) -> String {
    if s.trim().is_empty() {
        "（沒有）".to_string()
    } else {
        s.trim().to_string()
    }
}

/// 「教師的要求」 of a turn that makes something: what the teacher typed for this turn, or else
/// the session's first request. A bare chip or tool label is no request.
async fn teacher_request(db: &sqlx::PgPool, input: &TurnInput) -> AppResult<String> {
    let own = input.turn.request.trim();
    if !own.is_empty() && !is_bare_label(own) {
        return Ok(own.to_string());
    }
    let first: Option<String> = sqlx::query_scalar(
        "SELECT request FROM prep_turns WHERE session_id = $1 ORDER BY seq LIMIT 1",
    )
    .bind(input.session.id)
    .fetch_optional(db)
    .await?;
    Ok(first
        .map(|r| r.trim().to_string())
        .filter(|r| !r.is_empty() && !is_bare_label(r))
        .unwrap_or_default())
}

fn trace_of(input: &TurnInput, class: Option<&Value>, codes: Option<&[String]>) -> Value {
    json!({
        "session_id": input.session.id,
        "turn_id": input.turn.id,
        "kind": input.turn.kind,
        "material_id": input.session.material_id,
        "worksheet_id": input.worksheet_id,
        "class_context": class,
        "curriculum_codes": codes,
    })
}

// ---------------------------------------------------------------- 找教學資源

async fn resources(state: &AppState, input: &TurnInput) -> AppResult<Outcome> {
    let db = &state.db;
    let session = &input.session;
    let m = material(db, session).await?;
    let topic = session.topic.clone().unwrap_or_default();
    let (query, logged_query) = match &m {
        Some(m) => {
            let q = material_query(m);
            let logged = logged_material_query(&q);
            (q, logged)
        }
        None => (topic.clone(), topic.clone()),
    };
    if query.trim().is_empty() {
        return Ok(Err(MSG_MATERIAL_GONE.to_string()));
    }
    let found = retrieval::retrieve_curriculum_hybrid(
        db,
        &state.embedder,
        &input.track,
        &CurriculumFilter::default(),
        &query,
        CURRICULUM_TOP,
    )
    .await?;
    let curriculum: Vec<ResourceItem> = found
        .items
        .iter()
        .map(|s| {
            let r = curriculum_ref(&s.item);
            ResourceItem {
                code: r.code,
                text: r.text,
                school_track: Some(s.item.school_track.clone()).filter(|t| !t.is_empty()),
                similarity: s.similarity,
            }
        })
        .collect();
    let codes: Vec<String> = curriculum.iter().map(|c| c.code.clone()).collect();
    retrieval::log(db, None, &logged_query, &input.track, &codes, &found.trace).await?;
    let materials = own_materials(db, session, &query).await?;
    let note = if curriculum.is_empty() && materials.is_empty() {
        format!("{MSG_NO_KB}{MSG_NO_EXTERNAL}")
    } else {
        MSG_NO_EXTERNAL.to_string()
    };
    let result = Resources {
        curriculum,
        materials,
        note,
    };
    Ok(Ok((Some(json!(result)), None)))
}

/// The teacher's own library materials (我的教材: personal, not worksheet samples) that share
/// words with `query` (keyword ranking), best first, without the session's own.
async fn own_materials(
    db: &sqlx::PgPool,
    session: &SessionRow,
    query: &str,
) -> AppResult<Vec<MaterialRef>> {
    let rows: Vec<(i64, String, String)> = sqlx::query_as(
        "SELECT id, title, left(text, 6000) FROM materials
          WHERE teacher_id = $1 AND scope = 'personal' AND purpose = 'material'
            AND ($2::BIGINT IS NULL OR id <> $2)
          ORDER BY id DESC LIMIT 500",
    )
    .bind(session.teacher_id)
    .bind(session.material_id)
    .fetch_all(db)
    .await?;
    let titles: HashMap<String, (i64, String)> = rows
        .iter()
        .map(|(id, title, _)| (id.to_string(), (*id, title.clone())))
        .collect();
    let items: Vec<KbItem> = rows
        .into_iter()
        .map(|(id, title, text)| KbItem {
            item_code: id.to_string(),
            version: 1,
            category: "material".to_string(),
            school_track: String::new(),
            title,
            content: text,
            source_doc: String::new(),
            source_version: String::new(),
            source_page: String::new(),
            official_code: String::new(),
            item_kind: String::new(),
            strand_name: String::new(),
            stage: String::new(),
            subject: String::new(),
            advanced: false,
            recurring: false,
        })
        .collect();
    // Sharing a couple of common words ("students", "people") is not being related: a material
    // is listed when its title shares a word with the lesson, or when it shares at least 2
    // distinct words and at least a quarter of the shorter side's words.
    let query_words: HashSet<String> = retrieval::tokenize(query).into_iter().collect();
    let related = |item: &KbItem| {
        let words: HashSet<String> =
            retrieval::tokenize(&format!("{} {}", item.title, item.content))
                .into_iter()
                .collect();
        let shared = words.intersection(&query_words).count();
        let shorter = words.len().min(query_words.len());
        let title_shared = retrieval::tokenize(&item.title)
            .iter()
            .any(|w| query_words.contains(w));
        title_shared || (shared >= 2 && shared * 4 >= shorter)
    };
    let items: Vec<KbItem> = items.into_iter().filter(|i| related(i)).collect();
    let mut ranked = retrieval::rank(query, items, MATERIALS_TOP * 4);
    // Best score first, newest first among equals.
    ranked.sort_by(|a, b| {
        b.score.cmp(&a.score).then_with(|| {
            let id = |s: &retrieval::Scored| s.item.item_code.parse::<i64>().unwrap_or(0);
            id(b).cmp(&id(a))
        })
    });
    Ok(ranked
        .into_iter()
        .take(MATERIALS_TOP)
        .filter_map(|s| titles.get(&s.item.item_code).cloned())
        .map(|(id, title)| MaterialRef {
            id,
            title: if title.trim().is_empty() {
                "未命名教材".to_string()
            } else {
                title
            },
        })
        .collect())
}

// ---------------------------------------------------------------- 直接問 AI

/// A worksheet in a line: its block titles.
async fn worksheet_summary(db: &sqlx::PgPool, worksheet_id: Option<i64>) -> String {
    let Some(id) = worksheet_id else {
        return "學習單".to_string();
    };
    let content = ws_store::latest(db, id).await.map(|v| v.content).ok();
    let titles: Vec<String> = content
        .map(|c| {
            c.blocks
                .iter()
                .filter(|b| b.enabled && !b.title.trim().is_empty())
                .map(|b| b.title.trim().to_string())
                .collect()
        })
        .unwrap_or_default();
    if titles.is_empty() {
        "學習單（還沒有區塊）".to_string()
    } else {
        format!("學習單區塊：{}", titles.join("、"))
    }
}

/// What an earlier turn said, in a line (the reply text, a worksheet's block titles, a plan's
/// steps).
async fn turn_summary(db: &sqlx::PgPool, t: &TurnRow) -> AppResult<String> {
    if t.status == super::STATUS_FAILED {
        return Ok("（這一次沒有完成）".to_string());
    }
    if t.status == super::STATUS_RUNNING {
        return Ok("（還在處理中）".to_string());
    }
    let r = t.result.clone().unwrap_or(Value::Null);
    let text = match t.kind.as_str() {
        KIND_KIT => format!(
            "教學套件：{}；{}",
            r["title"].as_str().unwrap_or("教學活動規劃"),
            worksheet_summary(db, t.worksheet_id).await
        ),
        k if is_worksheet_kind(k) => worksheet_summary(db, t.worksheet_id).await,
        KIND_ACTIVITY => {
            let steps: Vec<String> = r["steps"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|s| {
                            format!(
                                "{}：{}",
                                s["phase"].as_str().unwrap_or_default(),
                                s["title"].as_str().unwrap_or_default()
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();
            format!(
                "{}（{}）",
                r["title"].as_str().unwrap_or("教學活動規劃"),
                steps.join("；")
            )
        }
        KIND_RESOURCES => {
            let codes: Vec<&str> = r["curriculum"]
                .as_array()
                .map(|a| a.iter().filter_map(|c| c["code"].as_str()).collect())
                .unwrap_or_default();
            if codes.is_empty() {
                "沒有找到相關的課綱條目".to_string()
            } else {
                format!("找到的課綱條目：{}", codes.join("、"))
            }
        }
        KIND_ASK => r["answer"].as_str().unwrap_or_default().to_string(),
        _ => r["reply"].as_str().unwrap_or_default().to_string(),
    };
    Ok(clip_chars(text.trim(), 300))
}

async fn ask(state: &AppState, input: &TurnInput) -> AppResult<Outcome> {
    let db = &state.db;
    let session = &input.session;
    let question = input.turn.request.trim().to_string();
    let m = material(db, session).await?;
    let (profile, class_snapshot) = class_profile(state, session).await?;
    let found = retrieval::retrieve_curriculum_hybrid(
        db,
        &state.embedder,
        &input.track,
        &CurriculumFilter::default(),
        &question,
        ASK_CURRICULUM_TOP,
    )
    .await?;
    let items: Vec<CurriculumItem> = found.items.iter().map(|s| s.item.clone()).collect();
    let codes: Vec<String> = items.iter().map(|i| i.item_code.clone()).collect();
    let earlier: Vec<TurnRow> = store::turns(db, session.id)
        .await?
        .into_iter()
        .filter(|t| t.seq < input.turn.seq)
        .collect();
    let mut recent = Vec::new();
    for t in earlier.iter().rev().take(3).rev() {
        recent.push(json!({
            "kind": t.kind,
            "request": clip_chars(&t.request, 300),
            "summary": turn_summary(db, t).await?,
        }));
    }
    let recent_text = if recent.is_empty() {
        "（這是這段對話的第一個問題）".to_string()
    } else {
        recent
            .iter()
            .map(|r| {
                format!(
                    "- {}｜教師：{}｜結果：{}",
                    chip_label(r["kind"].as_str().unwrap_or_default()),
                    r["request"].as_str().unwrap_or_default(),
                    r["summary"].as_str().unwrap_or_default()
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let passage_text = match &m {
        Some(m) => format!(
            "{}（共 {} 段）\n\n{}",
            material_title(m),
            m.paragraphs.len(),
            passage(m)
        ),
        None => "（教師沒有提供教材）".to_string(),
    };
    let vars: HashMap<&'static str, String> = HashMap::from([
        ("question", question.clone()),
        ("recent_turns", recent_text),
        ("class_profile", or_blank(&profile)),
        (
            "curriculum_items",
            format_curriculum(&items, "（知識庫裡沒有找到相關的課綱條目）"),
        ),
        ("passage_numbered", passage_text),
    ]);
    let c = Call {
        key: KEY_ASK,
        scaffold: SCAFFOLD_ASK,
        schema_name: ASK_SCHEMA_NAME,
        schema: ask_schema(),
        vars,
        required: vec![
            ("question", "教師的問題"),
            ("class_profile", "班級特性"),
            ("passage_numbered", "教材"),
        ],
        redact: vec![
            ("passage_numbered", "備課對話教材"),
            ("recent_turns", "先前的對話"),
        ],
        fake_context: json!({
            "task": KEY_ASK,
            "question": question,
            "has_material": m.is_some(),
            "recent": recent,
            "candidates": codes,
        }),
        max_tokens: ASK_MAX_TOKENS,
        temperature: BASE_TEMPERATURE,
        trace: trace_of(input, Some(&class_snapshot), Some(&codes)),
        progress: None,
        images: Vec::new(),
    };
    let track = input.track.clone();
    let trace = found.trace.clone();
    let logged_question = clip_chars(&question, 2000);
    let result = call(
        state,
        session.teacher_id,
        c,
        |raw: &RawAnswer| match clean_answer(raw) {
            Ok(a) => Checked {
                output: Some(a),
                problems: Vec::new(),
            },
            Err(p) => Checked {
                output: None,
                problems: vec![p],
            },
        },
        async |interaction_id| {
            retrieval::log(
                db,
                Some(interaction_id),
                &logged_question,
                &track,
                &codes,
                &trace,
            )
            .await?;
            Ok(())
        },
    )
    .await?;
    Ok(result.map(|(a, id): (Answer, i64)| (Some(json!(a)), Some(id))))
}

// ---------------------------------------------------------------- 修改學習單

/// A block's name in the reply: 「B 區」, or its title when it is not lettered.
fn block_name(label: &str, title: &str) -> String {
    if label.is_empty() {
        format!("「{}」", clip_chars(title.trim(), 30))
    } else {
        format!("{label} 區")
    }
}

/// Letters of the blocks as the student sheet of the worksheet's first level shows them.
fn labels(ws: &Worksheet, content: &Content) -> HashMap<String, String> {
    let level = content.levels.first().copied().unwrap_or(Level::Standard);
    project(&ws.title, content, level, Audience::Teacher)
        .blocks
        .into_iter()
        .map(|b| (b.id, b.label))
        .collect()
}

struct Accepted {
    block_id: String,
    section: CleanSection,
}

struct RevisePlan {
    reply: String,
    accepted: Vec<Accepted>,
    /// Blocks whose change did not pass the checks, with the reason.
    dropped: Vec<String>,
    /// The picture actions that passed the checks (applied with the text changes).
    pictures: Vec<PictureAction>,
    /// How many the model wrote that did not (told to the teacher).
    pictures_dropped: usize,
    /// The model wrote some while pictures are off.
    pictures_off: bool,
}

/// Checks every change like a template block (fixed labels, well-formed closed items, no
/// answer in the stem, dictionary syllables, at most 12 items), and the picture actions
/// ([`actions::check`]; a dropped action is not a reason for a second call).
fn check_revision(
    raw: &RawRevision,
    content: &Content,
    labels: &HashMap<String, String>,
    passage: &str,
    pictures_on: bool,
) -> Checked<RevisePlan> {
    let name_of = |id: &str| -> Option<String> {
        labels
            .get(id)
            .filter(|l| !l.is_empty())
            .map(|l| format!("{l} 區"))
    };
    let (pictures, picture_notes) = actions::check(
        &raw.picture_actions,
        &actions::CheckCtx {
            content,
            only_block: None,
            name_of: &name_of,
        },
        pictures_on,
    );
    let mut plan = RevisePlan {
        reply: clean_text(&raw.reply, REPLY_CHARS),
        accepted: Vec::new(),
        dropped: Vec::new(),
        pictures_dropped: if pictures_on {
            raw.picture_actions.len().saturating_sub(pictures.len())
        } else {
            0
        },
        pictures_off: !pictures_on && !raw.picture_actions.is_empty(),
        pictures,
    };
    if !picture_notes.is_empty() {
        tracing::debug!(notes = ?picture_notes, "picture actions were trimmed");
    }
    let mut problems = Vec::new();
    let mut seen = HashSet::new();
    let ctx = SectionCtx {
        passage,
        item_count: CHANGE_ITEMS_MAX,
        needs_external: false,
    };
    for change in raw.changes.iter().take(CHANGES_MAX) {
        let id = change.block_id.trim();
        if !seen.insert(id.to_string()) {
            continue;
        }
        let Some(block) = content.block(id) else {
            problems.push(format!(
                "block_id「{}」不在這份學習單裡。",
                clip_chars(id, 40)
            ));
            continue;
        };
        let label = labels.get(id).cloned().unwrap_or_default();
        let name = block_name(&label, &block.title);
        let section = RawSectionOut {
            title: change.title.clone(),
            instructions: change.instructions.clone(),
            scaffold: String::new(),
            items: change.items.clone(),
        };
        let e = evaluate_section(&section, &ctx);
        match e.output {
            Some(out) if !e.needs_repair() => plan.accepted.push(Accepted {
                block_id: id.to_string(),
                section: out,
            }),
            _ => {
                let failed: Vec<String> = e
                    .results
                    .iter()
                    .filter(|r| !r.passed)
                    .map(|r| r.detail.clone().unwrap_or_else(|| r.name.to_string()))
                    .collect();
                problems.push(format!(
                    "{name}（block_id {id}）：{}",
                    if failed.is_empty() {
                        "沒有通過檢查。".to_string()
                    } else {
                        failed.join("；")
                    }
                ));
                plan.dropped.push(name);
            }
        }
    }
    Checked {
        output: Some(plan),
        problems,
    }
}

/// The worksheet's blocks as the model sees them.
fn blocks_for_model(
    content: &Content,
    labels: &HashMap<String, String>,
    states: &[crate::worksheet::images::SlotState],
) -> Vec<Value> {
    content
        .blocks
        .iter()
        .filter(|b| b.enabled)
        .map(|b| {
            json!({
                "block_id": b.id,
                "label": labels.get(&b.id).cloned().unwrap_or_default(),
                "kind": b.kind,
                "title": b.title,
                "instructions": b.instructions,
                // The pictures the block has (slot ids a picture action may name).
                "pictures": actions::describe(b, states),
                "items": b.items.iter().map(|i| json!({
                    "answer_format": i.answer_format,
                    "prompt": i.prompt,
                    "options": i.options,
                    "answer": i.answer,
                })).collect::<Vec<_>>(),
            })
        })
        .collect()
}

/// An item id unique in the worksheet (≤ 40 characters).
fn item_id(block_id: &str, turn_id: i64, block_no: usize, k: usize) -> String {
    let id = format!("{block_id}-v{turn_id}i{k}");
    if id.chars().count() <= MAX_ID_CHARS {
        id
    } else {
        format!("v{turn_id}b{block_no}i{k}")
    }
}

/// The versions a revised item appears in: those of the item it replaces (same position);
/// otherwise all of the worksheet's (a 差異化學習單 has every question in all three). Always
/// some of the worksheet's versions (all of them when nothing is left).
fn revised_levels(replaced: Option<&Item>, levels: &[Level]) -> Vec<Level> {
    let kept: Vec<Level> = match replaced {
        Some(old) => levels
            .iter()
            .copied()
            .filter(|l| old.levels.contains(l))
            .collect(),
        None => Vec::new(),
    };
    if kept.is_empty() {
        levels.to_vec()
    } else {
        kept
    }
}

/// `old` with the change applied. Like the exit ticket's purpose, an item's Bloom label, hint
/// and versions stay with its position; a new item takes the block's Bloom level (blocks made
/// per Bloom level have one) and every version.
fn revised_block(
    old: &Block,
    out: &CleanSection,
    levels: &[Level],
    turn_id: i64,
    no: usize,
) -> Block {
    let mut block = old.clone();
    if !out.title.trim().is_empty() {
        block.title = out.title.clone();
    }
    block.instructions = out.instructions.clone();
    block.items = out
        .items
        .iter()
        .enumerate()
        .map(|(k, it)| {
            let replaced = old.items.get(k);
            let bloom = match replaced {
                Some(i) => i.bloom,
                None => old.bloom,
            };
            Item {
                id: item_id(&old.id, turn_id, no, k + 1),
                answer_format: Some(it.answer_format.clone()),
                prompt: it.prompt.clone(),
                options: it.options.clone(),
                answer: it.answer.clone(),
                evidence: it.evidence.clone().unwrap_or_default(),
                purpose: replaced.and_then(|i| i.purpose),
                levels: revised_levels(replaced, levels),
                origin: Origin::Ai,
                bloom,
                hint: replaced.map(|i| i.hint.clone()).unwrap_or_default(),
                // A 差異化學習單 question keeps its versions' support only while the question
                // itself is unchanged: a cloze cut from the old answer would not fit a new one.
                tiers: replaced
                    .filter(|i| i.prompt.trim() == it.prompt.trim())
                    .map(|i| i.tiers.clone())
                    .unwrap_or_default(),
                // The same for the teacher's table: a new prompt names its own columns.
                table: replaced
                    .filter(|i| i.prompt.trim() == it.prompt.trim())
                    .and_then(|i| i.table.clone()),
                kept_options: Vec::new(),
                // The revision's schema has no organizer: a question that stays 圖像組織 keeps
                // the one it had.
                organizer: it.organizer.clone().or_else(|| {
                    replaced
                        .filter(|i| i.answer_format.as_deref() == Some(it.answer_format.as_str()))
                        .and_then(|i| i.organizer.clone())
                }),
            }
        })
        .collect();
    block.message = None;
    block
}

/// Whether the worksheet's current questions were made as tiered practice (the generation's
/// settings; before any generation, the worksheet's own).
fn is_tiered(ws: &Worksheet, content: &Content) -> bool {
    content
        .chosen
        .as_ref()
        .map_or(ws.settings.output, |c| c.settings.output)
        == Output::Tiered
}

fn same_content(a: &Block, b: &Block) -> bool {
    let items = |x: &Block| -> Vec<(Option<String>, String, Vec<String>, String)> {
        x.items
            .iter()
            .map(|i| {
                (
                    i.answer_format.clone(),
                    i.prompt.clone(),
                    i.options.clone(),
                    i.answer.clone(),
                )
            })
            .collect()
    };
    a.title == b.title && a.instructions == b.instructions && items(a) == items(b)
}

/// A failed step ends the turn: the step that was running when it returns is the one that
/// failed (`spawn` settles the progress with the turn's error).
async fn revise(
    state: &AppState,
    input: &TurnInput,
    progress: &Arc<Tracker>,
) -> AppResult<Outcome> {
    let db = &state.db;
    let session = &input.session;
    let Some(worksheet_id) = input.worksheet_id else {
        return Ok(Err(MSG_WORKSHEET_GONE.to_string()));
    };
    let ws = match ws_store::owned(db, worksheet_id, session.teacher_id).await {
        Ok(ws) => ws,
        Err(AppError::NotFound(_)) => return Ok(Err(MSG_WORKSHEET_GONE.to_string())),
        Err(e) => return Err(e),
    };
    let content = ws_store::latest(db, worksheet_id).await?.content;
    progress.done(progress::READ).await;
    let labels = labels(&ws, &content);
    let states = crate::worksheet::images::current(db, worksheet_id).await?;
    let blocks = blocks_for_model(&content, &labels, &states);
    let block_ids: Vec<String> = content
        .blocks
        .iter()
        .filter(|b| b.enabled)
        .map(|b| b.id.clone())
        .collect();
    let pictures_on = state.images.enabled();
    let slot_ids = actions::slot_ids(&content, None);
    let attached = images::load(state, Some(input.turn.id), None).await?;
    let m = &ws.material;
    let text = m.text();
    let instruction = input.turn.request.trim().to_string();
    let blocks_json = serde_json::to_string_pretty(&blocks).map_err(anyhow::Error::from)?;
    let vars: HashMap<&'static str, String> = HashMap::from([
        ("instruction", instruction.clone()),
        ("worksheet_title", ws.title.clone()),
        ("blocks_json", blocks_json),
        ("picture_rules", actions::rules(state).await?),
        (
            "pictures_note",
            "目前的圖片寫在上面每個區塊的 pictures 裡（slot_id 就是 redraw、remove 要寫的 id）。"
                .to_string(),
        ),
        ("attachments_note", attached.note()),
        ("material_title", material_title(m)),
        ("paragraph_count", m.paragraphs.len().to_string()),
        ("passage_numbered", passage(m)),
        ("answer_format_labels", ANSWER_FORMATS.join("、")),
    ]);
    let c = Call {
        key: KEY_REVISE,
        scaffold: SCAFFOLD_REVISE,
        schema_name: REVISE_SCHEMA_NAME,
        schema: revise_schema(
            &block_ids,
            &slot_ids,
            if pictures_on { actions::MAX_ACTIONS } else { 0 },
        ),
        vars,
        required: vec![
            ("instruction", "教師的修改指示"),
            ("blocks_json", "學習單目前的區塊"),
        ],
        redact: vec![
            ("passage_numbered", "學習單教材"),
            ("blocks_json", "由教材衍生的內容"),
        ],
        fake_context: json!({
            "task": KEY_REVISE,
            "instruction": instruction,
            "blocks": blocks,
        }),
        max_tokens: REVISE_MAX_TOKENS,
        temperature: BASE_TEMPERATURE,
        trace: {
            let mut trace = trace_of(input, None, None);
            // Ids, sizes and hashes of the attached pictures; never their bytes.
            trace["attached_images"] = json!(attached.record);
            trace
        },
        progress: Some((progress.clone(), REVISE)),
        images: attached.images.clone(),
    };
    progress.start(REVISE).await;
    let result = call(
        state,
        session.teacher_id,
        c,
        |raw: &RawRevision| check_revision(raw, &content, &labels, &text, pictures_on),
        nothing,
    )
    .await?;
    let (plan, interaction_id) = match result {
        Ok(r) => r,
        Err(message) => return Ok(Err(message)),
    };
    progress.done(REVISE).await;
    progress.start(SAVE).await;
    apply_revision(state, input, &ws, plan, interaction_id).await
}

/// Writes the accepted changes into the worksheet as it is now: each replaced block goes to
/// the block history first, and all of them make one version (`cause = "revise"`).
async fn apply_revision(
    state: &AppState,
    input: &TurnInput,
    ws: &Worksheet,
    plan: RevisePlan,
    interaction_id: i64,
) -> AppResult<Outcome> {
    let db = &state.db;
    let RevisePlan {
        reply,
        accepted,
        mut dropped,
        pictures,
        pictures_dropped,
        pictures_off,
    } = plan;
    let mut changed: Vec<(Block, Block)> = Vec::new();
    let mut version_no = None;
    // Blocks that only got a picture slot (their previous state goes to the history too).
    let mut slot_only: Vec<Block> = Vec::new();
    let mut added: actions::Added = Vec::new();
    let mut outcomes: Vec<actions::PictureOutcome> = Vec::new();
    if !accepted.is_empty() || !pictures.is_empty() {
        let (_, running) = ws_ai::jobs(state, ws.id).await?;
        if running {
            return Ok(Err(MSG_WORKSHEET_BUSY.to_string()));
        }
        let latest = ws_store::latest(db, ws.id).await?;
        let mut next = latest.content.clone();
        let tiered = is_tiered(ws, &next);
        let levels: Vec<Level> = if !next.levels.is_empty() {
            next.levels.clone()
        } else if tiered {
            Level::ALL.to_vec()
        } else {
            vec![Level::Standard]
        };
        for (no, a) in accepted.iter().enumerate() {
            let Some(slot) = next.blocks.iter_mut().find(|b| b.id == a.block_id) else {
                dropped.push(format!("「{}」", a.block_id));
                continue;
            };
            let block = revised_block(slot, &a.section, &levels, input.turn.id, no + 1);
            if same_content(slot, &block) {
                continue;
            }
            let previous = std::mem::replace(slot, block.clone());
            changed.push((previous, block));
        }
        // Pictures to add are part of the version (a new slot in the block); the pictures
        // themselves are queued once it is saved. Redraws and removals only touch the pictures.
        for a in pictures
            .iter()
            .filter(|a| a.action == actions::PictureOp::Add)
        {
            if !changed.iter().any(|(p, _)| p.id == a.block_id)
                && !slot_only.iter().any(|b| b.id == a.block_id)
            {
                if let Some(b) = next.block(&a.block_id) {
                    slot_only.push(b.clone());
                }
            }
        }
        added = actions::add_slots(&mut next, &pictures, &format!("t{}", input.turn.id));
        let slots_added = added.iter().any(|(_, r)| r.is_ok());
        if !changed.is_empty() || slots_added {
            next.normalise().map_err(|e| {
                AppError::Internal(anyhow::anyhow!("revised worksheet failed validation: {e}"))
            })?;
            for (previous, _) in &changed {
                ws_store::push_history(db, ws.id, previous, Some(interaction_id)).await?;
            }
            for previous in &slot_only {
                ws_store::push_history(db, ws.id, previous, Some(interaction_id)).await?;
            }
            let block_ids: Vec<String> = changed
                .iter()
                .map(|(p, _)| p.id.clone())
                .chain(slot_only.iter().map(|b| b.id.clone()))
                .collect();
            let saved = ws_store::save_content_from(
                db,
                ws.id,
                &next,
                "revise",
                json!({
                    "cause": "prep_turn", "turn_id": input.turn.id,
                    "interaction_id": interaction_id, "block_ids": block_ids,
                }),
            )
            .await?;
            version_no = Some(saved.version_no);
        }
        outcomes = actions::run(state, ws, &pictures, &added, &next).await;
        if !outcomes.is_empty() {
            actions::log_applied(
                state,
                ws.teacher_id,
                ws.id,
                "prep_turn",
                json!({ "turn_id": input.turn.id, "interaction_id": interaction_id }),
                &outcomes,
            )
            .await;
        }
    }
    let mut changed_ids: Vec<String> = changed.iter().map(|(p, _)| p.id.clone()).collect();
    for (i, r) in &added {
        if r.is_ok() {
            if let Some(a) = pictures.get(*i) {
                if !changed_ids.contains(&a.block_id) {
                    changed_ids.push(a.block_id.clone());
                }
            }
        }
    }
    let mut reply = reply;
    if reply.trim().is_empty() {
        reply = if changed_ids.is_empty() && outcomes.is_empty() {
            "這次沒有修改學習單。".to_string()
        } else {
            "已依你的指示修改學習單。".to_string()
        };
    }
    if !dropped.is_empty() {
        reply = format!(
            "{reply}（{} 沒有改成：修改沒有通過學習單的檢查，維持原樣。）",
            dropped.join("、")
        );
    }
    let summary = actions::summary(&outcomes);
    if !summary.is_empty() {
        reply = format!("{reply}（{summary}）");
    }
    if pictures_dropped > 0 {
        reply = format!("{reply}（有 {pictures_dropped} 個圖片動作沒有通過檢查，沒有做。）");
    }
    if pictures_off {
        reply = format!("{reply}（這個環境沒有開啟 AI 產圖，圖片的部分沒有處理。）");
    }
    let result = Revision {
        reply,
        changed_block_ids: changed_ids,
        version_no,
        pictures: outcomes,
    };
    Ok(Ok((Some(json!(result)), Some(interaction_id))))
}

// ---------------------------------------------------------------- 做學習單／設計評量

/// The settings of an assessment: comprehension, vocabulary and grammar, closed formats plus
/// short answers, objectives shown, no warm-up, the standard version.
pub fn assessment_settings() -> Settings {
    Settings {
        material_types: Vec::new(),
        student_tasks: ["理解內容", "字彙練習", "文法／句型練習"]
            .map(String::from)
            .to_vec(),
        answer_formats: [
            FMT_CHOICE,
            FMT_TRUE_FALSE,
            FMT_BLANK,
            FMT_MATCHING,
            FMT_SHORT,
        ]
        .map(String::from)
        .to_vec(),
        generation: vec![GEN_OBJECTIVES.to_string()],
        levels: vec![Level::Standard],
        minutes: None,
        ..Settings::default()
    }
}

/// The 引起動機 ideas of the session's latest done activity plan.
async fn plan_warmup(db: &sqlx::PgPool, session_id: i64) -> AppResult<Vec<String>> {
    let result: Option<Value> = sqlx::query_scalar(
        "SELECT result FROM prep_turns
          WHERE session_id = $1 AND kind = 'activity' AND status = 'done' AND result IS NOT NULL
          ORDER BY seq DESC LIMIT 1",
    )
    .bind(session_id)
    .fetch_optional(db)
    .await?;
    Ok(result
        .and_then(|r| serde_json::from_value::<ActivityPlan>(r).ok())
        .map(|p| warmup_ideas(&p))
        .unwrap_or_default())
}

/// Creates the worksheet of a turn that makes one and starts its generation. The session's
/// options and 「教師的要求」 go into its settings and request; a 閱讀理解題／差異化學習單／教學套件
/// worksheet is made per Bloom level, without the session's template (templates only make the
/// full worksheet) and without warm-up ideas (it has no warm-up).
async fn start_worksheet(state: &AppState, input: &TurnInput) -> AppResult<()> {
    let db = &state.db;
    let session = &input.session;
    let teacher_id = session.teacher_id;
    let Some(material_id) = session.material_id else {
        return Err(AppError::bad_request(MSG_MATERIAL_GONE));
    };
    let kind = input.turn.kind.as_str();
    let assessment = kind == KIND_ASSESSMENT;
    // The template the turn recorded when it started (the session's then).
    let template_id = if kind == KIND_WORKSHEET {
        input.turn.template_id
    } else {
        None
    };
    let overrides = if assessment {
        let title: String = sqlx::query_scalar("SELECT title FROM materials WHERE id = $1")
            .bind(material_id)
            .fetch_optional(db)
            .await?
            .unwrap_or_default();
        let title = title.trim();
        CreateOverrides {
            title: Some(if title.is_empty() {
                "評量".to_string()
            } else {
                format!("評量：{title}")
            }),
            settings: Some(assessment_settings()),
            warmup: Vec::new(),
            prep_session_id: Some(session.id),
        }
    } else {
        CreateOverrides {
            warmup: if kind == KIND_WORKSHEET && template_id.is_none() {
                plan_warmup(db, session.id).await?
            } else {
                Vec::new()
            },
            prep_session_id: Some(session.id),
            ..CreateOverrides::default()
        }
    };
    let req = CreateWorksheet {
        class_id: session.class_id,
        material_id: Some(material_id),
        template_id,
        // An assessment keeps its own answer formats.
        settings: session
            .options
            .0
            .settings_patch(output_of(kind), !assessment),
        request: Some(teacher_request(db, input).await?),
        ..CreateWorksheet::default()
    };
    let worksheet_id = create_worksheet(db, teacher_id, req, overrides).await?;
    store::set_worksheet(db, input.turn.id, worksheet_id).await?;
    let ws = ws_store::owned(db, worksheet_id, teacher_id).await?;
    let run = run_input(state, teacher_id, &input.track, ws).await?;
    ws_ai::start_generation(state, run).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worksheet::template::RawOutItem;
    use crate::worksheet::Bloom;

    fn content() -> Content {
        Content {
            blocks: vec![Block {
                id: "b1".into(),
                title: "Details".into(),
                items: vec![Item {
                    id: "b1-i1".into(),
                    answer_format: Some(FMT_SHORT.into()),
                    prompt: "Why did she put it there?".into(),
                    answer: "To help people.".into(),
                    levels: vec![Level::Standard],
                    ..Item::default()
                }],
                ..Block::default()
            }],
            levels: vec![Level::Standard],
            ..Content::default()
        }
    }

    fn change(format: &str) -> RawRevision {
        RawRevision {
            reply: "改好了".into(),
            changes: vec![super::super::RawChange {
                block_id: "b1".into(),
                title: "Details".into(),
                instructions: "Answer.".into(),
                items: vec![RawOutItem {
                    answer_format: format.into(),
                    prompt: "Why did she put it there? (revised)".into(),
                    options: vec![],
                    answer: "To help people.".into(),
                    bloom: None,
                    organizer: None,
                }],
            }],
            picture_actions: Vec::new(),
        }
    }

    #[test]
    fn a_change_with_a_label_outside_the_set_is_dropped() {
        let labels = HashMap::from([("b1".to_string(), "A".to_string())]);
        let ok = check_revision(&change(FMT_SHORT), &content(), &labels, "text", true);
        assert!(ok.problems.is_empty(), "{:?}", ok.problems);
        assert_eq!(ok.output.as_ref().unwrap().accepted.len(), 1);
        let bad = check_revision(&change("連連看"), &content(), &labels, "text", true);
        let plan = bad.output.unwrap();
        assert!(plan.accepted.is_empty());
        assert_eq!(plan.dropped, ["A 區"]);
        assert!(bad.problems[0].contains("A 區"), "{:?}", bad.problems);
    }

    #[test]
    fn revised_items_get_new_ids_and_keep_their_versions() {
        let c = content();
        let labels = HashMap::new();
        let plan = check_revision(&change(FMT_SHORT), &c, &labels, "text", true)
            .output
            .unwrap();
        let levels = [Level::Basic, Level::Standard];
        let b = revised_block(&c.blocks[0], &plan.accepted[0].section, &levels, 7, 1);
        assert_eq!(b.items[0].id, "b1-v7i1");
        assert_eq!(b.items[0].levels, [Level::Standard], "by position");
        assert_eq!(b.items[0].origin, Origin::Ai);
        assert!(!same_content(&c.blocks[0], &b));
        assert_eq!(item_id(&"x".repeat(40), 7, 2, 3), "v7b2i3");
    }

    #[test]
    fn revised_items_keep_their_versions_by_position() {
        let all = Level::ALL;
        let old = Block {
            id: "b4".into(),
            title: "Part D · Look Closer".into(),
            bloom: Some(Bloom::Analyze),
            items: vec![Item {
                id: "b4-i1".into(),
                levels: vec![Level::Advanced],
                bloom: Some(Bloom::Analyze),
                hint: "Look at paragraph 2.".into(),
                ..Item::default()
            }],
            ..Block::default()
        };
        let section = |n: usize| CleanSection {
            title: "Part D · Look Closer".into(),
            instructions: "Answer.".into(),
            items: (0..n)
                .map(|k| crate::worksheet::checks::CleanSectionItem {
                    answer_format: FMT_SHORT.into(),
                    prompt: format!("Why? {k}"),
                    options: vec![],
                    answer: "Because.".into(),
                    evidence: None,
                    bloom: None,
                    organizer: None,
                })
                .collect(),
            scaffold: String::new(),
        };
        let b = revised_block(&old, &section(2), &all, 9, 1);
        // The replaced item keeps its versions, label and hint; a new one is in every version
        // (a 差異化學習單 has every question in all three) with the block's level.
        assert_eq!(b.items[0].levels, [Level::Advanced]);
        assert_eq!(b.items[0].bloom, Some(Bloom::Analyze));
        assert_eq!(b.items[0].hint, "Look at paragraph 2.");
        assert_eq!(b.items[1].levels, all);
        assert_eq!(b.items[1].bloom, Some(Bloom::Analyze));
        assert_eq!(b.items[1].hint, "");
        // Versions the worksheet no longer has are left out; none left → all of them.
        let only = [Level::Basic];
        let b = revised_block(&old, &section(2), &only, 9, 1);
        assert_eq!(b.items[0].levels, only);
        assert_eq!(b.items[1].levels, only);
        // A new item without a level: every version.
        let plain = Block {
            bloom: None,
            items: vec![],
            ..old.clone()
        };
        let b = revised_block(&plain, &section(1), &all, 9, 1);
        assert_eq!(b.items[0].levels, all);
        assert_eq!(b.items[0].bloom, None);
    }

    #[test]
    fn a_revised_differentiated_question_keeps_its_versions_only_while_unchanged() {
        let tier = |level| crate::worksheet::ItemTier {
            level,
            support_kind: crate::worksheet::SupportKind::None,
            model_sentence: String::new(),
            support: vec![],
            word_bank: vec![],
            answer: "Because.".into(),
            scoring: "完整句".into(),
        };
        let old = Block {
            id: "b1".into(),
            bloom: Some(Bloom::Analyze),
            items: vec![
                Item {
                    id: "b1-i1".into(),
                    prompt: "Why? 0".into(),
                    levels: Level::ALL.to_vec(),
                    tiers: Level::ALL.into_iter().map(tier).collect(),
                    ..Item::default()
                },
                Item {
                    id: "b1-i2".into(),
                    prompt: "Old question".into(),
                    levels: Level::ALL.to_vec(),
                    tiers: Level::ALL.into_iter().map(tier).collect(),
                    ..Item::default()
                },
            ],
            ..Block::default()
        };
        let section = CleanSection {
            title: String::new(),
            instructions: String::new(),
            items: ["Why? 0", "A new question?"]
                .iter()
                .map(|p| crate::worksheet::checks::CleanSectionItem {
                    answer_format: FMT_SHORT.into(),
                    prompt: p.to_string(),
                    options: vec![],
                    answer: "Because.".into(),
                    evidence: None,
                    bloom: None,
                    organizer: None,
                })
                .collect(),
            scaffold: String::new(),
        };
        let b = revised_block(&old, &section, &Level::ALL, 9, 1);
        assert_eq!(
            b.items[0].tiers.len(),
            3,
            "same question: its versions stay"
        );
        assert!(b.items[1].tiers.is_empty(), "a changed question loses them");
    }
}
