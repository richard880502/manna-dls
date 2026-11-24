//! 課例 SOP endpoints (docs/API.md「Phase 2.1」): the versioned document, its material, and the
//! two AI stages as background jobs.

use axum::body::Bytes;
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::auth::AuthTeacher;
use crate::curriculum;
use crate::error::{ApiJson, ApiPath, AppError, AppResult};
use crate::sop::ai::{self, Job, JobInput, JobKind, Scope, Swap};
use crate::sop::stages;
use crate::sop::store::{self, Material, Version, VersionSummary};
use crate::sop::{self, SopContent, MAX_FIELD_CHARS, MAX_MATERIAL_CHARS, MAX_TITLE_CHARS};
use crate::teaching_context::{owned_lesson, resolved_profile};
use crate::worksheet::Settings;
use crate::AppState;

#[derive(Debug, Serialize)]
pub struct Jobs {
    pub analysis: Option<Job>,
    pub objectives: Option<Job>,
    pub goals: Option<Job>,
    pub tasks: Option<Job>,
    pub warmup: Option<Job>,
    pub exit_ticket: Option<Job>,
    /// Jobs that redo one suggestion (換一個, 這一段再建議一次) since the last whole run.
    pub redo: Vec<Job>,
}

#[derive(Debug, Serialize)]
pub struct SopState {
    pub lesson_id: i64,
    pub version_no: Option<i32>,
    pub versions_count: i64,
    pub saved_at: Option<DateTime<Utc>>,
    pub material: Option<Material>,
    /// This version had a material and the teacher has deleted it since (「教材已刪除」).
    pub material_deleted: bool,
    pub content: SopContent,
    /// The plan's 「今天要教什麼？」 (latest `lesson_versions.objective`): the old single
    /// objective, offered to stage 3 instead of being rewritten into the SOP.
    pub today_objective: String,
    /// The generator settings a worksheet made from this lesson starts with: the SOP's content
    /// and the choices made in the steps (`content.options`, the labels of the task blocks).
    pub worksheet_settings: Settings,
    /// What the SOP's content alone suggests (the generator's ✦ marks).
    pub worksheet_suggested: Settings,
    pub jobs: Jobs,
}

async fn today_objective(db: &sqlx::PgPool, lesson_id: i64) -> sqlx::Result<String> {
    Ok(sqlx::query_scalar::<_, String>(
        "SELECT objective FROM lesson_versions WHERE lesson_id = $1
          ORDER BY version_no DESC LIMIT 1",
    )
    .bind(lesson_id)
    .fetch_optional(db)
    .await?
    .unwrap_or_default())
}

/// What a worksheet made from the lesson starts with, and the defaults the SOP's content alone
/// would give (the generator's ✦).
fn worksheet_settings_of(
    lesson_id: i64,
    material: Option<&Material>,
    content: &SopContent,
) -> (Settings, Settings) {
    let ws_material = material
        .map(|m| crate::worksheet::Material::from_lesson_sop(lesson_id, m).0)
        .unwrap_or_default();
    (
        crate::worksheet::suggest_from_sop(&ws_material, content),
        crate::worksheet::defaults_from_sop(&ws_material, content),
    )
}

async fn state_of(
    state: &AppState,
    lesson_id: i64,
    version: Option<Version>,
    with_jobs: bool,
) -> AppResult<SopState> {
    let db = &state.db;
    let material = store::material(db, version.as_ref().and_then(|v| v.material_id)).await?;
    let jobs = if with_jobs {
        Jobs {
            analysis: ai::latest_job(state, lesson_id, JobKind::Analysis).await?,
            objectives: ai::latest_job(state, lesson_id, JobKind::Objectives).await?,
            goals: ai::latest_job(state, lesson_id, JobKind::Goals).await?,
            tasks: ai::latest_job(state, lesson_id, JobKind::Tasks).await?,
            warmup: ai::latest_job(state, lesson_id, JobKind::Warmup).await?,
            exit_ticket: ai::latest_job(state, lesson_id, JobKind::ExitTicket).await?,
            redo: ai::redo_jobs(state, lesson_id).await?,
        }
    } else {
        Jobs {
            analysis: None,
            objectives: None,
            goals: None,
            tasks: None,
            warmup: None,
            exit_ticket: None,
            redo: Vec::new(),
        }
    };
    let content = version
        .as_ref()
        .map(|v| v.content.clone())
        .unwrap_or_default();
    let (worksheet_settings, worksheet_suggested) =
        worksheet_settings_of(lesson_id, material.as_ref(), &content);
    Ok(SopState {
        lesson_id,
        version_no: version.as_ref().map(|v| v.version_no),
        versions_count: store::versions_count(db, lesson_id).await?,
        saved_at: version.as_ref().map(|v| v.created_at),
        material_deleted: material.is_none()
            && version.as_ref().is_some_and(|v| v.material_deleted),
        material,
        content,
        today_objective: today_objective(db, lesson_id).await?,
        worksheet_settings,
        worksheet_suggested,
        jobs,
    })
}

pub async fn get(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<Json<SopState>> {
    owned_lesson(&state.db, id, teacher.id()).await?;
    let version = store::latest(&state.db, id).await?;
    Ok(Json(state_of(&state, id, version, true).await?))
}

#[derive(Debug, Deserialize)]
pub struct PutMaterial {
    /// Pick a material from the library. `null` (with no text) takes the material off the lesson.
    #[serde(default, deserialize_with = "crate::serde_support::double_option")]
    pub material_id: Option<Option<i64>>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub source_note: Option<String>,
    #[serde(default)]
    pub text: String,
}

pub async fn put_material(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    ApiJson(req): ApiJson<PutMaterial>,
) -> AppResult<Json<SopState>> {
    owned_lesson(&state.db, id, teacher.id()).await?;
    let version = match req.material_id {
        // Step 1 by picking from the library (or taking the material off the lesson).
        Some(material_id) => store::set_material(&state.db, teacher.id(), id, material_id).await?,
        // Step 1 by pasting.
        None => {
            let title = req.title.unwrap_or_default().trim().to_string();
            let source_note = req.source_note.unwrap_or_default().trim().to_string();
            if req.text.chars().count() > MAX_MATERIAL_CHARS {
                return Err(AppError::bad_request(format!(
                    "教材最多 {MAX_MATERIAL_CHARS} 字。"
                )));
            }
            if title.chars().count() > MAX_TITLE_CHARS {
                return Err(AppError::bad_request(format!(
                    "教材標題最多 {MAX_TITLE_CHARS} 字。"
                )));
            }
            if source_note.chars().count() > MAX_FIELD_CHARS {
                return Err(AppError::bad_request(format!(
                    "來源說明最多 {MAX_FIELD_CHARS} 字。"
                )));
            }
            store::save_material(&state.db, teacher.id(), id, &title, &source_note, &req.text)
                .await?
        }
    };
    Ok(Json(state_of(&state, id, version, true).await?))
}

#[derive(Debug, Deserialize)]
pub struct PutSop {
    #[serde(default)]
    pub content: SopContent,
}

#[derive(Debug, Serialize)]
pub struct SavedSop {
    pub version_no: i32,
    pub versions_count: i64,
    pub saved_at: DateTime<Utc>,
    pub content: SopContent,
    pub worksheet_settings: Settings,
    pub worksheet_suggested: Settings,
}

pub async fn put(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    ApiJson(req): ApiJson<PutSop>,
) -> AppResult<Json<SavedSop>> {
    owned_lesson(&state.db, id, teacher.id()).await?;
    let mut content = req.content;
    sop::validate(&content).map_err(AppError::bad_request)?;
    // Curriculum text and sources always come from the knowledge base, by code.
    let items = curriculum::by_codes(&state.db, &sop::linked_codes(&content)).await?;
    sop::resolve_links(&mut content, &items).map_err(|code| {
        AppError::bad_request(format!("課綱條目「{code}」不在知識庫裡，不能連結。"))
    })?;
    // The level of a key word is the word list's, never what the client sent.
    let lexicon = crate::worksheet::vocab::lexicon(&state.db).await?;
    for v in &mut content.warmup.vocabulary {
        v.level = lexicon
            .as_ref()
            .and_then(|lex| crate::vocabulary::word_level(lex, v.word.trim()).level)
            .map(i32::from);
    }
    let saved = store::save_content(&state.db, id, &content).await?;
    let material = store::material(&state.db, saved.material_id).await?;
    let (worksheet_settings, worksheet_suggested) =
        worksheet_settings_of(id, material.as_ref(), &saved.content);
    Ok(Json(SavedSop {
        version_no: saved.version_no,
        versions_count: store::versions_count(&state.db, id).await?,
        saved_at: saved.created_at,
        content: saved.content,
        worksheet_settings,
        worksheet_suggested,
    }))
}

pub async fn versions(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<Json<Vec<VersionSummary>>> {
    owned_lesson(&state.db, id, teacher.id()).await?;
    Ok(Json(store::list_versions(&state.db, id).await?))
}

pub async fn version(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath((id, version_no)): ApiPath<(i64, i32)>,
) -> AppResult<Json<SopState>> {
    owned_lesson(&state.db, id, teacher.id()).await?;
    let version = store::version(&state.db, id, version_no)
        .await?
        .ok_or_else(|| AppError::not_found("找不到這個版本。"))?;
    Ok(Json(state_of(&state, id, Some(version), false).await?))
}

/// 語言目標 are derived from the 內容目標, so they wait until one is decided.
const MSG_CONTENT_FIRST: &str =
    "請先決定內容目標（自己寫，或採用 Manna 的建議），Manna 才能依它推薦語言目標。";

#[derive(Debug, Default, Deserialize)]
pub struct StartJob {
    #[serde(default)]
    pub class_id: Option<i64>,
    /// 換一個 (objectives): the job whose suggestion is replaced, and its `item_index`.
    #[serde(default)]
    pub parent_job_id: Option<i64>,
    #[serde(default)]
    pub item_index: Option<i32>,
    /// 這一段再建議一次 (goals).
    #[serde(default)]
    pub paragraph_id: Option<String>,
    /// objectives: `content` or `language`. The 內容目標 are recommended first; the 語言目標 come
    /// after the teacher decided them and are derived from them. Absent: the 內容目標 while the
    /// lesson has none, otherwise the 語言目標.
    #[serde(default)]
    pub only_type: Option<String>,
}

async fn start_job(
    state: AppState,
    teacher: AuthTeacher,
    id: i64,
    body: Bytes,
    kind: JobKind,
) -> AppResult<(StatusCode, Json<Job>)> {
    let req: StartJob = if body.iter().all(u8::is_ascii_whitespace) {
        StartJob::default()
    } else {
        serde_json::from_slice(&body)
            .map_err(|e| AppError::bad_request(format!("請求內容格式不正確：{e}")))?
    };
    let lesson = owned_lesson(&state.db, id, teacher.id()).await?;
    let profile = resolved_profile(
        &state.db,
        teacher.id(),
        req.class_id,
        &lesson.class_description,
        &lesson.unit_class_profile,
    )
    .await?;
    let version = store::latest(&state.db, id).await?;
    let material = store::material(&state.db, version.as_ref().and_then(|v| v.material_id)).await?;
    let (Some(version), Some(material)) = (version, material) else {
        return Err(AppError::bad_request(
            "請先在第 1 步貼上教材，Manna 才有東西可以讀。",
        ));
    };
    if material.paragraphs.is_empty() {
        return Err(AppError::bad_request(
            "這份教材沒有可以讀的文字（圖片、影音、網址或掃描的 PDF）。請貼上文字，或換一份有文字的教材。",
        ));
    }
    let has_objectives = version
        .content
        .objectives
        .iter()
        .any(|o| !o.text.trim().is_empty() || !o.student_facing_en.trim().is_empty());
    if matches!(kind, JobKind::Goals | JobKind::ExitTicket) && !has_objectives {
        return Err(AppError::bad_request(
            "請先在第 3 步寫（或採用）至少一項目標，Manna 才知道要對應哪些目標。",
        ));
    }
    let has_content = version
        .content
        .objectives
        .iter()
        .any(|o| o.kind == sop::ObjectiveType::Content && !o.text.trim().is_empty());
    let only = match req.only_type.as_deref() {
        None => None,
        Some("content") => Some(sop::ObjectiveType::Content),
        Some("language") => Some(sop::ObjectiveType::Language),
        Some(_) => {
            return Err(AppError::bad_request(
                "only_type 需為 content 或 language。",
            ))
        }
    };
    if kind == JobKind::Objectives
        && req.parent_job_id.is_none()
        && only == Some(sop::ObjectiveType::Language)
        && !has_content
    {
        return Err(AppError::bad_request(MSG_CONTENT_FIRST));
    }
    let scope = match (kind, req.parent_job_id, req.paragraph_id.as_deref()) {
        (JobKind::Objectives, Some(parent_id), _) => {
            let index = req
                .item_index
                .ok_or_else(|| AppError::bad_request("請指定要換掉哪一項建議（item_index）。"))?;
            Scope::Swap(swap_of(&state, teacher.id(), id, parent_id, index, &version).await?)
        }
        (JobKind::Goals, parent, Some(paragraph_id)) => {
            if !material.paragraphs.iter().any(|p| p.id == paragraph_id) {
                return Err(AppError::bad_request("找不到這一段。"));
            }
            Scope::Paragraph {
                parent_job_id: parent,
                paragraph_id: paragraph_id.to_string(),
            }
        }
        (JobKind::Objectives, None, _) => Scope::Kind(only.unwrap_or(if has_content {
            sop::ObjectiveType::Language
        } else {
            sop::ObjectiveType::Content
        })),
        _ => Scope::Whole,
    };
    let today = today_objective(&state.db, id).await?;
    let job_id = ai::start(
        &state,
        JobInput {
            kind,
            scope,
            teacher_id: teacher.id(),
            track: teacher.track().to_string(),
            lesson,
            profile,
            version,
            material,
            today_objective: today,
        },
    )
    .await?;
    let job = ai::job(&state, job_id, teacher.id())
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("job {job_id} vanished")))?;
    Ok((StatusCode::ACCEPTED, Json(job)))
}

pub async fn start_analysis(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    body: Bytes,
) -> AppResult<(StatusCode, Json<Job>)> {
    start_job(state, teacher, id, body, JobKind::Analysis).await
}

/// The suggestion of an objectives job that 換一個 replaces (read from the stored result, never
/// from the request), and the objectives the new one must not repeat.
async fn swap_of(
    state: &AppState,
    teacher_id: i64,
    lesson_id: i64,
    parent_id: i64,
    index: i32,
    version: &Version,
) -> AppResult<Swap> {
    let parent = ai::job(state, parent_id, teacher_id)
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
        Some("language") => sop::ObjectiveType::Language,
        Some("content") => sop::ObjectiveType::Content,
        _ => return Err(AppError::bad_request("這一項建議沒有目標類型。")),
    };
    let text_of = |o: &serde_json::Value| o["text"].as_str().unwrap_or_default().trim().to_string();
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

pub async fn start_goals(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    body: Bytes,
) -> AppResult<(StatusCode, Json<Job>)> {
    start_job(state, teacher, id, body, JobKind::Goals).await
}

pub async fn start_tasks(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    body: Bytes,
) -> AppResult<(StatusCode, Json<Job>)> {
    start_job(state, teacher, id, body, JobKind::Tasks).await
}

pub async fn start_warmup(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    body: Bytes,
) -> AppResult<(StatusCode, Json<Job>)> {
    start_job(state, teacher, id, body, JobKind::Warmup).await
}

pub async fn start_exit_ticket(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    body: Bytes,
) -> AppResult<(StatusCode, Json<Job>)> {
    start_job(state, teacher, id, body, JobKind::ExitTicket).await
}

#[derive(Debug, Deserialize)]
pub struct EnglishRequest {
    #[serde(rename = "type")]
    pub kind: sop::ObjectiveType,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub student_facing_en: String,
}

/// 幫我修英文: one short call, answered in the request (docs/API.md「課例 SOP」).
pub async fn objective_english(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    ApiJson(req): ApiJson<EnglishRequest>,
) -> AppResult<Json<serde_json::Value>> {
    let lesson = owned_lesson(&state.db, id, teacher.id()).await?;
    if req.text.trim().is_empty() {
        return Err(AppError::bad_request(
            "請先寫中文的目標，Manna 才能照它修英文。",
        ));
    }
    if req.text.chars().count() > MAX_FIELD_CHARS
        || req.student_facing_en.chars().count() > MAX_FIELD_CHARS
    {
        return Err(AppError::bad_request(format!(
            "目標最多 {MAX_FIELD_CHARS} 字。"
        )));
    }
    let out = stages::suggest_english(
        &state,
        teacher.id(),
        &lesson,
        req.kind,
        &req.text,
        &req.student_facing_en,
    )
    .await?;
    Ok(Json(out))
}

#[derive(Debug, Deserialize)]
pub struct ProceduralRequest {
    #[serde(default)]
    pub paragraph_ids: Vec<String>,
    #[serde(default)]
    pub student_tasks: Vec<String>,
    #[serde(default)]
    pub answer_formats: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ProceduralSteps {
    pub steps: Vec<String>,
    /// The steps as the plain text field of the SOP holds them.
    pub text: String,
}

/// 產生程序鷹架: the steps the program makes for a task (spec §3.3, same rule as the worksheet's);
/// no model is involved.
pub async fn procedural_steps(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    ApiJson(req): ApiJson<ProceduralRequest>,
) -> AppResult<Json<ProceduralSteps>> {
    owned_lesson(&state.db, id, teacher.id()).await?;
    if req.paragraph_ids.len() > sop::MAX_LIST_ITEMS
        || req.student_tasks.len() > sop::MAX_LIST_ITEMS
        || req.answer_formats.len() > sop::MAX_LIST_ITEMS
    {
        return Err(AppError::bad_request("項目太多。"));
    }
    let steps =
        stages::auto_procedural_steps(&req.paragraph_ids, &req.student_tasks, &req.answer_formats);
    crate::audit::log_usage(
        &state.db,
        teacher.id(),
        "sop_procedural_steps",
        serde_json::json!({ "lesson_id": id, "steps": steps.len() }),
    )
    .await?;
    Ok(Json(ProceduralSteps {
        text: stages::steps_text(&steps),
        steps,
    }))
}

pub async fn start_objectives(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    body: Bytes,
) -> AppResult<(StatusCode, Json<Job>)> {
    start_job(state, teacher, id, body, JobKind::Objectives).await
}

pub async fn job(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<Json<Job>> {
    ai::job(&state, id, teacher.id())
        .await?
        .map(Json)
        .ok_or_else(|| AppError::not_found("找不到這次 AI 請求。"))
}
