//! 備課對話 endpoints (docs/API.md「Phase 2.6：備課對話」): sessions, their turns (all run in the
//! background; the page polls `GET /api/prep-sessions/:id`), the context PATCH, the hard
//! delete and the 教學套件 and 課堂活動 downloads. Every session belongs to one teacher; anyone
//! else's id is a 404.

use axum::extract::{Multipart, State};
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_TYPE};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::audit::log_usage;
use crate::auth::AuthTeacher;
use crate::error::{ApiJson, ApiPath, ApiQuery, AppError, AppResult};
use crate::materials::ocr;
use crate::materials::store::{self as material_store, MaterialRow};
use crate::memory;
use crate::prep::images as prep_images;
use crate::prep::kit::{self, KitWorksheet};
use crate::prep::run::{self, TurnInput};
use crate::prep::selfcheck::{self, SiopCheck};
use crate::prep::store::{self, Session, SessionRow, Summary};
use crate::prep::{
    chip_label, is_worksheet_kind, ActivityPlan, SessionOptions, AUTO_TITLE_CHARS, KINDS,
    KIND_ACTIVITY, KIND_ASK, KIND_CHAT, KIND_KIT, KIND_RESOURCES, KIND_REVISE, KIND_WORKSHEET,
    MAX_REQUEST_CHARS, MAX_TITLE_CHARS, MAX_TOPIC_CHARS, STATUS_RUNNING,
};
use crate::serde_support::double_option;
use crate::teaching_context::owned_class;
use crate::text::clip_chars;
use crate::worksheet::ai as ws_ai;
use crate::worksheet::bloom::Output;
use crate::worksheet::export;
use crate::worksheet::safe_filename;
use crate::worksheet::store as ws_store;
use crate::worksheet::template::extract::{
    self as tpl_extract, MSG_MATERIAL_NOT_FOUND, MSG_NO_TEXT,
};
use crate::worksheet::template::store as tpl_store;
use crate::worksheet::thumbnail as ws_thumbnail;
use crate::AppState;

const MSG_KIND: &str =
    "kind 需為 worksheet、assessment、comprehension、tiered、kit、activity、resources 或 chat。";
const MSG_LEGACY_KIND: &str = "「直接問」和「修改學習單」已經合併成對話：請用 kind = chat。";
const MSG_TURN_NOT_FOUND: &str = "找不到這一輪。";
const MSG_NOT_KIT: &str = "這一輪不是教學套件，沒有教學套件可以下載。";
const MSG_KIT_RUNNING: &str = "教學套件還在產生，好了之後再下載。";
const MSG_KIT_EMPTY: &str = "這個教學套件沒有完成，沒有內容可以下載。請按「再試一次」。";
const MSG_KIT_FORMAT: &str = "format 需為 docx。";
const MSG_NOT_ACTIVITY: &str = "這一輪不是課堂活動，沒有課堂活動可以下載。";
const MSG_ACTIVITY_RUNNING: &str = "課堂活動還在產生，好了之後再下載。";
const MSG_ACTIVITY_EMPTY: &str = "這個課堂活動沒有完成，沒有內容可以下載。請按「再試一次」。";
const MSG_NEEDS_MATERIAL: &str = "這個動作需要教材：請上傳檔案、從教材庫選，或貼上課文。";
const MSG_NEEDS_MATERIAL_OR_TOPIC: &str = "找教學資源需要教材或主題。";
const MSG_NEEDS_REQUEST: &str = "請寫下你想問或想改的內容。";
const MSG_MATERIAL_DELETED: &str = "這個備課對話的教材已經刪除了，請開一個新的備課對話。";
const MSG_NO_WORKSHEET: &str = "這個備課對話還沒有學習單可以修改。";
const MSG_WORKSHEET_RUNNING: &str = "Manna 正在產生這份學習單，好了之後再改。";
const MSG_SAMPLE_KIND: &str = "範本只適用於完整學習單";
const MSG_SAMPLE_DELETED: &str = "原本的學習單檔案已經刪除，無法重試；請重新上傳。";

#[derive(Debug, Serialize)]
pub struct SessionList {
    pub sessions: Vec<Summary>,
}

pub async fn list(
    State(state): State<AppState>,
    teacher: AuthTeacher,
) -> AppResult<Json<SessionList>> {
    Ok(Json(SessionList {
        sessions: store::list(&state.db, teacher.id()).await?,
    }))
}

/// A kind the server makes turns of. `ask` and `revise` were replaced by `chat`: earlier turns
/// still load, but no new ones are made (unless `PREP_LEGACY_TURNS` is on, which the tests of the
/// old paths use).
fn check_kind(state: &AppState, kind: &str) -> AppResult<()> {
    if !KINDS.contains(&kind) {
        return Err(AppError::bad_request(MSG_KIND));
    }
    if matches!(kind, KIND_ASK | KIND_REVISE) && !state.config.prep_legacy_turns {
        return Err(AppError::bad_request(MSG_LEGACY_KIND));
    }
    Ok(())
}

fn check_request(request: &str) -> AppResult<String> {
    let request = request.trim();
    if request.chars().count() > MAX_REQUEST_CHARS {
        return Err(AppError::bad_request(format!(
            "內容最多 {MAX_REQUEST_CHARS} 字。"
        )));
    }
    Ok(request.to_string())
}

/// The request a turn records: the teacher's words, or the chip's label.
fn request_or_label(kind: &str, request: String) -> String {
    if request.is_empty() {
        chip_label(kind).to_string()
    } else {
        request
    }
}

#[derive(Debug, Default, Deserialize)]
pub struct PastedMaterial {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub text: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateSession {
    pub kind: String,
    #[serde(default)]
    pub request: Option<String>,
    #[serde(default)]
    pub material_id: Option<i64>,
    #[serde(default)]
    pub material: Option<PastedMaterial>,
    #[serde(default)]
    pub topic: Option<String>,
    #[serde(default)]
    pub class_id: Option<i64>,
    #[serde(default)]
    pub template_id: Option<i64>,
    /// A worksheet sample (one of the teacher's materials with text): its template is reused or
    /// extracted and becomes the session's template. `worksheet` only; ignored with
    /// `template_id`.
    #[serde(default)]
    pub template_material_id: Option<i64>,
    /// 出題選項, passed into every worksheet the session makes.
    #[serde(default)]
    pub options: Option<SessionOptions>,
    /// Which page started the session (`home` or `create`), for the research log only. Anything
    /// else (another string, another type) is ignored, never an error.
    #[serde(default)]
    pub entry: Option<serde_json::Value>,
}

/// The entries the usage log records; any other value is dropped.
fn known_entry(entry: Option<&serde_json::Value>) -> Option<&'static str> {
    match entry.and_then(|e| e.as_str()).map(str::trim) {
        Some("home") => Some("home"),
        Some("create") => Some("create"),
        _ => None,
    }
}

/// Options checked with the worksheet settings' rules (400 with their message).
fn checked_options(options: Option<SessionOptions>) -> AppResult<SessionOptions> {
    options
        .unwrap_or_default()
        .normalised()
        .map_err(AppError::bad_request)
}

/// A library material the session can work with: the teacher's own or a platform one, with
/// extracted text.
async fn usable_material(db: &sqlx::PgPool, teacher_id: i64, id: i64) -> AppResult<(i64, String)> {
    let row = material_store::readable(db, id, teacher_id)
        .await
        .map_err(|_| AppError::bad_request(MSG_MATERIAL_NOT_FOUND))?;
    ocr::check_idle(&row)?;
    if row.text_status != "ready" || row.paragraphs.0.is_empty() {
        return Err(AppError::bad_request(MSG_NO_TEXT));
    }
    Ok((row.id, row.title))
}

/// A worksheet sample (照我的學習單做) and whether its OCR is still running: such a sample is
/// waited for (docs/API.md「Phase 2.10」); any other follows the rules of
/// `POST /api/worksheet-templates` (400 otherwise).
async fn sample_source(
    db: &sqlx::PgPool,
    teacher_id: i64,
    id: i64,
) -> AppResult<(MaterialRow, bool)> {
    // The teacher's own sample or a platform one (學習單範本).
    let row = material_store::readable(db, id, teacher_id)
        .await
        .map_err(|_| AppError::bad_request(MSG_MATERIAL_NOT_FOUND))?;
    if ocr::is_open(&row.ocr_status) {
        return Ok((row, true));
    }
    Ok((tpl_extract::source(db, teacher_id, id).await?, false))
}

async fn spawn_turn(
    state: &AppState,
    teacher: &AuthTeacher,
    session: SessionRow,
    kind: &str,
    request: &str,
    worksheet_id: Option<i64>,
    image_ids: &[i64],
) -> AppResult<i64> {
    // A worksheet turn follows the session's template; it waits while that is extracting.
    let template_id = if kind == KIND_WORKSHEET {
        session.template_id
    } else {
        None
    };
    // With the AI queue on, the turn's job is made in the same transaction as the turn.
    let queue =
        (state.config.ai_queue_enabled && run::queued_kind(kind)).then_some(store::QueueTurn {
            max_queued: state.config.ai_max_queued_per_teacher,
            track: teacher.track(),
            worksheet_id,
        });
    let turn = store::start_turn(&state.db, session.id, kind, request, template_id, queue).await?;
    // The pictures belong to the turn before it runs (the run reads them).
    if let Err(e) = prep_images::attach_to_turn(&state.db, image_ids, turn.id).await {
        let message = match &e {
            AppError::Conflict(m) | AppError::BadRequest(m) => m.clone(),
            _ => "附上的圖片沒有存好，這次沒有送出。".to_string(),
        };
        store::finish(&state.db, turn.id, Err(message)).await?;
        return Err(e);
    }
    let mut payload = json!({ "session_id": session.id, "turn_id": turn.id, "kind": kind });
    if !image_ids.is_empty() {
        payload["images"] = json!(image_ids.len());
    }
    log_usage(&state.db, teacher.id(), "prep_turn_created", payload).await?;
    let turn_id = turn.id;
    run::start(
        state,
        TurnInput {
            turn,
            session,
            track: teacher.track().to_string(),
            worksheet_id,
        },
    )
    .await?;
    Ok(turn_id)
}

/// What the teacher typed may state a lasting preference: Manna may offer to remember it (only a
/// suggestion, in the background, when the research switch is on).
async fn offer_habit(
    state: &AppState,
    teacher: &AuthTeacher,
    source: memory::Source,
    typed: bool,
    text: &str,
) {
    if typed {
        memory::detect::spawn(state, teacher.id(), source, text).await;
    }
}

pub async fn create(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiJson(req): ApiJson<CreateSession>,
) -> AppResult<(StatusCode, Json<Session>)> {
    let db = &state.db;
    let kind = req.kind.trim();
    check_kind(&state, kind)?;
    if kind == KIND_REVISE {
        return Err(AppError::bad_request(
            "新的備課對話不能從「修改」開始；請先做學習單或設計評量。",
        ));
    }
    // The worksheet sample (照我的學習單做); a template given as well wins.
    let sample_id = req
        .template_material_id
        .filter(|_| req.template_id.is_none());
    if sample_id.is_some() && kind != KIND_WORKSHEET {
        return Err(AppError::bad_request(MSG_SAMPLE_KIND));
    }
    let request = check_request(req.request.as_deref().unwrap_or_default())?;
    let options = checked_options(req.options.clone())?;
    let topic = req
        .topic
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string);
    if topic
        .as_deref()
        .is_some_and(|t| t.chars().count() > MAX_TOPIC_CHARS)
    {
        return Err(AppError::bad_request(format!(
            "主題最多 {MAX_TOPIC_CHARS} 字。"
        )));
    }
    let pasted = req
        .material
        .as_ref()
        .filter(|m| !m.text.trim().is_empty() || m.title.is_some());
    let has_material = req.material_id.is_some() || pasted.is_some();
    match kind {
        KIND_ASK | KIND_CHAT if request.is_empty() => {
            return Err(AppError::bad_request("請寫下你想問 Manna 的問題。"))
        }
        KIND_RESOURCES if !has_material && topic.is_none() => {
            return Err(AppError::bad_request(MSG_NEEDS_MATERIAL_OR_TOPIC))
        }
        KIND_ASK | KIND_CHAT | KIND_RESOURCES => {}
        _ if !has_material => return Err(AppError::bad_request(MSG_NEEDS_MATERIAL)),
        _ => {}
    }
    if let Some(class_id) = req.class_id {
        owned_class(db, class_id, teacher.id()).await?;
    }
    if let Some(template_id) = req.template_id {
        tpl_store::ready_for_use(db, teacher.id(), template_id).await?;
    }
    let sample = match sample_id {
        Some(id) => Some(sample_source(db, teacher.id(), id).await?),
        None => None,
    };
    // A library material wins over pasted text (which is then not stored), as for worksheets.
    let material: Option<(i64, String)> = match (req.material_id, pasted) {
        (Some(id), _) => Some(usable_material(db, teacher.id(), id).await?),
        (None, Some(p)) => {
            let title = p.title.as_deref().unwrap_or_default().trim();
            let row = material_store::create_text(db, teacher.id(), title, "", &p.text).await?;
            if row.text_status != "ready" || row.paragraphs.0.is_empty() {
                return Err(AppError::bad_request(MSG_NO_TEXT));
            }
            Some((row.id, row.title))
        }
        (None, None) => None,
    };
    // 只選要用的段落 needs a library material whose paragraphs have these ids.
    if !options.paragraph_ids.is_empty() {
        let known = match &material {
            Some((id, _)) => {
                let row = material_store::readable(db, *id, teacher.id())
                    .await
                    .map_err(|_| AppError::bad_request(MSG_MATERIAL_NOT_FOUND))?;
                row.paragraphs
                    .0
                    .iter()
                    .filter(|p| options.paragraph_ids.contains(&p.id))
                    .count()
            }
            None => 0,
        };
        if known != options.paragraph_ids.len() {
            return Err(AppError::bad_request(
                crate::worksheet::material::MSG_BAD_SELECTION,
            ));
        }
    }
    let typed = !request.is_empty();
    let request = request_or_label(kind, request);
    let title = match (&material, &topic) {
        (Some((_, t)), _) if !t.trim().is_empty() => clip_chars(t.trim(), MAX_TITLE_CHARS - 1),
        (_, Some(t)) => clip_chars(t, MAX_TITLE_CHARS - 1),
        _ => clip_chars(&request, AUTO_TITLE_CHARS),
    };
    // The sample's template: the newest ready one made from it (or one still extracting),
    // otherwise a new one, inserted and extracted exactly as `POST /api/worksheet-templates`.
    // A sample still being read by OCR gets a new template (or one already waiting for it)
    // that is extracted once the OCR is done; the turn waits for it as for any extraction.
    let (template_id, extraction) = match sample {
        Some((m, after_ocr)) => {
            tpl_store::expire_stale(
                db,
                teacher.id(),
                tpl_extract::stale_after(&state),
                state.config.ai_queue_enabled,
            )
            .await?;
            let reusable = if after_ocr {
                tpl_store::extracting_for_material(db, teacher.id(), m.id).await?
            } else {
                tpl_store::reusable_for_material(db, teacher.id(), m.id).await?
            };
            match reusable {
                Some(t) => (Some(t.id), None),
                None => {
                    let title = tpl_extract::default_title(&m.title);
                    let t = tpl_store::insert(db, teacher.id(), &title, m.id).await?;
                    // `None`: the text comes from the OCR.
                    let text = (!after_ocr).then_some(m.text);
                    (Some(t.id), Some((t, text)))
                }
            }
        }
        None => (req.template_id, None),
    };
    let made = async {
        let session = store::insert(
            db,
            store::NewSession {
                teacher_id: teacher.id(),
                title: &title,
                material_id: material.as_ref().map(|(id, _)| *id),
                topic: topic.as_deref(),
                class_id: req.class_id,
                template_id,
                options: &options,
            },
        )
        .await?;
        log_usage(
            db,
            teacher.id(),
            "prep_session_created",
            json!({
                "session_id": session.id, "kind": kind, "material_id": session.material_id,
                "class_id": session.class_id, "template_id": session.template_id,
                "template_material_id": sample_id,
                "entry": known_entry(req.entry.as_ref()), "ui_version": 4,
            }),
        )
        .await?;
        if let Some((t, _)) = &extraction {
            log_usage(
                db,
                teacher.id(),
                "worksheet_template_created",
                json!({ "template_id": t.id, "material_id": t.source_material_id,
                        "prep_session_id": session.id }),
            )
            .await?;
        }
        let id = session.id;
        let turn_id = spawn_turn(&state, &teacher, session, kind, &request, None, &[]).await?;
        let source = memory::Source::PrepTurn {
            session_id: id,
            turn_id,
            entry: known_entry(req.entry.as_ref()),
        };
        offer_habit(&state, &teacher, source, typed, &request).await;
        // Read the answer before the extraction starts (a fast one could otherwise finish first).
        store::session(&state, id).await
    }
    .await;
    // The template exists either way: its extraction always runs.
    match extraction {
        Some((t, Some(text))) => tpl_extract::start(&state, &t, text).await,
        Some((t, None)) => tpl_extract::spawn_after_ocr(&state, &t),
        None => {}
    }
    Ok((StatusCode::CREATED, Json(made?)))
}

/// Before a new `worksheet` turn: a session template whose extraction failed is extracted
/// again (like `POST /api/worksheet-templates/:id/retry`), so that 「再試一次」 retries it. The
/// template's material, to start the job with once the turn is open; `None` when nothing is
/// to be retried.
async fn template_to_retry(
    state: &AppState,
    teacher_id: i64,
    session: &SessionRow,
) -> AppResult<Option<(i64, String)>> {
    let db = &state.db;
    let Some(template_id) = session.template_id else {
        return Ok(None);
    };
    tpl_store::expire_stale(
        db,
        teacher_id,
        tpl_extract::stale_after(state),
        state.config.ai_queue_enabled,
    )
    .await?;
    let Some(t) = tpl_store::find(db, template_id, teacher_id).await? else {
        return Ok(None);
    };
    // A platform template is retried by its admins, never from a teacher's session.
    if t.status != tpl_store::STATUS_FAILED || t.is_platform() {
        return Ok(None);
    }
    let Some(material_id) = t.source_material_id else {
        return Err(AppError::bad_request(MSG_SAMPLE_DELETED));
    };
    let material = tpl_extract::source(db, teacher_id, material_id).await?;
    Ok(Some((template_id, material.text)))
}

pub async fn get(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<Json<Session>> {
    store::owned(&state.db, id, teacher.id()).await?;
    Ok(Json(store::session(&state, id).await?))
}

#[derive(Debug, Deserialize)]
pub struct PatchSession {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub class_id: Option<Option<i64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub template_id: Option<Option<i64>>,
    /// Replaces all the options; `null` clears them.
    #[serde(default, deserialize_with = "double_option")]
    pub options: Option<Option<SessionOptions>>,
}

pub async fn patch(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    ApiJson(req): ApiJson<PatchSession>,
) -> AppResult<Json<Session>> {
    let db = &state.db;
    store::owned(db, id, teacher.id()).await?;
    let title = match req.title.as_deref().map(str::trim) {
        Some("") => return Err(AppError::bad_request("標題不能是空的。")),
        Some(t) if t.chars().count() > MAX_TITLE_CHARS => {
            return Err(AppError::bad_request(format!(
                "標題最多 {MAX_TITLE_CHARS} 字。"
            )))
        }
        other => other,
    };
    if let Some(Some(class_id)) = req.class_id {
        owned_class(db, class_id, teacher.id()).await?;
    }
    if let Some(Some(template_id)) = req.template_id {
        tpl_store::ready_for_use(db, teacher.id(), template_id).await?;
    }
    // 只選要用的段落 changes only through `PUT …/material-selection`: a PATCH of the options
    // keeps what is stored.
    let options = match req.options {
        Some(o) => {
            let mut checked = checked_options(o)?;
            checked.paragraph_ids = store::session_row(db, id).await?.options.0.paragraph_ids;
            Some(checked)
        }
        None => None,
    };
    store::update(
        db,
        id,
        title,
        req.class_id,
        req.template_id,
        options.as_ref(),
    )
    .await?;
    Ok(Json(store::session(&state, id).await?))
}

#[derive(Debug, Deserialize)]
pub struct MaterialSelection {
    /// The paragraph ids (of the session's material) the turns work with; `null` or empty =
    /// all of them.
    #[serde(default)]
    pub paragraph_ids: Option<Vec<String>>,
}

/// `PUT /api/prep-sessions/:id/material-selection`: 只選要用的段落. Stored in the session's
/// options; the material itself is not changed. The turns after this one (and 重新產生) use it.
pub async fn put_material_selection(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    ApiJson(req): ApiJson<MaterialSelection>,
) -> AppResult<Json<Session>> {
    let db = &state.db;
    let row = store::owned(db, id, teacher.id()).await?;
    let Some(material_id) = row.material_id else {
        return Err(AppError::bad_request("這個備課對話沒有教材。"));
    };
    let ids = crate::worksheet::material::clean_selection(
        req.paragraph_ids.as_deref().unwrap_or_default(),
    )
    .map_err(AppError::bad_request)?
    .unwrap_or_default();
    let material = material_store::readable(db, material_id, teacher.id())
        .await
        .map_err(|_| AppError::bad_request(MSG_MATERIAL_NOT_FOUND))?;
    let total = material.paragraphs.0.len();
    let known = material
        .paragraphs
        .0
        .iter()
        .filter(|p| ids.contains(&p.id))
        .count();
    if known != ids.len() {
        return Err(AppError::bad_request(
            crate::worksheet::material::MSG_BAD_SELECTION,
        ));
    }
    let mut options = row.options.0.clone();
    options.paragraph_ids = ids;
    store::update(db, id, None, None, None, Some(&options)).await?;
    log_usage(
        db,
        teacher.id(),
        "prep_material_selection",
        json!({
            "session_id": id, "material_id": material_id, "total": total,
            "selected": if options.paragraph_ids.is_empty() { total } else { known },
            "all": options.paragraph_ids.is_empty(),
        }),
    )
    .await?;
    Ok(Json(store::session(&state, id).await?))
}

pub async fn delete(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<Json<serde_json::Value>> {
    store::owned(&state.db, id, teacher.id()).await?;
    // The attached pictures leave the storage first; if that fails nothing is deleted.
    prep_images::purge_session(&state, id).await?;
    let worksheets = store::delete(&state.db, id, teacher.id()).await?;
    // Their kept previews go too; a failure there never undoes or blocks the delete.
    for ws_id in &worksheets {
        if let Ok(Some(ws)) = ws_store::by_id(&state.db, *ws_id).await {
            ws_thumbnail::forget(&state, &ws).await;
        }
    }
    Ok(Json(
        json!({ "ok": true, "worksheets_deleted": worksheets.len() }),
    ))
}

// ---------------------------------------------------------------- attached pictures

/// `POST /api/prep-sessions/:id/images` (multipart `file`): one picture for the next revise
/// request (PNG, JPEG or WebP, ≤ 5 MB). 201 with the picture; it is re-encoded as JPEG (≤ 1568
/// px) and waits to be named in `image_ids`.
pub async fn upload_image(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    mut form: Multipart,
) -> AppResult<(StatusCode, Json<prep_images::PrepImage>)> {
    store::owned(&state.db, id, teacher.id()).await?;
    if state.storage.is_none() {
        return Err(material_store::storage_missing());
    }
    // Uploads are buffered: only a few at a time.
    let _slot = state
        .upload_slots
        .clone()
        .acquire_owned()
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("upload slots closed: {e}")))?;
    let bad_form = |e: axum::extract::multipart::MultipartError| {
        AppError::bad_request(format!("上傳的內容讀不出來：{}", e.body_text()))
    };
    let mut file: Option<(String, bytes::BytesMut)> = None;
    while let Some(mut field) = form.next_field().await.map_err(bad_form)? {
        if field.name() != Some("file") {
            continue;
        }
        let filename = field.file_name().unwrap_or_default().to_string();
        // The type is checked before the body is read.
        if !matches!(
            crate::materials::file_type_for_name(&filename).map(|t| t.ext),
            Some("png" | "jpg" | "webp")
        ) {
            return Err(AppError::bad_request(prep_images::MSG_TYPE));
        }
        let mut data = bytes::BytesMut::new();
        while let Some(chunk) = field.chunk().await.map_err(bad_form)? {
            if (data.len() + chunk.len()) as u64 > prep_images::MAX_BYTES {
                return Err(AppError::bad_request(prep_images::MSG_TOO_BIG));
            }
            data.extend_from_slice(&chunk);
        }
        file = Some((filename, data));
    }
    let Some((filename, data)) = file else {
        return Err(AppError::bad_request("請選擇要附上的圖片。"));
    };
    let original = data.len();
    let image = prep_images::upload(&state, teacher.id(), id, &filename, data.freeze()).await?;
    log_usage(
        &state.db,
        teacher.id(),
        "prep_image_uploaded",
        json!({ "session_id": id, "image_id": image.id, "original_bytes": original,
                "stored_bytes": image.bytes, "width": image.width, "height": image.height }),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(image)))
}

/// `GET /api/prep-sessions/:id/images/:image_id`: the picture (JPEG), to its teacher only.
pub async fn image_file(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath((id, image_id)): ApiPath<(i64, i64)>,
) -> AppResult<Response> {
    store::owned(&state.db, id, teacher.id()).await?;
    let row = prep_images::find(&state.db, id, image_id).await?;
    let bytes = prep_images::bytes_of(&state, &row.storage_key).await?;
    Response::builder()
        .status(StatusCode::OK)
        .header(axum::http::header::CACHE_CONTROL, "private, max-age=3600")
        .header(axum::http::header::VARY, "Authorization")
        .header(CONTENT_TYPE, prep_images::MIME)
        .header(axum::http::header::CONTENT_LENGTH, bytes.len())
        .header(axum::http::header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(
            axum::http::header::CONTENT_SECURITY_POLICY,
            "default-src 'none'; sandbox",
        )
        .body(axum::body::Body::from(bytes))
        .map_err(|e| AppError::Internal(e.into()))
}

/// `DELETE /api/prep-sessions/:id/images/:image_id`: a picture that was not sent yet (409 once
/// it is attached to a message).
pub async fn delete_image(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath((id, image_id)): ApiPath<(i64, i64)>,
) -> AppResult<Json<serde_json::Value>> {
    store::owned(&state.db, id, teacher.id()).await?;
    prep_images::delete_pending(&state, id, image_id).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Debug, Deserialize)]
pub struct CreateTurn {
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub request: Option<String>,
    /// Pictures uploaded with `POST /api/prep-sessions/:id/images` to send with this request
    /// (at most 2; only a revise takes them).
    #[serde(default)]
    pub image_ids: Vec<i64>,
    /// `chat`: the worksheet the turn works on, when it is not the session's latest one. A
    /// worksheet of another session or teacher is a 404.
    #[serde(default)]
    pub worksheet_id: Option<i64>,
}

pub async fn create_turn(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    ApiJson(req): ApiJson<CreateTurn>,
) -> AppResult<(StatusCode, Json<Session>)> {
    let db = &state.db;
    let session = store::owned(db, id, teacher.id()).await?;
    let request = check_request(req.request.as_deref().unwrap_or_default())?;
    store::refresh(&state, id).await?;
    let turns = store::turns(db, id).await?;
    let kind = match req.kind.as_deref().map(str::trim).filter(|k| !k.is_empty()) {
        Some(k) => {
            check_kind(&state, k)?;
            k.to_string()
        }
        // The bottom bar: a revision once there is a worksheet, otherwise a question.
        None => KIND_CHAT.to_string(),
    };
    let kind = kind.as_str();
    // Pictures go with a revision only (the other turns do not read them).
    if !req.image_ids.is_empty() && kind != KIND_REVISE && kind != KIND_CHAT {
        return Err(AppError::bad_request("只有對話的訊息可以附上圖片。"));
    }
    if req.worksheet_id.is_some() && kind != KIND_CHAT {
        return Err(AppError::bad_request("只有對話可以指定要處理的學習單。"));
    }
    let image_ids = prep_images::check_ids(db, teacher.id(), id, &req.image_ids).await?;
    let has_material = session.material_id.is_some();
    let has_topic = session
        .topic
        .as_deref()
        .is_some_and(|t| !t.trim().is_empty());
    let mut worksheet_id = None;
    match kind {
        KIND_ASK | KIND_REVISE | KIND_CHAT if request.is_empty() => {
            return Err(AppError::bad_request(MSG_NEEDS_REQUEST))
        }
        KIND_RESOURCES if !has_material && !has_topic => {
            return Err(AppError::bad_request(MSG_NEEDS_MATERIAL_OR_TOPIC))
        }
        k if (is_worksheet_kind(k) || k == KIND_ACTIVITY) && !has_material => {
            let had = turns
                .iter()
                .any(|t| !matches!(t.kind.as_str(), KIND_ASK | KIND_RESOURCES | KIND_CHAT));
            return Err(AppError::bad_request(if had {
                MSG_MATERIAL_DELETED
            } else {
                MSG_NEEDS_MATERIAL
            }));
        }
        KIND_CHAT => {
            // The worksheet asked for, which must be one of this session's; else the latest.
            worksheet_id = match req.worksheet_id {
                Some(wid) => {
                    let own: Option<i64> = sqlx::query_scalar(
                        "SELECT id FROM worksheets
                          WHERE id = $1 AND teacher_id = $2 AND prep_session_id = $3
                            AND deleted_at IS NULL",
                    )
                    .bind(wid)
                    .bind(teacher.id())
                    .bind(id)
                    .fetch_optional(db)
                    .await?;
                    Some(own.ok_or_else(|| AppError::not_found("找不到這份學習單。"))?)
                }
                None => store::revise_target(db, id).await?,
            };
        }
        KIND_REVISE => {
            let Some(target) = store::revise_target(db, id).await? else {
                return Err(AppError::Conflict(MSG_NO_WORKSHEET.to_string()));
            };
            if ws_ai::jobs(&state, target).await?.1 {
                return Err(AppError::Conflict(MSG_WORKSHEET_RUNNING.to_string()));
            }
            worksheet_id = Some(target);
        }
        _ => {}
    }
    if turns
        .iter()
        .any(|t| t.status == crate::prep::STATUS_RUNNING)
    {
        return Err(AppError::Conflict(store::MSG_BUSY.to_string()));
    }
    let retry = if kind == KIND_WORKSHEET {
        template_to_retry(&state, teacher.id(), &session).await?
    } else {
        None
    };
    // `None` when another request restarted it meanwhile: the turn waits for that one.
    let restarted = match &retry {
        Some((template_id, _)) => tpl_store::restart(db, *template_id).await?,
        None => None,
    };
    let typed = !request.is_empty();
    let request = request_or_label(kind, request);
    let made = async {
        if let Some(t) = &restarted {
            log_usage(
                db,
                teacher.id(),
                "worksheet_template_retried",
                json!({ "template_id": t.id, "run": t.extract_run, "prep_session_id": id }),
            )
            .await?;
        }
        let turn_id = spawn_turn(
            &state,
            &teacher,
            session,
            kind,
            &request,
            worksheet_id,
            &image_ids,
        )
        .await?;
        let source = memory::Source::PrepTurn {
            session_id: id,
            turn_id,
            entry: None,
        };
        offer_habit(&state, &teacher, source, typed, &request).await;
        // Read the answer before the extraction starts (a fast one could otherwise finish first).
        store::session(&state, id).await
    }
    .await;
    // The template was restarted either way: its extraction always runs.
    if let (Some(t), Some((_, text))) = (restarted, retry) {
        tpl_extract::start(&state, &t, text).await;
    }
    Ok((StatusCode::ACCEPTED, Json(made?)))
}

// ---------------------------------------------------------------- 教學套件 and 課堂活動 downloads

/// The query of both downloads (`format` is DOCX or left out).
#[derive(Debug, Deserialize)]
pub struct KitQuery {
    #[serde(default)]
    pub format: Option<String>,
}

/// `GET /api/prep-sessions/:id/turns/:turn_id/kit?format=docx`: the kit as one DOCX (教學流程,
/// the student version of each level, 教師版). 404 for another teacher's session or a turn of
/// another session, 400 for a turn that is not a kit, 409 while it runs. A kit that failed is
/// exported with the part that succeeded (409 when neither did).
pub async fn kit(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath((id, turn_id)): ApiPath<(i64, i64)>,
    ApiQuery(q): ApiQuery<KitQuery>,
) -> AppResult<Response> {
    let db = &state.db;
    let (session, turn) = export_turn(&state, &teacher, id, turn_id, &q).await?;
    if turn.kind != KIND_KIT {
        return Err(AppError::bad_request(MSG_NOT_KIT));
    }
    if turn.status == STATUS_RUNNING {
        return Err(AppError::Conflict(MSG_KIT_RUNNING.to_string()));
    }
    let plan: Option<ActivityPlan> = turn
        .result
        .clone()
        .and_then(|r| serde_json::from_value(r).ok());
    let worksheet = match turn.worksheet_id {
        Some(ws_id) => match ws_store::owned(db, ws_id, teacher.id()).await {
            Ok(ws) => Some((ws, ws_store::latest(db, ws_id).await?)),
            Err(AppError::NotFound(_)) => None,
            Err(e) => return Err(e),
        },
        None => None,
    };
    // A worksheet without questions (it could not start, or every level failed) is no part.
    let worksheet = worksheet.filter(|(_, v)| v.content.blocks.iter().any(|b| !b.items.is_empty()));
    if plan.is_none() && worksheet.is_none() {
        return Err(AppError::Conflict(MSG_KIT_EMPTY.to_string()));
    }
    let title = session.title.clone();
    let made = {
        let worksheet = worksheet.clone();
        let plan = plan.clone();
        tokio::task::spawn_blocking(move || {
            let ws = worksheet.as_ref().map(|(ws, v)| KitWorksheet {
                title: &ws.title,
                content: &v.content,
                material: Some(&ws.material),
                tiered: v
                    .content
                    .chosen
                    .as_ref()
                    .map_or(ws.settings.output, |c| c.settings.output)
                    == Output::Tiered,
            });
            kit::docx(&title, plan.as_ref(), ws)
        })
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!(e)))??
    };
    let name = format!("{}_教學套件.docx", safe_filename(&session.title));
    log_usage(
        db,
        teacher.id(),
        "prep_kit_exported",
        json!({
            "session_id": id, "turn_id": turn_id,
            "worksheet_id": worksheet.as_ref().map(|(ws, _)| ws.id),
            "version_no": worksheet.as_ref().map(|(_, v)| v.version_no),
            "has_plan": plan.is_some(), "bytes": made.len(),
        }),
    )
    .await?;
    Ok(docx_download(&name, made))
}

/// `GET /api/prep-sessions/:id/turns/:turn_id/activity?format=docx`: a 課堂活動 as one DOCX, the
/// same 教學流程 a 教學套件 starts with ([`kit::activity_docx`]). 404 for another teacher's
/// session or a turn of another session, 400 for a turn that is not a 課堂活動 (a kit's plan
/// comes with its kit), 409 while it runs or when it has no plan (it failed).
pub async fn activity(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath((id, turn_id)): ApiPath<(i64, i64)>,
    ApiQuery(q): ApiQuery<KitQuery>,
) -> AppResult<Response> {
    let (session, turn) = export_turn(&state, &teacher, id, turn_id, &q).await?;
    if turn.kind != KIND_ACTIVITY {
        return Err(AppError::bad_request(MSG_NOT_ACTIVITY));
    }
    if turn.status == STATUS_RUNNING {
        return Err(AppError::Conflict(MSG_ACTIVITY_RUNNING.to_string()));
    }
    let plan: ActivityPlan = turn
        .result
        .and_then(|r| serde_json::from_value(r).ok())
        .ok_or_else(|| AppError::Conflict(MSG_ACTIVITY_EMPTY.to_string()))?;
    let title = session.title.clone();
    let made = tokio::task::spawn_blocking(move || kit::activity_docx(&title, &plan))
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!(e)))??;
    let name = format!("{}_課堂活動.docx", safe_filename(&session.title));
    log_usage(
        &state.db,
        teacher.id(),
        "prep_activity_exported",
        json!({ "session_id": id, "turn_id": turn_id, "bytes": made.len() }),
    )
    .await?;
    Ok(docx_download(&name, made))
}

/// The session and the turn a download asks for, the session brought up to date first: 404 for
/// another teacher's session or a turn of another session, 400 for a format other than DOCX.
async fn export_turn(
    state: &AppState,
    teacher: &AuthTeacher,
    id: i64,
    turn_id: i64,
    q: &KitQuery,
) -> AppResult<(SessionRow, store::TurnRow)> {
    let session = store::owned(&state.db, id, teacher.id()).await?;
    if q.format.as_deref().is_some_and(|f| f.trim() != "docx") {
        return Err(AppError::bad_request(MSG_KIT_FORMAT));
    }
    store::refresh(state, id).await?;
    let turn = store::turns(&state.db, id)
        .await?
        .into_iter()
        .find(|t| t.id == turn_id)
        .ok_or_else(|| AppError::not_found(MSG_TURN_NOT_FOUND))?;
    Ok((session, turn))
}

/// A DOCX as a download named `name`.
fn docx_download(name: &str, made: Vec<u8>) -> Response {
    (
        [
            (CONTENT_TYPE, export::DOCX_TYPE.to_string()),
            (CONTENT_DISPOSITION, export::content_disposition(name)),
        ],
        made,
    )
        .into_response()
}

// ---------------------------------------------------------------- SIOP 自我檢核

/// The turn `turn_id` of the teacher's session `id` (404 for another teacher's session or a
/// turn of another session), the session brought up to date first.
async fn own_turn(
    state: &AppState,
    teacher: &AuthTeacher,
    id: i64,
    turn_id: i64,
) -> AppResult<store::TurnRow> {
    store::owned(&state.db, id, teacher.id()).await?;
    store::refresh(state, id).await?;
    selfcheck::expire_stale(state, id).await?;
    store::turns(&state.db, id)
        .await?
        .into_iter()
        .find(|t| t.id == turn_id)
        .ok_or_else(|| AppError::not_found(MSG_TURN_NOT_FOUND))
}

/// `GET /api/prep-sessions/:id/turns/:turn_id/siop-check`: the turn's latest SIOP 自我檢核
/// (the page polls it while it runs). 404 when the turn has none (or is not the teacher's).
pub async fn siop_check(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath((id, turn_id)): ApiPath<(i64, i64)>,
) -> AppResult<Json<SiopCheck>> {
    let turn = own_turn(&state, &teacher, id, turn_id).await?;
    let mut check = selfcheck::latest(&state.db, turn.id)
        .await?
        .ok_or_else(|| AppError::not_found(selfcheck::MSG_NO_CHECK))?;
    selfcheck::with_queue(&state, &mut check).await?;
    Ok(Json(check))
}

/// `POST /api/prep-sessions/:id/turns/:turn_id/siop-check`: runs the turn's SIOP 自我檢核
/// again (再試一次, or after the worksheet was revised). 202 with the new run (`running`); 400
/// for a kind without a check; 409 while the turn is not `done` or a run of its check runs.
pub async fn rerun_siop_check(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath((id, turn_id)): ApiPath<(i64, i64)>,
) -> AppResult<(StatusCode, Json<SiopCheck>)> {
    let turn = own_turn(&state, &teacher, id, turn_id).await?;
    let mut check = selfcheck::restart(&state, &turn).await?;
    selfcheck::with_queue(&state, &mut check).await?;
    log_usage(
        &state.db,
        teacher.id(),
        "siop_check_rerun",
        json!({ "session_id": id, "turn_id": turn_id, "run_no": check.run_no }),
    )
    .await?;
    Ok((StatusCode::ACCEPTED, Json(check)))
}

/// `POST /api/prep-sessions/:id/turns/:turn_id/undo`: takes back everything a chat turn changed
/// in the worksheet, as one new version (`cause = "undo"`). Owner only (404 for anyone else's
/// session). 409 with a reason when a changed block was edited since, or the turn changed
/// nothing / was already undone. Answers with the updated turn.
pub async fn undo_turn(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath((id, turn_id)): ApiPath<(i64, i64)>,
) -> AppResult<Json<crate::prep::store::Turn>> {
    store::owned(&state.db, id, teacher.id()).await?;
    store::refresh(&state, id).await?;
    crate::prep::chat::undo(&state, teacher.id(), id, turn_id).await?;
    let session = store::session(&state, id).await?;
    session
        .turns
        .into_iter()
        .find(|t| t.id == turn_id)
        .map(Json)
        .ok_or_else(|| AppError::not_found(MSG_TURN_NOT_FOUND))
}
