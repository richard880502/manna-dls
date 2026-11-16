//! Reading and writing worksheets. Versions are append-only: a save is a new row, and a save
//! that changes nothing adds none. Version numbers and dates are the server's.

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::types::Json;
use sqlx::{PgPool, Postgres, Transaction};

use super::{Block, Brief, Content, Level, Material, Settings};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone)]
pub struct Worksheet {
    pub id: i64,
    pub teacher_id: i64,
    pub lesson_id: Option<i64>,
    pub class_id: Option<i64>,
    pub title: String,
    pub filename: String,
    pub material: Material,
    pub paragraph_seq: i32,
    pub settings: Settings,
    pub suggested: Settings,
    pub brief: Brief,
    pub source_sop_version_id: Option<i64>,
    pub generated: bool,
    /// The teacher's template this worksheet follows, and the version it uses.
    pub template_id: Option<i64>,
    pub template_version_no: Option<i32>,
    /// The teacher's request (「教師的要求」, ≤2000), sent to every model call; "" when none.
    pub request: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct WorksheetRow {
    id: i64,
    teacher_id: i64,
    lesson_id: Option<i64>,
    class_id: Option<i64>,
    title: String,
    filename: String,
    material: serde_json::Value,
    paragraph_seq: i32,
    settings: serde_json::Value,
    suggested: serde_json::Value,
    brief: serde_json::Value,
    source_sop_version_id: Option<i64>,
    generated: bool,
    template_id: Option<i64>,
    template_version_no: Option<i32>,
    request: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<WorksheetRow> for Worksheet {
    fn from(r: WorksheetRow) -> Self {
        // Stored documents were validated on the way in; an older shape still reads, with
        // defaults for whatever it lacks.
        Self {
            id: r.id,
            teacher_id: r.teacher_id,
            lesson_id: r.lesson_id,
            class_id: r.class_id,
            title: r.title,
            filename: r.filename,
            material: serde_json::from_value(r.material).unwrap_or_default(),
            paragraph_seq: r.paragraph_seq,
            settings: serde_json::from_value(r.settings).unwrap_or_default(),
            suggested: serde_json::from_value(r.suggested).unwrap_or_default(),
            brief: serde_json::from_value(r.brief).unwrap_or_default(),
            source_sop_version_id: r.source_sop_version_id,
            generated: r.generated,
            template_id: r.template_id,
            template_version_no: r.template_version_no,
            request: r.request,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

const WORKSHEET_COLS: &str = "id, teacher_id, lesson_id, class_id, title, filename, material,
    paragraph_seq, settings, suggested, brief, source_sop_version_id, generated, template_id,
    template_version_no, request, created_at, updated_at";

/// The worksheet if it exists and belongs to `teacher_id`; otherwise 404 (no existence leak).
pub async fn owned(db: &PgPool, id: i64, teacher_id: i64) -> AppResult<Worksheet> {
    let row: Option<WorksheetRow> = sqlx::query_as(&format!(
        "SELECT {WORKSHEET_COLS} FROM worksheets
          WHERE id = $1 AND teacher_id = $2 AND deleted_at IS NULL"
    ))
    .bind(id)
    .bind(teacher_id)
    .fetch_optional(db)
    .await?;
    row.map(Worksheet::from)
        .ok_or_else(|| AppError::not_found("找不到這份學習單。"))
}

/// For background work: the row by id, deleted or not.
pub async fn by_id(db: &PgPool, id: i64) -> sqlx::Result<Option<Worksheet>> {
    let row: Option<WorksheetRow> = sqlx::query_as(&format!(
        "SELECT {WORKSHEET_COLS} FROM worksheets WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(db)
    .await?;
    Ok(row.map(Worksheet::from))
}

#[derive(Debug, Clone, Serialize)]
pub struct LessonInfo {
    pub id: i64,
    pub title: String,
    pub unit_id: i64,
    pub unit_title: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub id: i64,
    pub title: String,
    pub filename: String,
    pub lesson: Option<LessonInfo>,
    pub class_id: Option<i64>,
    /// The 備課對話 that made this worksheet (「回到備課對話」); cleared when it is deleted.
    pub prep_session_id: Option<i64>,
    pub generated: bool,
    pub levels: Vec<Level>,
    pub version_no: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct SummaryRow {
    id: i64,
    title: String,
    filename: String,
    lesson_id: Option<i64>,
    lesson_title: Option<String>,
    unit_id: Option<i64>,
    unit_title: Option<String>,
    class_id: Option<i64>,
    prep_session_id: Option<i64>,
    generated: bool,
    levels: Option<serde_json::Value>,
    version_no: i32,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<SummaryRow> for Summary {
    fn from(r: SummaryRow) -> Self {
        let lesson = match (r.lesson_id, r.lesson_title, r.unit_id, r.unit_title) {
            (Some(id), Some(title), Some(unit_id), Some(unit_title)) => Some(LessonInfo {
                id,
                title,
                unit_id,
                unit_title,
            }),
            _ => None,
        };
        Self {
            id: r.id,
            title: r.title,
            filename: r.filename,
            lesson,
            class_id: r.class_id,
            prep_session_id: r.prep_session_id,
            generated: r.generated,
            levels: r
                .levels
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_default(),
            version_no: r.version_no,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

const SUMMARY_SELECT: &str = "SELECT w.id, w.title, w.filename, l.id AS lesson_id,
        l.title AS lesson_title, l.unit_id, u.title AS unit_title, w.class_id, w.prep_session_id,
        w.generated,
        v.content -> 'levels' AS levels, v.version_no, w.created_at, w.updated_at
   FROM worksheets w
   LEFT JOIN lessons l ON l.id = w.lesson_id AND l.deleted_at IS NULL
   LEFT JOIN units u ON u.id = l.unit_id
   JOIN LATERAL (SELECT version_no, content FROM worksheet_versions
                  WHERE worksheet_id = w.id ORDER BY version_no DESC LIMIT 1) v ON TRUE";

pub async fn list(
    db: &PgPool,
    teacher_id: i64,
    lesson_id: Option<i64>,
) -> sqlx::Result<Vec<Summary>> {
    let rows: Vec<SummaryRow> = sqlx::query_as(&format!(
        "{SUMMARY_SELECT}
          WHERE w.teacher_id = $1 AND w.deleted_at IS NULL
            AND ($2::BIGINT IS NULL OR w.lesson_id = $2)
          ORDER BY w.updated_at DESC, w.id DESC"
    ))
    .bind(teacher_id)
    .bind(lesson_id)
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(Summary::from).collect())
}

pub async fn summary(db: &PgPool, id: i64) -> sqlx::Result<Summary> {
    let row: SummaryRow = sqlx::query_as(&format!("{SUMMARY_SELECT} WHERE w.id = $1"))
        .bind(id)
        .fetch_one(db)
        .await?;
    Ok(row.into())
}

pub struct NewWorksheet<'a> {
    pub teacher_id: i64,
    pub lesson_id: Option<i64>,
    pub class_id: Option<i64>,
    pub title: &'a str,
    pub filename: &'a str,
    pub material: &'a Material,
    pub paragraph_seq: i32,
    pub settings: &'a Settings,
    pub brief: &'a Brief,
    pub source_sop_version_id: Option<i64>,
    pub content: &'a Content,
    /// (template id, version number)
    pub template: Option<(i64, i32)>,
    /// The 備課對話 the worksheet is made in.
    pub prep_session_id: Option<i64>,
    /// 「教師的要求」 ("" when none).
    pub request: &'a str,
}

/// Creates the worksheet with its first version. `settings` starts as the suggestion.
pub async fn insert(db: &PgPool, new: NewWorksheet<'_>) -> AppResult<i64> {
    let mut tx = db.begin().await?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO worksheets (teacher_id, lesson_id, class_id, title, filename, material,
                                 paragraph_seq, settings, suggested, brief, source_sop_version_id,
                                 template_id, template_version_no, prep_session_id, request)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $8, $9, $10, $11, $12, $13, $14) RETURNING id",
    )
    .bind(new.teacher_id)
    .bind(new.lesson_id)
    .bind(new.class_id)
    .bind(new.title)
    .bind(new.filename)
    .bind(Json(new.material))
    .bind(new.paragraph_seq)
    .bind(Json(new.settings))
    .bind(Json(new.brief))
    .bind(new.source_sop_version_id)
    .bind(new.template.map(|(id, _)| id))
    .bind(new.template.map(|(_, no)| no))
    .bind(new.prep_session_id)
    .bind(new.request)
    .fetch_one(&mut *tx)
    .await?;
    insert_version(&mut tx, id, new.content, "created").await?;
    tx.commit().await?;
    Ok(id)
}

// ---------------------------------------------------------------- versions

#[derive(Debug, Clone)]
pub struct Version {
    pub id: i64,
    pub version_no: i32,
    pub content: Content,
    pub cause: String,
    pub created_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct VersionRow {
    id: i64,
    version_no: i32,
    content: serde_json::Value,
    cause: String,
    created_at: DateTime<Utc>,
}

impl From<VersionRow> for Version {
    fn from(r: VersionRow) -> Self {
        Self {
            id: r.id,
            version_no: r.version_no,
            content: serde_json::from_value(r.content).unwrap_or_default(),
            cause: r.cause,
            created_at: r.created_at,
        }
    }
}

const VERSION_COLS: &str = "id, version_no, content, cause, created_at";

pub async fn latest(db: &PgPool, worksheet_id: i64) -> AppResult<Version> {
    let row: Option<VersionRow> = sqlx::query_as(&format!(
        "SELECT {VERSION_COLS} FROM worksheet_versions
          WHERE worksheet_id = $1 ORDER BY version_no DESC LIMIT 1"
    ))
    .bind(worksheet_id)
    .fetch_optional(db)
    .await?;
    row.map(Version::from).ok_or_else(|| {
        AppError::Internal(anyhow::anyhow!("worksheet {worksheet_id} has no version"))
    })
}

pub async fn versions_count(db: &PgPool, worksheet_id: i64) -> sqlx::Result<i64> {
    sqlx::query_scalar("SELECT COUNT(*) FROM worksheet_versions WHERE worksheet_id = $1")
        .bind(worksheet_id)
        .fetch_one(db)
        .await
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct VersionSummary {
    pub version_no: i32,
    pub cause: String,
    pub created_at: DateTime<Utc>,
}

pub async fn list_versions(db: &PgPool, worksheet_id: i64) -> sqlx::Result<Vec<VersionSummary>> {
    sqlx::query_as(
        "SELECT version_no, cause, created_at FROM worksheet_versions
          WHERE worksheet_id = $1 ORDER BY version_no DESC",
    )
    .bind(worksheet_id)
    .fetch_all(db)
    .await
}

async fn insert_version(
    tx: &mut Transaction<'_, Postgres>,
    worksheet_id: i64,
    content: &Content,
    cause: &str,
) -> AppResult<Version> {
    let json = serde_json::to_value(content).map_err(anyhow::Error::from)?;
    let row: VersionRow = sqlx::query_as(&format!(
        "INSERT INTO worksheet_versions (worksheet_id, version_no, content, cause)
         VALUES ($1, (SELECT COALESCE(MAX(version_no), 0) + 1 FROM worksheet_versions
                       WHERE worksheet_id = $1), $2, $3)
         RETURNING {VERSION_COLS}"
    ))
    .bind(worksheet_id)
    .bind(json)
    .bind(cause)
    .fetch_one(&mut **tx)
    .await?;
    sqlx::query("UPDATE worksheets SET updated_at = now() WHERE id = $1")
        .bind(worksheet_id)
        .execute(&mut **tx)
        .await?;
    Ok(row.into())
}

/// Appends a version (already validated). Returns the latest one unchanged when the content is
/// the same. Saves of one worksheet are serialised by locking its row.
pub async fn save_content(
    db: &PgPool,
    worksheet_id: i64,
    content: &Content,
    cause: &str,
) -> AppResult<Version> {
    let mut tx = db.begin().await?;
    sqlx::query("SELECT id FROM worksheets WHERE id = $1 FOR UPDATE")
        .bind(worksheet_id)
        .execute(&mut *tx)
        .await?;
    let current: Option<VersionRow> = sqlx::query_as(&format!(
        "SELECT {VERSION_COLS} FROM worksheet_versions
          WHERE worksheet_id = $1 ORDER BY version_no DESC LIMIT 1"
    ))
    .bind(worksheet_id)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(v) = current.map(Version::from) {
        if v.content == *content {
            return Ok(v);
        }
    }
    let saved = insert_version(&mut tx, worksheet_id, content, cause).await?;
    tx.commit().await?;
    Ok(saved)
}

// ---------------------------------------------------------------- fields

pub async fn update_fields(
    db: &PgPool,
    id: i64,
    title: &str,
    filename: &str,
    settings: &Settings,
    brief: &Brief,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE worksheets SET title = $2, filename = $3, settings = $4, brief = $5,
                updated_at = CASE WHEN title = $2 AND filename = $3 AND settings = $4
                                       AND brief = $5 THEN updated_at ELSE now() END
          WHERE id = $1",
    )
    .bind(id)
    .bind(title)
    .bind(filename)
    .bind(Json(settings))
    .bind(Json(brief))
    .execute(db)
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn set_material(
    db: &PgPool,
    id: i64,
    material: &Material,
    paragraph_seq: i32,
    settings: &Settings,
    suggested: &Settings,
    title: &str,
    filename: &str,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE worksheets SET material = $2, paragraph_seq = $3, settings = $4, suggested = $5,
                title = $6, filename = $7, updated_at = now()
          WHERE id = $1",
    )
    .bind(id)
    .bind(Json(material))
    .bind(paragraph_seq)
    .bind(Json(settings))
    .bind(Json(suggested))
    .bind(title)
    .bind(filename)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn set_lesson(
    db: &PgPool,
    id: i64,
    lesson_id: Option<i64>,
    source_sop_version_id: Option<i64>,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE worksheets SET lesson_id = $2, source_sop_version_id = $3, updated_at = now()
          WHERE id = $1",
    )
    .bind(id)
    .bind(lesson_id)
    .bind(source_sop_version_id)
    .execute(db)
    .await?;
    Ok(())
}

/// Sets the teacher's request ("" clears it).
pub async fn set_request(db: &PgPool, id: i64, request: &str) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE worksheets SET request = $2,
                updated_at = CASE WHEN request = $2 THEN updated_at ELSE now() END
          WHERE id = $1",
    )
    .bind(id)
    .bind(request)
    .execute(db)
    .await?;
    Ok(())
}

/// Sets (or, with `None`, clears) the template a worksheet follows.
pub async fn set_template(db: &PgPool, id: i64, template: Option<(i64, i32)>) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE worksheets SET template_id = $2, template_version_no = $3, updated_at = now()
          WHERE id = $1",
    )
    .bind(id)
    .bind(template.map(|(t, _)| t))
    .bind(template.map(|(_, no)| no))
    .execute(db)
    .await?;
    Ok(())
}

pub async fn set_generated(db: &PgPool, id: i64) -> sqlx::Result<()> {
    sqlx::query("UPDATE worksheets SET generated = TRUE, updated_at = now() WHERE id = $1")
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn soft_delete(db: &PgPool, id: i64) -> sqlx::Result<()> {
    sqlx::query("UPDATE worksheets SET deleted_at = now() WHERE id = $1")
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

// ---------------------------------------------------------------- per-block history

/// Keeps `block` as it is now, before a regeneration replaces it.
pub async fn push_history(
    db: &PgPool,
    worksheet_id: i64,
    block: &Block,
    interaction_id: Option<i64>,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO worksheet_block_history (worksheet_id, block_id, block, interaction_id)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(worksheet_id)
    .bind(&block.id)
    .bind(Json(block))
    .bind(interaction_id)
    .execute(db)
    .await?;
    Ok(())
}

/// Takes the newest kept state of a block back out: the block and the interaction that had
/// replaced it.
pub async fn pop_history(
    db: &PgPool,
    worksheet_id: i64,
    block_id: &str,
) -> sqlx::Result<Option<(Block, Option<i64>)>> {
    let row: Option<(serde_json::Value, Option<i64>)> = sqlx::query_as(
        "DELETE FROM worksheet_block_history
          WHERE id = (SELECT id FROM worksheet_block_history
                       WHERE worksheet_id = $1 AND block_id = $2 ORDER BY id DESC LIMIT 1)
        RETURNING block, interaction_id",
    )
    .bind(worksheet_id)
    .bind(block_id)
    .fetch_optional(db)
    .await?;
    Ok(row.and_then(|(block, interaction)| {
        serde_json::from_value::<Block>(block)
            .ok()
            .map(|b| (b, interaction))
    }))
}

/// Ids of the blocks that have a kept earlier state.
pub async fn undoable(db: &PgPool, worksheet_id: i64) -> sqlx::Result<Vec<String>> {
    sqlx::query_scalar(
        "SELECT DISTINCT block_id FROM worksheet_block_history
          WHERE worksheet_id = $1 ORDER BY block_id",
    )
    .bind(worksheet_id)
    .fetch_all(db)
    .await
}
