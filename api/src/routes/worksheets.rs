//! 學習單產生器 endpoints (docs/API.md「Phase 2.3」): worksheets, their versions, generation as
//! background jobs, single-block regeneration and undo, sync back to the lesson SOP, the
//! vocabulary check and DOCX / PDF export. Every worksheet belongs to one teacher; anything else
//! is a 404.

use axum::body::{Body, Bytes};
use axum::extract::{Multipart, State};
use axum::http::header::{
    CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_SECURITY_POLICY, CONTENT_TYPE,
    ETAG, IF_NONE_MATCH, VARY, X_CONTENT_TYPE_OPTIONS,
};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::application::worksheets::{
    editing::{self, MaterialSelection, SaveWorksheet},
    synchronization::{self, lesson_sop_diff, ApplySync, LinkLesson, SaveAsLesson},
    CreateOverrides, CreateWorksheet, PastedMaterial, WorksheetService,
};
use crate::audit::log_usage;
use crate::auth::AuthTeacher;
use crate::error::{ApiJson, ApiPath, ApiQuery, AppError, AppResult};
use crate::materials::store as material_store;
use crate::progress::Progress;
use crate::worksheet::ai::{self as ws_ai, Job};
use crate::worksheet::assemble::{keep_original_text, project, Sheet};
use crate::worksheet::export::{self, ExportError};
use crate::worksheet::history;
use crate::worksheet::images::{self, WorksheetImages};
use crate::worksheet::material::SOURCE_LESSON_SOP;
use crate::worksheet::store::{self, Summary, Worksheet};
use crate::worksheet::sync::{self, Preview};
use crate::worksheet::thumbnail;
use crate::worksheet::vocab::{self, VocabCheck};
use crate::worksheet::{Audience, Brief, Content, Level, Material, Settings};
use crate::AppState;

// ---------------------------------------------------------------- state

#[derive(Debug, Serialize)]
pub struct SopSyncState {
    pub linked: bool,
    pub changes: usize,
}

/// The template a worksheet follows (「照我的版型」).
#[derive(Debug, Serialize)]
pub struct TemplateRef {
    pub id: i64,
    pub title: String,
    pub version_no: i32,
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
    /// The steps of the latest run (docs/API.md「產生進度」); `null` for a run made before they
    /// were kept, or a worksheet never generated.
    pub progress: Option<Progress>,
    pub undoable: Vec<String>,
    pub sop_sync: SopSyncState,
    pub template: Option<TemplateRef>,
    /// 「教師的要求」 ("" when none).
    pub request: String,
    /// The pictures of the blocks' slots (docs/API.md「學習單的圖片」).
    pub images: WorksheetImages,
    /// 「改編自 …」: the shared worksheet this one was remixed from, while it is still shared and
    /// the 社群 switch is on; otherwise `null`.
    pub remixed_from: Option<crate::community::RemixSource>,
}

pub(crate) async fn state_of(state: &AppState, id: i64) -> AppResult<WorksheetState> {
    let db = &state.db;
    let ws = store::by_id(db, id)
        .await?
        .ok_or_else(|| AppError::not_found("找不到這份學習單。"))?;
    let latest = store::latest(db, id).await?;
    let (jobs, running) = ws_ai::jobs(state, id).await?;
    let progress = ws_ai::progress(state, id, &jobs, running).await?;
    let sop_sync = match lesson_sop_diff(&state.db, &ws, &latest.content).await? {
        Some((_, _, changes)) => SopSyncState {
            linked: true,
            changes: sync::pending(&changes),
        },
        None => SopSyncState {
            linked: false,
            changes: 0,
        },
    };
    let template = match ws.template_id {
        Some(template_id) => {
            let title: Option<String> =
                sqlx::query_scalar("SELECT title FROM worksheet_templates WHERE id = $1")
                    .bind(template_id)
                    .fetch_optional(db)
                    .await?;
            title.map(|title| TemplateRef {
                id: template_id,
                title,
                version_no: ws.template_version_no.unwrap_or(1),
            })
        }
        None => None,
    };
    Ok(WorksheetState {
        template,
        summary: store::summary(db, id).await?,
        versions_count: store::versions_count(db, id).await?,
        saved_at: latest.created_at,
        content: latest.content,
        undoable: store::undoable(db, id).await?,
        jobs,
        running,
        progress,
        sop_sync,
        material: ws.material,
        settings: ws.settings,
        suggested: ws.suggested,
        brief: ws.brief,
        request: ws.request,
        images: images::states(state, id).await?,
        remixed_from: crate::community::remix_source(db, id).await?,
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
    Ok(Json(
        store::list(&state.db, teacher.id(), q.lesson_id).await?,
    ))
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
    // 「教師的要求」 typed when the worksheet is made (not the autosaves of later edits).
    let typed = req.request.clone().unwrap_or_default();
    let id = WorksheetService::new(&state.db)
        .create(teacher.id(), req, CreateOverrides::default())
        .await?;
    crate::memory::detect::spawn(
        &state,
        teacher.id(),
        crate::memory::Source::WorksheetRequest { worksheet_id: id },
        &typed,
    )
    .await;
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
    editing::delete(&state, teacher.id(), id).await?;
    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------- save

pub async fn save(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    ApiJson(req): ApiJson<SaveWorksheet>,
) -> AppResult<Json<WorksheetState>> {
    editing::save(&state, teacher.id(), id, req).await?;
    Ok(Json(state_of(&state, id).await?))
}

pub async fn put_material(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    ApiJson(req): ApiJson<PastedMaterial>,
) -> AppResult<Json<WorksheetState>> {
    editing::put_material(&state, teacher.id(), id, req).await?;
    Ok(Json(state_of(&state, id).await?))
}

/// Replace the library-material paragraph selection through the application command.
pub async fn put_material_selection(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    ApiJson(req): ApiJson<MaterialSelection>,
) -> AppResult<Json<WorksheetState>> {
    editing::put_material_selection(&state, teacher.id(), id, req).await?;
    Ok(Json(state_of(&state, id).await?))
}

// ---------------------------------------------------------------- generation

pub async fn generate(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<(StatusCode, Json<WorksheetState>)> {
    let input = WorksheetService::new(&state.db)
        .prepare_generation(teacher.id(), teacher.track(), id)
        .await?;
    ws_ai::start_generation(&state, input).await?;
    Ok((StatusCode::ACCEPTED, Json(state_of(&state, id).await?)))
}

#[derive(Debug, Serialize)]
pub struct JobsState {
    pub running: bool,
    pub jobs: Vec<Job>,
    pub version_no: i32,
    /// As in the worksheet state.
    pub progress: Option<Progress>,
}

pub async fn jobs(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<Json<JobsState>> {
    store::owned(&state.db, id, teacher.id()).await?;
    let (jobs, running) = ws_ai::jobs(&state, id).await?;
    let version_no = store::latest(&state.db, id).await?.version_no;
    let progress = ws_ai::progress(&state, id, &jobs, running).await?;
    Ok(Json(JobsState {
        running,
        jobs,
        version_no,
        progress,
    }))
}

pub async fn regenerate_block(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath((id, block_id)): ApiPath<(i64, String)>,
) -> AppResult<(StatusCode, Json<WorksheetState>)> {
    let input = WorksheetService::new(&state.db)
        .prepare_generation(teacher.id(), teacher.track(), id)
        .await?;
    ws_ai::start_block(&state, input, &block_id).await?;
    Ok((StatusCode::ACCEPTED, Json(state_of(&state, id).await?)))
}

pub async fn undo_block(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath((id, block_id)): ApiPath<(i64, String)>,
) -> AppResult<Json<WorksheetState>> {
    editing::undo_block(&state, teacher.id(), id, &block_id).await?;
    Ok(Json(state_of(&state, id).await?))
}

/// `POST /api/worksheets/:id/blocks/:block_id/restore-word-table` (「恢復字詞表」)
pub async fn restore_word_table(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath((id, block_id)): ApiPath<(i64, String)>,
) -> AppResult<Json<WorksheetState>> {
    editing::restore_word_table(&state, teacher.id(), id, &block_id).await?;
    Ok(Json(state_of(&state, id).await?))
}

#[derive(Debug, Serialize)]
pub struct FilledMeaningsReply {
    #[serde(flatten)]
    pub worksheet: WorksheetState,
    /// Said plainly to the teacher when some words keep their English meaning; `null` otherwise.
    pub notice: Option<String>,
    /// How many words got a Chinese meaning from the model in this call.
    pub filled: usize,
}

/// `POST /api/worksheets/:id/vocabulary-meanings` `{ "tables": bool, "banks": bool }`
/// (「補上中文字義」 after switching Key Vocabulary to Chinese; the word banks' part of speech
/// and meanings)
pub async fn fill_vocabulary_meanings(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    Json(scope): Json<crate::worksheet::key_vocab::Scope>,
) -> AppResult<Json<FilledMeaningsReply>> {
    let done = editing::fill_vocabulary_meanings(&state, teacher.id(), id, scope).await?;
    Ok(Json(FilledMeaningsReply {
        worksheet: state_of(&state, id).await?,
        notice: done.notice,
        filled: done.filled,
    }))
}

// ---------------------------------------------------------------- sheets and versions

#[derive(Debug, Default, Deserialize)]
pub struct SheetQuery {
    pub level: Option<String>,
    pub audience: Option<String>,
    pub format: Option<String>,
}

fn sheet_for(ws: &Worksheet, content: &Content, q: &SheetQuery) -> AppResult<Sheet> {
    let level = match q.level.as_deref() {
        Some(l) => Level::parse(l)
            .ok_or_else(|| AppError::bad_request("level 需為 basic、standard 或 advanced。"))?,
        None => content.levels.first().copied().unwrap_or(Level::Standard),
    };
    if !content.levels.contains(&level) {
        return Err(AppError::bad_request(format!(
            "這份學習單沒有{}。",
            level.label()
        )));
    }
    let audience = match q.audience.as_deref() {
        Some(a) => Audience::parse(a)
            .ok_or_else(|| AppError::bad_request("audience 需為 student 或 teacher。"))?,
        None => Audience::Student,
    };
    let mut sheet = project(&ws.title, content, level, audience);
    keep_original_text(&mut sheet, content, &ws.material);
    Ok(sheet)
}

/// The projected sheet with its pictures (or their empty boxes).
async fn sheet_with_images(
    state: &AppState,
    ws: &Worksheet,
    content: &Content,
    q: &SheetQuery,
) -> AppResult<Sheet> {
    let mut sheet = sheet_for(ws, content, q)?;
    let slots = images::states(state, ws.id).await?.slots;
    images::decorate(&mut sheet, content, &slots);
    Ok(sheet)
}

/// The sheet of an earlier version: its pictures are the rows it recorded (the current ones for
/// a version from before pictures were kept).
pub(crate) async fn sheet_of_version(
    state: &AppState,
    ws: &Worksheet,
    content: &Content,
    pictures: Option<&serde_json::Value>,
    q: &SheetQuery,
) -> AppResult<Sheet> {
    let mut sheet = sheet_for(ws, content, q)?;
    let slots = match pictures {
        Some(p) => images::of_version(&state.db, ws.id, p).await?,
        None => images::states(state, ws.id).await?.slots,
    };
    images::decorate(&mut sheet, content, &slots);
    Ok(sheet)
}

pub async fn sheet(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    ApiQuery(q): ApiQuery<SheetQuery>,
) -> AppResult<Json<Sheet>> {
    let ws = store::owned(&state.db, id, teacher.id()).await?;
    let content = store::latest(&state.db, id).await?.content;
    Ok(Json(sheet_with_images(&state, &ws, &content, &q).await?))
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
    let format = q.format.as_deref().unwrap_or("docx");
    if !matches!(format, "docx" | "pdf") {
        return Err(AppError::bad_request("format 需為 docx 或 pdf。"));
    }
    let sheet = sheet_with_images(&state, &ws, &latest.content, &q).await?;
    let bank = images::bank_for(&state, id, &sheet).await?;
    let pictures = sheet.blocks.iter().map(|b| b.images.len()).sum::<usize>();
    let embedded = bank.len();
    let (level, audience) = (sheet.level, sheet.audience);
    // A student's file names the neutral code, never the level (T9): 「…_A版_學生版」.
    let version = match audience {
        Audience::Student => sheet
            .version_code
            .as_deref()
            .map(|code| format!("_{code}版"))
            .unwrap_or_default(),
        Audience::Teacher => format!("_{}", level.label()),
    };
    let (bytes, content_type, extension) = match format {
        "docx" => {
            let bytes =
                tokio::task::spawn_blocking(move || export::docx_with_images(&sheet, &bank))
                    .await
                    .map_err(|e| AppError::Internal(anyhow::anyhow!(e)))??;
            (bytes, export::DOCX_TYPE, "docx")
        }
        "pdf" => {
            let made = tokio::task::spawn_blocking(move || export::pdf_with_images(&sheet, &bank))
                .await
                .map_err(|e| AppError::Internal(anyhow::anyhow!(e)))?;
            match made {
                Ok(bytes) => (bytes, export::PDF_TYPE, "pdf"),
                Err(ExportError::NoCjkFont) => {
                    return Err(AppError::Conflict(
                        "這份學習單裡有中文（例如教師備註或字彙的中文提示），但伺服器沒有中文字型，無法做成 PDF。請改用 DOCX，或請管理者安裝中文字型。".to_string(),
                    ))
                }
                Err(ExportError::Other(e)) => return Err(AppError::Internal(e)),
            }
        }
        _ => return Err(AppError::bad_request("format 需為 docx 或 pdf。")),
    };
    let name = format!(
        "{}{version}_{}.{extension}",
        if ws.filename.trim().is_empty() {
            "學習單"
        } else {
            ws.filename.trim()
        },
        audience.label()
    );
    log_usage(
        db,
        teacher.id(),
        "worksheet_exported",
        json!({
            "worksheet_id": id, "version_no": latest.version_no, "format": format,
            "level": level, "audience": audience, "bytes": bytes.len(),
            "pictures": pictures, "pictures_embedded": embedded,
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

/// `GET /api/worksheets/:id/thumbnail`: page 1 of the student PDF (default level, pictures
/// included) as a 480 px wide JPEG, for the cards of 我的作品. A preview, not an export: no
/// usage log. 409 while the worksheet is being made. `Cache-Control: private, max-age=0,
/// must-revalidate` + an `ETag` of the state the picture shows (304 on `If-None-Match`).
pub async fn thumbnail(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    headers: HeaderMap,
) -> AppResult<Response> {
    let db = &state.db;
    let ws = store::owned(db, id, teacher.id()).await?;
    if !ws.generated {
        return Err(AppError::Conflict(thumbnail::MSG_NOT_GENERATED.to_string()));
    }
    let (_, running) = ws_ai::jobs(&state, id).await?;
    if running {
        return Err(AppError::Conflict(thumbnail::MSG_RUNNING.to_string()));
    }
    let latest = store::latest(db, id).await?;
    let fingerprint = thumbnail::fingerprint(db, &ws).await?;
    let etag = thumbnail::etag(id, latest.version_no, &fingerprint);
    let response = Response::builder()
        .header(CACHE_CONTROL, "private, max-age=0, must-revalidate")
        .header(ETAG, &etag)
        .header(VARY, "Authorization");
    if crate::routes::library::etag_matches(
        headers.get(IF_NONE_MATCH).and_then(|v| v.to_str().ok()),
        &etag,
    ) {
        return response
            .status(StatusCode::NOT_MODIFIED)
            .body(Body::empty())
            .map_err(|e| AppError::Internal(e.into()));
    }
    let key = thumbnail::cache_key(id, latest.version_no, &fingerprint);
    let bytes = thumbnail::get_or_render(&state, &key, || async {
        let q = SheetQuery::default();
        let sheet = sheet_with_images(&state, &ws, &latest.content, &q).await?;
        let bank = images::bank_for(&state, id, &sheet).await?;
        let made = tokio::task::spawn_blocking(move || export::pdf_with_images(&sheet, &bank))
            .await
            .map_err(|e| AppError::Internal(anyhow::anyhow!(e)))?;
        match made {
            Ok(bytes) => Ok(bytes),
            Err(ExportError::NoCjkFont) => Err(AppError::Conflict(
                "這份學習單裡有中文，但伺服器沒有中文字型，做不出預覽圖。".to_string(),
            )),
            Err(ExportError::Other(e)) => Err(AppError::Internal(e)),
        }
    })
    .await?;
    response
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, thumbnail::CONTENT_TYPE)
        .header(CONTENT_LENGTH, bytes.len())
        .header(X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(CONTENT_SECURITY_POLICY, "default-src 'none'; sandbox")
        .body(Body::from(bytes))
        .map_err(|e| AppError::Internal(e.into()))
}

// ---------------------------------------------------------------- pictures

/// `GET /api/worksheets/:id/images`: the pictures of the slots (polled while some are made).
pub async fn images_state(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<Json<WorksheetImages>> {
    store::owned(&state.db, id, teacher.id()).await?;
    Ok(Json(images::states(&state, id).await?))
}

/// How long a browser keeps a picture before asking again (`If-None-Match`). A picture never
/// changes: a new one has a new id.
const IMAGE_MAX_AGE_SECS: u32 = 3600;

/// `GET /api/worksheets/:id/images/:image_id`: one stored picture (JPEG) of the teacher's own
/// worksheet. ETag = its SHA-256; `Cache-Control: private`; `Vary: Authorization`.
pub async fn image_file(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath((id, image_id)): ApiPath<(i64, i64)>,
    headers: HeaderMap,
) -> AppResult<Response> {
    store::owned(&state.db, id, teacher.id()).await?;
    let (key, sha) = images::stored(&state.db, id, image_id)
        .await?
        .ok_or_else(|| AppError::not_found("找不到這張圖。"))?;
    serve_image(&state, &key, &sha, &headers).await
}

/// A stored picture as a response (JPEG, ETag = its SHA-256, 304 on `If-None-Match`); the caller
/// has already checked who may read it.
pub(crate) async fn serve_image(
    state: &AppState,
    key: &str,
    sha: &str,
    headers: &HeaderMap,
) -> AppResult<Response> {
    let etag = format!("\"{sha}\"");
    let response = Response::builder()
        .header(
            CACHE_CONTROL,
            format!("private, max-age={IMAGE_MAX_AGE_SECS}"),
        )
        .header(ETAG, &etag)
        .header(VARY, "Authorization");
    let fresh = crate::routes::library::etag_matches(
        headers.get(IF_NONE_MATCH).and_then(|v| v.to_str().ok()),
        &etag,
    );
    if fresh {
        return response
            .status(StatusCode::NOT_MODIFIED)
            .body(Body::empty())
            .map_err(|e| AppError::Internal(e.into()));
    }
    let bytes = images::bytes_of(state, key).await?;
    response
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, images::MIME)
        .header(CONTENT_LENGTH, bytes.len())
        .header(X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(CONTENT_SECURITY_POLICY, "default-src 'none'; sandbox")
        .body(Body::from(bytes))
        .map_err(|e| AppError::Internal(e.into()))
}

#[derive(Debug, Default, Deserialize)]
pub struct RegenerateImage {
    /// The teacher's note (≤200), e.g. 「改成晴天」.
    #[serde(default)]
    pub hint: Option<String>,
}

/// `POST /api/worksheets/:id/image-slots/:slot_id/regenerate` (「重畫」) → 202 with the pictures.
pub async fn regenerate_image(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath((id, slot_id)): ApiPath<(i64, String)>,
    ApiJson(req): ApiJson<RegenerateImage>,
) -> AppResult<(StatusCode, Json<WorksheetImages>)> {
    let ws = store::owned(&state.db, id, teacher.id()).await?;
    images::regenerate(&state, &ws, &slot_id, req.hint.as_deref()).await?;
    history::record_picture_button(
        &state.db,
        id,
        &slot_id,
        history::PictureButton::Regenerate,
        req.hint.as_deref().unwrap_or_default(),
    )
    .await;
    Ok((
        StatusCode::ACCEPTED,
        Json(images::states(&state, id).await?),
    ))
}

/// `POST /api/worksheets/:id/image-slots/:slot_id/upload` (「換成我的圖」): multipart `file`, a
/// png / jpg / webp / gif within the upload limit for pictures.
pub async fn upload_image(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath((id, slot_id)): ApiPath<(i64, String)>,
    mut form: Multipart,
) -> AppResult<Json<WorksheetImages>> {
    let ws = store::owned(&state.db, id, teacher.id()).await?;
    if state.storage.is_none() {
        return Err(material_store::storage_missing());
    }
    // Uploads are buffered: only a few at a time (the others wait their turn).
    let _slot = state
        .upload_slots
        .clone()
        .acquire_owned()
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("upload slots closed: {e}")))?;
    let bad_form = |e: axum::extract::multipart::MultipartError| {
        AppError::bad_request(format!("上傳的內容讀不出來：{}", e.body_text()))
    };
    let mut file: Option<(String, Bytes)> = None;
    while let Some(mut field) = form.next_field().await.map_err(bad_form)? {
        if field.name() != Some("file") {
            continue;
        }
        let is_image = crate::materials::file_type_for_name(field.file_name().unwrap_or_default())
            .is_some_and(|t| t.kind == crate::materials::Kind::Image);
        if !is_image {
            return Err(AppError::bad_request(
                "只能上傳圖片（.png、.jpg、.webp、.gif）。",
            ));
        }
        file = Some(crate::routes::library::read_file_field(&mut field).await?);
    }
    let Some((filename, data)) = file else {
        return Err(AppError::bad_request("請選擇要上傳的圖片。"));
    };
    images::replace_with_upload(&state, &ws, &slot_id, &filename, data).await?;
    history::record_picture_button(&state.db, id, &slot_id, history::PictureButton::Upload, "")
        .await;
    Ok(Json(images::states(&state, id).await?))
}

/// `DELETE /api/worksheets/:id/image-slots/:slot_id` (「拿掉」).
pub async fn remove_image(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath((id, slot_id)): ApiPath<(i64, String)>,
) -> AppResult<Json<WorksheetImages>> {
    let ws = store::owned(&state.db, id, teacher.id()).await?;
    images::remove(&state, &ws, &slot_id).await?;
    history::record_picture_button(&state.db, id, &slot_id, history::PictureButton::Remove, "")
        .await;
    Ok(Json(images::states(&state, id).await?))
}

// ---------------------------------------------------------------- lesson link and sync

pub async fn sync_preview(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<Json<Preview>> {
    let preview = synchronization::sync_preview(&state.db, teacher.id(), id).await?;
    Ok(Json(preview))
}

pub async fn sync_apply(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    ApiJson(req): ApiJson<ApplySync>,
) -> AppResult<Json<serde_json::Value>> {
    let result = synchronization::sync_apply(&state.db, teacher.id(), id, req).await?;
    Ok(Json(result))
}

pub async fn link(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    ApiJson(req): ApiJson<LinkLesson>,
) -> AppResult<Json<WorksheetState>> {
    synchronization::link(&state.db, teacher.id(), id, req).await?;
    Ok(Json(state_of(&state, id).await?))
}

pub async fn save_as_lesson(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    ApiJson(req): ApiJson<SaveAsLesson>,
) -> AppResult<Json<WorksheetState>> {
    synchronization::save_as_lesson(&state.db, teacher.id(), id, req).await?;
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
