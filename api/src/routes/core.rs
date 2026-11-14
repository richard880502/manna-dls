//! CoRe (spec §4.1): focuses × Q1–Q8, drafts with per-cell revision history, completion, and the
//! three-level "需要協助" AI assist.

use std::collections::{BTreeMap, HashMap, HashSet};

use axum::extract::State;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;

use crate::ai::pipeline::{self, AiResult, ContextList, InteractionMeta};
use crate::ai::schemas::{self, CoreAssistOutput};
use crate::ai::LlmRequest;
use crate::auth::AuthTeacher;
use crate::checks;
use crate::core_questions::{self, CoreQuestion, CORE_QUESTIONS, QUESTION_IDS};
use crate::error::{ApiJson, ApiPath, AppError, AppResult};
use crate::preferences::{self, Hardest};
use crate::prompts;
use crate::routes::preferences::{preference_summary, snapshot as pref_snapshot};
use crate::routes::{or_blank, owned_unit, resolved_profile, scaffold_id, track_label};
use crate::AppState;

pub const MAX_FOCUSES: usize = 3;

pub async fn questions() -> Json<&'static [CoreQuestion]> {
    Json(&CORE_QUESTIONS)
}

// ---------------------------------------------------------------- read model

#[derive(Debug, Serialize, Clone)]
pub struct CoreResponseOut {
    pub answer: String,
    pub status: String,
    pub content_source: String,
    pub updated_at: Option<DateTime<Utc>>,
}

impl Default for CoreResponseOut {
    fn default() -> Self {
        Self {
            answer: String::new(),
            status: "blank".into(),
            content_source: "self".into(),
            updated_at: None,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct CoreFocusOut {
    pub id: i64,
    pub position: i32,
    pub big_idea: String,
    pub responses: BTreeMap<String, CoreResponseOut>,
}

#[derive(Debug, Serialize)]
pub struct CoreVersionOut {
    pub id: i64,
    pub unit_id: i64,
    pub phase: String,
    pub version_no: i32,
    pub completed_at: Option<DateTime<Utc>>,
    pub saw_ai_assist: bool,
    pub focuses: Vec<CoreFocusOut>,
}

#[derive(Debug, sqlx::FromRow)]
struct VersionRow {
    id: i64,
    unit_id: i64,
    phase: String,
    version_no: i32,
    completed_at: Option<DateTime<Utc>>,
    saw_ai_assist: bool,
}

#[derive(Debug, sqlx::FromRow)]
struct FocusRow {
    id: i64,
    position: i32,
    big_idea: String,
}

#[derive(Debug, sqlx::FromRow)]
struct ResponseRow {
    core_focus_id: i64,
    question_id: String,
    answer: String,
    status: String,
    content_source: String,
    updated_at: DateTime<Utc>,
}

async fn latest_pre(db: &PgPool, unit_id: i64) -> sqlx::Result<Option<VersionRow>> {
    sqlx::query_as(
        "SELECT id, unit_id, phase, version_no, completed_at, saw_ai_assist
           FROM core_versions WHERE unit_id = $1 AND phase = 'pre'
          ORDER BY version_no DESC LIMIT 1",
    )
    .bind(unit_id)
    .fetch_optional(db)
    .await
}

/// The unit's latest pre-unit version, creating an empty version 1 on first access.
async fn ensure_pre(db: &PgPool, unit_id: i64) -> AppResult<VersionRow> {
    if let Some(v) = latest_pre(db, unit_id).await? {
        return Ok(v);
    }
    sqlx::query(
        "INSERT INTO core_versions (unit_id, phase, version_no) VALUES ($1, 'pre', 1)
         ON CONFLICT (unit_id, phase, version_no) DO NOTHING",
    )
    .bind(unit_id)
    .execute(db)
    .await?;
    latest_pre(db, unit_id)
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("core version not created")))
}

async fn focus_rows(db: &PgPool, version_id: i64) -> sqlx::Result<Vec<FocusRow>> {
    sqlx::query_as(
        "SELECT id, position, big_idea FROM core_focuses
          WHERE core_version_id = $1 AND deleted_at IS NULL
          ORDER BY position, id",
    )
    .bind(version_id)
    .fetch_all(db)
    .await
}

async fn response_rows(db: &PgPool, focus_ids: &[i64]) -> sqlx::Result<Vec<ResponseRow>> {
    sqlx::query_as(
        "SELECT core_focus_id, question_id, answer, status, content_source, updated_at
           FROM core_responses WHERE core_focus_id = ANY($1)",
    )
    .bind(focus_ids)
    .fetch_all(db)
    .await
}

/// All eight questions for each focus, defaulting to blank.
fn responses_by_focus(rows: Vec<ResponseRow>) -> HashMap<i64, BTreeMap<String, CoreResponseOut>> {
    let mut map: HashMap<i64, BTreeMap<String, CoreResponseOut>> = HashMap::new();
    for r in rows {
        map.entry(r.core_focus_id).or_default().insert(
            r.question_id,
            CoreResponseOut {
                answer: r.answer,
                status: r.status,
                content_source: r.content_source,
                updated_at: Some(r.updated_at),
            },
        );
    }
    map
}

fn full_responses(
    mut partial: BTreeMap<String, CoreResponseOut>,
) -> BTreeMap<String, CoreResponseOut> {
    for q in QUESTION_IDS {
        partial.entry(q.to_string()).or_default();
    }
    partial
}

async fn load_version(db: &PgPool, v: VersionRow) -> AppResult<CoreVersionOut> {
    let focuses = focus_rows(db, v.id).await?;
    let ids: Vec<i64> = focuses.iter().map(|f| f.id).collect();
    let mut responses = responses_by_focus(response_rows(db, &ids).await?);
    Ok(CoreVersionOut {
        id: v.id,
        unit_id: v.unit_id,
        phase: v.phase,
        version_no: v.version_no,
        completed_at: v.completed_at,
        saw_ai_assist: v.saw_ai_assist,
        focuses: focuses
            .into_iter()
            .map(|f| CoreFocusOut {
                responses: full_responses(responses.remove(&f.id).unwrap_or_default()),
                id: f.id,
                position: f.position,
                big_idea: f.big_idea,
            })
            .collect(),
    })
}

pub async fn get_core(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(unit_id): ApiPath<i64>,
) -> AppResult<Json<CoreVersionOut>> {
    owned_unit(&state.db, unit_id, teacher.id()).await?;
    let v = ensure_pre(&state.db, unit_id).await?;
    Ok(Json(load_version(&state.db, v).await?))
}

// ---------------------------------------------------------------- draft

#[derive(Debug, Deserialize)]
pub struct DraftRequest {
    pub focuses: Vec<DraftFocus>,
}

#[derive(Debug, Deserialize)]
pub struct DraftFocus {
    pub id: Option<i64>,
    pub position: i32,
    #[serde(default)]
    pub big_idea: String,
    #[serde(default)]
    pub responses: HashMap<String, DraftResponse>,
}

fn default_source() -> String {
    "self".into()
}

#[derive(Debug, Deserialize)]
pub struct DraftResponse {
    #[serde(default)]
    pub answer: String,
    pub status: String,
    #[serde(default = "default_source")]
    pub content_source: String,
}

pub fn valid_status(s: &str) -> bool {
    matches!(s, "filled" | "blank" | "no_idea")
}

pub fn valid_content_source(s: &str) -> bool {
    matches!(
        s,
        "self" | "self_after_ai" | "modified_ai_example" | "adopted_ai_example"
    )
}

/// Normalizes a saved cell: 「目前沒有想法」 is a status only and stores no answer text
/// (spec §4.1); otherwise the status follows the text (empty → blank, else filled).
pub fn normalize_cell(answer: &str, status: &str) -> (String, &'static str) {
    if status == "no_idea" {
        (String::new(), "no_idea")
    } else if answer.trim().is_empty() {
        (String::new(), "blank")
    } else {
        (answer.to_string(), "filled")
    }
}

fn validate_draft(req: &DraftRequest) -> AppResult<()> {
    if req.focuses.len() > MAX_FOCUSES {
        return Err(AppError::bad_request("每個單元最多 3 個學習重點。"));
    }
    let mut ids = HashSet::new();
    for f in &req.focuses {
        if let Some(id) = f.id {
            if !ids.insert(id) {
                return Err(AppError::bad_request("同一個學習重點出現了兩次。"));
            }
        }
        for (qid, r) in &f.responses {
            if core_questions::find(qid).is_none() {
                return Err(AppError::bad_request(format!("題號 {qid} 不正確。")));
            }
            if !valid_status(&r.status) {
                return Err(AppError::bad_request(format!("{qid} 的狀態值不正確。")));
            }
            if !valid_content_source(&r.content_source) {
                return Err(AppError::bad_request(format!("{qid} 的內容來源不正確。")));
            }
        }
    }
    Ok(())
}

pub async fn save_draft(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(unit_id): ApiPath<i64>,
    ApiJson(req): ApiJson<DraftRequest>,
) -> AppResult<Json<CoreVersionOut>> {
    owned_unit(&state.db, unit_id, teacher.id()).await?;
    validate_draft(&req)?;
    let version = ensure_pre(&state.db, unit_id).await?;

    let mut tx = state.db.begin().await?;
    sqlx::query("SELECT id FROM core_versions WHERE id = $1 FOR UPDATE")
        .bind(version.id)
        .execute(&mut *tx)
        .await?;
    let active: HashSet<i64> = sqlx::query_scalar::<_, i64>(
        "SELECT id FROM core_focuses WHERE core_version_id = $1 AND deleted_at IS NULL",
    )
    .bind(version.id)
    .fetch_all(&mut *tx)
    .await?
    .into_iter()
    .collect();
    let keep: Vec<i64> = req.focuses.iter().filter_map(|f| f.id).collect();
    if let Some(bad) = keep.iter().find(|id| !active.contains(id)) {
        return Err(AppError::bad_request(format!(
            "學習重點 {bad} 不屬於這個單元的 CoRe。"
        )));
    }
    // Focuses missing from the request are removed (soft delete).
    sqlx::query(
        "UPDATE core_focuses SET deleted_at = now()
          WHERE core_version_id = $1 AND deleted_at IS NULL AND NOT (id = ANY($2))",
    )
    .bind(version.id)
    .bind(&keep)
    .execute(&mut *tx)
    .await?;

    for f in &req.focuses {
        let focus_id: i64 = match f.id {
            Some(id) => {
                sqlx::query("UPDATE core_focuses SET position = $2, big_idea = $3 WHERE id = $1")
                    .bind(id)
                    .bind(f.position)
                    .bind(&f.big_idea)
                    .execute(&mut *tx)
                    .await?;
                id
            }
            None => {
                sqlx::query_scalar(
                    "INSERT INTO core_focuses (core_version_id, position, big_idea)
                     VALUES ($1, $2, $3) RETURNING id",
                )
                .bind(version.id)
                .bind(f.position)
                .bind(&f.big_idea)
                .fetch_one(&mut *tx)
                .await?
            }
        };
        let existing: HashMap<String, (String, String, String)> =
            sqlx::query_as::<_, (String, String, String, String)>(
                "SELECT question_id, answer, status, content_source
                   FROM core_responses WHERE core_focus_id = $1",
            )
            .bind(focus_id)
            .fetch_all(&mut *tx)
            .await?
            .into_iter()
            .map(|(q, a, s, c)| (q, (a, s, c)))
            .collect();

        let mut qids: Vec<&String> = f.responses.keys().collect();
        qids.sort();
        for qid in qids {
            let r = &f.responses[qid];
            let (answer, status) = normalize_cell(&r.answer, &r.status);
            let new = (answer, status.to_string(), r.content_source.clone());
            match existing.get(qid.as_str()) {
                Some(old) if *old == new => continue,
                None if new.1 == "blank" && new.2 == "self" => continue,
                _ => {}
            }
            let response_id: i64 = sqlx::query_scalar(
                "INSERT INTO core_responses (core_focus_id, question_id, answer, status, content_source)
                 VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT (core_focus_id, question_id) DO UPDATE
                   SET answer = EXCLUDED.answer, status = EXCLUDED.status,
                       content_source = EXCLUDED.content_source, updated_at = now()
                 RETURNING id",
            )
            .bind(focus_id)
            .bind(qid)
            .bind(&new.0)
            .bind(&new.1)
            .bind(&new.2)
            .fetch_one(&mut *tx)
            .await?;
            // Every change is kept (spec §3: content is versioned, never overwritten).
            sqlx::query(
                "INSERT INTO core_response_revisions (core_response_id, answer, status, content_source)
                 VALUES ($1, $2, $3, $4)",
            )
            .bind(response_id)
            .bind(&new.0)
            .bind(&new.1)
            .bind(&new.2)
            .execute(&mut *tx)
            .await?;
        }
    }
    tx.commit().await?;

    let v = ensure_pre(&state.db, unit_id).await?;
    Ok(Json(load_version(&state.db, v).await?))
}

pub async fn complete(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(unit_id): ApiPath<i64>,
) -> AppResult<Json<CoreVersionOut>> {
    owned_unit(&state.db, unit_id, teacher.id()).await?;
    let v = ensure_pre(&state.db, unit_id).await?;
    // Blanks are allowed (spec §4.1, §6.4). The first completion time is kept.
    sqlx::query(
        "UPDATE core_versions SET completed_at = COALESCE(completed_at, now()) WHERE id = $1",
    )
    .bind(v.id)
    .execute(&state.db)
    .await?;
    let v = ensure_pre(&state.db, unit_id).await?;
    Ok(Json(load_version(&state.db, v).await?))
}

// ---------------------------------------------------------------- assist

/// Level-2 emphasis when the teacher's P3 includes 學習目標 (docs/API.md Phase 1.1).
pub const GOAL_EMPHASIS: &str = "這位老師表示「學習目標」是備課時最花時間或最困難的部分。這次提出的面向請優先涵蓋三個角度：選這篇課文或教材的理由、語言以外的內容目標（例如主題知識、思考能力、文化內容）、與課綱學習表現及考點的連結。每個角度仍然只說明可以從哪裡想，不替老師寫出目標或答案。";
pub const NO_EMPHASIS: &str = "（這位老師沒有指定要特別強調的面向，依學習重點本身提出即可。）";

#[derive(Debug, Deserialize)]
pub struct AssistRequest {
    pub unit_id: i64,
    pub focus_id: i64,
    pub question_id: String,
    pub level: u8,
    /// The class the teacher is working in (Phase 1.4), optional.
    #[serde(default)]
    pub class_id: Option<i64>,
}

#[derive(Debug, sqlx::FromRow)]
struct AssistFocus {
    id: i64,
    big_idea: String,
    core_version_id: i64,
}

fn describe_cell(r: Option<&CoreResponseOut>) -> String {
    match r {
        Some(r) if r.status == "filled" => r.answer.clone(),
        Some(r) if r.status == "no_idea" => "（老師選了「目前沒有想法」）".into(),
        _ => "（尚未填寫）".into(),
    }
}

pub async fn assist(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiJson(req): ApiJson<AssistRequest>,
) -> AppResult<Json<AiResult<CoreAssistOutput>>> {
    let unit = owned_unit(&state.db, req.unit_id, teacher.id()).await?;
    let profile = resolved_profile(
        &state.db,
        teacher.id(),
        req.class_id,
        "",
        &unit.class_profile,
    )
    .await?;
    if !(1..=3).contains(&req.level) {
        return Err(AppError::bad_request("協助層級必須是 1、2 或 3。"));
    }
    let question = core_questions::find(&req.question_id)
        .ok_or_else(|| AppError::bad_request("題號不正確。"))?;
    let focus: AssistFocus = sqlx::query_as(
        "SELECT f.id, f.big_idea, f.core_version_id
           FROM core_focuses f JOIN core_versions v ON v.id = f.core_version_id
          WHERE f.id = $1 AND v.unit_id = $2 AND f.deleted_at IS NULL",
    )
    .bind(req.focus_id)
    .bind(unit.id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::not_found("找不到這個學習重點。"))?;

    // Asking for level N+1 means the teacher continued after level N.
    if req.level > 1 {
        sqlx::query(
            "UPDATE core_assist_logs SET continued = true
              WHERE id = (SELECT id FROM core_assist_logs
                           WHERE core_version_id = $1 AND core_focus_id = $2
                             AND question_id = $3 AND level = $4
                           ORDER BY created_at DESC, id DESC LIMIT 1)",
        )
        .bind(focus.core_version_id)
        .bind(focus.id)
        .bind(question.id)
        .bind(i32::from(req.level - 1))
        .execute(&state.db)
        .await?;
    }

    let log = |interaction_id: Option<i64>| {
        sqlx::query(
            "INSERT INTO core_assist_logs (core_version_id, core_focus_id, question_id, level, ai_interaction_id)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(focus.core_version_id)
        .bind(focus.id)
        .bind(question.id)
        .bind(i32::from(req.level))
        .bind(interaction_id)
        .execute(&state.db)
    };

    if !state.ai.enabled() {
        log(None).await?;
        return Ok(Json(AiResult::disabled()));
    }

    let key = prompts::KEY_CORE_ASSIST[usize::from(req.level - 1)];
    let prompt = prompts::latest(&state.db, key)
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("no prompt version for {key}")))?;

    let mut responses = responses_by_focus(response_rows(&state.db, &[focus.id]).await?)
        .remove(&focus.id)
        .unwrap_or_default();
    let current = responses.remove(question.id);
    let teacher_answer = describe_cell(current.as_ref());
    let others: Vec<String> = CORE_QUESTIONS
        .iter()
        .filter(|q| q.id != question.id)
        .filter_map(|q| {
            let r = responses.get(q.id)?;
            (r.status != "blank")
                .then(|| format!("{} {}：{}", q.id, q.prompt, describe_cell(Some(r))))
        })
        .collect();
    let other_answers = if others.is_empty() {
        "（尚未填寫）".to_string()
    } else {
        others.join("\n")
    };
    let big_idea = if focus.big_idea.trim().is_empty() {
        "（尚未填寫）".to_string()
    } else {
        focus.big_idea.clone()
    };
    let prefs = preferences::latest_answers(&state.db, teacher.id()).await?;
    let goal_emphasis = req.level == 2 && prefs.has_hardest(Hardest::Goal);
    let class_profile = profile.text.clone();

    let vars: HashMap<&str, String> = HashMap::from([
        ("unit_title", unit.title.clone()),
        ("school_track", track_label(teacher.track()).to_string()),
        ("core_preamble", core_questions::CORE_PREAMBLE.to_string()),
        ("big_idea", big_idea.clone()),
        ("question_id", question.id.to_string()),
        ("question_prompt", question.prompt.to_string()),
        ("question_help", question.help.to_string()),
        ("teacher_answer", teacher_answer.clone()),
        ("other_answers", other_answers.clone()),
        ("level", req.level.to_string()),
        ("class_profile", or_blank(&class_profile)),
        ("detail_instruction", prefs.detail_instruction().to_string()),
        (
            "focus_emphasis",
            if goal_emphasis {
                GOAL_EMPHASIS
            } else {
                NO_EMPHASIS
            }
            .to_string(),
        ),
    ]);
    let mut required = vec![
        ("class_profile", "班級特性"),
        ("detail_instruction", "回覆長度"),
    ];
    if goal_emphasis {
        required.push(("focus_emphasis", "這次請特別強調"));
    }
    let system = prompts::render(&prompt.system_text, &vars);
    let user = prompts::render_user_with(&prompt, &vars, &required);
    let snapshot = json!({
        "inputs": vars,
        "preferences": pref_snapshot(&prefs),
        "class_context": profile.snapshot(),
        "prompt": { "key": key, "version_no": prompt.version_no, "system": system, "user": user },
        "schema": schemas::CORE_ASSIST_SCHEMA_NAME,
    });
    let meta = InteractionMeta {
        teacher_id: teacher.id(),
        module: "core_assist",
        unit_id: Some(unit.id),
        lesson_id: None,
        scaffold_id: scaffold_id(&state.db, "SC-CORE-HELP").await?,
        prompt_version_id: Some(prompt.id),
    };
    let first_id = pipeline::insert_interaction(
        &state.db,
        &meta,
        state.ai.provider_name(),
        &state.ai.model,
        &snapshot,
        1,
        None,
    )
    .await?;
    let llm_req = LlmRequest {
        system,
        user,
        schema_name: schemas::CORE_ASSIST_SCHEMA_NAME,
        schema: schemas::core_assist_schema(),
        fake_context: json!({
            "task": "core_assist",
            "level": req.level,
            "question_help": question.help,
            "big_idea": focus.big_idea,
            "teacher_answer": current.map(|c| c.answer).unwrap_or_default(),
            "goal_emphasis": goal_emphasis,
        }),
        attempt: 1,
    };
    let level = req.level;
    let outcome = pipeline::run(
        &state,
        &meta,
        first_id,
        &snapshot,
        llm_req,
        |raw: &Value| {
            pipeline::parse_and_check::<CoreAssistOutput>(raw, |o| {
                (checks::check_core_assist(o, level), Vec::new())
            })
        },
    )
    .await?;

    log(Some(outcome.interaction_id())).await?;
    if matches!(outcome, pipeline::Outcome::Shown { .. }) {
        sqlx::query("UPDATE core_versions SET saw_ai_assist = true WHERE id = $1")
            .bind(focus.core_version_id)
            .execute(&state.db)
            .await?;
    }
    let mut context = ContextList::default();
    let emphasis: &[&str] = if goal_emphasis {
        &["這次強調：選文理由、內容目標、課綱與考點"]
    } else {
        &[]
    };
    context
        .add(&profile.context_label(), &class_profile)
        .add("學習重點", &focus.big_idea)
        .add(
            &format!("{} 目前寫的內容", question.id),
            &teacher_answer_text(&teacher_answer),
        )
        .add("同一重點的其他題", &others.join("；"))
        .add("你的備課偏好", &preference_summary(&prefs, emphasis));
    Ok(Json(
        outcome
            .into_result(|_| Vec::new())
            .with_context(context.into_vec()),
    ))
}

/// The cell text for the context list (placeholders for blank cells are not "injected info").
fn teacher_answer_text(described: &str) -> String {
    if described == "（尚未填寫）" {
        String::new()
    } else {
        described.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_idea_stores_no_text_and_status_follows_text() {
        assert_eq!(
            normalize_cell("something", "no_idea"),
            (String::new(), "no_idea")
        );
        assert_eq!(normalize_cell("  ", "filled"), (String::new(), "blank"));
        assert_eq!(
            normalize_cell("答案", "blank"),
            ("答案".to_string(), "filled")
        );
    }

    #[test]
    fn full_responses_has_eight_entries() {
        let r = full_responses(BTreeMap::new());
        assert_eq!(r.len(), 8);
        assert_eq!(r["Q1"].status, "blank");
    }
}
