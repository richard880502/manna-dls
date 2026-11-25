//! Reading and appending SOP versions. Versions are append-only: a save is a new row, and a
//! save that changes nothing adds none.
//!
//! The material is not part of this history. It is a material of the teacher's library
//! (`materials`, docs/API.md Phase 2.2) that a version points at: a pasted text is edited in
//! place, and deleting it deletes it everywhere. A version then only remembers that it had one
//! (`material_deleted`, shown as 「教材已刪除」).

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{PgPool, Postgres, Transaction};

use super::{namespace_paragraph_refs, restore_paragraph_refs, Paragraph, SopContent};
use crate::error::{AppError, AppResult};
use crate::materials::store::{self as library, LinkTarget, MaterialRow};

/// The material a SOP version works on: a library material as the SOP steps need it.
#[derive(Debug, Clone, Serialize)]
pub struct Material {
    pub id: i64,
    /// The material's revision (+1 whenever its text changes).
    pub version_no: i32,
    pub kind: String,
    pub origin: String,
    pub title: String,
    pub source_note: String,
    pub url: Option<String>,
    pub filename: Option<String>,
    pub has_file: bool,
    /// `ready` | `no_text` | `failed` | `not_applicable`: only `ready` has paragraphs.
    pub text_status: String,
    pub text_truncated: bool,
    pub page_count: Option<i32>,
    pub text: String,
    pub paragraphs: Vec<Paragraph>,
    pub word_count: usize,
    pub created_at: DateTime<Utc>,
}

impl From<MaterialRow> for Material {
    fn from(r: MaterialRow) -> Self {
        Self {
            id: r.id,
            version_no: r.revision,
            kind: r.kind,
            origin: r.origin,
            title: r.title,
            source_note: r.source_note,
            url: r.url,
            filename: r.filename,
            has_file: r.storage_key.is_some(),
            text_status: r.text_status,
            text_truncated: r.text_truncated,
            page_count: r.page_count,
            word_count: super::word_count(&r.text),
            text: r.text,
            paragraphs: r.paragraphs.0,
            created_at: r.created_at,
        }
    }
}

/// One stored SOP version.
#[derive(Debug, Clone)]
pub struct Version {
    pub id: i64,
    pub version_no: i32,
    pub material_id: Option<i64>,
    /// The material's revision when this version was saved.
    pub material_revision: Option<i32>,
    /// This version had a material that the teacher has since deleted.
    pub material_deleted: bool,
    pub content: SopContent,
    pub created_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct VersionRow {
    id: i64,
    version_no: i32,
    material_id: Option<i64>,
    material_revision: Option<i32>,
    material_deleted: bool,
    content: serde_json::Value,
    created_at: DateTime<Utc>,
}

impl From<VersionRow> for Version {
    fn from(r: VersionRow) -> Self {
        Self {
            id: r.id,
            version_no: r.version_no,
            material_id: r.material_id,
            material_revision: r.material_revision,
            material_deleted: r.material_deleted,
            // Stored documents were validated on the way in; an older shape still reads, with
            // defaults for whatever it lacks.
            content: serde_json::from_value(r.content).unwrap_or_default(),
            created_at: r.created_at,
        }
    }
}

const VERSION_COLS: &str =
    "id, version_no, material_id, material_revision, material_deleted, content, created_at";

pub async fn latest(
    db: impl sqlx::PgExecutor<'_>,
    lesson_id: i64,
) -> sqlx::Result<Option<Version>> {
    let row: Option<VersionRow> = sqlx::query_as(&format!(
        "SELECT {VERSION_COLS} FROM lesson_sop_versions
          WHERE lesson_id = $1 ORDER BY version_no DESC LIMIT 1"
    ))
    .bind(lesson_id)
    .fetch_optional(db)
    .await?;
    Ok(row.map(Version::from))
}

pub async fn version(
    db: &PgPool,
    lesson_id: i64,
    version_no: i32,
) -> sqlx::Result<Option<Version>> {
    let row: Option<VersionRow> = sqlx::query_as(&format!(
        "SELECT {VERSION_COLS} FROM lesson_sop_versions WHERE lesson_id = $1 AND version_no = $2"
    ))
    .bind(lesson_id)
    .bind(version_no)
    .fetch_optional(db)
    .await?;
    Ok(row.map(Version::from))
}

/// A version by its id (a job keeps the version it was asked for).
pub async fn version_by_id(db: &PgPool, id: i64) -> sqlx::Result<Option<Version>> {
    let row: Option<VersionRow> = sqlx::query_as(&format!(
        "SELECT {VERSION_COLS} FROM lesson_sop_versions WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(db)
    .await?;
    Ok(row.map(Version::from))
}

pub async fn versions_count(db: &PgPool, lesson_id: i64) -> sqlx::Result<i64> {
    sqlx::query_scalar("SELECT COUNT(*) FROM lesson_sop_versions WHERE lesson_id = $1")
        .bind(lesson_id)
        .fetch_one(db)
        .await
}

/// The library material a version points at, as it is now (earlier texts are not kept).
pub async fn material(
    db: impl sqlx::PgExecutor<'_>,
    id: Option<i64>,
) -> sqlx::Result<Option<Material>> {
    let Some(id) = id else { return Ok(None) };
    let row: Option<MaterialRow> = sqlx::query_as(&format!(
        "SELECT {} FROM materials WHERE id = $1",
        library::COLS
    ))
    .bind(id)
    .fetch_optional(db)
    .await?;
    Ok(row.map(Material::from))
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct VersionSummary {
    pub version_no: i32,
    pub created_at: DateTime<Utc>,
    /// The material's revision at that save; null when the version had no material.
    pub material_version_no: Option<i32>,
    pub material_deleted: bool,
}

pub async fn list_versions(db: &PgPool, lesson_id: i64) -> sqlx::Result<Vec<VersionSummary>> {
    sqlx::query_as(
        "SELECT version_no, created_at,
                CASE WHEN material_id IS NULL THEN NULL ELSE material_revision END
                  AS material_version_no,
                material_deleted
           FROM lesson_sop_versions
          WHERE lesson_id = $1 ORDER BY version_no DESC",
    )
    .bind(lesson_id)
    .fetch_all(db)
    .await
}

/// Serialises concurrent saves of one lesson, so version numbers stay gapless and unique.
pub(crate) async fn lock_lesson(
    tx: &mut Transaction<'_, Postgres>,
    lesson_id: i64,
) -> sqlx::Result<()> {
    sqlx::query("SELECT id FROM lessons WHERE id = $1 FOR UPDATE")
        .bind(lesson_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn latest_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    lesson_id: i64,
) -> sqlx::Result<Option<Version>> {
    latest(&mut **tx, lesson_id).await
}

async fn material_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    id: i64,
    teacher_id: i64,
) -> sqlx::Result<Option<MaterialRow>> {
    sqlx::query_as(&format!(
        "SELECT {} FROM materials WHERE id = $1 AND teacher_id = $2 AND scope = 'personal'
            FOR UPDATE",
        library::COLS
    ))
    .bind(id)
    .bind(teacher_id)
    .fetch_optional(&mut **tx)
    .await
}

async fn insert_version(
    tx: &mut Transaction<'_, Postgres>,
    lesson_id: i64,
    material: Option<(i64, i32)>,
    content: &SopContent,
) -> AppResult<Version> {
    let json = serde_json::to_value(content).map_err(anyhow::Error::from)?;
    let row: VersionRow = sqlx::query_as(&format!(
        "INSERT INTO lesson_sop_versions (lesson_id, version_no, material_id, material_revision, content)
         VALUES ($1, (SELECT COALESCE(MAX(version_no), 0) + 1 FROM lesson_sop_versions
                       WHERE lesson_id = $1), $2, $3, $4)
         RETURNING {VERSION_COLS}"
    ))
    .bind(lesson_id)
    .bind(material.map(|(id, _)| id))
    .bind(material.map(|(_, revision)| revision))
    .bind(json)
    .fetch_one(&mut **tx)
    .await?;
    Ok(row.into())
}

/// Appends a version with `content` (already validated and normalised), keeping the current
/// material. Returns the latest version unchanged when the content is the same.
pub async fn save_content(db: &PgPool, lesson_id: i64, content: &SopContent) -> AppResult<Version> {
    let mut tx = db.begin().await?;
    let saved = save_content_in_tx(&mut tx, lesson_id, content).await?;
    tx.commit().await?;
    Ok(saved)
}

pub(crate) async fn save_content_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    lesson_id: i64,
    content: &SopContent,
) -> AppResult<Version> {
    lock_lesson(tx, lesson_id).await?;
    let current = latest_in_tx(tx, lesson_id).await?;
    if let Some(v) = &current {
        if v.content == *content {
            return Ok(v.clone());
        }
    }
    let material = current
        .as_ref()
        .and_then(|v| Some((v.material_id?, v.material_revision.unwrap_or(1))));
    let saved = insert_version(tx, lesson_id, material, content).await?;
    Ok(saved)
}

/// The SOP content for a version that points at another material than the current one.
/// Bare paragraph references belong to `from`; they are set aside under its id, and references
/// that were set aside for `to` earlier come back (see `sop::namespace_paragraph_refs`).
fn move_refs(content: &mut SopContent, from: Option<i64>, to: Option<i64>) {
    if from == to {
        return;
    }
    // Without a material there should be no bare references; 0 covers documents written
    // before materials had ids.
    namespace_paragraph_refs(content, from.unwrap_or(0));
    if let Some(to) = to {
        restore_paragraph_refs(content, to);
    }
}

/// The lesson's SOP stops using its material (which stays in the teacher's library). The SOP
/// content is kept; nothing happens when there is no material.
async fn detach(
    tx: &mut Transaction<'_, Postgres>,
    lesson_id: i64,
    current: Option<Version>,
) -> AppResult<Option<Version>> {
    let Some(current) = current else {
        return Ok(None);
    };
    let Some(material_id) = current.material_id else {
        return Ok(Some(current));
    };
    let mut content = current.content;
    move_refs(&mut content, Some(material_id), None);
    library::unlink(&mut **tx, material_id, LinkTarget::Lesson(lesson_id)).await?;
    Ok(Some(insert_version(tx, lesson_id, None, &content).await?))
}

/// Step 1 by pasting. Blank text takes the material off the lesson (it stays in the library;
/// deleting is a separate, explicit action). Otherwise:
///
/// * the lesson's current material is a pasted text that nothing else uses → it is edited in
///   place (paragraphs split again, unchanged ones keep their ids, revision + 1);
/// * otherwise a new pasted material is created. When the lesson has (or last had) a pasted
///   material, the new one continues its paragraph ids, so what the SOP says about unchanged
///   paragraphs stays attached.
///
/// Nothing is written when title, note and text are unchanged. `None`: there was no version and
/// nothing was pasted.
pub async fn save_material(
    db: &PgPool,
    teacher_id: i64,
    lesson_id: i64,
    title: &str,
    source_note: &str,
    text: &str,
) -> AppResult<Option<Version>> {
    let mut tx = db.begin().await?;
    let saved =
        save_material_in_tx(&mut tx, teacher_id, lesson_id, title, source_note, text).await?;
    tx.commit().await?;
    Ok(saved)
}

pub(crate) async fn save_material_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    teacher_id: i64,
    lesson_id: i64,
    title: &str,
    source_note: &str,
    text: &str,
) -> AppResult<Option<Version>> {
    lock_lesson(tx, lesson_id).await?;
    let current = latest_in_tx(tx, lesson_id).await?;
    let target = LinkTarget::Lesson(lesson_id);

    if text.trim().is_empty() {
        let saved = detach(tx, lesson_id, current).await?;
        return Ok(saved);
    }

    let current_id = current.as_ref().and_then(|v| v.material_id);
    let mut content = current
        .as_ref()
        .map(|v| v.content.clone())
        .unwrap_or_default();
    let current_material = match current_id {
        Some(id) => material_in_tx(tx, id, teacher_id).await?,
        None => None,
    };

    // ---- edit in place
    if let Some(m) = current_material.as_ref().filter(|m| m.origin == "paste") {
        if m.title == title && m.source_note == source_note && m.text == text {
            return Ok(current);
        }
        if !library::used_elsewhere(tx, m.id, target).await? {
            let mut row = m.clone();
            if m.title != title || m.source_note != source_note {
                row = library::set_meta(tx, m.id, title, source_note).await?;
            }
            if m.text != text {
                row = library::replace_text(tx, &row, text).await?;
            }
            library::link(&mut **tx, row.id, target).await?;
            let saved =
                insert_version(tx, lesson_id, Some((row.id, row.revision)), &content).await?;
            return Ok(Some(saved));
        }
    }

    // ---- a new material, continuing the paragraph ids of the lesson's pasted material
    let base = match &current_material {
        Some(m) => Some(m.clone()),
        None => {
            let last: Option<i64> = sqlx::query_scalar(
                "SELECT material_id FROM lesson_sop_versions
                  WHERE lesson_id = $1 AND material_id IS NOT NULL
                  ORDER BY version_no DESC LIMIT 1",
            )
            .bind(lesson_id)
            .fetch_optional(&mut **tx)
            .await?;
            match last {
                Some(id) => material_in_tx(tx, id, teacher_id).await?,
                None => None,
            }
        }
    }
    .filter(|m| m.origin == "paste");
    let row = library::insert_text(
        tx,
        teacher_id,
        title,
        source_note,
        text,
        base.as_ref()
            .map(|m| (m.paragraphs.0.as_slice(), m.paragraph_seq)),
    )
    .await?;
    // The new material carries on its base's ids: references to the base stay as they are.
    match &base {
        Some(base) => {
            move_refs(&mut content, current_id, Some(base.id));
        }
        None => move_refs(&mut content, current_id, Some(row.id)),
    }
    if let Some(old) = current_id {
        library::unlink(&mut **tx, old, target).await?;
    }
    library::link(&mut **tx, row.id, target).await?;
    let saved = insert_version(tx, lesson_id, Some((row.id, row.revision)), &content).await?;
    Ok(Some(saved))
}

/// Step 1 by picking a material from the library (`Some`), or taking the material off the
/// lesson (`None`). Someone else's material is a 404.
pub async fn set_material(
    db: &PgPool,
    teacher_id: i64,
    lesson_id: i64,
    material_id: Option<i64>,
) -> AppResult<Option<Version>> {
    let mut tx = db.begin().await?;
    lock_lesson(&mut tx, lesson_id).await?;
    let current = latest_in_tx(&mut tx, lesson_id).await?;
    let Some(material_id) = material_id else {
        let saved = detach(&mut tx, lesson_id, current).await?;
        tx.commit().await?;
        return Ok(saved);
    };
    let row = material_in_tx(&mut tx, material_id, teacher_id)
        .await?
        .ok_or_else(|| AppError::not_found("找不到這份教材。"))?;
    let current_id = current.as_ref().and_then(|v| v.material_id);
    if current_id == Some(row.id) {
        return Ok(current);
    }
    let mut content = current.map(|v| v.content).unwrap_or_default();
    move_refs(&mut content, current_id, Some(row.id));
    let target = LinkTarget::Lesson(lesson_id);
    if let Some(old) = current_id {
        library::unlink(&mut *tx, old, target).await?;
    }
    library::link(&mut *tx, row.id, target).await?;
    let saved = insert_version(&mut tx, lesson_id, Some((row.id, row.revision)), &content).await?;
    tx.commit().await?;
    Ok(Some(saved))
}
