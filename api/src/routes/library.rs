//! 教材庫 endpoints (docs/API.md「Phase 2.2：教材與注記」): the teacher's own materials, their
//! original files, lesson links and annotations. Every endpoint only reaches the signed-in
//! teacher's materials; anyone else's is a 404.

use axum::body::Body;
use axum::extract::{Multipart, Query, State};
use axum::http::header::{
    ACCEPT_RANGES, CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE,
    CONTENT_SECURITY_POLICY, CONTENT_TYPE, RANGE, X_CONTENT_TYPE_OPTIONS,
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
use crate::materials::store::{self, Deleted, Material, MaterialDetail, Usage};
use crate::materials::{self, file_type_for_name, Kind, FILE_TYPES};
use crate::routes::owned_lesson;
use crate::storage::{ByteRange, StorageError};
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
    Ok(Json(Library {
        materials: store::list(&state.db, teacher.id(), q.lesson_id).await?,
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
    let row = store::owned(&state.db, id, teacher.id()).await?;
    Ok(Json(store::detail(&state.db, row).await?))
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
    let title = req.title.trim();
    let source_note = req.source_note.trim();
    let row = match req.kind.as_str() {
        "text" => {
            store::create_text(&state.db, teacher.id(), title, source_note, &req.text).await?
        }
        "link" => {
            store::create_link(&state.db, teacher.id(), title, source_note, req.url.trim()).await?
        }
        _ => {
            return Err(AppError::bad_request(
                "kind 需為 text 或 link；檔案請用上傳。",
            ))
        }
    };
    link_new(&state, teacher.id(), row.id, req.lesson_id).await?;
    Ok(Json(store::detail(&state.db, row).await?))
}

/// `multipart/form-data` with the fields `file` (required), `title`, `source_note` and
/// `lesson_id` (optional). The file is read into memory while its size is checked against the
/// limit of its kind, so an oversized upload is refused before it is all received.
pub async fn upload(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    mut form: Multipart,
) -> AppResult<Json<MaterialDetail>> {
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
    let mut file: Option<(String, Bytes)> = None;
    while let Some(mut field) = form.next_field().await.map_err(bad_form)? {
        match field.name().unwrap_or_default() {
            "title" => title = field.text().await.map_err(bad_form)?,
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
            "file" => {
                let filename = field.file_name().unwrap_or_default().to_string();
                let Some(file_type) = file_type_for_name(&filename) else {
                    return Err(AppError::bad_request(format!(
                        "不支援這種檔案。可以上傳：{}。",
                        materials::allowed_extensions_label()
                    )));
                };
                let max = file_type.kind.max_bytes();
                let mut data = BytesMut::new();
                while let Some(chunk) = field.chunk().await.map_err(bad_form)? {
                    if (data.len() + chunk.len()) as u64 > max {
                        return Err(AppError::bad_request(format!(
                            "{}檔案最大 {} MB，這個檔案超過了。",
                            file_type.kind.label(),
                            max / (1024 * 1024)
                        )));
                    }
                    data.extend_from_slice(&chunk);
                }
                file = Some((filename, data.freeze()));
            }
            _ => {}
        }
    }
    let Some((filename, data)) = file else {
        return Err(AppError::bad_request("請選擇要上傳的檔案。"));
    };
    if let Some(lesson_id) = lesson_id {
        owned_lesson(&state.db, lesson_id, teacher.id()).await?;
    }
    let row = store::create_upload(
        &state,
        teacher.id(),
        &filename,
        title.trim(),
        source_note.trim(),
        data,
    )
    .await?;
    link_new(&state, teacher.id(), row.id, lesson_id).await?;
    Ok(Json(store::detail(&state.db, row).await?))
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
    let row = store::update(
        &state.db,
        teacher.id(),
        id,
        store::Patch {
            title: req.title.map(|t| t.trim().to_string()),
            source_note: req.source_note.map(|t| t.trim().to_string()),
            text: req.text,
            url: req.url.map(|u| u.trim().to_string()),
        },
    )
    .await?;
    Ok(Json(store::detail(&state.db, row).await?))
}

pub async fn remove(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<Json<Deleted>> {
    teacher.require(Capability::UseMaterials)?;
    Ok(Json(store::delete(&state, teacher.id(), id).await?))
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
    let row = store::owned(&state.db, id, teacher.id()).await?;
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
    // A link outlives nothing: the material must still exist and still be this teacher's.
    let row = store::owned(&state.db, id, teacher_id).await?;
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

// ---------------------------------------------------------------- annotations

pub async fn list_annotations(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<Json<Vec<Annotation>>> {
    teacher.require(Capability::UseMaterials)?;
    let row = store::owned(&state.db, id, teacher.id()).await?;
    Ok(Json(
        annotations::list(&state.db, id, &row.paragraphs.0).await?,
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
            "注記最多 {} 字。",
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
            "一則注記最多標記 {} 字。",
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
    let row = store::owned(&state.db, id, teacher.id()).await?;
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
    let row = store::owned(&state.db, material_id, teacher.id()).await?;
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
    store::owned(&state.db, id, teacher.id()).await?;
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
}
