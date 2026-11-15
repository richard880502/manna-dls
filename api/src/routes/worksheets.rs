//! 學習單產生器 endpoints (docs/API.md「Phase 2.3」): worksheets, their versions, generation as
//! background jobs, single-block regeneration and undo, sync back to the lesson SOP, the
//! vocabulary check and DOCX / PDF export. Every worksheet belongs to one teacher; anything else
//! is a 404.

use axum::body::Bytes;
use axum::extract::State;
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_TYPE};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::auth::AuthTeacher;
use crate::error::{ApiJson, ApiPath, ApiQuery, AppError, AppResult};
use crate::routes::{log_usage, owned_class, owned_lesson, owned_unit, resolved_profile};
use crate::sop::{self, store as sop_store, MAX_MATERIAL_CHARS};
use crate::worksheet::ai::{self as ws_ai, Job, RunInput};
use crate::worksheet::assemble::{project, Sheet};
use crate::worksheet::export::{self, ExportError};
use crate::worksheet::material::SOURCE_LESSON_SOP;
use crate::worksheet::store::{self, Summary, Worksheet};
use crate::worksheet::sync::{self, Preview};
use crate::worksheet::vocab::{self, VocabCheck};
use crate::worksheet::{
    auto_filename, auto_title, safe_filename, suggest_direct, suggest_from_sop, Audience, Brief,
    Content, Level, Material, Objective, Origin, SeedExit, SeedTask, Settings, MAX_TITLE_CHARS,
};
use crate::AppState;

// ---------------------------------------------------------------- state

#[derive(Debug, Serialize)]
pub struct SopSyncState {
    pub linked: bool,
    pub changes: usize,
}

#[derive(Debug, Serialize)]
pub struct WorksheetState {
    #[serde(flatten)]
    pub summary: Summary,
    pub material: Material,
    pub settings: Settings,
    pub suggested: Settings,
    pub brief: Brief,
    pub versions_count: i64,
    pub saved_at: DateTime<Utc>,
    pub content: Content,
    pub jobs: Vec<Job>,
    pub running: bool,
    pub undoable: Vec<String>,
    pub sop_sync: SopSyncState,
}

async fn lesson_sop_diff(
    state: &AppState,
    ws: &Worksheet,
    content: &Content,
) -> AppResult<Option<(i64, Option<sop_store::Version>, Vec<sync::Change>)>> {
    let Some(lesson_id) = ws.lesson_id else {
        return Ok(None);
    };
    // A lesson deleted since the link was made: the worksheet stays, unlinked in effect.
    if owned_lesson(&state.db, lesson_id, ws.teacher_id).await.is_err() {
        return Ok(None);
    }
    let sop_version = sop_store::latest(&state.db, lesson_id).await?;
    let sop_content = sop_version
        .as_ref()
        .map(|v| v.content.clone())
        .unwrap_or_default();
    let changes = sync::diff(content, &ws.material, &sop_content);
    Ok(Some((lesson_id, sop_version, changes)))
}

async fn state_of(state: &AppState, id: i64) -> AppResult<WorksheetState> {
    let db = &state.db;
    let ws = store::by_id(db, id)
        .await?
        .ok_or_else(|| AppError::not_found("找不到這份學習單。"))?;
    let latest = store::latest(db, id).await?;
    let (jobs, running) = ws_ai::jobs(state, id).await?;
    let sop_sync = match lesson_sop_diff(state, &ws, &latest.content).await? {
        Some((_, _, changes)) => SopSyncState {
            linked: true,
            changes: sync::pending(&changes),
        },
        None => SopSyncState {
            linked: false,
            changes: 0,
        },
    };
    Ok(WorksheetState {
        summary: store::summary(db, id).await?,
        versions_count: store::versions_count(db, id).await?,
        saved_at: latest.created_at,
        content: latest.content,
        undoable: store::undoable(db, id).await?,
        jobs,
        running,
        sop_sync,
        material: ws.material,
        settings: ws.settings,
        suggested: ws.suggested,
        brief: ws.brief,
    })
}

// ---------------------------------------------------------------- list, create, read

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub lesson_id: Option<i64>,
}

pub async fn list(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiQuery(q): ApiQuery<ListQuery>,
) -> AppResult<Json<Vec<Summary>>> {
    Ok(Json(store::list(&state.db, teacher.id(), q.lesson_id).await?))
}

#[derive(Debug, Default, Deserialize)]
pub struct PastedMaterial {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub source_note: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct CreateWorksheet {
    #[serde(default)]
    pub lesson_id: Option<i64>,
    #[serde(default)]
    pub class_id: Option<i64>,
    #[serde(default)]
    pub material: Option<PastedMaterial>,
}

fn check_pasted(m: &PastedMaterial) -> AppResult<()> {
    if m.text.chars().count() > MAX_MATERIAL_CHARS {
        return Err(AppError::bad_request(format!(
            "教材最多 {MAX_MATERIAL_CHARS} 字。"
        )));
    }
    if m.title.as_deref().unwrap_or_default().chars().count() > MAX_TITLE_CHARS {
        return Err(AppError::bad_request(format!(
            "教材標題最多 {MAX_TITLE_CHARS} 字。"
        )));
    }
    Ok(())
}

/// What a lesson's SOP carries over: objectives into the document, the rest into the brief.
fn prefill_from_sop(content: &sop::SopContent) -> (Vec<Objective>, Brief) {
    let objectives = content
        .objectives
        .iter()
        .filter(|o| !o.text.trim().is_empty() || !o.student_facing_en.trim().is_empty())
        .map(|o| Objective {
            id: o.id.clone(),
            kind: o.kind,
            text: o.text.clone(),
            student_facing_en: o.student_facing_en.clone(),
            origin: Origin::Teacher,
            sop_id: Some(o.id.clone()),
        })
        .collect();
    let tasks = content
        .task_blocks
        .iter()
        .flat_map(|b| {
            b.tasks
                .iter()
                .filter(|t| !t.text.trim().is_empty())
                .map(move |t| SeedTask {
                    id: format!("{}-{}", b.id, t.id).chars().take(40).collect(),
                    paragraph_ids: b.paragraph_ids.clone(),
                    function: t.function,
                    text: t.text.clone(),
                    sop_block_id: Some(b.id.clone()),
                    sop_task_id: Some(t.id.clone()),
                })
        })
        .collect();
    let brief = Brief {
        tasks,
        warmup: content
            .warmup
            .background
            .iter()
            .map(|a| a.text.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect(),
        vocabulary: content
            .warmup
            .vocabulary
            .iter()
            .map(|v| v.word.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect(),
        exit_ticket: content
            .exit_ticket
            .items
            .iter()
            .filter(|i| !i.text.trim().is_empty())
            .map(|i| SeedExit {
                kind: i.kind,
                text: i.text.clone(),
            })
            .collect(),
    };
    (objectives, brief)
}

pub async fn create(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    body: Bytes,
) -> AppResult<(StatusCode, Json<WorksheetState>)> {
    let req: CreateWorksheet = if body.iter().all(u8::is_ascii_whitespace) {
        CreateWorksheet::default()
    } else {
        serde_json::from_slice(&body)
            .map_err(|e| AppError::bad_request(format!("請求內容格式不正確：{e}")))?
    };
    let db = &state.db;
    if let Some(class_id) = req.class_id {
        owned_class(db, class_id, teacher.id()).await?;
    }
    let pasted = req.material.unwrap_or_default();
    check_pasted(&pasted)?;
    let pasted_material = || {
        Material::pasted(
            None,
            0,
            pasted.title.as_deref().unwrap_or_default(),
            &pasted.text,
            pasted.source_note.as_deref().unwrap_or_default(),
        )
    };

    let (material, seq, settings, objectives, brief, sop_version_id, lesson_title) =
        match req.lesson_id {
            Some(lesson_id) => {
                let lesson = owned_lesson(db, lesson_id, teacher.id()).await?;
                let version = sop_store::latest(db, lesson_id).await?;
                let sop_material = sop_store::material(db, version.as_ref().and_then(|v| v.material_id)).await?;
                let (material, seq) = match &sop_material {
                    Some(m) if !m.paragraphs.is_empty() => Material::from_lesson_sop(lesson_id, m),
                    _ => pasted_material(),
                };
                let sop_content = version.as_ref().map(|v| v.content.clone()).unwrap_or_default();
                let (objectives, brief) = prefill_from_sop(&sop_content);
                let settings = suggest_from_sop(&material, &sop_content);
                (material, seq, settings, objectives, brief, version.map(|v| v.id), Some(lesson.title))
            }
            None => {
                let (material, seq) = pasted_material();
                let settings = suggest_direct(&material);
                (material, seq, settings, Vec::new(), Brief::default(), None, None)
            }
        };
    let mut title = auto_title(&material);
    if material.title.trim().is_empty() {
        if let Some(t) = &lesson_title {
            title = format!("{}: Worksheet", t.trim());
        }
    }
    let filename = auto_filename(&material, lesson_title.as_deref());
    let content = Content {
        objectives,
        levels: settings.levels.clone(),
        ..Content::default()
    };
    let id = store::insert(
        db,
        store::NewWorksheet {
            teacher_id: teacher.id(),
            lesson_id: req.lesson_id,
            class_id: req.class_id,
            title: &title,
            filename: &filename,
            material: &material,
            paragraph_seq: seq,
            settings: &settings,
            brief: &brief,
            source_sop_version_id: sop_version_id,
            content: &content,
        },
    )
    .await?;
    log_usage(
        db,
        teacher.id(),
        "worksheet_created",
        json!({
            "worksheet_id": id, "lesson_id": req.lesson_id, "class_id": req.class_id,
            "material_source": material.source.kind, "paragraphs": material.paragraphs.len(),
        }),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(state_of(&state, id).await?)))
}

pub async fn get(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<Json<WorksheetState>> {
    store::owned(&state.db, id, teacher.id()).await?;
    Ok(Json(state_of(&state, id).await?))
}

pub async fn delete(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<Json<serde_json::Value>> {
    store::owned(&state.db, id, teacher.id()).await?;
    store::soft_delete(&state.db, id).await?;
    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------- save

#[derive(Debug, Deserialize)]
pub struct SaveWorksheet {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default)]
    pub settings: Option<Settings>,
    #[serde(default)]
    pub brief: Option<Brief>,
    #[serde(default)]
    pub content: Option<Content>,
}

pub async fn save(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    ApiJson(req): ApiJson<SaveWorksheet>,
) -> AppResult<Json<WorksheetState>> {
    let db = &state.db;
    let ws = store::owned(db, id, teacher.id()).await?;
    let title = match req.title {
        Some(t) => {
            let t = t.trim().to_string();
            if t.chars().count() > MAX_TITLE_CHARS {
                return Err(AppError::bad_request(format!("標題最多 {MAX_TITLE_CHARS} 字。")));
            }
            t
        }
        None => ws.title.clone(),
    };
    let filename = match req.filename {
        Some(f) if f.trim().is_empty() => auto_filename(&ws.material, None),
        Some(f) => safe_filename(&f),
        None => ws.filename.clone(),
    };
    let settings = match req.settings {
        Some(s) => s.normalised().map_err(AppError::bad_request)?,
        None => ws.settings.clone(),
    };
    let brief = req.brief.unwrap_or_else(|| ws.brief.clone());
    brief.validate().map_err(AppError::bad_request)?;
    let mut content = req.content;
    if let Some(c) = &mut content {
        c.normalise().map_err(AppError::bad_request)?;
        let (_, running) = ws_ai::jobs(&state, id).await?;
        if running {
            return Err(AppError::Conflict(
                "Manna 正在產生這份學習單，好了之後再改。".to_string(),
            ));
        }
        let lexicon = vocab::lexicon(db).await?;
        vocab::relookup_levels(c, lexicon.as_ref());
    }
    store::update_fields(db, id, &title, &filename, &settings, &brief).await?;
    if let Some(c) = content {
        store::save_content(db, id, &c, "edit").await?;
    }
    Ok(Json(state_of(&state, id).await?))
}

pub async fn put_material(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    ApiJson(req): ApiJson<PastedMaterial>,
) -> AppResult<Json<WorksheetState>> {
    let db = &state.db;
    let ws = store::owned(db, id, teacher.id()).await?;
    check_pasted(&req)?;
    let (_, running) = ws_ai::jobs(&state, id).await?;
    if running {
        return Err(AppError::Conflict(
            "Manna 正在產生這份學習單，好了之後再換教材。".to_string(),
        ));
    }
    let (material, seq) = Material::pasted(
        Some(&ws.material),
        ws.paragraph_seq,
        req.title.as_deref().unwrap_or_default(),
        &req.text,
        req.source_note.as_deref().unwrap_or_default(),
    );
    let suggested = suggest_direct(&material);
    // Selections the teacher never touched follow the new suggestion; her own stay.
    let settings = if ws.settings == ws.suggested {
        suggested.clone()
    } else {
        ws.settings.clone()
    };
    let title = if ws.title == auto_title(&ws.material) || ws.title.trim().is_empty() {
        auto_title(&material)
    } else {
        ws.title.clone()
    };
    let filename = if ws.filename == auto_filename(&ws.material, None) {
        auto_filename(&material, None)
    } else {
        ws.filename.clone()
    };
    store::set_material(db, id, &material, seq, &settings, &suggested, &title, &filename).await?;
    Ok(Json(state_of(&state, id).await?))
}

// ---------------------------------------------------------------- generation

async fn run_input(state: &AppState, teacher: &AuthTeacher, ws: Worksheet) -> AppResult<RunInput> {
    let db = &state.db;
    let content = store::latest(db, ws.id).await?.content;
    let lesson = match ws.lesson_id {
        Some(lesson_id) => owned_lesson(db, lesson_id, teacher.id()).await.ok(),
        None => None,
    };
    let (description, unit_profile) = lesson
        .as_ref()
        .map(|l| (l.class_description.clone(), l.unit_class_profile.clone()))
        .unwrap_or_default();
    let profile = resolved_profile(db, teacher.id(), ws.class_id, &description, &unit_profile).await?;
    Ok(RunInput {
        content,
        track: teacher.track().to_string(),
        class_profile: profile.text.clone(),
        class_profile_label: profile.context_label(),
        class_snapshot: profile.snapshot(),
        unit_id: lesson.map(|l| l.unit_id),
        worksheet: ws,
    })
}

pub async fn generate(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<(StatusCode, Json<WorksheetState>)> {
    let ws = store::owned(&state.db, id, teacher.id()).await?;
    let input = run_input(&state, &teacher, ws).await?;
    ws_ai::start_generation(&state, input).await?;
    Ok((StatusCode::ACCEPTED, Json(state_of(&state, id).await?)))
}

#[derive(Debug, Serialize)]
pub struct JobsState {
    pub running: bool,
    pub jobs: Vec<Job>,
    pub version_no: i32,
}

pub async fn jobs(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<Json<JobsState>> {
    store::owned(&state.db, id, teacher.id()).await?;
    let (jobs, running) = ws_ai::jobs(&state, id).await?;
    let version_no = store::latest(&state.db, id).await?.version_no;
    Ok(Json(JobsState {
        running,
        jobs,
        version_no,
    }))
}

pub async fn regenerate_block(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath((id, block_id)): ApiPath<(i64, String)>,
) -> AppResult<(StatusCode, Json<WorksheetState>)> {
    let ws = store::owned(&state.db, id, teacher.id()).await?;
    let input = run_input(&state, &teacher, ws).await?;
    ws_ai::start_block(&state, input, &block_id).await?;
    Ok((StatusCode::ACCEPTED, Json(state_of(&state, id).await?)))
}

pub async fn undo_block(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath((id, block_id)): ApiPath<(i64, String)>,
) -> AppResult<Json<WorksheetState>> {
    let db = &state.db;
    store::owned(db, id, teacher.id()).await?;
    let (_, running) = ws_ai::jobs(&state, id).await?;
    if running {
        return Err(AppError::Conflict(
            "Manna 正在產生這份學習單，好了之後再復原。".to_string(),
        ));
    }
    let mut content = store::latest(db, id).await?.content;
    let Some(pos) = content.blocks.iter().position(|b| b.id == block_id) else {
        return Err(AppError::not_found("找不到這個區塊。"));
    };
    let (previous, interaction_id) = store::pop_history(db, id, &block_id)
        .await?
        .ok_or_else(|| AppError::Conflict("這個區塊沒有上一版可以復原。".to_string()))?;
    content.blocks[pos] = previous;
    content.normalise().map_err(AppError::bad_request)?;
    let saved = store::save_content(db, id, &content, "undo").await?;
    if let Some(interaction_id) = interaction_id {
        // Going back is the teacher turning the regenerated block down.
        sqlx::query(
            "INSERT INTO teacher_decisions (ai_interaction_id, teacher_id, item_index, decision, reason_note)
             VALUES ($1, $2, 0, 'reject', '學習單：復原上一版')",
        )
        .bind(interaction_id)
        .bind(teacher.id())
        .execute(db)
        .await?;
    }
    log_usage(
        db,
        teacher.id(),
        "worksheet_block_undone",
        json!({ "worksheet_id": id, "block_id": block_id, "version_no": saved.version_no }),
    )
    .await?;
    Ok(Json(state_of(&state, id).await?))
}

// ---------------------------------------------------------------- sheets and versions

#[derive(Debug, Deserialize)]
pub struct SheetQuery {
    pub level: Option<String>,
    pub audience: Option<String>,
    pub format: Option<String>,
}

fn sheet_for(ws: &Worksheet, content: &Content, q: &SheetQuery) -> AppResult<Sheet> {
    let level = match q.level.as_deref() {
        Some(l) => Level::parse(l).ok_or_else(|| AppError::bad_request("level 需為 basic、standard 或 advanced。"))?,
        None => content.levels.first().copied().unwrap_or(Level::Standard),
    };
    if !content.levels.contains(&level) {
        return Err(AppError::bad_request(format!("這份學習單沒有{}。", level.label())));
    }
    let audience = match q.audience.as_deref() {
        Some(a) => Audience::parse(a).ok_or_else(|| AppError::bad_request("audience 需為 student 或 teacher。"))?,
        None => Audience::Student,
    };
    Ok(project(&ws.title, content, level, audience))
}

pub async fn sheet(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    ApiQuery(q): ApiQuery<SheetQuery>,
) -> AppResult<Json<Sheet>> {
    let ws = store::owned(&state.db, id, teacher.id()).await?;
    let content = store::latest(&state.db, id).await?.content;
    Ok(Json(sheet_for(&ws, &content, &q)?))
}

pub async fn versions(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<Json<Vec<store::VersionSummary>>> {
    store::owned(&state.db, id, teacher.id()).await?;
    Ok(Json(store::list_versions(&state.db, id).await?))
}

pub async fn export(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    ApiQuery(q): ApiQuery<SheetQuery>,
) -> AppResult<Response> {
    let db = &state.db;
    let ws = store::owned(db, id, teacher.id()).await?;
    let latest = store::latest(db, id).await?;
    let sheet = sheet_for(&ws, &latest.content, &q)?;
    let format = q.format.as_deref().unwrap_or("docx");
    let (level, audience) = (sheet.level, sheet.audience);
    let (bytes, content_type, extension) = match format {
        "docx" => {
            let bytes = tokio::task::spawn_blocking(move || export::docx(&sheet))
                .await
                .map_err(|e| AppError::Internal(anyhow::anyhow!(e)))??;
            (bytes, export::DOCX_TYPE, "docx")
        }
        "pdf" => {
            let made = tokio::task::spawn_blocking(move || export::pdf(&sheet))
                .await
                .map_err(|e| AppError::Internal(anyhow::anyhow!(e)))?;
            match made {
                Ok(bytes) => (bytes, export::PDF_TYPE, "pdf"),
                Err(ExportError::NoCjkFont) => {
                    return Err(AppError::Conflict(
                        "這份學習單裡有中文（例如教師備註），但伺服器沒有中文字型，無法做成 PDF。請改用 DOCX，或請管理者安裝中文字型。".to_string(),
                    ))
                }
                Err(ExportError::Other(e)) => return Err(AppError::Internal(e)),
            }
        }
        _ => return Err(AppError::bad_request("format 需為 docx 或 pdf。")),
    };
    let name = format!(
        "{}_{}_{}.{extension}",
        if ws.filename.trim().is_empty() { "學習單" } else { ws.filename.trim() },
        level.label(),
        audience.label()
    );
    log_usage(
        db,
        teacher.id(),
        "worksheet_exported",
        json!({
            "worksheet_id": id, "version_no": latest.version_no, "format": format,
            "level": level, "audience": audience, "bytes": bytes.len(),
        }),
    )
    .await?;
    Ok((
        [
            (CONTENT_TYPE, content_type.to_string()),
            (CONTENT_DISPOSITION, export::content_disposition(&name)),
        ],
        bytes,
    )
        .into_response())
}

// ---------------------------------------------------------------- lesson link and sync

pub async fn sync_preview(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<Json<Preview>> {
    let ws = store::owned(&state.db, id, teacher.id()).await?;
    let content = store::latest(&state.db, id).await?.content;
    let Some((lesson_id, sop_version, changes)) = lesson_sop_diff(&state, &ws, &content).await? else {
        return Err(AppError::Conflict("這份學習單沒有連結課例。".to_string()));
    };
    Ok(Json(Preview {
        lesson_id,
        sop_version_no: sop_version.map(|v| v.version_no),
        changes,
        core_suggestions: sync::core_suggestions(&content),
    }))
}

#[derive(Debug, Deserialize)]
pub struct ApplySync {
    #[serde(default)]
    pub confirm: bool,
    #[serde(default)]
    pub keys: Vec<String>,
    #[serde(default)]
    pub sop_version_no: Option<i32>,
}

/// Writes the chosen changes into a new SOP version. Returns the version number and how many
/// changes were applied; the worksheet records the SOP ids of what it added.
async fn write_back(
    state: &AppState,
    ws: &Worksheet,
    lesson_id: i64,
    keys: &[String],
) -> AppResult<(i32, usize)> {
    let db = &state.db;
    let version = sop_store::latest(db, lesson_id).await?;
    let sop_material = sop_store::material(db, version.as_ref().and_then(|v| v.material_id)).await?;
    let mut sop_content = version.map(|v| v.content).unwrap_or_default();
    let mut content = store::latest(db, ws.id).await?.content;
    let applied = sync::apply(
        &mut content,
        &ws.material,
        &mut sop_content,
        sop_material.as_ref().map(|m| m.paragraphs.as_slice()).unwrap_or(&[]),
        keys,
    );
    sop::validate(&sop_content).map_err(AppError::bad_request)?;
    let items = crate::curriculum::by_codes(db, &sop::linked_codes(&sop_content)).await?;
    sop::resolve_links(&mut sop_content, &items).map_err(|code| {
        AppError::bad_request(format!("課綱條目「{code}」不在知識庫裡，不能連結。"))
    })?;
    let saved = sop_store::save_content(db, lesson_id, &sop_content).await?;
    content.normalise().map_err(AppError::bad_request)?;
    store::save_content(db, ws.id, &content, "edit").await?;
    Ok((saved.version_no, applied))
}

pub async fn sync_apply(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    ApiJson(req): ApiJson<ApplySync>,
) -> AppResult<Json<serde_json::Value>> {
    let db = &state.db;
    let ws = store::owned(db, id, teacher.id()).await?;
    if !req.confirm {
        return Err(AppError::bad_request(
            "要同步回課例，需要教師確認（confirm: true）。",
        ));
    }
    let Some(lesson_id) = ws.lesson_id else {
        return Err(AppError::Conflict("這份學習單沒有連結課例。".to_string()));
    };
    owned_lesson(db, lesson_id, teacher.id()).await?;
    let current = sop_store::latest(db, lesson_id).await?.map(|v| v.version_no);
    if current != req.sop_version_no {
        return Err(AppError::Conflict(
            "課例在你預覽之後改過了，請重新預覽要同步的內容。".to_string(),
        ));
    }
    if req.keys.is_empty() {
        return Err(AppError::bad_request("沒有勾選要同步的項目。"));
    }
    let (sop_version_no, applied) = write_back(&state, &ws, lesson_id, &req.keys).await?;
    log_usage(
        db,
        teacher.id(),
        "worksheet_sop_synced",
        json!({ "worksheet_id": id, "lesson_id": lesson_id, "sop_version_no": sop_version_no, "applied": applied }),
    )
    .await?;
    Ok(Json(json!({ "sop_version_no": sop_version_no, "applied": applied })))
}

#[derive(Debug, Deserialize)]
pub struct LinkLesson {
    pub lesson_id: Option<i64>,
}

pub async fn link(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    ApiJson(req): ApiJson<LinkLesson>,
) -> AppResult<Json<WorksheetState>> {
    let db = &state.db;
    store::owned(db, id, teacher.id()).await?;
    let sop_version_id = match req.lesson_id {
        Some(lesson_id) => {
            owned_lesson(db, lesson_id, teacher.id()).await?;
            sop_store::latest(db, lesson_id).await?.map(|v| v.id)
        }
        None => None,
    };
    store::set_lesson(db, id, req.lesson_id, sop_version_id).await?;
    log_usage(
        db,
        teacher.id(),
        "worksheet_linked",
        json!({ "worksheet_id": id, "lesson_id": req.lesson_id }),
    )
    .await?;
    Ok(Json(state_of(&state, id).await?))
}

#[derive(Debug, Deserialize)]
pub struct SaveAsLesson {
    pub unit_id: i64,
    #[serde(default)]
    pub title: String,
}

pub async fn save_as_lesson(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    ApiJson(req): ApiJson<SaveAsLesson>,
) -> AppResult<Json<WorksheetState>> {
    let db = &state.db;
    let ws = store::owned(db, id, teacher.id()).await?;
    owned_unit(db, req.unit_id, teacher.id()).await?;
    let title = if req.title.trim().is_empty() {
        ws.title.clone()
    } else {
        req.title.trim().to_string()
    };
    if title.chars().count() > MAX_TITLE_CHARS {
        return Err(AppError::bad_request(format!("課例名稱最多 {MAX_TITLE_CHARS} 字。")));
    }
    let lesson_id: i64 = sqlx::query_scalar(
        "INSERT INTO lessons (unit_id, title, class_description, my_example)
         VALUES ($1, $2, '', '') RETURNING id",
    )
    .bind(req.unit_id)
    .bind(&title)
    .fetch_one(db)
    .await?;
    if !ws.material.is_empty() {
        sop_store::save_material(
            db,
            lesson_id,
            &ws.material.title,
            &ws.material.source.note,
            &ws.material.text(),
        )
        .await?;
    }
    store::set_lesson(db, id, Some(lesson_id), None).await?;
    // Everything the worksheet has becomes the new lesson's SOP: the teacher asked for it.
    let ws = store::owned(db, id, teacher.id()).await?;
    let content = store::latest(db, id).await?.content;
    let keys: Vec<String> = sync::diff(&content, &ws.material, &sop::SopContent::default())
        .into_iter()
        .filter(|c| c.action != "remove")
        .map(|c| c.key)
        .collect();
    if !keys.is_empty() {
        write_back(&state, &ws, lesson_id, &keys).await?;
    }
    let sop_version_id = sop_store::latest(db, lesson_id).await?.map(|v| v.id);
    store::set_lesson(db, id, Some(lesson_id), sop_version_id).await?;
    log_usage(
        db,
        teacher.id(),
        "worksheet_linked",
        json!({ "worksheet_id": id, "lesson_id": lesson_id, "new_lesson": true }),
    )
    .await?;
    Ok(Json(state_of(&state, id).await?))
}

// ---------------------------------------------------------------- final check

#[derive(Debug, Deserialize)]
pub struct VocabularyCheckRequest {
    pub level: String,
    #[serde(default)]
    pub target_level: Option<i64>,
}

pub async fn check_vocabulary(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    ApiJson(req): ApiJson<VocabularyCheckRequest>,
) -> AppResult<Json<VocabCheck>> {
    let ws = store::owned(&state.db, id, teacher.id()).await?;
    let level = Level::parse(&req.level)
        .ok_or_else(|| AppError::bad_request("level 需為 basic、standard 或 advanced。"))?;
    let target = req
        .target_level
        .unwrap_or(crate::vocabulary::DEFAULT_TARGET_LEVEL as i64);
    if !(1..=6).contains(&target) {
        return Err(AppError::bad_request("target_level 需為 1 到 6。"));
    }
    let check = vocab::run(&state.db, &ws, level, target as u8).await?;
    log_usage(
        &state.db,
        teacher.id(),
        "worksheet_vocabulary_checked",
        json!({ "worksheet_id": id, "level": level, "version_no": check.version_no, "target_level": target }),
    )
    .await?;
    Ok(Json(check))
}

pub async fn checks(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<Json<Vec<VocabCheck>>> {
    store::owned(&state.db, id, teacher.id()).await?;
    Ok(Json(vocab::latest(&state.db, id).await?))
}

/// The material kind a worksheet made from a lesson starts with (used by tests).
pub const FROM_LESSON: &str = SOURCE_LESSON_SOP;
