//! 教材庫 endpoints (docs/API.md「Phase 2.2：教材與註記」, 「Phase 2.9」): the teacher's own
//! materials, their original files, lesson links and annotations. A teacher reaches her own
//! (personal) materials and reads every platform material (教材參考, platform samples); another
//! teacher's personal material is a 404. Platform materials are changed or deleted only by
//! ManageKnowledge (403 for everyone else). Annotations are personal on every material.

use axum::body::Body;
use axum::extract::{Multipart, Query, State};
use axum::http::header::{
    ACCEPT_RANGES, CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE,
    CONTENT_SECURITY_POLICY, CONTENT_TYPE, ETAG, IF_NONE_MATCH, RANGE, VARY,
    X_CONTENT_TYPE_OPTIONS,
};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use axum::Json;
use bytes::{Bytes, BytesMut};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::auth::{AuthTeacher, Capability};
use crate::error::{ApiJson, ApiPath, ApiQuery, AppError, AppResult};
use crate::materials::annotations::{self, AiNote, Annotation, Patch as AnnotationPatch};
use crate::materials::catalog::{self, Placement, PURPOSE_MATERIAL, PURPOSE_SAMPLE};
use crate::materials::ocr;
use crate::materials::store::{self, Deleted, Material, MaterialDetail, Upload, Usage};
use crate::materials::thumbnail;
use crate::materials::{self, file_type_for_name, Kind, FILE_TYPES};
use crate::storage::{ByteRange, StorageError};
use crate::teaching_context::owned_lesson;
use crate::worksheet::template::store as template_store;
use crate::AppState;

// ---------------------------------------------------------------- list, limits

#[derive(Debug, Serialize)]
pub struct FileRule {
    pub ext: &'static str,
    pub kind: &'static str,
    pub max_bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct Limits {
    pub files: Vec<FileRule>,
    pub max_pasted_chars: usize,
    pub max_note_chars: usize,
}

#[derive(Debug, Serialize)]
pub struct Library {
    pub materials: Vec<Material>,
    pub usage: Usage,
    pub limits: Limits,
    /// `"s3"`, `"fs"` or `"none"` (uploads are off; pasted text and links still work).
    pub storage: &'static str,
    pub uploads_enabled: bool,
}

fn limits() -> Limits {
    Limits {
        files: FILE_TYPES
            .iter()
            .map(|t| FileRule {
                ext: t.ext,
                kind: t.kind.as_str(),
                max_bytes: t.kind.max_bytes(),
            })
            .collect(),
        max_pasted_chars: materials::MAX_PASTED_CHARS,
        max_note_chars: annotations::MAX_NOTE_CHARS,
    }
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    /// Only the materials linked to this lesson.
    #[serde(default)]
    pub lesson_id: Option<i64>,
    /// `material` (我的教材, the default) or `worksheet_sample` (the teacher's samples). With
    /// `lesson_id` and no `purpose`, every material linked to the lesson is listed.
    #[serde(default)]
    pub purpose: Option<String>,
    /// With `purpose=worksheet_sample` only: just the samples no template uses (我的範本's
    /// 「沒有範本在用的原檔」).
    #[serde(default)]
    pub orphaned: Option<bool>,
}

pub async fn list(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiQuery(q): ApiQuery<ListQuery>,
) -> AppResult<Json<Library>> {
    teacher.require(Capability::UseMaterials)?;
    if let Some(lesson_id) = q.lesson_id {
        owned_lesson(&state.db, lesson_id, teacher.id()).await?;
    }
    ocr::expire_stale(&state, teacher.id()).await?;
    let purpose = match (q.purpose.as_deref(), q.lesson_id) {
        (Some(p), _) => Some(catalog::parse_purpose(Some(p))?),
        (None, Some(_)) => None,
        (None, None) => Some(PURPOSE_MATERIAL),
    };
    let orphaned = q.orphaned == Some(true);
    if orphaned && purpose != Some(PURPOSE_SAMPLE) {
        return Err(AppError::bad_request(
            "orphaned=true 只能和 purpose=worksheet_sample 一起用。",
        ));
    }
    Ok(Json(Library {
        materials: store::list(&state.db, teacher.id(), q.lesson_id, purpose, orphaned).await?,
        usage: store::usage(&state.db, teacher.id()).await?,
        limits: limits(),
        storage: state
            .storage
            .as_ref()
            .map(|s| s.backend())
            .unwrap_or("none"),
        uploads_enabled: state.storage.is_some(),
    }))
}

pub async fn get_one(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<Json<MaterialDetail>> {
    teacher.require(Capability::UseMaterials)?;
    ocr::expire_stale(&state, teacher.id()).await?;
    let row = store::readable(&state.db, id, teacher.id()).await?;
    Ok(Json(store::detail(&state.db, row, teacher.id()).await?))
}

// ---------------------------------------------------------------- create

#[derive(Debug, Deserialize)]
pub struct CreateMaterial {
    /// `"text"` (pasted) or `"link"`.
    pub kind: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub source_note: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub url: String,
    /// Link the new material to this lesson straight away.
    #[serde(default)]
    pub lesson_id: Option<i64>,
    /// `material` (default) or `worksheet_sample` (a sample of the teacher's worksheet: it goes
    /// to 我的範本 instead of 我的教材).
    #[serde(default)]
    pub purpose: Option<String>,
}

async fn link_new(
    state: &AppState,
    teacher_id: i64,
    material_id: i64,
    lesson_id: Option<i64>,
) -> AppResult<()> {
    if let Some(lesson_id) = lesson_id {
        owned_lesson(&state.db, lesson_id, teacher_id).await?;
        store::link(&state.db, material_id, store::LinkTarget::Lesson(lesson_id)).await?;
    }
    Ok(())
}

pub async fn create(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiJson(req): ApiJson<CreateMaterial>,
) -> AppResult<Json<MaterialDetail>> {
    teacher.require(Capability::UseMaterials)?;
    if let Some(lesson_id) = req.lesson_id {
        owned_lesson(&state.db, lesson_id, teacher.id()).await?;
    }
    let purpose = catalog::parse_purpose(req.purpose.as_deref())?;
    let title = req.title.trim();
    let source_note = req.source_note.trim();
    let row = match req.kind.as_str() {
        "text" => {
            store::create_text_placed(
                &state.db,
                teacher.id(),
                title,
                source_note,
                &req.text,
                &Placement::personal(purpose),
            )
            .await?
        }
        "link" => {
            store::create_link(
                &state.db,
                teacher.id(),
                title,
                source_note,
                req.url.trim(),
                purpose,
            )
            .await?
        }
        _ => {
            return Err(AppError::bad_request(
                "kind 需為 text 或 link；檔案請用上傳。",
            ))
        }
    };
    link_new(&state, teacher.id(), row.id, req.lesson_id).await?;
    Ok(Json(store::detail(&state.db, row, teacher.id()).await?))
}

/// `multipart/form-data` with the fields `file` (required), `title`, `source_note`,
/// `lesson_id` and `purpose` (`material`, the default, or `worksheet_sample`; optional). The
/// file is read into memory while its size is checked against the limit of its kind, so an
/// oversized upload is refused before it is all received.
pub async fn upload(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    mut form: Multipart,
) -> AppResult<Json<serde_json::Value>> {
    teacher.require(Capability::UseMaterials)?;
    if state.storage.is_none() {
        return Err(store::storage_missing());
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
    let mut title = String::new();
    let mut source_note = String::new();
    let mut lesson_id: Option<i64> = None;
    let mut purpose = PURPOSE_MATERIAL;
    let mut file: Option<(String, Bytes)> = None;
    while let Some(mut field) = form.next_field().await.map_err(bad_form)? {
        match field.name().unwrap_or_default() {
            "title" => title = field.text().await.map_err(bad_form)?,
            "purpose" => {
                purpose = catalog::parse_purpose(Some(&field.text().await.map_err(bad_form)?))?
            }
            "source_note" => source_note = field.text().await.map_err(bad_form)?,
            "lesson_id" => {
                let raw = field.text().await.map_err(bad_form)?;
                if !raw.trim().is_empty() {
                    lesson_id = Some(
                        raw.trim()
                            .parse()
                            .map_err(|_| AppError::bad_request("lesson_id 需為數字。"))?,
                    );
                }
            }
            "file" => file = Some(read_file_field(&mut field).await?),
            _ => {}
        }
    }
    let Some((filename, data)) = file else {
        return Err(AppError::bad_request("請選擇要上傳的檔案。"));
    };
    if let Some(lesson_id) = lesson_id {
        owned_lesson(&state.db, lesson_id, teacher.id()).await?;
    }
    let uploaded = store::create_upload(
        &state,
        teacher.id(),
        Upload {
            filename: &filename,
            title: title.trim(),
            source_note: source_note.trim(),
            data,
        },
        &Placement::personal(purpose),
        &store::accept_any,
    )
    .await?;
    let row = uploaded.row;
    link_new(&state, teacher.id(), row.id, lesson_id).await?;
    // Read the answer before OCR starts: a fast job could otherwise finish first.
    let detail = store::detail(&state.db, row.clone(), teacher.id()).await?;
    if !uploaded.reused {
        ocr::spawn_for(&state, &row);
    }
    // The same file she already had (for this purpose): that one again, said so.
    let mut body = serde_json::to_value(&detail)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("material detail: {e}")))?;
    if uploaded.reused {
        body["reused"] = serde_json::Value::Bool(true);
    }
    Ok(Json(body))
}

/// Reads the `file` field of an upload form: its name and bytes. The size is checked against
/// the limit of its kind while it arrives, so an oversized upload is refused before it is all
/// received; an extension that is not on the list is refused at once.
pub async fn read_file_field(
    field: &mut axum::extract::multipart::Field<'_>,
) -> AppResult<(String, Bytes)> {
    let filename = field.file_name().unwrap_or_default().to_string();
    let Some(file_type) = file_type_for_name(&filename) else {
        return Err(AppError::bad_request(format!(
            "不支援這種檔案。可以上傳：{}。",
            materials::allowed_extensions_label()
        )));
    };
    let max = file_type.kind.max_bytes();
    let mut data = BytesMut::new();
    while let Some(chunk) = field
        .chunk()
        .await
        .map_err(|e| AppError::bad_request(format!("上傳的內容讀不出來：{}", e.body_text())))?
    {
        if (data.len() + chunk.len()) as u64 > max {
            return Err(AppError::bad_request(format!(
                "{}檔案最大 {} MB，這個檔案超過了。",
                file_type.kind.label(),
                max / (1024 * 1024)
            )));
        }
        data.extend_from_slice(&chunk);
    }
    Ok((filename, data.freeze()))
}

/// `POST /api/materials/:id/ocr` (「用 OCR 重新讀取」): reads an uploaded PDF or image again
/// with OCR. 202 with the material (`ocr.status = "queued"`); 409 while a run is open; 400 for
/// anything else; 503 when OCR is off.
pub async fn run_ocr(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<(StatusCode, Json<MaterialDetail>)> {
    teacher.require(Capability::UseMaterials)?;
    ocr::expire_stale(&state, teacher.id()).await?;
    // The owner, or ManageKnowledge for a platform material.
    let row = store::writable(&state.db, id, &teacher).await?;
    let row = ocr::start_manual(&state, &row).await?;
    let detail = store::detail(&state.db, row.clone(), teacher.id()).await?;
    ocr::spawn_for(&state, &row);
    Ok((StatusCode::ACCEPTED, Json(detail)))
}

// ---------------------------------------------------------------- change, delete

#[derive(Debug, Deserialize)]
pub struct UpdateMaterial {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub source_note: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}

pub async fn update(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    ApiJson(req): ApiJson<UpdateMaterial>,
) -> AppResult<Json<MaterialDetail>> {
    teacher.require(Capability::UseMaterials)?;
    let row = store::writable(&state.db, id, &teacher).await?;
    let row = store::update(
        &state.db,
        row,
        store::Patch {
            title: req.title.map(|t| t.trim().to_string()),
            source_note: req.source_note.map(|t| t.trim().to_string()),
            text: req.text,
            url: req.url.map(|u| u.trim().to_string()),
        },
    )
    .await?;
    Ok(Json(store::detail(&state.db, row, teacher.id()).await?))
}

pub async fn remove(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<Json<Deleted>> {
    teacher.require(Capability::UseMaterials)?;
    let row = store::writable(&state.db, id, &teacher).await?;
    // A teacher's sample still behind one of her templates goes with (the last of) them; deleting
    // it here would leave the template without its original. A platform row keeps the old rule
    // (its templates keep working without it).
    if !row.is_platform() {
        let (count, titles) = template_store::made_from(&state.db, row.id, teacher.id()).await?;
        if count > 0 {
            return Err(AppError::Conflict(sample_in_use(count, &titles)));
        }
    }
    Ok(Json(store::delete(&state, teacher.id(), row).await?))
}

/// 「這份檔案是範本「L2 學習單版型」的原檔…」: why a sample still in use is not deleted here.
/// `count` templates use it; `titles` are those the teacher may read.
fn sample_in_use(count: i64, titles: &[String]) -> String {
    let named: Vec<String> = titles
        .iter()
        .take(3)
        .map(|t| {
            format!(
                "「{}」",
                if t.trim().is_empty() {
                    "未命名的版型"
                } else {
                    t.trim()
                }
            )
        })
        .collect();
    match (count, named.as_slice()) {
        (1, [one]) => format!(
            "這份檔案是範本{one}的原檔，範本還在用它。要刪除的話，請刪除那個範本，原檔會一起刪除。"
        ),
        (_, []) => "這份檔案還有範本在用，所以不能在這裡刪除。".to_string(),
        _ => {
            let more = if count > named.len() as i64 {
                "等"
            } else {
                ""
            };
            format!(
                "這份檔案是 {count} 個範本（{}{more}）的原檔，範本還在用它。請刪除這些範本，原檔會跟著最後一個範本一起刪除。",
                named.join("、")
            )
        }
    }
}

// ---------------------------------------------------------------- lesson links

pub async fn link_lesson(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath((lesson_id, material_id)): ApiPath<(i64, i64)>,
) -> AppResult<Json<Value>> {
    teacher.require(Capability::UseMaterials)?;
    owned_lesson(&state.db, lesson_id, teacher.id()).await?;
    store::owned(&state.db, material_id, teacher.id()).await?;
    store::link(&state.db, material_id, store::LinkTarget::Lesson(lesson_id)).await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn unlink_lesson(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath((lesson_id, material_id)): ApiPath<(i64, i64)>,
) -> AppResult<Json<Value>> {
    teacher.require(Capability::UseMaterials)?;
    owned_lesson(&state.db, lesson_id, teacher.id()).await?;
    store::owned(&state.db, material_id, teacher.id()).await?;
    let in_sop: bool = sqlx::query_scalar(
        "SELECT COALESCE((SELECT material_id = $2 FROM lesson_sop_versions
                           WHERE lesson_id = $1 ORDER BY version_no DESC LIMIT 1), false)",
    )
    .bind(lesson_id)
    .bind(material_id)
    .fetch_one(&state.db)
    .await?;
    if in_sop {
        return Err(AppError::Conflict(
            "這份教材是這個課例第 1 步的教材。請先在第 1 步換一份教材或移除它。".into(),
        ));
    }
    store::unlink(&state.db, material_id, store::LinkTarget::Lesson(lesson_id)).await?;
    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------- the original file

#[derive(Debug, Serialize)]
pub struct FileLink {
    /// Path on this API, valid until `expires_at`; usable as `src` of img, audio and video.
    pub url: String,
    pub expires_at: DateTime<Utc>,
}

pub async fn file_link(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<Json<FileLink>> {
    teacher.require(Capability::UseMaterials)?;
    let row = store::readable(&state.db, id, teacher.id()).await?;
    if row.storage_key.is_none() {
        return Err(AppError::not_found("這份教材沒有原檔。"));
    }
    let (token, expires_at) = state.links.issue(id, teacher.id());
    Ok(Json(FileLink {
        url: format!("/api/materials/{id}/file?t={token}"),
        expires_at,
    }))
}

#[derive(Debug, Deserialize)]
pub struct FileQuery {
    #[serde(default)]
    pub t: Option<String>,
    /// `1`: send as a download instead of showing it.
    #[serde(default)]
    pub download: Option<String>,
}

/// The requested range lies outside the file (HTTP 416).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unsatisfiable;

/// `Range: bytes=a-b`, `a-` or `-n` for a file of `size` bytes. `Err` when it cannot be
/// satisfied; `Ok(None)` when there is no (usable single) range and the whole file is sent.
pub fn parse_range(header: Option<&str>, size: u64) -> Result<Option<ByteRange>, Unsatisfiable> {
    let Some(spec) = header.and_then(|h| h.trim().strip_prefix("bytes=")) else {
        return Ok(None);
    };
    if spec.contains(',') {
        return Ok(None);
    }
    let Some((a, b)) = spec.split_once('-') else {
        return Err(Unsatisfiable);
    };
    let (a, b) = (a.trim(), b.trim());
    let range = if a.is_empty() {
        let n: u64 = b.parse().map_err(|_| Unsatisfiable)?;
        if n == 0 || size == 0 {
            return Err(Unsatisfiable);
        }
        ByteRange {
            start: size.saturating_sub(n),
            end: size - 1,
        }
    } else {
        let start: u64 = a.parse().map_err(|_| Unsatisfiable)?;
        let end = if b.is_empty() {
            size.saturating_sub(1)
        } else {
            b.parse::<u64>()
                .map_err(|_| Unsatisfiable)?
                .min(size.saturating_sub(1))
        };
        if start >= size || start > end {
            return Err(Unsatisfiable);
        }
        ByteRange { start, end }
    };
    Ok(Some(range))
}

/// `filename*=UTF-8''…` (RFC 5987) plus an ASCII fallback.
fn content_disposition(disposition: &str, filename: &str) -> String {
    let ascii: String = filename
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ' ') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let mut encoded = String::new();
    for b in filename.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_') {
            encoded.push(b as char);
        } else {
            encoded.push_str(&format!("%{b:02X}"));
        }
    }
    format!("{disposition}; filename=\"{ascii}\"; filename*=UTF-8''{encoded}")
}

/// Streams the original file through the API after the ownership check. Authentication is
/// either the usual bearer header or a link from [`file_link`]. Supports `Range` so audio and
/// video can seek. The response can never run as a page of this site: the type comes from the
/// server's list, sniffing is off and a sandboxing CSP is set.
pub async fn file(
    State(state): State<AppState>,
    ApiPath(id): ApiPath<i64>,
    Query(q): Query<FileQuery>,
    headers: HeaderMap,
) -> AppResult<Response> {
    let teacher = match (crate::auth::bearer_token(&headers), q.t.as_deref()) {
        (Some(bearer), _) => crate::auth::authenticate(&state.db, &bearer).await?,
        (None, Some(token)) => {
            let teacher_id = state.links.verify(id, token).ok_or_else(|| {
                AppError::Unauthorized("這個連結已經失效，請重新開啟教材。".into())
            })?;
            crate::auth::teacher_by_id(&state.db, teacher_id).await?
        }
        (None, None) => return Err(AppError::Unauthorized("請先登入。".into())),
    };
    teacher.require(Capability::UseMaterials)?;
    let teacher_id = teacher.id();
    // A link outlives nothing: the material must still exist and still be readable by this
    // teacher (her own, or a platform one).
    let row = store::readable(&state.db, id, teacher_id).await?;
    let (Some(key), Some(storage)) = (row.storage_key.as_deref(), state.storage.as_ref()) else {
        return Err(AppError::not_found("這份教材沒有原檔。"));
    };
    let size = u64::try_from(row.size_bytes).unwrap_or(0);
    let range_header = headers.get(RANGE).and_then(|v| v.to_str().ok());
    let range = match parse_range(range_header, size) {
        Ok(r) => r,
        Err(Unsatisfiable) => {
            return Response::builder()
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(CONTENT_RANGE, format!("bytes */{size}"))
                .body(Body::empty())
                .map_err(|e| AppError::Internal(e.into()));
        }
    };
    let stream = storage.get(key, range).await.map_err(|e| match e {
        StorageError::NotFound => AppError::not_found("原檔已經不在儲存空間裡了。"),
        StorageError::Other(e) => {
            tracing::error!(material_id = id, error = %e, "reading a material object failed");
            AppError::Unavailable("原檔暫時讀不到，請稍後再試。".into())
        }
    })?;

    let kind = row.kind.as_str();
    let inline = q.download.is_none()
        && [Kind::Image, Kind::Audio, Kind::Video]
            .iter()
            .any(|k| k.as_str() == kind);
    let filename = row.filename.clone().unwrap_or_else(|| "material".into());
    let mut response = Response::builder()
        .header(
            CONTENT_TYPE,
            row.content_type
                .as_deref()
                .unwrap_or("application/octet-stream"),
        )
        .header(ACCEPT_RANGES, "bytes")
        .header(X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(CONTENT_SECURITY_POLICY, "default-src 'none'; sandbox")
        .header(CACHE_CONTROL, "private, no-store")
        .header(
            CONTENT_DISPOSITION,
            HeaderValue::from_str(&content_disposition(
                if inline { "inline" } else { "attachment" },
                &filename,
            ))
            .unwrap_or(HeaderValue::from_static("attachment")),
        );
    response = match range {
        Some(r) => response
            .status(StatusCode::PARTIAL_CONTENT)
            .header(CONTENT_LENGTH, r.len())
            .header(CONTENT_RANGE, format!("bytes {}-{}/{size}", r.start, r.end)),
        None => response.status(StatusCode::OK).header(CONTENT_LENGTH, size),
    };
    response
        .body(Body::from_stream(stream))
        .map_err(|e| AppError::Internal(e.into()))
}

// ---------------------------------------------------------------- the preview image

/// How long a browser keeps a preview before asking again (with `If-None-Match`).
const THUMBNAIL_MAX_AGE_SECS: u32 = 3600;

/// Whether `If-None-Match` names this validator (or `*`).
pub(crate) fn etag_matches(header: Option<&str>, etag: &str) -> bool {
    header.is_some_and(|h| {
        h.split(',').any(|t| {
            let t = t.trim();
            t == "*" || t.strip_prefix("W/").unwrap_or(t) == etag
        })
    })
}

/// A small preview of an uploaded PDF (its first page) or image, for the 知識庫 gallery
/// (docs/API.md「知識庫：教材牆與預覽圖」). Read like the original file: the material must be
/// readable by this teacher. Rendered once per file and kept ([`thumbnail::preview`]); the
/// browser keeps it for an hour, then revalidates (`ETag`, 304). `Vary: Authorization`, so a
/// shared computer never shows one account's preview to the next.
pub async fn thumbnail(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    headers: HeaderMap,
) -> AppResult<Response> {
    teacher.require(Capability::UseMaterials)?;
    let row = store::readable(&state.db, id, teacher.id()).await?;
    if !thumbnail::has_preview(&row) {
        return Err(AppError::not_found(thumbnail::MSG_NONE));
    }
    let etag = thumbnail::etag(&row);
    let response = Response::builder()
        .header(
            CACHE_CONTROL,
            format!("private, max-age={THUMBNAIL_MAX_AGE_SECS}"),
        )
        .header(ETAG, &etag)
        .header(VARY, "Authorization");
    let fresh = etag_matches(
        headers.get(IF_NONE_MATCH).and_then(|v| v.to_str().ok()),
        &etag,
    );
    if fresh {
        return response
            .status(StatusCode::NOT_MODIFIED)
            .body(Body::empty())
            .map_err(|e| AppError::Internal(e.into()));
    }
    let bytes = thumbnail::preview(&state, &row).await?;
    response
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, thumbnail::MIME)
        .header(CONTENT_LENGTH, bytes.len())
        .header(X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(CONTENT_SECURITY_POLICY, "default-src 'none'; sandbox")
        .body(Body::from(bytes))
        .map_err(|e| AppError::Internal(e.into()))
}

// ---------------------------------------------------------------- annotations

pub async fn list_annotations(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<Json<Vec<Annotation>>> {
    teacher.require(Capability::UseMaterials)?;
    let row = store::readable(&state.db, id, teacher.id()).await?;
    Ok(Json(
        annotations::list(&state.db, id, teacher.id(), &row.paragraphs.0).await?,
    ))
}

#[derive(Debug, Deserialize)]
pub struct AnchorInput {
    pub paragraph_id: String,
    /// UTF-16 offsets inside the paragraph's text (what a browser selection gives).
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Deserialize)]
pub struct CreateAnnotation {
    #[serde(flatten)]
    pub anchor: AnchorInput,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub share_with_ai: bool,
}

fn check_note(note: &str) -> AppResult<()> {
    if note.chars().count() > annotations::MAX_NOTE_CHARS {
        return Err(AppError::bad_request(format!(
            "註記最多 {} 字。",
            annotations::MAX_NOTE_CHARS
        )));
    }
    Ok(())
}

fn stored_anchor(
    row: &store::MaterialRow,
    input: &AnchorInput,
) -> AppResult<annotations::StoredAnchor> {
    let paragraph = row
        .paragraphs
        .0
        .iter()
        .find(|p| p.id == input.paragraph_id)
        .ok_or_else(|| {
            AppError::bad_request("找不到這個段落；教材可能已經改過，請重新整理後再選一次。")
        })?;
    let anchor = annotations::make_anchor(paragraph, input.start, input.end)
        .ok_or_else(|| AppError::bad_request("選取的範圍不在這個段落裡。"))?;
    if anchor.quote.chars().count() > annotations::MAX_QUOTE_CHARS {
        return Err(AppError::bad_request(format!(
            "一則註記最多標記 {} 字。",
            annotations::MAX_QUOTE_CHARS
        )));
    }
    if anchor.quote.trim().is_empty() {
        return Err(AppError::bad_request("請選取一段文字。"));
    }
    Ok(anchor)
}

pub async fn create_annotation(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    ApiJson(req): ApiJson<CreateAnnotation>,
) -> AppResult<Json<Annotation>> {
    teacher.require(Capability::UseMaterials)?;
    // Platform materials can be annotated too: the notes are the teacher's own.
    let row = store::readable(&state.db, id, teacher.id()).await?;
    let note = req.note.trim();
    check_note(note)?;
    let anchor = stored_anchor(&row, &req.anchor)?;
    let created = annotations::create(
        &state.db,
        annotations::NewAnnotation {
            material_id: id,
            teacher_id: teacher.id(),
            material_revision: row.revision,
            anchor: &anchor,
            note,
            share_with_ai: req.share_with_ai,
        },
        &row.paragraphs.0,
    )
    .await?;
    Ok(Json(created))
}

#[derive(Debug, Deserialize)]
pub struct UpdateAnnotation {
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub share_with_ai: Option<bool>,
    #[serde(default)]
    pub hidden: Option<bool>,
    /// A new position, when the teacher confirms where a note belongs.
    #[serde(default)]
    pub anchor: Option<AnchorInput>,
}

pub async fn update_annotation(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    ApiJson(req): ApiJson<UpdateAnnotation>,
) -> AppResult<Json<Annotation>> {
    teacher.require(Capability::UseMaterials)?;
    let material_id = annotations::owned(&state.db, id, teacher.id()).await?;
    let row = store::readable(&state.db, material_id, teacher.id()).await?;
    let note = req.note.as_deref().map(str::trim);
    if let Some(note) = note {
        check_note(note)?;
    }
    let anchor = match &req.anchor {
        Some(input) => Some(stored_anchor(&row, input)?),
        None => None,
    };
    let updated = annotations::update(
        &state.db,
        id,
        AnnotationPatch {
            note,
            share_with_ai: req.share_with_ai,
            hidden: req.hidden,
            anchor: anchor.as_ref().map(|a| (a, row.revision)),
        },
        &row.paragraphs.0,
    )
    .await?;
    Ok(Json(updated))
}

pub async fn delete_annotation(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<Json<Value>> {
    teacher.require(Capability::UseMaterials)?;
    annotations::owned(&state.db, id, teacher.id()).await?;
    annotations::delete(&state.db, id).await?;
    Ok(Json(json!({ "deleted": true })))
}

#[derive(Debug, Serialize)]
pub struct AiNotes {
    pub material_id: i64,
    pub notes: Vec<AiNote>,
    /// The notes as the block of prompt text every pipeline uses (empty when there are none).
    pub prompt_block: String,
}

/// What an AI pipeline may see of this material's annotations: only the notes the teacher
/// switched to 「提供給 AI」. The same list as `annotations::shared_with_ai`.
pub async fn ai_annotations(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<Json<AiNotes>> {
    teacher.require(Capability::UseMaterials)?;
    store::readable(&state.db, id, teacher.id()).await?;
    let notes = annotations::shared_with_ai(&state.db, teacher.id(), id).await?;
    Ok(Json(AiNotes {
        material_id: id,
        prompt_block: annotations::ai_context_block(&notes),
        notes,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sample_in_use_names_its_templates() {
        let t = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            sample_in_use(1, &t(&["L2 學習單版型"])),
            "這份檔案是範本「L2 學習單版型」的原檔，範本還在用它。要刪除的話，請刪除那個範本，原檔會一起刪除。"
        );
        let two = sample_in_use(2, &t(&["A", " "]));
        assert!(
            two.starts_with("這份檔案是 2 個範本（「A」、「未命名的版型」）的原檔"),
            "{two}"
        );
        let many = sample_in_use(5, &t(&["A", "B", "C", "D"]));
        assert!(many.contains("（「A」、「B」、「C」等）"), "{many}");
        assert_eq!(
            sample_in_use(1, &[]),
            "這份檔案還有範本在用，所以不能在這裡刪除。"
        );
    }

    #[test]
    fn ranges() {
        let r = |h: &str| parse_range(Some(h), 1000);
        assert_eq!(parse_range(None, 1000), Ok(None));
        assert_eq!(r("bytes=0-99"), Ok(Some(ByteRange { start: 0, end: 99 })));
        assert_eq!(
            r("bytes=900-"),
            Ok(Some(ByteRange {
                start: 900,
                end: 999
            }))
        );
        assert_eq!(
            r("bytes=-100"),
            Ok(Some(ByteRange {
                start: 900,
                end: 999
            }))
        );
        assert_eq!(
            r("bytes=990-5000"),
            Ok(Some(ByteRange {
                start: 990,
                end: 999
            }))
        );
        assert_eq!(r("bytes=0-0"), Ok(Some(ByteRange { start: 0, end: 0 })));
        // Several ranges: the whole file is simpler than multipart/byteranges.
        assert_eq!(r("bytes=0-1,5-6"), Ok(None));
        assert_eq!(r("items=0-1"), Ok(None));
        for bad in [
            "bytes=1000-",
            "bytes=5-2",
            "bytes=abc-",
            "bytes=-0",
            "bytes=7",
        ] {
            assert_eq!(r(bad), Err(Unsatisfiable), "{bad}");
        }
        assert_eq!(parse_range(Some("bytes=0-"), 0), Err(Unsatisfiable));
    }

    #[test]
    fn download_names_survive_chinese_and_quotes() {
        let d = content_disposition("attachment", "第三課 閱讀.pdf");
        assert!(d.starts_with("attachment; filename=\"___ __.pdf\""), "{d}");
        assert!(d.contains("filename*=UTF-8''%E7%AC%AC"), "{d}");
        assert!(d.is_ascii());
    }

    #[test]
    fn if_none_match() {
        let tag = "\"ab12-w480\"";
        assert!(etag_matches(Some("\"ab12-w480\""), tag));
        assert!(etag_matches(Some("W/\"ab12-w480\""), tag));
        assert!(etag_matches(Some("\"x\", \"ab12-w480\""), tag));
        assert!(etag_matches(Some("*"), tag));
        assert!(!etag_matches(Some("\"ab12-w360\""), tag));
        assert!(!etag_matches(None, tag));
    }
}
