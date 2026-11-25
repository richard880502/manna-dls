//! 教材與範本管理 (docs/API.md「Phase 2.9」): the platform library's materials (教材參考) and
//! templates (平台範本), uploaded and managed by ManageKnowledge (`pi`, `kb_manager`); everyone
//! else gets 403. The uploading admin stays the rows' `teacher_id`, for audit.
//!
//! Uploads go through the same checks and text extraction as a teacher's, into the platform
//! scope: per-teacher quotas do not apply, `PLATFORM_LIBRARY_BYTES` caps all platform files
//! together. A platform template is extracted exactly as `POST /api/worksheet-templates` does.

use axum::extract::{Multipart, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use bytes::Bytes;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::audit::log_usage;
use crate::auth::{AuthTeacher, Capability};
use crate::error::{ApiJson, ApiPath, ApiQuery, AppError, AppResult};
use crate::materials::browse::{
    self, MaterialCard, MaterialQuery, Params, TemplateCard, TemplateQuery,
};
use crate::materials::catalog::{
    self, Placement, PURPOSE_MATERIAL, PURPOSE_SAMPLE, REFERENCE_CATEGORIES, SAMPLE_CATEGORIES,
    SCOPE_PLATFORM,
};
use crate::materials::extract::Extracted;
use crate::materials::store::{self as material_store, Deleted, MaterialRow, Upload};
use crate::materials::Kind;
use crate::routes::library::read_file_field;
use crate::routes::templates::{self, TagsInput};
use crate::worksheet::template::extract::{self as tpl_extract, check_source_text, stale_after};
use crate::worksheet::template::store::{self as tpl_store, NewTemplate};
use crate::worksheet::template::MAX_TITLE_CHARS as MAX_TEMPLATE_TITLE_CHARS;
use crate::AppState;

// ---------------------------------------------------------------- list

/// `GET /api/admin/library?kind=materials|templates&q=&category=` (also `tag`, `file_type`,
/// `lang`, `sort`, `cursor`, `limit` as in the browse APIs): every platform material (教材參考)
/// or every platform template (any status), as the browse answer with `uploaded_by` and
/// `size_bytes` on each card (templates also `error`).
pub async fn list(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiQuery(pairs): ApiQuery<Vec<(String, String)>>,
) -> AppResult<Response> {
    teacher.require(Capability::ManageKnowledge)?;
    let params = Params(pairs);
    match params.get("kind") {
        None | Some("materials") => {
            let query = MaterialQuery::admin(&params)?;
            Ok(Json(browse::materials(&state.db, &query).await?).into_response())
        }
        Some("templates") => {
            let query = TemplateQuery::admin(&params)?;
            tpl_store::expire_stale(
                &state.db,
                teacher.id(),
                stale_after(&state),
                state.config.ai_queue_enabled,
            )
            .await?;
            Ok(Json(browse::templates(&state.db, &query).await?).into_response())
        }
        Some(_) => Err(AppError::bad_request("kind 需為 materials 或 templates。")),
    }
}

// ---------------------------------------------------------------- uploads

/// The fields of an admin upload form.
#[derive(Default)]
struct UploadForm {
    file: Option<(String, Bytes)>,
    text: String,
    title: String,
    category: String,
    tags: String,
    source_note: String,
}

async fn read_form(mut form: Multipart) -> AppResult<UploadForm> {
    let bad_form = |e: axum::extract::multipart::MultipartError| {
        AppError::bad_request(format!("上傳的內容讀不出來：{}", e.body_text()))
    };
    let mut out = UploadForm::default();
    while let Some(mut field) = form.next_field().await.map_err(bad_form)? {
        match field.name().unwrap_or_default() {
            "file" => out.file = Some(read_file_field(&mut field).await?),
            "text" => out.text = field.text().await.map_err(bad_form)?,
            "title" => out.title = field.text().await.map_err(bad_form)?.trim().to_string(),
            "category" => out.category = field.text().await.map_err(bad_form)?,
            "tags" => out.tags = field.text().await.map_err(bad_form)?,
            "source_note" => {
                out.source_note = field.text().await.map_err(bad_form)?.trim().to_string()
            }
            _ => {}
        }
    }
    Ok(out)
}

/// Stores the form's file (or, without one, its pasted text) as a platform material.
async fn create_material(
    state: &AppState,
    admin_id: i64,
    form: UploadForm,
    placement: &Placement,
    accept: &(dyn Fn(Kind, &Extracted) -> AppResult<()> + Sync),
) -> AppResult<MaterialRow> {
    match form.file {
        Some((filename, data)) => material_store::create_upload(
            state,
            admin_id,
            Upload {
                filename: &filename,
                title: &form.title,
                source_note: &form.source_note,
                data,
            },
            placement,
            accept,
        )
        .await
        .map(|u| u.row),
        None if !form.text.trim().is_empty() => {
            material_store::create_text_placed(
                &state.db,
                admin_id,
                &form.title,
                &form.source_note,
                &form.text,
                placement,
            )
            .await
        }
        None => Err(AppError::bad_request("請選擇要上傳的檔案，或貼上文字。")),
    }
}

async fn upload_slot(state: &AppState) -> AppResult<tokio::sync::OwnedSemaphorePermit> {
    state
        .upload_slots
        .clone()
        .acquire_owned()
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("upload slots closed: {e}")))
}

/// `POST /api/admin/library` (multipart): `file` (or `text` with `title`), `title?`,
/// `category` (教材參考 list, required), `tags?` (comma-separated), `source_note?`. A platform
/// material (`purpose = material`); the answer is its card.
pub async fn upload(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    form: Multipart,
) -> AppResult<Json<MaterialCard>> {
    teacher.require(Capability::ManageKnowledge)?;
    let _slot = upload_slot(&state).await?;
    let form = read_form(form).await?;
    let category = catalog::check_category(&REFERENCE_CATEGORIES, &form.category, false)?;
    let tags = catalog::split_tags(&form.tags)?;
    let placement = Placement::platform(PURPOSE_MATERIAL, category, tags);
    let row = create_material(
        &state,
        teacher.id(),
        form,
        &placement,
        &material_store::accept_any,
    )
    .await?;
    // A scanned PDF or a photo is read by OCR in the background (docs/API.md「Phase 2.10」).
    // Read the answer first: a fast job could otherwise finish before it.
    let card = browse::material_card(&state.db, row.id, true).await?;
    crate::materials::ocr::spawn_for(&state, &row);
    Ok(Json(card))
}

/// A sample must read as one worksheet (the rules of `POST /api/worksheet-templates`).
fn accept_sample(_: Kind, extracted: &Extracted) -> AppResult<()> {
    check_source_text(extracted.status.as_str(), &extracted.text())
}

/// `POST /api/admin/library/templates` (multipart): `file` (or `text`), `title?`, `category?`
/// (sample list; blank: suggested after the extraction), `tags?`. A platform sample material
/// (`purpose = worksheet_sample`) and a platform template read from it, extracted as
/// `POST /api/worksheet-templates` does. 202 with the template's card (`status: "extracting"`).
pub async fn upload_template(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    form: Multipart,
) -> AppResult<(StatusCode, Json<TemplateCard>)> {
    teacher.require(Capability::ManageKnowledge)?;
    let db = &state.db;
    let _slot = upload_slot(&state).await?;
    let form = read_form(form).await?;
    let category = catalog::check_category(&SAMPLE_CATEGORIES, &form.category, true)?;
    let tags = catalog::split_tags(&form.tags)?;
    let title = check_template_title(&form.title)?;
    if form.file.is_none() && !form.text.trim().is_empty() {
        check_source_text("ready", &form.text)?;
    }
    let placement = Placement::platform(PURPOSE_SAMPLE, category.clone(), tags.clone());
    let material = create_material(&state, teacher.id(), form, &placement, &accept_sample).await?;
    let title = title.unwrap_or_else(|| tpl_extract::default_title(&material.title));
    let row = tpl_store::insert_placed(
        db,
        NewTemplate {
            teacher_id: teacher.id(),
            title: &title,
            material_id: material.id,
            scope: SCOPE_PLATFORM,
            category: &category,
            tags: &tags,
        },
    )
    .await?;
    // Read the answer before the job starts: a fast extraction could otherwise finish first and
    // the 202 would say "ready" (or "failed") instead of "extracting".
    let card = browse::template_card(db, row.id, true).await?;
    tpl_extract::start(&state, &row, material.text).await;
    log_usage(
        db,
        teacher.id(),
        "worksheet_template_created",
        json!({ "template_id": row.id, "material_id": material.id, "scope": SCOPE_PLATFORM }),
    )
    .await?;
    Ok((StatusCode::ACCEPTED, Json(card)))
}

fn check_template_title(raw: &str) -> AppResult<Option<String>> {
    let title = raw.trim();
    if title.is_empty() {
        return Ok(None);
    }
    if title.chars().count() > MAX_TEMPLATE_TITLE_CHARS {
        return Err(AppError::bad_request(format!(
            "範本名稱最多 {MAX_TEMPLATE_TITLE_CHARS} 字。"
        )));
    }
    Ok(Some(title.to_string()))
}

// ---------------------------------------------------------------- edit, delete, retry

/// `{ title?, category?, tags? }`; `tags` is a list or one comma-separated string.
#[derive(Debug, Deserialize)]
pub struct PatchItem {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub tags: Option<TagsInput>,
}

/// `PATCH /api/admin/library/:id`: a platform material's title, category (its purpose's list)
/// and tags.
pub async fn patch_material(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    ApiJson(req): ApiJson<PatchItem>,
) -> AppResult<Json<MaterialCard>> {
    teacher.require(Capability::ManageKnowledge)?;
    let db = &state.db;
    let row = material_store::platform(db, id).await?;
    let title = match req.title.as_deref().map(str::trim) {
        Some("") => return Err(AppError::bad_request("標題不能是空的。")),
        Some(t) => {
            material_store::check_meta(t, &row.source_note)?;
            t.to_string()
        }
        None => row.title.clone(),
    };
    let category = match req.category.as_deref() {
        Some(c) => catalog::check_category(
            catalog::categories_for(&row.purpose),
            c,
            row.purpose == PURPOSE_SAMPLE,
        )?,
        None => row.category.clone(),
    };
    let tags = match &req.tags {
        Some(t) => t.cleaned()?,
        None => row.tags.clone(),
    };
    material_store::set_catalog(db, id, &title, &category, &tags).await?;
    log_usage(
        db,
        teacher.id(),
        "library_material_updated",
        json!({ "material_id": id }),
    )
    .await?;
    Ok(Json(browse::material_card(db, id, true).await?))
}

/// `DELETE /api/admin/library/:id`: a hard delete, as for a teacher's material (the file, the
/// text and every teacher's notes go). Worksheets made from it keep their snapshot; a template
/// read from it keeps working without its `source_material`.
pub async fn delete_material(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<Json<Deleted>> {
    teacher.require(Capability::ManageKnowledge)?;
    let row = material_store::platform(&state.db, id).await?;
    Ok(Json(
        material_store::delete(&state, teacher.id(), row).await?,
    ))
}

/// `PATCH /api/admin/library/templates/:id`: a platform template's title, category (sample
/// list; blank: suggested from its content when it has one) and tags.
pub async fn patch_template(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    ApiJson(req): ApiJson<PatchItem>,
) -> AppResult<Json<TemplateCard>> {
    teacher.require(Capability::ManageKnowledge)?;
    let db = &state.db;
    let row = tpl_store::platform(db, id).await?;
    let title = match req.title.as_deref().map(str::trim) {
        Some("") => return Err(AppError::bad_request("範本名稱不能是空的。")),
        Some(t) => check_template_title(t)?,
        None => None,
    };
    let category = match req.category.as_deref() {
        Some(c) => {
            let c = catalog::check_category(&SAMPLE_CATEGORIES, c, true)?;
            match (c.is_empty(), tpl_store::latest_version(db, id).await?) {
                (true, Some((_, content))) => {
                    let t = title.as_deref().unwrap_or(&row.title);
                    catalog::suggest_sample_category(&content.worksheet_kind, t).to_string()
                }
                _ => c,
            }
        }
        None => row.category.clone(),
    };
    let tags = match &req.tags {
        Some(t) => t.cleaned()?,
        None => row.tags.clone(),
    };
    if let Some(t) = &title {
        tpl_store::set_title(db, id, t).await?;
    }
    tpl_store::set_catalog(db, id, &category, &tags).await?;
    log_usage(
        db,
        teacher.id(),
        "library_template_updated",
        json!({ "template_id": id }),
    )
    .await?;
    Ok(Json(browse::template_card(db, id, true).await?))
}

/// `DELETE /api/admin/library/templates/:id`: a hard delete of the template (and of its platform
/// sample, unless another template uses it). Worksheets made from it keep their content.
pub async fn delete_template(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<Json<Value>> {
    teacher.require(Capability::ManageKnowledge)?;
    let row = tpl_store::platform(&state.db, id).await?;
    templates::remove(&state, teacher.id(), &row).await?;
    Ok(Json(json!({ "ok": true })))
}

/// `POST /api/admin/library/templates/:id/retry`: another extraction of a failed platform
/// template. 202 with its card (`status: "extracting"`); 409 unless it failed.
pub async fn retry_template(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<(StatusCode, Json<TemplateCard>)> {
    teacher.require(Capability::ManageKnowledge)?;
    let db = &state.db;
    tpl_store::expire_stale(
        db,
        teacher.id(),
        stale_after(&state),
        state.config.ai_queue_enabled,
    )
    .await?;
    let row = tpl_store::platform(db, id).await?;
    let (row, text) = templates::begin_retry(&state, &row).await?;
    // Read the answer before the job starts.
    let card = browse::template_card(db, id, true).await?;
    templates::spawn_retry(&state, teacher.id(), &row, text).await?;
    Ok((StatusCode::ACCEPTED, Json(card)))
}
