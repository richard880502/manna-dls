//! Reading, creating, changing and deleting library materials.
//!
//! Unlike plans and SOP documents, materials are not append-only: a pasted text is edited in
//! place and a delete is a real delete (see [`delete`]). That is the point of this table.

use bytes::Bytes;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::types::Json;
use sqlx::{PgPool, Postgres, Transaction};

use super::extract::{self, Extracted, TextStatus};
use super::{
    allowed_extensions_label, clean_filename, content_matches, file_type_for_name, Kind,
    MAX_PASTED_CHARS, TEACHER_MAX_MATERIALS, TEACHER_QUOTA_BYTES,
};
use crate::auth::sha256_hex;
use crate::error::{AppError, AppResult};
use crate::routes::log_usage;
use crate::sop::{
    assign_paragraph_ids, namespace_paragraph_refs, scrub_material_evidence, split_paragraphs,
    word_count, Paragraph, SopContent, DELETED_MATERIAL_MARK,
};
use crate::storage::StorageError;
use crate::AppState;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct MaterialRow {
    pub id: i64,
    pub teacher_id: i64,
    pub kind: String,
    pub origin: String,
    pub title: String,
    pub source_note: String,
    pub url: Option<String>,
    pub filename: Option<String>,
    pub ext: Option<String>,
    pub content_type: Option<String>,
    pub size_bytes: i64,
    pub sha256: Option<String>,
    pub storage_key: Option<String>,
    pub text_status: String,
    pub text: String,
    pub text_truncated: bool,
    pub paragraphs: Json<Vec<Paragraph>>,
    pub paragraph_seq: i32,
    pub page_count: Option<i32>,
    pub revision: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub const COLS: &str = "id, teacher_id, kind, origin, title, source_note, url, filename, ext,
    content_type, size_bytes, sha256, storage_key, text_status, text, text_truncated, paragraphs,
    paragraph_seq, page_count, revision, created_at, updated_at";

/// A lesson that uses a material.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct LessonUse {
    pub lesson_id: i64,
    pub lesson_title: String,
    pub unit_title: String,
}

/// A material without its text (the library list).
#[derive(Debug, Clone, Serialize)]
pub struct Material {
    pub id: i64,
    pub kind: String,
    pub origin: String,
    pub title: String,
    pub source_note: String,
    pub url: Option<String>,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub size_bytes: i64,
    pub sha256: Option<String>,
    /// There is an original file to preview or download.
    pub has_file: bool,
    pub text_status: String,
    pub text_truncated: bool,
    pub page_count: Option<i32>,
    pub paragraph_count: usize,
    pub word_count: usize,
    /// +1 whenever the text changes (pasted text only).
    pub revision: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub lessons: Vec<LessonUse>,
    pub worksheet_count: i64,
    pub annotation_count: i64,
}

/// A material with its text and paragraphs (the viewer).
#[derive(Debug, Clone, Serialize)]
pub struct MaterialDetail {
    #[serde(flatten)]
    pub material: Material,
    pub text: String,
    pub paragraphs: Vec<Paragraph>,
}

impl MaterialRow {
    pub fn summary(
        &self,
        lessons: Vec<LessonUse>,
        worksheet_count: i64,
        annotation_count: i64,
    ) -> Material {
        Material {
            id: self.id,
            kind: self.kind.clone(),
            origin: self.origin.clone(),
            title: self.title.clone(),
            source_note: self.source_note.clone(),
            url: self.url.clone(),
            filename: self.filename.clone(),
            content_type: self.content_type.clone(),
            size_bytes: self.size_bytes,
            sha256: self.sha256.clone(),
            has_file: self.storage_key.is_some(),
            text_status: self.text_status.clone(),
            text_truncated: self.text_truncated,
            page_count: self.page_count,
            paragraph_count: self.paragraphs.0.len(),
            word_count: word_count(&self.text),
            revision: self.revision,
            created_at: self.created_at,
            updated_at: self.updated_at,
            lessons,
            worksheet_count,
            annotation_count,
        }
    }
}

// ---------------------------------------------------------------- reading

/// The material if it exists and belongs to `teacher_id`; otherwise 404 (no existence leak).
pub async fn owned(db: &PgPool, id: i64, teacher_id: i64) -> AppResult<MaterialRow> {
    sqlx::query_as(&format!(
        "SELECT {COLS} FROM materials WHERE id = $1 AND teacher_id = $2"
    ))
    .bind(id)
    .bind(teacher_id)
    .fetch_optional(db)
    .await?
    .ok_or_else(|| AppError::not_found("找不到這份教材。"))
}

async fn lesson_uses(db: &PgPool, material_ids: &[i64]) -> sqlx::Result<Vec<(i64, LessonUse)>> {
    #[derive(sqlx::FromRow)]
    struct Use {
        material_id: i64,
        lesson_id: i64,
        lesson_title: String,
        unit_title: String,
    }
    let rows: Vec<Use> = sqlx::query_as(
        "SELECT k.material_id, l.id AS lesson_id, l.title AS lesson_title, u.title AS unit_title
           FROM material_links k
           JOIN lessons l ON l.id = k.target_id AND l.deleted_at IS NULL
           JOIN units u ON u.id = l.unit_id AND u.deleted_at IS NULL
          WHERE k.target_kind = 'lesson' AND k.material_id = ANY($1)
          ORDER BY l.id",
    )
    .bind(material_ids)
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| {
            (
                r.material_id,
                LessonUse {
                    lesson_id: r.lesson_id,
                    lesson_title: r.lesson_title,
                    unit_title: r.unit_title,
                },
            )
        })
        .collect())
}

async fn counts(db: &PgPool, sql: &str, material_ids: &[i64]) -> sqlx::Result<Vec<(i64, i64)>> {
    sqlx::query_as(sql).bind(material_ids).fetch_all(db).await
}

async fn summaries(db: &PgPool, rows: &[MaterialRow]) -> sqlx::Result<Vec<Material>> {
    let ids: Vec<i64> = rows.iter().map(|r| r.id).collect();
    let uses = lesson_uses(db, &ids).await?;
    let worksheets = counts(
        db,
        "SELECT material_id, COUNT(*) FROM material_links
          WHERE target_kind = 'worksheet' AND material_id = ANY($1) GROUP BY material_id",
        &ids,
    )
    .await?;
    let annotations = counts(
        db,
        "SELECT material_id, COUNT(*) FROM material_annotations
          WHERE material_id = ANY($1) GROUP BY material_id",
        &ids,
    )
    .await?;
    let count_of = |list: &[(i64, i64)], id: i64| {
        list.iter()
            .find(|(m, _)| *m == id)
            .map(|(_, n)| *n)
            .unwrap_or(0)
    };
    Ok(rows
        .iter()
        .map(|row| {
            let lessons = uses
                .iter()
                .filter(|(m, _)| *m == row.id)
                .map(|(_, u)| u.clone())
                .collect();
            row.summary(
                lessons,
                count_of(&worksheets, row.id),
                count_of(&annotations, row.id),
            )
        })
        .collect())
}

pub async fn detail(db: &PgPool, row: MaterialRow) -> sqlx::Result<MaterialDetail> {
    let material = summaries(db, std::slice::from_ref(&row))
        .await?
        .pop()
        .expect("one row in, one summary out");
    Ok(MaterialDetail {
        material,
        text: row.text,
        paragraphs: row.paragraphs.0,
    })
}

/// The teacher's materials, newest first; with `lesson_id`, only those linked to that lesson.
pub async fn list(
    db: &PgPool,
    teacher_id: i64,
    lesson_id: Option<i64>,
) -> sqlx::Result<Vec<Material>> {
    let rows: Vec<MaterialRow> = match lesson_id {
        None => {
            sqlx::query_as(&format!(
                "SELECT {COLS} FROM materials WHERE teacher_id = $1 ORDER BY id DESC"
            ))
            .bind(teacher_id)
            .fetch_all(db)
            .await?
        }
        Some(lesson_id) => {
            sqlx::query_as(&format!(
                "SELECT {COLS} FROM materials m
                  WHERE teacher_id = $1 AND EXISTS (
                        SELECT 1 FROM material_links k
                         WHERE k.material_id = m.id AND k.target_kind = 'lesson' AND k.target_id = $2)
                  ORDER BY id DESC"
            ))
            .bind(teacher_id)
            .bind(lesson_id)
            .fetch_all(db)
            .await?
        }
    };
    summaries(db, &rows).await
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Usage {
    pub used_bytes: i64,
    pub quota_bytes: i64,
    pub count: i64,
    pub max_count: i64,
}

pub async fn usage(db: &PgPool, teacher_id: i64) -> sqlx::Result<Usage> {
    let (used, count): (i64, i64) = sqlx::query_as(
        "SELECT COALESCE(SUM(size_bytes) FILTER (WHERE storage_key IS NOT NULL), 0)::BIGINT,
                COUNT(*)
           FROM materials WHERE teacher_id = $1",
    )
    .bind(teacher_id)
    .fetch_one(db)
    .await?;
    Ok(Usage {
        used_bytes: used,
        quota_bytes: TEACHER_QUOTA_BYTES as i64,
        count,
        max_count: TEACHER_MAX_MATERIALS,
    })
}

async fn check_count(db: &PgPool, teacher_id: i64) -> AppResult<Usage> {
    let usage = usage(db, teacher_id).await?;
    if usage.count >= TEACHER_MAX_MATERIALS {
        return Err(AppError::bad_request(format!(
            "教材庫最多 {TEACHER_MAX_MATERIALS} 份教材。請先刪除不再使用的教材。"
        )));
    }
    Ok(usage)
}

// ---------------------------------------------------------------- creating

pub fn check_meta(title: &str, source_note: &str) -> AppResult<()> {
    if title.chars().count() > super::MAX_TITLE_CHARS {
        return Err(AppError::bad_request(format!(
            "教材標題最多 {} 字。",
            super::MAX_TITLE_CHARS
        )));
    }
    if source_note.chars().count() > super::MAX_SOURCE_NOTE_CHARS {
        return Err(AppError::bad_request(format!(
            "來源說明最多 {} 字。",
            super::MAX_SOURCE_NOTE_CHARS
        )));
    }
    Ok(())
}

pub fn check_pasted(text: &str) -> AppResult<()> {
    if text.chars().count() > MAX_PASTED_CHARS {
        return Err(AppError::bad_request(format!(
            "教材最多 {MAX_PASTED_CHARS} 字。"
        )));
    }
    Ok(())
}

/// Inserts a pasted-text material. `previous` continues another material's paragraph ids
/// (a paragraph with the same text keeps its id), `None` starts at `p1`.
pub async fn insert_text(
    tx: &mut Transaction<'_, Postgres>,
    teacher_id: i64,
    title: &str,
    source_note: &str,
    text: &str,
    previous: Option<(&[Paragraph], i32)>,
) -> sqlx::Result<MaterialRow> {
    let (prev, seq) = previous.unwrap_or((&[], 0));
    let (paragraphs, seq) = assign_paragraph_ids(prev, seq, split_paragraphs(text));
    sqlx::query_as(&format!(
        "INSERT INTO materials (teacher_id, kind, origin, title, source_note, size_bytes, sha256,
                                text_status, text, paragraphs, paragraph_seq)
         VALUES ($1, 'text', 'paste', $2, $3, $4, $5, $6, $7, $8, $9)
         RETURNING {COLS}"
    ))
    .bind(teacher_id)
    .bind(title)
    .bind(source_note)
    .bind(text.len() as i64)
    .bind(sha256_hex(text))
    .bind(if paragraphs.is_empty() {
        TextStatus::NoText.as_str()
    } else {
        TextStatus::Ready.as_str()
    })
    .bind(text)
    .bind(Json(&paragraphs))
    .bind(seq)
    .fetch_one(&mut **tx)
    .await
}

pub async fn create_text(
    db: &PgPool,
    teacher_id: i64,
    title: &str,
    source_note: &str,
    text: &str,
) -> AppResult<MaterialRow> {
    check_meta(title, source_note)?;
    check_pasted(text)?;
    if text.trim().is_empty() {
        return Err(AppError::bad_request("請貼上教材文字。"));
    }
    check_count(db, teacher_id).await?;
    let mut tx = db.begin().await?;
    let row = insert_text(&mut tx, teacher_id, title, source_note, text, None).await?;
    log_usage(
        &mut *tx,
        teacher_id,
        "material_created",
        json!({ "material_id": row.id, "kind": "text", "origin": "paste" }),
    )
    .await?;
    tx.commit().await?;
    Ok(row)
}

/// A link the teacher wants to keep with the lesson. Only the address, a title and a note are
/// stored: nothing is fetched or copied from the site.
pub async fn create_link(
    db: &PgPool,
    teacher_id: i64,
    title: &str,
    source_note: &str,
    url: &str,
) -> AppResult<MaterialRow> {
    check_meta(title, source_note)?;
    if !super::valid_link(url) {
        return Err(AppError::bad_request(
            "網址需以 http:// 或 https:// 開頭，且不能有空白。",
        ));
    }
    check_count(db, teacher_id).await?;
    let mut tx = db.begin().await?;
    let row: MaterialRow = sqlx::query_as(&format!(
        "INSERT INTO materials (teacher_id, kind, origin, title, source_note, url, text_status)
         VALUES ($1, 'link', 'url', $2, $3, $4, 'not_applicable')
         RETURNING {COLS}"
    ))
    .bind(teacher_id)
    .bind(title)
    .bind(source_note)
    .bind(url)
    .fetch_one(&mut *tx)
    .await?;
    log_usage(
        &mut *tx,
        teacher_id,
        "material_created",
        json!({ "material_id": row.id, "kind": "link", "origin": "url" }),
    )
    .await?;
    tx.commit().await?;
    Ok(row)
}

fn megabytes(bytes: u64) -> String {
    let mb = bytes as f64 / (1024.0 * 1024.0);
    if mb >= 10.0 || mb.fract() == 0.0 {
        format!("{mb:.0} MB")
    } else {
        format!("{mb:.1} MB")
    }
}

pub fn storage_missing() -> AppError {
    AppError::Unavailable(
        "這個環境還沒有設定檔案儲存空間，所以現在不能上傳檔案。你仍然可以貼上文字或加入網址。"
            .into(),
    )
}

/// Stores an uploaded file: type and size checks, quota, SHA-256, text extraction, then the
/// object and the row. Nothing is left behind when a step fails.
pub async fn create_upload(
    state: &AppState,
    teacher_id: i64,
    filename: &str,
    title: &str,
    source_note: &str,
    data: Bytes,
) -> AppResult<MaterialRow> {
    let Some(storage) = state.storage.clone() else {
        return Err(storage_missing());
    };
    check_meta(title, source_note)?;
    let filename = clean_filename(filename);
    let Some(file_type) = file_type_for_name(&filename) else {
        return Err(AppError::bad_request(format!(
            "不支援這種檔案。可以上傳：{}。",
            allowed_extensions_label()
        )));
    };
    let size = data.len() as u64;
    if size == 0 {
        return Err(AppError::bad_request("這個檔案是空的。"));
    }
    if size > file_type.kind.max_bytes() {
        return Err(AppError::bad_request(format!(
            "{}檔案最大 {}，這個檔案是 {}。",
            file_type.kind.label(),
            megabytes(file_type.kind.max_bytes()),
            megabytes(size)
        )));
    }
    if !content_matches(file_type, &data) {
        return Err(AppError::bad_request(format!(
            "檔案內容不是 .{} 格式，可能是副檔名被改過或檔案已損壞。",
            file_type.ext
        )));
    }
    let db = &state.db;
    let usage = check_count(db, teacher_id).await?;
    if usage.used_bytes as u64 + size > TEACHER_QUOTA_BYTES {
        return Err(AppError::bad_request(format!(
            "教材庫的空間不夠：上限 {}，已使用 {}，這個檔案 {}。請先刪除不再使用的教材。",
            megabytes(TEACHER_QUOTA_BYTES),
            megabytes(usage.used_bytes as u64),
            megabytes(size)
        )));
    }

    let (sha256, extracted) = {
        let data = data.clone();
        tokio::task::spawn_blocking(move || {
            let sha256 = sha256_hex(&data);
            let extracted = if file_type.kind == Kind::Document {
                extract::extract(file_type, &data)
            } else {
                Extracted {
                    status: TextStatus::NotApplicable,
                    blocks: Vec::new(),
                    page_count: None,
                    truncated: false,
                }
            };
            (sha256, extracted)
        })
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("extraction task failed: {e}")))?
    };
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT title FROM materials WHERE teacher_id = $1 AND sha256 = $2 AND storage_key IS NOT NULL
          LIMIT 1",
    )
    .bind(teacher_id)
    .bind(&sha256)
    .fetch_optional(db)
    .await?;
    if let Some(existing) = existing {
        return Err(AppError::Conflict(format!(
            "這個檔案已經在教材庫裡了（{}）。",
            if existing.trim().is_empty() {
                "未命名教材"
            } else {
                existing.trim()
            }
        )));
    }

    let text = extracted.text();
    let (mut paragraphs, seq) = assign_paragraph_ids(
        &[],
        0,
        extracted.blocks.iter().map(|b| b.text.clone()).collect(),
    );
    for (p, block) in paragraphs.iter_mut().zip(&extracted.blocks) {
        p.page = block.page;
    }
    let title = if title.trim().is_empty() {
        filename
            .rsplit_once('.')
            .map(|(stem, _)| stem.to_string())
            .unwrap_or_else(|| filename.clone())
    } else {
        title.to_string()
    };

    let key = format!(
        "t{teacher_id}/{}.{}",
        crate::auth::new_token(),
        file_type.ext
    );
    storage
        .put(&key, file_type.content_type, data)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "storing an uploaded material failed");
            AppError::Unavailable("檔案沒有存成功（儲存空間暫時無法使用），請稍後再試。".into())
        })?;
    let inserted: Result<MaterialRow, sqlx::Error> = async {
        let mut tx = db.begin().await?;
        let row: MaterialRow = sqlx::query_as(&format!(
            "INSERT INTO materials
               (teacher_id, kind, origin, title, source_note, filename, ext, content_type,
                size_bytes, sha256, storage_key, text_status, text, text_truncated, paragraphs,
                paragraph_seq, page_count)
             VALUES ($1, $2, 'upload', $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)
             RETURNING {COLS}"
        ))
        .bind(teacher_id)
        .bind(file_type.kind.as_str())
        .bind(&title)
        .bind(source_note)
        .bind(&filename)
        .bind(file_type.ext)
        .bind(file_type.content_type)
        .bind(size as i64)
        .bind(&sha256)
        .bind(&key)
        .bind(extracted.status.as_str())
        .bind(&text)
        .bind(extracted.truncated)
        .bind(Json(&paragraphs))
        .bind(seq)
        .bind(extracted.page_count)
        .fetch_one(&mut *tx)
        .await?;
        log_usage(
            &mut *tx,
            teacher_id,
            "material_created",
            json!({ "material_id": row.id, "kind": row.kind, "origin": "upload",
                    "ext": file_type.ext, "size_bytes": size, "text_status": row.text_status }),
        )
        .await?;
        tx.commit().await?;
        Ok(row)
    }
    .await;
    match inserted {
        Ok(row) => Ok(row),
        Err(e) => {
            // The row is what makes the object reachable and deletable: without it, remove it.
            if let Err(cleanup) = storage.delete(&key).await {
                tracing::error!(key, error = %cleanup, "orphaned object after a failed insert");
            }
            Err(e.into())
        }
    }
}

// ---------------------------------------------------------------- changing

/// New text for a pasted material, in place: paragraphs are split again, unchanged ones keep
/// their ids, and the revision goes up. The previous text is not kept anywhere.
pub async fn replace_text(
    tx: &mut Transaction<'_, Postgres>,
    row: &MaterialRow,
    text: &str,
) -> sqlx::Result<MaterialRow> {
    let (paragraphs, seq) =
        assign_paragraph_ids(&row.paragraphs.0, row.paragraph_seq, split_paragraphs(text));
    sqlx::query_as(&format!(
        "UPDATE materials
            SET text = $2, paragraphs = $3, paragraph_seq = $4, revision = revision + 1,
                size_bytes = $5, sha256 = $6, text_status = $7, updated_at = now()
          WHERE id = $1
          RETURNING {COLS}"
    ))
    .bind(row.id)
    .bind(text)
    .bind(Json(&paragraphs))
    .bind(seq)
    .bind(text.len() as i64)
    .bind(sha256_hex(text))
    .bind(if paragraphs.is_empty() {
        TextStatus::NoText.as_str()
    } else {
        TextStatus::Ready.as_str()
    })
    .fetch_one(&mut **tx)
    .await
}

pub async fn set_meta(
    tx: &mut Transaction<'_, Postgres>,
    id: i64,
    title: &str,
    source_note: &str,
) -> sqlx::Result<MaterialRow> {
    sqlx::query_as(&format!(
        "UPDATE materials SET title = $2, source_note = $3, updated_at = now()
          WHERE id = $1 RETURNING {COLS}"
    ))
    .bind(id)
    .bind(title)
    .bind(source_note)
    .fetch_one(&mut **tx)
    .await
}

#[derive(Debug, Default)]
pub struct Patch {
    pub title: Option<String>,
    pub source_note: Option<String>,
    /// Pasted text only.
    pub text: Option<String>,
    /// Link materials only.
    pub url: Option<String>,
}

pub async fn update(db: &PgPool, teacher_id: i64, id: i64, patch: Patch) -> AppResult<MaterialRow> {
    let mut row = owned(db, id, teacher_id).await?;
    let title = patch.title.unwrap_or_else(|| row.title.clone());
    let source_note = patch.source_note.unwrap_or_else(|| row.source_note.clone());
    check_meta(&title, &source_note)?;
    let mut tx = db.begin().await?;
    if title != row.title || source_note != row.source_note {
        row = set_meta(&mut tx, id, &title, &source_note).await?;
    }
    if let Some(text) = patch.text {
        if row.origin != "paste" {
            return Err(AppError::bad_request(
                "上傳的檔案不能改文字；請刪除後重新上傳，或另外貼上文字。",
            ));
        }
        check_pasted(&text)?;
        if text.trim().is_empty() {
            return Err(AppError::bad_request(
                "教材文字不能是空的。不需要這份教材時請刪除它。",
            ));
        }
        if text != row.text {
            row = replace_text(&mut tx, &row, &text).await?;
        }
    }
    if let Some(url) = patch.url {
        if row.origin != "url" {
            return Err(AppError::bad_request("只有網址教材可以改網址。"));
        }
        if !super::valid_link(&url) {
            return Err(AppError::bad_request(
                "網址需以 http:// 或 https:// 開頭，且不能有空白。",
            ));
        }
        row = sqlx::query_as(&format!(
            "UPDATE materials SET url = $2, updated_at = now() WHERE id = $1 RETURNING {COLS}"
        ))
        .bind(id)
        .bind(&url)
        .fetch_one(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(row)
}

// ---------------------------------------------------------------- links

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkTarget {
    Lesson(i64),
    /// For the worksheet generator: it checks that the worksheet is the teacher's.
    Worksheet(i64),
}

impl LinkTarget {
    fn parts(self) -> (&'static str, i64) {
        match self {
            Self::Lesson(id) => ("lesson", id),
            Self::Worksheet(id) => ("worksheet", id),
        }
    }
}

/// Records that `target` uses the material (idempotent). The caller has checked that both
/// belong to the same teacher.
pub async fn link<'e, E: sqlx::PgExecutor<'e>>(
    executor: E,
    material_id: i64,
    target: LinkTarget,
) -> sqlx::Result<()> {
    let (kind, id) = target.parts();
    sqlx::query(
        "INSERT INTO material_links (material_id, target_kind, target_id) VALUES ($1, $2, $3)
         ON CONFLICT (material_id, target_kind, target_id) DO NOTHING",
    )
    .bind(material_id)
    .bind(kind)
    .bind(id)
    .execute(executor)
    .await?;
    Ok(())
}

pub async fn unlink<'e, E: sqlx::PgExecutor<'e>>(
    executor: E,
    material_id: i64,
    target: LinkTarget,
) -> sqlx::Result<()> {
    let (kind, id) = target.parts();
    sqlx::query(
        "DELETE FROM material_links WHERE material_id = $1 AND target_kind = $2 AND target_id = $3",
    )
    .bind(material_id)
    .bind(kind)
    .bind(id)
    .execute(executor)
    .await?;
    Ok(())
}

/// The ids of the materials a worksheet (or lesson) uses, oldest link first.
pub async fn linked_ids(db: &PgPool, target: LinkTarget) -> sqlx::Result<Vec<i64>> {
    let (kind, id) = target.parts();
    sqlx::query_scalar(
        "SELECT material_id FROM material_links WHERE target_kind = $1 AND target_id = $2 ORDER BY id",
    )
    .bind(kind)
    .bind(id)
    .fetch_all(db)
    .await
}

/// Whether anything other than `except` uses the material.
pub async fn used_elsewhere(
    tx: &mut Transaction<'_, Postgres>,
    material_id: i64,
    except: LinkTarget,
) -> sqlx::Result<bool> {
    let (kind, id) = except.parts();
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM material_links
                         WHERE material_id = $1 AND NOT (target_kind = $2 AND target_id = $3))",
    )
    .bind(material_id)
    .bind(kind)
    .bind(id)
    .fetch_one(&mut **tx)
    .await
}

// ---------------------------------------------------------------- deleting

/// Replaces every non-empty string under an `evidence` key. Model outputs of the SOP analysis
/// quote the material verbatim there and nowhere else (an output cut off at `max_tokens` keeps
/// an excerpt that may, under [`crate::ai::RUNAWAY_EXCERPT_KEY`]).
pub fn scrub_evidence(value: &mut Value) -> bool {
    match value {
        Value::Object(map) => {
            let mut changed = false;
            for (key, v) in map.iter_mut() {
                if key == "evidence" || key == crate::ai::RUNAWAY_EXCERPT_KEY {
                    if let Value::String(s) = v {
                        if !s.trim().is_empty() && s != DELETED_MATERIAL_MARK {
                            *s = DELETED_MATERIAL_MARK.to_string();
                            changed = true;
                        }
                        continue;
                    }
                }
                changed |= scrub_evidence(v);
            }
            changed
        }
        Value::Array(list) => list.iter_mut().fold(false, |c, v| scrub_evidence(v) | c),
        _ => false,
    }
}

/// A stored job result names the material it read (「教材：<title>（3 段，120 字）」) next to
/// the quotes: both go.
fn scrub_job_result(result: &mut Value) -> bool {
    let mut changed = scrub_evidence(result);
    if let Some(items) = result.get_mut("context").and_then(Value::as_array_mut) {
        for item in items {
            if item["label"] == "教材" && item["preview"] != DELETED_MATERIAL_MARK {
                item["preview"] = Value::String(DELETED_MATERIAL_MARK.to_string());
                changed = true;
            }
        }
    }
    changed
}

#[derive(Debug, Serialize)]
pub struct Deleted {
    pub deleted: bool,
    /// How many annotations went with it.
    pub annotations: i64,
    /// Lessons whose SOP now shows 「教材已刪除」.
    pub lessons: Vec<i64>,
}

/// Deletes a material for good (docs/BLUEPRINT.md §13.3):
///
/// 1. the original object is removed from the store first. If that fails nothing else happens
///    and the teacher is told, so a file can never be left behind unreachable;
/// 2. in one transaction: the row (with the extracted text and paragraphs), its annotations and
///    links; SOP versions that pointed at it keep `material_deleted` (「教材已刪除」) and lose the
///    verbatim quotes taken from it; the same quotes are removed from the logged AI outputs and
///    job results. The teacher's own analysis, objectives and tasks stay;
/// 3. a row in `material_deletions` records that a material was deleted: no title, no text.
pub async fn delete(state: &AppState, teacher_id: i64, id: i64) -> AppResult<Deleted> {
    let db = &state.db;
    let row = owned(db, id, teacher_id).await?;
    if let Some(key) = &row.storage_key {
        let Some(storage) = state.storage.as_ref() else {
            return Err(AppError::Unavailable(
                "這個環境目前沒有連上檔案儲存空間，無法刪除原檔，所以這份教材還沒有刪除。請聯絡計畫團隊。"
                    .into(),
            ));
        };
        match storage.delete(key).await {
            Ok(()) | Err(StorageError::NotFound) => {}
            Err(e) => {
                tracing::error!(material_id = id, error = %e, "deleting a material object failed");
                return Err(AppError::Unavailable(
                    "原檔沒有刪除成功（儲存空間暫時無法使用），所以這份教材還沒有刪除。請稍後再試一次。".into(),
                ));
            }
        }
    }

    let mut tx = db.begin().await?;
    // Lock the row: a concurrent SOP save must not link it again halfway through.
    let still_there: Option<i64> =
        sqlx::query_scalar("SELECT id FROM materials WHERE id = $1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?;
    if still_there.is_none() {
        return Err(AppError::not_found("找不到這份教材。"));
    }

    // ---- SOP versions: tombstone, no quotes, references marked as belonging to the deleted one
    #[derive(sqlx::FromRow)]
    struct VersionRow {
        id: i64,
        lesson_id: i64,
        material_id: Option<i64>,
        content: Value,
    }
    let versions: Vec<VersionRow> = sqlx::query_as(
        "SELECT id, lesson_id, material_id, content FROM lesson_sop_versions
          WHERE material_id = $1
             OR lesson_id IN (SELECT lesson_id FROM lesson_sop_versions WHERE material_id = $1)
          ORDER BY id",
    )
    .bind(id)
    .fetch_all(&mut *tx)
    .await?;
    let mut lessons: Vec<i64> = Vec::new();
    for v in versions {
        if !lessons.contains(&v.lesson_id) {
            lessons.push(v.lesson_id);
        }
        let current = v.material_id == Some(id);
        let mut content: SopContent = serde_json::from_value(v.content).unwrap_or_default();
        let mut changed = scrub_material_evidence(&mut content, id, current);
        if current {
            let before = content.clone();
            namespace_paragraph_refs(&mut content, id);
            changed |= content != before;
        }
        if changed || current {
            sqlx::query(
                "UPDATE lesson_sop_versions
                    SET content = $2,
                        material_deleted = material_deleted OR $3,
                        material_id = CASE WHEN $3 THEN NULL ELSE material_id END
                  WHERE id = $1",
            )
            .bind(v.id)
            .bind(serde_json::to_value(&content).map_err(anyhow::Error::from)?)
            .bind(current)
            .execute(&mut *tx)
            .await?;
        }
    }

    // ---- logged AI outputs and job results of those lessons: no verbatim quotes
    #[derive(sqlx::FromRow)]
    struct Logged {
        id: i64,
        body: Value,
    }
    let outputs: Vec<Logged> = sqlx::query_as(
        "SELECT id, output AS body FROM ai_interactions
          WHERE lesson_id = ANY($1) AND module = 'sop_analysis' AND output IS NOT NULL
            AND COALESCE(input_snapshot #>> '{sop,material_id}', $2) = $2",
    )
    .bind(&lessons)
    .bind(id.to_string())
    .fetch_all(&mut *tx)
    .await?;
    for mut o in outputs {
        if scrub_evidence(&mut o.body) {
            sqlx::query("UPDATE ai_interactions SET output = $2 WHERE id = $1")
                .bind(o.id)
                .bind(&o.body)
                .execute(&mut *tx)
                .await?;
        }
    }
    let results: Vec<Logged> = sqlx::query_as(
        "SELECT id, result AS body FROM lesson_sop_jobs
          WHERE lesson_id = ANY($1) AND result IS NOT NULL
            AND COALESCE(material_id, $2) = $2",
    )
    .bind(&lessons)
    .bind(id)
    .fetch_all(&mut *tx)
    .await?;
    for mut r in results {
        if scrub_job_result(&mut r.body) {
            sqlx::query("UPDATE lesson_sop_jobs SET result = $2 WHERE id = $1")
                .bind(r.id)
                .bind(&r.body)
                .execute(&mut *tx)
                .await?;
        }
    }
    sqlx::query("UPDATE lesson_sop_jobs SET material_id = NULL WHERE material_id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;

    // ---- the material itself; annotations and links go with it (ON DELETE CASCADE)
    let annotations: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM material_annotations WHERE material_id = $1")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
    sqlx::query("DELETE FROM materials WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO material_deletions (material_id, teacher_id, kind, size_bytes, had_file, annotations)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(id)
    .bind(teacher_id)
    .bind(&row.kind)
    .bind(row.size_bytes)
    .bind(row.storage_key.is_some())
    .bind(annotations as i32)
    .execute(&mut *tx)
    .await?;
    log_usage(
        &mut *tx,
        teacher_id,
        "material_deleted",
        json!({ "material_id": id, "kind": row.kind, "lessons": lessons.len() }),
    )
    .await?;
    tx.commit().await?;
    Ok(Deleted {
        deleted: true,
        annotations,
        lessons,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evidence_is_scrubbed_wherever_it_sits() {
        let mut v = json!({
            "status": "shown",
            "data": {
                "main_idea": "信任",
                "language_focus": [{ "item": "however", "evidence": "However, something followed." }],
                "paragraphs": [
                    { "paragraph_no": 1, "summary": "開場",
                      "features": [{ "feature": "故事", "evidence": "Mrs. Lin put an umbrella stand" },
                                   { "feature": "空的", "evidence": "" }] }
                ]
            }
        });
        assert!(scrub_evidence(&mut v));
        let text = v.to_string();
        assert!(
            !text.contains("umbrella") && !text.contains("However"),
            "{text}"
        );
        assert_eq!(
            v["data"]["paragraphs"][0]["features"][0]["evidence"],
            DELETED_MATERIAL_MARK
        );
        assert_eq!(v["data"]["paragraphs"][0]["features"][1]["evidence"], "");
        assert_eq!(v["data"]["paragraphs"][0]["summary"], "開場");
        assert_eq!(v["data"]["language_focus"][0]["item"], "however");
        assert!(!scrub_evidence(&mut v), "second pass changes nothing");
    }

    #[test]
    fn sizes_read_naturally() {
        assert_eq!(megabytes(20 * 1024 * 1024), "20 MB");
        assert_eq!(megabytes(1024 * 1024 * 3 / 2), "1.5 MB");
        assert_eq!(megabytes(1024 * 1024 * 1024), "1024 MB");
    }
}
