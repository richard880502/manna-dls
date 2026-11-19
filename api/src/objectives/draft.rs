//! G2/G3: drafting 內容目標 and 語言目標 from the material when the teacher left both empty
//! (`POST /api/objective-drafts`, docs/API.md「SIOP 目標」). Synchronous: the teacher waits on
//! the generator page, so a round is two calls (draft, then judge) and there is at most one
//! more round.
//!
//! 1. **Draft** (`objective_draft`): the model writes 1–2 內容目標, then 1–2 語言目標 derived
//!    from them (strategy first, R2), each as the students' "I can …" line and the teacher's
//!    Chinese line, a 語言目標 with its 「學生要用……，才能……」. The server checks R1 by rule
//!    ([`super::check_rules`]); a group left empty by the rules is asked for once more in the
//!    same call (temperature +0.4, the problems listed: the gateway caches identical requests).
//! 2. **Judge** (`objective_check`): one call judges R3 for every remaining draft on its own,
//!    like `worksheet_bloom_check` ([`super::evaluate_check`]). Only drafts without problems
//!    are shown (G3).
//! 3. **Once more**: when a group has no draft left, the server drafts a second round with the
//!    judge's reasons (what passed may be kept) and judges it; the round with both groups (or
//!    more drafts) is shown.
//!
//! Every call is its own `ai_interactions` row; the round shown is the one the teacher's
//! decisions refer to (`item_index` = [`super::Draft::index`]), and its `check_results` say
//! which drafts were kept back and why. The material is logged as its length and a hash.

use std::collections::HashMap;
use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};

use super::{
    check_rules, check_schema, draft_schema, evaluate_check, passing, Draft, RawCheck, RawDrafts,
    RuleOutcome, Verdict, CATEGORIES, CHECK_SCHEMA_NAME, DRAFT_SCHEMA_NAME, KEY_CHECK, KEY_DRAFT,
    STRATEGY,
};
use crate::ai::pipeline::{AiContextItem, ContextList, InteractionMeta};
use crate::ai::LlmRequest;
use crate::checks::{self as base_checks, CheckResult};
use crate::curriculum::{self, CurriculumItem};
use crate::error::AppResult;
use crate::progress::{self, Step, Tracker};
use crate::prompts;
use crate::retrieval;
use crate::routes::{clip_chars, or_blank, scaffold_id, track_label};
use crate::sop::ai::{attempt, finish_failure, finish_output, prompt, set_final, Failure};
use crate::sop::ObjectiveType;
use crate::worksheet::{Material, MAX_AI_PARAGRAPHS};
use crate::AppState;

pub const SCAFFOLD_DRAFT: &str = "SC-OBJ-DRAFT";
pub const SCAFFOLD_CHECK: &str = "SC-OBJ-CHECK";
pub const DRAFT_MAX_TOKENS: u32 = 1500;
pub const CHECK_MAX_TOKENS: u32 = 900;
const BASE_TEMPERATURE: f32 = 0.2;
const RETRY_TEMPERATURE_STEP: f32 = 0.4;
/// Curriculum items put in front of the model (the same as the activity plan).
const CURRICULUM_TOP: usize = 8;
/// Earlier drafts looked at for R2's variety within a unit.
const RECENT_DRAFTS: i64 = 3;

pub const MSG_DISABLED: &str = "這個環境沒有開啟 AI 協助。";
pub const MSG_TIMEOUT: &str = "AI 目前沒有回應，這次沒有草擬目標。你可以自己寫，或直接產生。";
pub const MSG_ERROR: &str = "AI 暫時無法使用，這次沒有草擬目標。你可以自己寫，或直接產生。";
pub const MSG_NONE: &str =
    "Manna 這次草擬的目標沒有通過檢查，所以沒有列出。你可以自己寫，或直接產生。";
pub const MSG_NO_LANGUAGE: &str =
    "Manna 這次草擬的語言目標沒有通過檢查，所以只列出內容目標。語言目標可以自己寫。";
pub const MSG_NO_CONTENT: &str =
    "Manna 這次草擬的內容目標沒有通過檢查，所以沒有列出。你可以自己寫，或直接產生。";

/// What the teacher is making, so the drafts fit it (and two tools on the same text are not
/// the same request to the gateway's cache).
pub fn purpose(kind: &str) -> &'static str {
    match kind {
        "comprehension" => "閱讀理解題（依 Bloom 認知層次出題，附答案與文本依據）",
        "tiered" => "差異化學習單（基礎／一般／進階三個版本共用同一組目標，差在提示與支持的多寡）",
        "activity" => "課堂活動（引起動機到統整的一節課教學流程）",
        "kit" => "教學套件（課堂活動加上一份差異化學習單）",
        _ => "完整學習單（暖身、逐段任務到 Exit ticket）",
    }
}

/// What a draft is made from (checked by the route).
pub struct DraftInput {
    pub teacher_id: i64,
    pub track: String,
    pub kind: String,
    pub material: Material,
    pub class_id: Option<i64>,
    /// 「教師的要求」 ("" when none).
    pub request: String,
    /// Lines of an earlier draft the teacher wants other than (「換一組」).
    pub avoid: Vec<String>,
}

/// `POST /api/objective-drafts` (docs/API.md「SIOP 目標」).
#[derive(Debug, Serialize)]
pub struct DraftResult {
    /// The draft call whose output is shown: the decisions are recorded against it.
    pub interaction_id: Option<i64>,
    /// `shown` | `blocked` | `timeout` | `error` | `disabled`
    pub status: &'static str,
    pub message: Option<String>,
    /// The drafts that passed R1 and R3: the 內容目標 first, then the 語言目標.
    pub objectives: Vec<Draft>,
    /// How many drafts of the round shown were kept back by the checks.
    pub hidden: usize,
    /// 「Manna 已帶入的資訊」.
    pub context: Vec<AiContextItem>,
}

impl DraftResult {
    fn failed(
        status: &'static str,
        interaction_id: Option<i64>,
        context: Vec<AiContextItem>,
    ) -> Self {
        let message = match status {
            "timeout" => MSG_TIMEOUT,
            "error" => MSG_ERROR,
            "disabled" => MSG_DISABLED,
            _ => MSG_NONE,
        };
        Self {
            interaction_id,
            status,
            message: Some(message.to_string()),
            objectives: Vec::new(),
            hidden: 0,
            context,
        }
    }
}

// ---------------------------------------------------------------- one call, retried once

struct Spec {
    key: &'static str,
    scaffold: &'static str,
    schema_name: &'static str,
    schema: Value,
    vars: HashMap<&'static str, String>,
    required: Vec<(&'static str, &'static str)>,
    fake_context: Value,
    max_tokens: u32,
    trace: Value,
    /// The step of the progress this call is (its retry is said there as 「重試中」).
    progress: Option<(Arc<Tracker>, &'static str)>,
}

enum Called<T> {
    Shown {
        output: T,
        id: i64,
    },
    Failed {
        status: &'static str,
        id: Option<i64>,
    },
}

/// One call and, when its output could not be read or `check` reports problems, one retry
/// with the temperature raised and the problems listed. The retry's usable output wins; when it
/// has none, the first attempt's usable output is used.
async fn call<R, T>(
    state: &AppState,
    teacher_id: i64,
    s: Spec,
    check: impl Fn(&R) -> (Option<T>, Vec<String>),
    on_first: impl AsyncFnOnce(i64) -> AppResult<()>,
) -> AppResult<Called<T>>
where
    R: DeserializeOwned,
{
    let db = &state.db;
    let prompt = prompt(db, s.key).await?;
    let system = prompts::render(&prompt.system_text, &s.vars);
    let user = prompts::render_user_with(&prompt, &s.vars, &s.required);
    let mut logged = s.vars.clone();
    if let Some(p) = logged.get("passage_numbered").cloned() {
        logged.insert("passage_numbered", prompts::redacted("草擬目標的教材", &p));
    }
    let logged_user = prompts::render_user_with(&prompt, &logged, &s.required);
    let meta = InteractionMeta {
        teacher_id,
        module: s.key,
        unit_id: None,
        lesson_id: None,
        scaffold_id: scaffold_id(db, s.scaffold).await?,
        prompt_version_id: Some(prompt.id),
    };
    let mut on_first = Some(on_first);
    let mut retry_of: Option<i64> = None;
    let mut note = String::new();
    let mut first: Option<(T, i64)> = None;
    for attempt_no in 1..=2 {
        if attempt_no == 2 {
            if let Some((tracker, key)) = &s.progress {
                tracker.note(key, Some(progress::NOTE_RETRY)).await;
            }
        }
        let temperature = BASE_TEMPERATURE + RETRY_TEMPERATURE_STEP * (attempt_no - 1) as f32;
        let snapshot = json!({
            "inputs": logged,
            "objective_draft": s.trace,
            "prompt": { "key": s.key, "version_no": prompt.version_no,
                        "system": system, "user": format!("{logged_user}{note}") },
            "schema": s.schema_name,
            "request": { "temperature": temperature, "max_tokens": s.max_tokens },
            "retry_of": retry_of,
        });
        let req = LlmRequest {
            system: system.clone(),
            user: format!("{user}{note}"),
            schema_name: s.schema_name,
            schema: s.schema.clone(),
            fake_context: s.fake_context.clone(),
            attempt: attempt_no,
            max_tokens: Some(s.max_tokens),
            temperature: Some(temperature),
            images: Vec::new(),
        };
        let a = attempt(state, &meta, &snapshot, &req, retry_of).await?;
        if let Some(f) = on_first.take() {
            f(a.id).await?;
        }
        retry_of = Some(a.id);
        let problems: Vec<String> = match &a.response {
            Ok(resp) => match serde_json::from_value::<R>(resp.json.clone()) {
                Ok(raw) => {
                    let (output, problems) = check(&raw);
                    let passed = problems.is_empty();
                    let results = if passed {
                        vec![CheckResult::pass("schema"), CheckResult::pass("rules")]
                    } else {
                        vec![
                            CheckResult::pass("schema"),
                            CheckResult::fail("rules", problems.join("；")),
                        ]
                    };
                    let report = base_checks::report(&results);
                    match output {
                        Some(output) if passed || attempt_no == 2 => {
                            finish_output(db, &a, resp, "shown", report, Vec::new()).await?;
                            return Ok(Called::Shown { output, id: a.id });
                        }
                        Some(output) => {
                            finish_output(db, &a, resp, "blocked", report, Vec::new()).await?;
                            first = Some((output, a.id));
                        }
                        None => {
                            finish_output(db, &a, resp, "blocked", report, Vec::new()).await?;
                        }
                    }
                    if problems.is_empty() {
                        vec!["上一次沒有交出可用的內容。".to_string()]
                    } else {
                        problems
                    }
                }
                Err(e) => {
                    let report = base_checks::report(&[CheckResult::fail("schema", e.to_string())]);
                    finish_output(db, &a, resp, "blocked", report, Vec::new()).await?;
                    vec!["上一次的輸出不符合指定的 JSON 格式。".to_string()]
                }
            },
            Err(e) => match finish_failure(db, &a, e).await? {
                Failure::Timeout | Failure::Error if first.is_some() => break,
                Failure::Timeout => {
                    return Ok(Called::Failed {
                        status: "timeout",
                        id: Some(a.id),
                    })
                }
                Failure::Error => {
                    let status = if matches!(e, crate::ai::ProviderError::Disabled) {
                        "disabled"
                    } else {
                        "error"
                    };
                    return Ok(Called::Failed {
                        status,
                        id: Some(a.id),
                    });
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
    if let Some((output, id)) = first {
        set_final(
            db,
            id,
            "shown",
            base_checks::report(&[CheckResult::pass("schema")]),
        )
        .await?;
        return Ok(Called::Shown { output, id });
    }
    Ok(Called::Failed {
        status: "blocked",
        id: retry_of,
    })
}

async fn nothing(_: i64) -> AppResult<()> {
    Ok(())
}

// ---------------------------------------------------------------- context

fn format_curriculum(items: &[CurriculumItem]) -> String {
    if items.is_empty() {
        return "（知識庫目前沒有適用的課綱條目）".to_string();
    }
    items
        .iter()
        .map(|i| {
            format!(
                "{}｜{}｜{}",
                i.item_code,
                i.title,
                clip_chars(&i.content, 300)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `[第 n 段]` paragraphs, at most [`MAX_AI_PARAGRAPHS`] of them.
fn passage(m: &Material) -> String {
    let shown: Vec<String> = m
        .paragraphs
        .iter()
        .take(MAX_AI_PARAGRAPHS)
        .enumerate()
        .map(|(i, p)| format!("[第 {} 段] {}", i + 1, p.text))
        .collect();
    let mut out = shown.join("\n\n");
    if m.paragraphs.len() > MAX_AI_PARAGRAPHS {
        out.push_str(&format!(
            "\n\n（教材共 {} 段，這裡只列出前 {MAX_AI_PARAGRAPHS} 段。）",
            m.paragraphs.len()
        ));
    }
    out
}

fn material_title(m: &Material) -> String {
    if m.title.trim().is_empty() {
        "未命名教材".to_string()
    } else {
        m.title.trim().to_string()
    }
}

/// The 語言目標 kinds of the teacher's latest drafts, newest first: the kind each one put first.
async fn recent_categories(db: &sqlx::PgPool, teacher_id: i64) -> sqlx::Result<Vec<String>> {
    let outputs: Vec<Value> = sqlx::query_scalar(
        "SELECT output FROM ai_interactions
          WHERE teacher_id = $1 AND module = $2 AND status = 'shown' AND output IS NOT NULL
          ORDER BY id DESC LIMIT $3",
    )
    .bind(teacher_id)
    .bind(KEY_DRAFT)
    .bind(RECENT_DRAFTS)
    .fetch_all(db)
    .await?;
    Ok(outputs
        .into_iter()
        .filter_map(|o| serde_json::from_value::<RawDrafts>(o).ok())
        .filter_map(|d| {
            let cats: Vec<&str> = d
                .language_objectives
                .iter()
                .map(|l| l.category.as_str())
                .collect();
            if cats.contains(&STRATEGY) {
                Some(STRATEGY.to_string())
            } else {
                cats.first().map(|c| c.to_string())
            }
        })
        .collect())
}

/// R2 within a unit: a 學習策略 first, unless the last two drafts both put one first.
pub fn strategy_first(recent: &[String]) -> bool {
    !(recent.len() >= 2 && recent.iter().take(2).all(|c| c == STRATEGY))
}

fn recent_text(recent: &[String]) -> String {
    if recent.is_empty() {
        return "（這是第一次草擬）".to_string();
    }
    recent
        .iter()
        .map(|c| super::category_label(c))
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("、")
}

fn variety_note(strategy_first: bool) -> String {
    if strategy_first {
        "第一個語言目標先考慮學習策略。".to_string()
    } else {
        format!(
            "最近兩次的語言目標都以學習策略為主：這次第一個語言目標請改從{}中挑一類，讓一個單元裡的語言目標有變化。",
            CATEGORIES[1..]
                .iter()
                .map(|(_, l)| *l)
                .collect::<Vec<_>>()
                .join("、")
        )
    }
}

/// A draft as the judge sees it.
fn draft_text(d: &Draft) -> String {
    let mut text = match d.kind {
        ObjectiveType::Content => format!("{}（內容目標）{}\n  中文：{}", d.code, d.text, d.zh),
        ObjectiveType::Language => format!(
            "{}（語言目標，{}，支持 {}）{}\n  中文：{}\n  關係：{}",
            d.code,
            super::category_label(d.category.as_deref().unwrap_or_default()),
            d.supports.as_deref().unwrap_or_default(),
            d.text,
            d.zh,
            d.relation_zh
        ),
    };
    if !d.check_zh.is_empty() {
        text.push_str(&format!("\n  怎麼檢核：{}", d.check_zh));
    }
    text
}

// ---------------------------------------------------------------- the rounds

struct Round {
    draft_id: i64,
    rules: RuleOutcome,
    /// The drafts that passed R3 (empty when the judge did not answer).
    shown: Vec<Draft>,
    /// Why the others were kept back.
    hidden: Vec<String>,
    verdicts: Vec<Verdict>,
    judge_status: Option<&'static str>,
}

impl Round {
    fn has(&self, kind: ObjectiveType) -> bool {
        self.shown.iter().any(|d| d.kind == kind)
    }

    fn complete(&self) -> bool {
        self.has(ObjectiveType::Content) && self.has(ObjectiveType::Language)
    }

    fn hidden_count(&self) -> usize {
        self.rules.dropped.len() + self.hidden.len()
    }

    fn report(&self, shown: bool) -> Value {
        let mut results = vec![CheckResult::pass("schema")];
        results.push(if self.rules.dropped.is_empty() {
            CheckResult::pass("r1_rules")
        } else {
            CheckResult::fail("r1_rules", self.rules.dropped.join("；"))
        });
        results.push(match self.judge_status {
            Some(_) => CheckResult::fail("r3_judge", "審查沒有完成".to_string()),
            None if self.hidden.is_empty() => CheckResult::pass("r3_judge"),
            None => CheckResult::fail("r3_judge", self.hidden.join("；")),
        });
        let mut report = base_checks::report(&results);
        report["repairs"] = json!(self.rules.repairs);
        report["shown"] = json!(self.shown.iter().map(|d| &d.code).collect::<Vec<_>>());
        report["verdicts"] = json!(self.verdicts);
        report["round_shown"] = json!(shown);
        report
    }
}

struct Ctx<'a> {
    input: &'a DraftInput,
    vars: HashMap<&'static str, String>,
    strategy_first: bool,
    codes: Vec<String>,
    query: String,
    trace: retrieval::Trace,
    class_snapshot: Value,
    progress: Arc<Tracker>,
}

// ---------------------------------------------------------------- progress (docs/API.md「產生進度」)

const DRAFT: &str = "draft";
const CHECK: &str = "check";
const DRAFT_AGAIN: &str = "draft2";
const CHECK_AGAIN: &str = "check2";

/// The steps of drafting objectives, as the generator page shows them while it waits.
pub fn steps() -> Vec<Step> {
    vec![
        Step::new(progress::READ, progress::L_READ),
        Step::new(progress::CURRICULUM, progress::L_CURRICULUM),
        Step::new(DRAFT, "草擬學習目標"),
        Step::new(CHECK, "檢查目標"),
    ]
}

/// What a failed call says (the same words as the result's `message`).
fn failed_message(status: &str) -> &'static str {
    match status {
        "timeout" => MSG_TIMEOUT,
        "error" => MSG_ERROR,
        "disabled" => MSG_DISABLED,
        _ => MSG_NONE,
    }
}

/// One round: the draft (retried once inside when the rules leave a group empty, `retry_rules`)
/// and the judge.
async fn round(
    state: &AppState,
    ctx: &Ctx<'_>,
    no: u8,
    redo: Option<String>,
) -> AppResult<Result<Round, (&'static str, Option<i64>)>> {
    let input = ctx.input;
    let db = &state.db;
    let (draft_key, check_key) = if no == 1 {
        (DRAFT, CHECK)
    } else {
        ctx.progress
            .add(Step::new(DRAFT_AGAIN, "重新草擬沒通過的目標"))
            .await;
        ctx.progress.add(Step::new(CHECK_AGAIN, "再檢查一次")).await;
        (DRAFT_AGAIN, CHECK_AGAIN)
    };
    ctx.progress.start(draft_key).await;
    let mut vars = ctx.vars.clone();
    vars.insert("redo_note", redo.clone().unwrap_or_default());
    let strategy_first = ctx.strategy_first;
    let retry_rules = no == 1;
    let spec = Spec {
        key: KEY_DRAFT,
        scaffold: SCAFFOLD_DRAFT,
        schema_name: DRAFT_SCHEMA_NAME,
        schema: draft_schema(),
        vars,
        required: vec![
            ("purpose", "這一課要做"),
            ("class_profile", "班級特性"),
            ("teacher_request", "教師的要求"),
        ],
        fake_context: json!({
            "task": KEY_DRAFT,
            "kind": input.kind,
            "material_title": input.material.title,
            "paragraphs": input.material.paragraphs.iter().map(|p| p.text.clone()).collect::<Vec<_>>(),
            "round": no,
            "avoid": input.avoid,
            "strategy_first": strategy_first,
        }),
        max_tokens: DRAFT_MAX_TOKENS,
        trace: json!({
            "kind": input.kind, "round": no, "class_context": ctx.class_snapshot,
            "curriculum_codes": ctx.codes, "strategy_first": strategy_first,
        }),
        progress: Some((ctx.progress.clone(), draft_key)),
    };
    let logged_query = clip_chars(&prompts::redacted("草擬目標的教材", &ctx.query), 2000);
    let drafted = call(
        state,
        input.teacher_id,
        spec,
        |raw: &RawDrafts| {
            let rules = check_rules(raw, strategy_first);
            let problems = if retry_rules && !rules.complete() {
                let mut p = rules.dropped.clone();
                if !rules.has(ObjectiveType::Content) {
                    p.push("沒有可用的內容目標：content_objectives 至少要有 1 個通過檢查。".to_string());
                }
                if !rules.has(ObjectiveType::Language) {
                    p.push("沒有可用的語言目標：language_objectives 至少要有 1 個通過檢查，並支持一個可用的內容目標。".to_string());
                }
                p
            } else {
                Vec::new()
            };
            let usable = !rules.drafts.is_empty();
            (usable.then_some(rules), problems)
        },
        async |interaction_id| {
            if no == 1 {
                retrieval::log(
                    db,
                    Some(interaction_id),
                    &logged_query,
                    &input.track,
                    &ctx.codes,
                    &ctx.trace,
                )
                .await?;
            }
            Ok(())
        },
    )
    .await?;
    let (rules, draft_id) = match drafted {
        Called::Shown { output, id } => (output, id),
        Called::Failed { status, id } => {
            ctx.progress.fail(draft_key, failed_message(status)).await;
            return Ok(Err((status, id)));
        }
    };
    ctx.progress.done(draft_key).await;
    ctx.progress.start(check_key).await;

    // ---- the judge: every draft on its own (R3)
    let codes: Vec<String> = rules.drafts.iter().map(|d| d.code.clone()).collect();
    let judge_vars: HashMap<&'static str, String> = HashMap::from([
        ("class_profile", ctx.vars["class_profile"].clone()),
        ("purpose", ctx.vars["purpose"].clone()),
        ("objective_count", codes.len().to_string()),
        (
            "objectives",
            rules
                .drafts
                .iter()
                .map(draft_text)
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        ("curriculum_items", ctx.vars["curriculum_items"].clone()),
        ("material_title", ctx.vars["material_title"].clone()),
        ("passage_numbered", ctx.vars["passage_numbered"].clone()),
    ]);
    let judge = Spec {
        key: KEY_CHECK,
        scaffold: SCAFFOLD_CHECK,
        schema_name: CHECK_SCHEMA_NAME,
        schema: check_schema(&codes),
        vars: judge_vars,
        required: vec![("objectives", "要檢查的目標")],
        fake_context: json!({
            "task": KEY_CHECK,
            "round": no,
            "items": rules.drafts.iter()
                .map(|d| json!({ "id": d.code, "kind": d.kind, "text": d.text }))
                .collect::<Vec<_>>(),
        }),
        max_tokens: CHECK_MAX_TOKENS,
        trace: json!({ "kind": input.kind, "round": no, "draft_interaction_id": draft_id }),
        progress: Some((ctx.progress.clone(), check_key)),
    };
    let drafts = rules.drafts.clone();
    let judged = call(
        state,
        input.teacher_id,
        judge,
        |raw: &RawCheck| {
            let (verdicts, missing) = evaluate_check(raw, &drafts);
            let problems = missing
                .iter()
                .map(|c| format!("{c} 沒有判斷：每一個目標都要有一筆。"))
                .collect();
            ((!verdicts.is_empty()).then_some(verdicts), problems)
        },
        nothing,
    )
    .await?;
    let mut r = Round {
        draft_id,
        rules,
        shown: Vec::new(),
        hidden: Vec::new(),
        verdicts: Vec::new(),
        judge_status: None,
    };
    match judged {
        Called::Shown { output, .. } => {
            ctx.progress.done(check_key).await;
            let (shown, hidden) = passing(&r.rules.drafts, &output);
            r.shown = shown;
            r.hidden = hidden;
            r.verdicts = output;
        }
        // G3: what was not judged is not shown.
        Called::Failed { status, .. } => {
            ctx.progress.fail(check_key, failed_message(status)).await;
            r.hidden = r
                .rules
                .drafts
                .iter()
                .map(|d| format!("{}：沒有完成檢查", d.code))
                .collect();
            r.judge_status = Some(status);
        }
    }
    Ok(Ok(r))
}

/// What the second round is told: what the checks found, and what passed (it may stay).
fn redo_note(r: &Round) -> String {
    let lines: Vec<String> = r
        .rules
        .dropped
        .iter()
        .chain(&r.hidden)
        .map(|h| format!("- {}", clip_chars(h, 200)))
        .collect();
    let kept: Vec<String> = r
        .shown
        .iter()
        .map(|d| format!("- {}：{}", d.code, d.text))
        .collect();
    let mut note = format!(
        "\n\n【重新草擬】上一次草擬的目標有些沒有通過檢查：\n{}\n請重新草擬一組，兩組目標都要有，並修正上面的問題。",
        lines.join("\n")
    );
    if !kept.is_empty() {
        note.push_str(&format!(
            "\n下列目標已經通過，可以照原句保留（代號可能改變）：\n{}",
            kept.join("\n")
        ));
    }
    note
}

/// Drafts the objectives (G2) and shows only the ones that pass R1 and R3 (G3).
///
/// `progress` hears every step change (docs/API.md「產生進度」); the caller settles it with the
/// result.
pub async fn run(
    state: &AppState,
    input: DraftInput,
    progress: Arc<Tracker>,
) -> AppResult<DraftResult> {
    let db = &state.db;
    let profile =
        crate::routes::resolved_profile(db, input.teacher_id, input.class_id, "", "").await?;
    let query = clip_chars(
        &format!("{}\n{}", input.material.title, input.material.text()),
        4000,
    );
    let mut context = ContextList::default();
    context
        .add("教材", &input.material.label())
        .add(&profile.context_label(), &profile.text)
        .add("教師的要求", &clip_chars(&input.request, 80));
    if !state.ai.enabled() {
        return Ok(DraftResult::failed("disabled", None, context.into_vec()));
    }
    progress.done(progress::READ).await;
    progress.start(progress::CURRICULUM).await;
    let (items, trace) = curriculum::items_for_track_hybrid(
        db,
        &state.embedder,
        &input.track,
        &query,
        CURRICULUM_TOP,
    )
    .await?;
    progress.done(progress::CURRICULUM).await;
    let codes: Vec<String> = items.iter().map(|i| i.item_code.clone()).collect();
    if !items.is_empty() {
        context.add("可對應的課綱條目", &format!("{} 條", items.len()));
    }
    let context = context.into_vec();
    let recent = recent_categories(db, input.teacher_id).await?;
    let strategy_first = strategy_first(&recent);
    let avoid_note = if input.avoid.is_empty() {
        String::new()
    } else {
        format!(
            "教師想換一組目標，請不要重複這些：\n{}",
            input
                .avoid
                .iter()
                .map(|a| format!("- {a}"))
                .collect::<Vec<_>>()
                .join("\n")
        )
    };
    let vars: HashMap<&'static str, String> = HashMap::from([
        ("purpose", purpose(&input.kind).to_string()),
        ("school_track", track_label(&input.track).to_string()),
        ("class_profile", or_blank(&profile.text)),
        (
            "teacher_request",
            if input.request.trim().is_empty() {
                super::NONE.to_string()
            } else {
                input.request.trim().to_string()
            },
        ),
        ("recent_categories", recent_text(&recent)),
        ("variety_note", variety_note(strategy_first)),
        ("avoid_note", avoid_note),
        ("curriculum_items", format_curriculum(&items)),
        ("material_title", material_title(&input.material)),
        (
            "paragraph_count",
            input.material.paragraphs.len().to_string(),
        ),
        ("passage_numbered", passage(&input.material)),
    ]);
    let ctx = Ctx {
        input: &input,
        vars,
        strategy_first,
        codes,
        query,
        trace,
        class_snapshot: profile.snapshot(),
        progress,
    };

    let first = match round(state, &ctx, 1, None).await? {
        Ok(r) => r,
        Err((status, id)) => return Ok(DraftResult::failed(status, id, context)),
    };
    // G3: drafts that fail are not shown; a group left without one is drafted once more.
    let (chosen, other) = if first.complete() {
        (first, None)
    } else {
        match round(state, &ctx, 2, Some(redo_note(&first))).await? {
            Ok(second)
                if second.complete()
                    || (!first.complete() && second.shown.len() > first.shown.len()) =>
            {
                (second, Some(first))
            }
            Ok(second) => (first, Some(second)),
            Err(_) => (first, None),
        }
    };
    if let Some(o) = &other {
        set_final(db, o.draft_id, "blocked", o.report(false)).await?;
    }
    let status = if chosen.shown.is_empty() {
        "blocked"
    } else {
        "shown"
    };
    set_final(db, chosen.draft_id, status, chosen.report(true)).await?;
    let message = if chosen.shown.is_empty() {
        match chosen.judge_status {
            Some("timeout") => Some(MSG_TIMEOUT),
            Some("error") | Some("disabled") => Some(MSG_ERROR),
            _ => Some(MSG_NONE),
        }
    } else if !chosen.has(ObjectiveType::Language) {
        Some(MSG_NO_LANGUAGE)
    } else if !chosen.has(ObjectiveType::Content) {
        Some(MSG_NO_CONTENT)
    } else {
        None
    };
    let status = match (status, chosen.judge_status) {
        ("blocked", Some(s)) => s,
        (s, _) => s,
    };
    Ok(DraftResult {
        interaction_id: Some(chosen.draft_id),
        status,
        message: message.map(str::to_string),
        hidden: chosen.hidden_count(),
        objectives: chosen.shown,
        context,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strategy_comes_first_unless_the_last_two_drafts_had_it() {
        assert!(strategy_first(&[]));
        assert!(strategy_first(&["strategy".into()]));
        assert!(strategy_first(&["strategy".into(), "function".into()]));
        assert!(!strategy_first(&["strategy".into(), "strategy".into()]));
        assert!(variety_note(false).contains("語言功能"));
        assert_eq!(
            recent_text(&["strategy".into(), "vocabulary".into()]),
            "學習策略、學術字彙"
        );
    }

    #[test]
    fn every_tool_has_its_own_purpose() {
        let all: std::collections::HashSet<&str> =
            ["comprehension", "tiered", "activity", "kit", "worksheet"]
                .into_iter()
                .map(purpose)
                .collect();
        assert_eq!(all.len(), 5);
    }
}
