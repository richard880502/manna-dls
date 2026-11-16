//! The turns of a prep session, run in the background.
//!
//! `worksheet` / `assessment` create an ordinary worksheet (the same code as `POST
//! /api/worksheets`) and start its generation while the request is still there; the turn then
//! stays `running` until the worksheet's jobs are over ([`super::store::refresh`]). Every other
//! kind is one spawned task that ends with [`super::store::finish`].
//!
//! The model calls follow the template extraction: `max_tokens` always set, one retry with the
//! temperature raised by 0.4 (the gateway caches identical requests) when the output ran away,
//! could not be read or failed the checks, each attempt its own `ai_interactions` row, and the
//! material only ever logged as its length and a hash.

use std::collections::{HashMap, HashSet};

use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use super::store::{self, SessionRow, TurnRow};
use super::{
    activity_schema, ask_schema, chip_label, clean_answer, clean_plan, clean_text, revise_schema,
    warmup_ideas, ActivityPlan, Answer, CurriculumRef, MaterialRef, RawPlan, RawRevision,
    ResourceItem, Resources, Revision, ACTIVITY_SCHEMA_NAME, ASK_CURRICULUM_TOP, ASK_SCHEMA_NAME,
    CHANGES_MAX, CHANGE_ITEMS_MAX, CURRICULUM_TOP, KEY_ACTIVITY, KEY_ASK, KEY_REVISE,
    KIND_ACTIVITY, KIND_ASK, KIND_ASSESSMENT, KIND_RESOURCES, KIND_REVISE, KIND_WORKSHEET,
    MATERIALS_TOP, REPLY_CHARS, REVISE_SCHEMA_NAME,
};
use crate::ai::pipeline::InteractionMeta;
use crate::ai::LlmRequest;
use crate::checks::{self as base_checks, CheckResult};
use crate::curriculum::{self, CurriculumItem};
use crate::error::{AppError, AppResult};
use crate::materials::store as material_store;
use crate::prompts;
use crate::retrieval::{self, CurriculumFilter, KbItem};
use crate::routes::worksheets::{create_worksheet, run_input, CreateOverrides, CreateWorksheet};
use crate::routes::{clip_chars, or_blank, resolved_profile, scaffold_id};
use crate::sop::ai::{attempt, finish_failure, finish_output, prompt, set_final, Failure};
use crate::sop::store as sop_store;
use crate::worksheet::ai::template::marked_passage;
use crate::worksheet::ai::{self as ws_ai};
use crate::worksheet::assemble::project;
use crate::worksheet::checks::{evaluate_section, CleanSection, SectionCtx};
use crate::worksheet::material::SOURCE_LIBRARY;
use crate::worksheet::store::{self as ws_store, Worksheet};
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
pub const ACTIVITY_MAX_TOKENS: u32 = 6000;
pub const ASK_MAX_TOKENS: u32 = 3000;
pub const REVISE_MAX_TOKENS: u32 = 6000;
const BASE_TEMPERATURE: f32 = 0.2;
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

/// What a turn works with, loaded while the request is still there.
#[derive(Debug, Clone)]
pub struct TurnInput {
    pub turn: TurnRow,
    pub session: SessionRow,
    pub track: String,
    /// `revise`: the worksheet to change.
    pub worksheet_id: Option<i64>,
}

/// Starts a turn already opened with [`store::start_turn`]. A worksheet / assessment turn makes
/// its worksheet now (a failure closes the turn as `failed`); the other kinds are spawned.
pub async fn start(state: &AppState, input: TurnInput) -> AppResult<()> {
    let kind = input.turn.kind.as_str();
    if kind == KIND_WORKSHEET || kind == KIND_ASSESSMENT {
        if let Err(e) = start_worksheet(state, &input).await {
            let message = teacher_message(&e);
            if matches!(e, AppError::Internal(_)) {
                tracing::error!(turn_id = input.turn.id, error = %e, "prep worksheet turn failed");
            }
            store::finish(&state.db, input.turn.id, Err(message)).await?;
        }
        return Ok(());
    }
    spawn(state, input);
    Ok(())
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

fn spawn(state: &AppState, input: TurnInput) {
    let state = state.clone();
    let turn_id = input.turn.id;
    tokio::spawn(async move {
        // The work runs in its own task so that a panic there still closes the turn.
        let work = {
            let state = state.clone();
            tokio::spawn(async move { run(&state, &input).await })
        };
        let outcome: Outcome = match work.await {
            Ok(Ok(outcome)) => outcome,
            Ok(Err(e)) => {
                tracing::error!(turn_id, error = %e, "prep turn failed");
                Err(teacher_message(&e))
            }
            Err(e) => {
                tracing::error!(turn_id, error = %e, "prep turn panicked");
                Err(MSG_INTERNAL.to_string())
            }
        };
        if let Err(e) = store::finish(&state.db, turn_id, outcome).await {
            tracing::error!(turn_id, error = %e, "could not close the prep turn");
        }
    });
}

async fn run(state: &AppState, input: &TurnInput) -> AppResult<Outcome> {
    match input.turn.kind.as_str() {
        KIND_ACTIVITY => activity(state, input).await,
        KIND_RESOURCES => resources(state, input).await,
        KIND_ASK => ask(state, input).await,
        KIND_REVISE => revise(state, input).await,
        other => Err(AppError::Internal(anyhow::anyhow!(
            "prep turn kind {other} is not run in the background"
        ))),
    }
}

// ---------------------------------------------------------------- shared context

/// The session's material with its paragraphs, if it still exists and has text.
async fn material(db: &sqlx::PgPool, session: &SessionRow) -> AppResult<Option<Material>> {
    let Some(id) = session.material_id else {
        return Ok(None);
    };
    let Ok(row) = material_store::owned(db, id, session.teacher_id).await else {
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
fn passage(m: &Material) -> String {
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

async fn class_profile(state: &AppState, session: &SessionRow) -> AppResult<(String, Value)> {
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

struct Call {
    key: &'static str,
    scaffold: &'static str,
    schema_name: &'static str,
    schema: Value,
    vars: HashMap<&'static str, String>,
    /// Placeholders that must reach the model even with a prompt version that lacks them.
    required: Vec<(&'static str, &'static str)>,
    /// Variables logged only as their length and a hash.
    redact: Vec<(&'static str, &'static str)>,
    fake_context: Value,
    max_tokens: u32,
    /// Logged with every attempt (ids, never text).
    trace: Value,
}

/// The output of an attempt after the server's checks: what can be used, and what to tell the
/// model in the retry.
struct Checked<T> {
    output: Option<T>,
    problems: Vec<String>,
}

/// One call and, when the output ran away, could not be read or has problems, one retry with
/// a higher temperature and the problems listed. The retry's usable output wins; when it has
/// none, the first attempt's usable output (if any) is used. `Ok(Err(message))`: nothing usable.
async fn call<R, T>(
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
    let system = prompts::render(&prompt.system_text, &c.vars);
    let user = prompts::render_user_with(&prompt, &c.vars, &c.required);
    let mut logged = c.vars.clone();
    for (name, label) in &c.redact {
        if let Some(v) = logged.get(name).cloned() {
            logged.insert(name, prompts::redacted(label, &v));
        }
    }
    let logged_system = prompts::render(&prompt.system_text, &logged);
    let logged_user = prompts::render_user_with(&prompt, &logged, &c.required);
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
        let temperature = BASE_TEMPERATURE + RETRY_TEMPERATURE_STEP * (attempt_no - 1) as f32;
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

async fn nothing(_: i64) -> AppResult<()> {
    Ok(())
}

// ---------------------------------------------------------------- 規劃教學活動

async fn activity(state: &AppState, input: &TurnInput) -> AppResult<Outcome> {
    let db = &state.db;
    let session = &input.session;
    let Some(m) = material(db, session).await? else {
        return Ok(Err(MSG_MATERIAL_GONE.to_string()));
    };
    let (profile, class_snapshot) = class_profile(state, session).await?;
    // The same retrieval as the SOP objectives, over the material.
    let query = clip_chars(&format!("{}\n{}", m.title, m.text()), 4000);
    let (items, trace) = curriculum::items_for_track_hybrid(
        db,
        &state.embedder,
        &input.track,
        &query,
        CURRICULUM_TOP,
    )
    .await?;
    let candidates: Vec<CurriculumRef> = items.iter().map(curriculum_ref).collect();
    let codes: Vec<String> = candidates.iter().map(|c| c.code.clone()).collect();
    let vars: HashMap<&'static str, String> = HashMap::from([
        ("class_profile", or_blank(&profile)),
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
    let logged_query = logged_material_query(&query);
    let c = Call {
        key: KEY_ACTIVITY,
        scaffold: SCAFFOLD_ACTIVITY,
        schema_name: ACTIVITY_SCHEMA_NAME,
        schema: activity_schema(&codes),
        vars,
        required: vec![
            ("class_profile", "班級特性"),
            ("curriculum_items", "可對應的課綱條目（只能從這裡選代碼）"),
        ],
        redact: vec![("passage_numbered", "備課對話教材")],
        fake_context: json!({
            "task": KEY_ACTIVITY,
            "material_title": m.title,
            "paragraphs": m.paragraphs.iter().map(|p| p.text.clone()).collect::<Vec<_>>(),
            "candidates": codes,
        }),
        max_tokens: ACTIVITY_MAX_TOKENS,
        trace: trace_of(input, Some(&class_snapshot), Some(&codes)),
    };
    let track = input.track.clone();
    let result = call(
        state,
        session.teacher_id,
        c,
        |raw: &RawPlan| match clean_plan(raw, &candidates) {
            Ok(plan) => Checked {
                output: Some(plan),
                problems: Vec::new(),
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
    Ok(result.map(|(plan, id): (ActivityPlan, i64)| (Some(json!(plan)), Some(id))))
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

/// The teacher's own library materials that share words with `query` (keyword ranking), best
/// first, without the session's own.
async fn own_materials(
    db: &sqlx::PgPool,
    session: &SessionRow,
    query: &str,
) -> AppResult<Vec<MaterialRef>> {
    let rows: Vec<(i64, String, String)> = sqlx::query_as(
        "SELECT id, title, left(text, 6000) FROM materials
          WHERE teacher_id = $1 AND ($2::BIGINT IS NULL OR id <> $2)
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

/// What an earlier turn said, in a line (the reply text, or a worksheet's block titles).
async fn turn_summary(db: &sqlx::PgPool, t: &TurnRow) -> AppResult<String> {
    if t.status == super::STATUS_FAILED {
        return Ok("（這一次沒有完成）".to_string());
    }
    if t.status == super::STATUS_RUNNING {
        return Ok("（還在處理中）".to_string());
    }
    let r = t.result.clone().unwrap_or(Value::Null);
    let text = match t.kind.as_str() {
        KIND_WORKSHEET | KIND_ASSESSMENT => match t.worksheet_id {
            Some(id) => {
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
            None => "學習單".to_string(),
        },
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
        trace: trace_of(input, Some(&class_snapshot), Some(&codes)),
    };
    let track = input.track.clone();
    let trace = found.trace.clone();
    let logged_question = clip_chars(&question, 2000);
    let result = call(
        state,
        session.teacher_id,
        c,
        |raw: &Answer| match clean_answer(raw) {
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
}

/// Checks every change like a template block (fixed labels, well-formed closed items, no
/// answer in the stem, dictionary syllables, at most 12 items).
fn check_revision(
    raw: &RawRevision,
    content: &Content,
    labels: &HashMap<String, String>,
    passage: &str,
) -> Checked<RevisePlan> {
    let mut plan = RevisePlan {
        reply: clean_text(&raw.reply, REPLY_CHARS),
        accepted: Vec::new(),
        dropped: Vec::new(),
    };
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
fn blocks_for_model(content: &Content, labels: &HashMap<String, String>) -> Vec<Value> {
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
        .map(|(k, it)| Item {
            id: item_id(&old.id, turn_id, no, k + 1),
            answer_format: Some(it.answer_format.clone()),
            prompt: it.prompt.clone(),
            options: it.options.clone(),
            answer: it.answer.clone(),
            evidence: it.evidence.clone().unwrap_or_default(),
            purpose: old.items.get(k).and_then(|i| i.purpose),
            levels: levels.to_vec(),
            origin: Origin::Ai,
        })
        .collect();
    block.message = None;
    block
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

async fn revise(state: &AppState, input: &TurnInput) -> AppResult<Outcome> {
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
    let labels = labels(&ws, &content);
    let blocks = blocks_for_model(&content, &labels);
    let block_ids: Vec<String> = content
        .blocks
        .iter()
        .filter(|b| b.enabled)
        .map(|b| b.id.clone())
        .collect();
    let m = &ws.material;
    let text = m.text();
    let instruction = input.turn.request.trim().to_string();
    let blocks_json = serde_json::to_string_pretty(&blocks).map_err(anyhow::Error::from)?;
    let vars: HashMap<&'static str, String> = HashMap::from([
        ("instruction", instruction.clone()),
        ("worksheet_title", ws.title.clone()),
        ("blocks_json", blocks_json),
        ("material_title", material_title(m)),
        ("paragraph_count", m.paragraphs.len().to_string()),
        ("passage_numbered", passage(m)),
        ("answer_format_labels", ANSWER_FORMATS.join("、")),
    ]);
    let c = Call {
        key: KEY_REVISE,
        scaffold: SCAFFOLD_REVISE,
        schema_name: REVISE_SCHEMA_NAME,
        schema: revise_schema(&block_ids),
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
        trace: trace_of(input, None, None),
    };
    let result = call(
        state,
        session.teacher_id,
        c,
        |raw: &RawRevision| check_revision(raw, &content, &labels, &text),
        nothing,
    )
    .await?;
    let (plan, interaction_id) = match result {
        Ok(r) => r,
        Err(message) => return Ok(Err(message)),
    };
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
    } = plan;
    let mut changed: Vec<(Block, Block)> = Vec::new();
    let mut version_no = None;
    if !accepted.is_empty() {
        let (_, running) = ws_ai::jobs(state, ws.id).await?;
        if running {
            return Ok(Err(MSG_WORKSHEET_BUSY.to_string()));
        }
        let latest = ws_store::latest(db, ws.id).await?;
        let mut next = latest.content.clone();
        let levels: Vec<Level> = if next.levels.is_empty() {
            vec![Level::Standard]
        } else {
            next.levels.clone()
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
        if !changed.is_empty() {
            next.normalise().map_err(|e| {
                AppError::Internal(anyhow::anyhow!("revised worksheet failed validation: {e}"))
            })?;
            for (previous, _) in &changed {
                ws_store::push_history(db, ws.id, previous, Some(interaction_id)).await?;
            }
            let saved = ws_store::save_content(db, ws.id, &next, "revise").await?;
            version_no = Some(saved.version_no);
        }
    }
    let changed_ids: Vec<String> = changed.iter().map(|(p, _)| p.id.clone()).collect();
    let mut reply = reply;
    if reply.trim().is_empty() {
        reply = if changed_ids.is_empty() {
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
    let result = Revision {
        reply,
        changed_block_ids: changed_ids,
        version_no,
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

async fn start_worksheet(state: &AppState, input: &TurnInput) -> AppResult<()> {
    let db = &state.db;
    let session = &input.session;
    let teacher_id = session.teacher_id;
    let Some(material_id) = session.material_id else {
        return Err(AppError::bad_request(MSG_MATERIAL_GONE));
    };
    let assessment = input.turn.kind == KIND_ASSESSMENT;
    let template_id = if assessment {
        None
    } else {
        session.template_id
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
            warmup: if template_id.is_none() {
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
                }],
            }],
        }
    }

    #[test]
    fn a_change_with_a_label_outside_the_set_is_dropped() {
        let labels = HashMap::from([("b1".to_string(), "A".to_string())]);
        let ok = check_revision(&change(FMT_SHORT), &content(), &labels, "text");
        assert!(ok.problems.is_empty(), "{:?}", ok.problems);
        assert_eq!(ok.output.as_ref().unwrap().accepted.len(), 1);
        let bad = check_revision(&change("連連看"), &content(), &labels, "text");
        let plan = bad.output.unwrap();
        assert!(plan.accepted.is_empty());
        assert_eq!(plan.dropped, ["A 區"]);
        assert!(bad.problems[0].contains("A 區"), "{:?}", bad.problems);
    }

    #[test]
    fn revised_items_get_new_ids_and_all_levels() {
        let c = content();
        let labels = HashMap::new();
        let plan = check_revision(&change(FMT_SHORT), &c, &labels, "text")
            .output
            .unwrap();
        let levels = [Level::Basic, Level::Standard];
        let b = revised_block(&c.blocks[0], &plan.accepted[0].section, &levels, 7, 1);
        assert_eq!(b.items[0].id, "b1-v7i1");
        assert_eq!(b.items[0].levels, levels);
        assert_eq!(b.items[0].origin, Origin::Ai);
        assert!(!same_content(&c.blocks[0], &b));
        assert_eq!(item_id(&"x".repeat(40), 7, 2, 3), "v7b2i3");
    }
}
