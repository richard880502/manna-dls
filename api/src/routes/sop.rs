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
use crate::sop::ai::{self, Job, JobInput, JobKind};
use crate::sop::store::{self, Material, Version, VersionSummary};
use crate::sop::{self, SopContent, MAX_FIELD_CHARS, MAX_MATERIAL_CHARS, MAX_TITLE_CHARS};
use crate::teaching_context::{owned_lesson, resolved_profile};
use crate::AppState;

#[derive(Debug, Serialize)]
pub struct Jobs {
    pub analysis: Option<Job>,
    pub objectives: Option<Job>,
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
        }
    } else {
        Jobs {
            analysis: None,
            objectives: None,
        }
    };
    Ok(SopState {
        lesson_id,
        version_no: version.as_ref().map(|v| v.version_no),
        versions_count: store::versions_count(db, lesson_id).await?,
        saved_at: version.as_ref().map(|v| v.created_at),
        material_deleted: material.is_none()
            && version.as_ref().is_some_and(|v| v.material_deleted),
        material,
        content: version.map(|v| v.content).unwrap_or_default(),
        today_objective: today_objective(db, lesson_id).await?,
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
    let saved = store::save_content(&state.db, id, &content).await?;
    Ok(Json(SavedSop {
        version_no: saved.version_no,
        versions_count: store::versions_count(&state.db, id).await?,
        saved_at: saved.created_at,
        content: saved.content,
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

#[derive(Debug, Default, Deserialize)]
pub struct StartJob {
    #[serde(default)]
    pub class_id: Option<i64>,
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
    let today = today_objective(&state.db, id).await?;
    let job_id = ai::start(
        &state,
        JobInput {
            kind,
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
