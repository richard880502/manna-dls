//! Reading and appending SOP versions and materials. Both tables are append-only: a save is a
//! new row, and a save that changes nothing adds none.

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::types::Json;
use sqlx::{PgPool, Postgres, Transaction};

use super::{assign_paragraph_ids, split_paragraphs, word_count, Paragraph, SopContent};
use crate::error::AppResult;

#[derive(Debug, Clone, Serialize)]
pub struct Material {
    pub id: i64,
    pub version_no: i32,
    pub title: String,
    pub source_note: String,
    pub text: String,
    pub paragraphs: Vec<Paragraph>,
    pub word_count: usize,
    pub created_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct MaterialRow {
    id: i64,
    version_no: i32,
    title: String,
    source_note: String,
    text: String,
    paragraphs: Json<Vec<Paragraph>>,
    paragraph_seq: i32,
    created_at: DateTime<Utc>,
}

impl From<MaterialRow> for Material {
    fn from(r: MaterialRow) -> Self {
        Self {
            id: r.id,
            version_no: r.version_no,
            title: r.title,
            source_note: r.source_note,
            word_count: word_count(&r.text),
            text: r.text,
            paragraphs: r.paragraphs.0,
            created_at: r.created_at,
        }
    }
}

const MATERIAL_COLS: &str =
    "id, version_no, title, source_note, text, paragraphs, paragraph_seq, created_at";

/// One stored SOP version.
#[derive(Debug, Clone)]
pub struct Version {
    pub id: i64,
    pub version_no: i32,
    pub material_id: Option<i64>,
    pub content: SopContent,
    pub created_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct VersionRow {
    id: i64,
    version_no: i32,
    material_id: Option<i64>,
    content: serde_json::Value,
    created_at: DateTime<Utc>,
}

impl From<VersionRow> for Version {
    fn from(r: VersionRow) -> Self {
        Self {
            id: r.id,
            version_no: r.version_no,
            material_id: r.material_id,
            // Stored documents were validated on the way in; an older shape still reads, with
            // defaults for whatever it lacks.
            content: serde_json::from_value(r.content).unwrap_or_default(),
            created_at: r.created_at,
        }
    }
}

const VERSION_COLS: &str = "id, version_no, material_id, content, created_at";

pub async fn latest(db: &PgPool, lesson_id: i64) -> sqlx::Result<Option<Version>> {
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

pub async fn versions_count(db: &PgPool, lesson_id: i64) -> sqlx::Result<i64> {
    sqlx::query_scalar("SELECT COUNT(*) FROM lesson_sop_versions WHERE lesson_id = $1")
        .bind(lesson_id)
        .fetch_one(db)
        .await
}

pub async fn material(db: &PgPool, id: Option<i64>) -> sqlx::Result<Option<Material>> {
    let Some(id) = id else { return Ok(None) };
    let row: Option<MaterialRow> = sqlx::query_as(&format!(
        "SELECT {MATERIAL_COLS} FROM lesson_sop_materials WHERE id = $1"
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
    pub material_version_no: Option<i32>,
}

pub async fn list_versions(db: &PgPool, lesson_id: i64) -> sqlx::Result<Vec<VersionSummary>> {
    sqlx::query_as(
        "SELECT v.version_no, v.created_at, m.version_no AS material_version_no
           FROM lesson_sop_versions v LEFT JOIN lesson_sop_materials m ON m.id = v.material_id
          WHERE v.lesson_id = $1 ORDER BY v.version_no DESC",
    )
    .bind(lesson_id)
    .fetch_all(db)
    .await
}

/// Serialises concurrent saves of one lesson, so version numbers stay gapless and unique.
async fn lock_lesson(tx: &mut Transaction<'_, Postgres>, lesson_id: i64) -> sqlx::Result<()> {
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
    let row: Option<VersionRow> = sqlx::query_as(&format!(
        "SELECT {VERSION_COLS} FROM lesson_sop_versions
          WHERE lesson_id = $1 ORDER BY version_no DESC LIMIT 1"
    ))
    .bind(lesson_id)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(row.map(Version::from))
}

async fn insert_version(
    tx: &mut Transaction<'_, Postgres>,
    lesson_id: i64,
    material_id: Option<i64>,
    content: &SopContent,
) -> AppResult<Version> {
    let json = serde_json::to_value(content).map_err(anyhow::Error::from)?;
    let row: VersionRow = sqlx::query_as(&format!(
        "INSERT INTO lesson_sop_versions (lesson_id, version_no, material_id, content)
         VALUES ($1, (SELECT COALESCE(MAX(version_no), 0) + 1 FROM lesson_sop_versions
                       WHERE lesson_id = $1), $2, $3)
         RETURNING {VERSION_COLS}"
    ))
    .bind(lesson_id)
    .bind(material_id)
    .bind(json)
    .fetch_one(&mut **tx)
    .await?;
    Ok(row.into())
}

/// Appends a version with `content` (already validated and normalised), keeping the current
/// material. Returns the latest version unchanged when the content is the same.
pub async fn save_content(db: &PgPool, lesson_id: i64, content: &SopContent) -> AppResult<Version> {
    let mut tx = db.begin().await?;
    lock_lesson(&mut tx, lesson_id).await?;
    let current = latest_in_tx(&mut tx, lesson_id).await?;
    if let Some(v) = &current {
        if v.content == *content {
            return Ok(v.clone());
        }
    }
    let material_id = current.and_then(|v| v.material_id);
    let saved = insert_version(&mut tx, lesson_id, material_id, content).await?;
    tx.commit().await?;
    Ok(saved)
}

/// Stores the pasted material (splitting it into paragraphs with stable ids) and appends a
/// version pointing at it; blank text removes the material from the SOP. The SOP content is
/// carried over untouched. Nothing is written when title, note and text are unchanged.
pub async fn save_material(
    db: &PgPool,
    lesson_id: i64,
    title: &str,
    source_note: &str,
    text: &str,
) -> AppResult<Version> {
    let mut tx = db.begin().await?;
    lock_lesson(&mut tx, lesson_id).await?;
    let current = latest_in_tx(&mut tx, lesson_id).await?;
    let content = current
        .as_ref()
        .map(|v| v.content.clone())
        .unwrap_or_default();
    let current_material_id = current.as_ref().and_then(|v| v.material_id);

    if text.trim().is_empty() {
        // Nothing to remove.
        if let Some(v) = current.filter(|v| v.material_id.is_none()) {
            return Ok(v);
        }
        let saved = insert_version(&mut tx, lesson_id, None, &content).await?;
        tx.commit().await?;
        return Ok(saved);
    }

    // Ids continue from the lesson's most recent material, also after a removal.
    let previous: Option<MaterialRow> = sqlx::query_as(&format!(
        "SELECT {MATERIAL_COLS} FROM lesson_sop_materials
          WHERE lesson_id = $1 ORDER BY version_no DESC LIMIT 1"
    ))
    .bind(lesson_id)
    .fetch_optional(&mut *tx)
    .await?;
    if let (Some(prev), Some(v)) = (&previous, &current) {
        if Some(prev.id) == current_material_id
            && prev.title == title
            && prev.source_note == source_note
            && prev.text == text
        {
            return Ok(v.clone());
        }
    }
    let (prev_paragraphs, prev_seq) = previous
        .as_ref()
        .map(|p| (p.paragraphs.0.clone(), p.paragraph_seq))
        .unwrap_or_default();
    let (paragraphs, seq) =
        assign_paragraph_ids(&prev_paragraphs, prev_seq, split_paragraphs(text));
    let material_id: i64 = sqlx::query_scalar(
        "INSERT INTO lesson_sop_materials
           (lesson_id, version_no, title, source_note, text, paragraphs, paragraph_seq)
         VALUES ($1, (SELECT COALESCE(MAX(version_no), 0) + 1 FROM lesson_sop_materials
                       WHERE lesson_id = $1), $2, $3, $4, $5, $6)
         RETURNING id",
    )
    .bind(lesson_id)
    .bind(title)
    .bind(source_note)
    .bind(text)
    .bind(Json(&paragraphs))
    .bind(seq)
    .fetch_one(&mut *tx)
    .await?;
    let saved = insert_version(&mut tx, lesson_id, Some(material_id), &content).await?;
    tx.commit().await?;
    Ok(saved)
}
