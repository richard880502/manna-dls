//! Reading and writing templates. Versions are append-only; a template and its versions are
//! deleted for good (no soft delete).

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::types::Json;
use sqlx::PgPool;

use super::WsTemplate;
use crate::error::{AppError, AppResult};

pub const STATUS_EXTRACTING: &str = "extracting";
pub const STATUS_READY: &str = "ready";
pub const STATUS_FAILED: &str = "failed";

pub const MSG_NOT_FOUND: &str = "找不到這個版型。";

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TemplateRow {
    pub id: i64,
    pub teacher_id: i64,
    pub title: String,
    pub status: String,
    pub error: Option<String>,
    pub source_material_id: Option<i64>,
    pub extract_run: i32,
    pub extract_started_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

const COLS: &str = "id, teacher_id, title, status, error, source_material_id, extract_run,
    extract_started_at, created_at, updated_at";

/// The template if it exists and belongs to `teacher_id`; otherwise 404 (no existence leak).
pub async fn owned(db: &PgPool, id: i64, teacher_id: i64) -> AppResult<TemplateRow> {
    find(db, id, teacher_id)
        .await?
        .ok_or_else(|| AppError::not_found(MSG_NOT_FOUND))
}

pub async fn find(db: &PgPool, id: i64, teacher_id: i64) -> sqlx::Result<Option<TemplateRow>> {
    sqlx::query_as(&format!(
        "SELECT {COLS} FROM worksheet_templates WHERE id = $1 AND teacher_id = $2"
    ))
    .bind(id)
    .bind(teacher_id)
    .fetch_optional(db)
    .await
}

/// The teacher's template made from `material_id` that a new worksheet can follow: the newest
/// `ready` one, otherwise the newest still `extracting` (its extraction is already under way).
/// Failed ones are not reused.
pub async fn reusable_for_material(
    db: &PgPool,
    teacher_id: i64,
    material_id: i64,
) -> sqlx::Result<Option<TemplateRow>> {
    sqlx::query_as(&format!(
        "SELECT {COLS} FROM worksheet_templates
          WHERE teacher_id = $1 AND source_material_id = $2
            AND status IN ('{STATUS_READY}', '{STATUS_EXTRACTING}')
          ORDER BY status = '{STATUS_READY}' DESC, created_at DESC, id DESC
          LIMIT 1"
    ))
    .bind(teacher_id)
    .bind(material_id)
    .fetch_optional(db)
    .await
}

/// The latest version: its number and content.
pub async fn latest_version(db: &PgPool, id: i64) -> sqlx::Result<Option<(i32, WsTemplate)>> {
    let row: Option<(i32, serde_json::Value)> = sqlx::query_as(
        "SELECT version_no, content FROM worksheet_template_versions
          WHERE template_id = $1 ORDER BY version_no DESC LIMIT 1",
    )
    .bind(id)
    .fetch_optional(db)
    .await?;
    Ok(row.map(|(no, c)| (no, serde_json::from_value(c).unwrap_or_default())))
}

pub async fn version(db: &PgPool, id: i64, version_no: i32) -> sqlx::Result<Option<WsTemplate>> {
    let row: Option<serde_json::Value> = sqlx::query_scalar(
        "SELECT content FROM worksheet_template_versions WHERE template_id = $1 AND version_no = $2",
    )
    .bind(id)
    .bind(version_no)
    .fetch_optional(db)
    .await?;
    Ok(row.map(|c| serde_json::from_value(c).unwrap_or_default()))
}

pub async fn insert(
    db: &PgPool,
    teacher_id: i64,
    title: &str,
    material_id: i64,
) -> sqlx::Result<TemplateRow> {
    sqlx::query_as(&format!(
        "INSERT INTO worksheet_templates (teacher_id, title, status, source_material_id,
                                          extract_started_at)
         VALUES ($1, $2, '{STATUS_EXTRACTING}', $3, now()) RETURNING {COLS}"
    ))
    .bind(teacher_id)
    .bind(title)
    .bind(material_id)
    .fetch_one(db)
    .await
}

/// Starts another extraction of a failed template. `None` when it is not failed (anymore).
pub async fn restart(db: &PgPool, id: i64) -> sqlx::Result<Option<TemplateRow>> {
    sqlx::query_as(&format!(
        "UPDATE worksheet_templates
            SET status = '{STATUS_EXTRACTING}', error = NULL, extract_run = extract_run + 1,
                extract_started_at = now(), updated_at = now()
          WHERE id = $1 AND status = '{STATUS_FAILED}'
          RETURNING {COLS}"
    ))
    .bind(id)
    .fetch_optional(db)
    .await
}

/// Appends a version (already validated) and returns its number.
pub async fn add_version<'e, E: sqlx::PgExecutor<'e>>(
    executor: E,
    id: i64,
    content: &WsTemplate,
    origin: &str,
) -> sqlx::Result<i32> {
    sqlx::query_scalar(
        "WITH v AS (
           INSERT INTO worksheet_template_versions (template_id, version_no, content, origin)
           VALUES ($1, (SELECT COALESCE(MAX(version_no), 0) + 1 FROM worksheet_template_versions
                         WHERE template_id = $1), $2, $3)
           RETURNING version_no)
         UPDATE worksheet_templates SET updated_at = now() WHERE id = $1
         RETURNING (SELECT version_no FROM v)",
    )
    .bind(id)
    .bind(Json(content))
    .bind(origin)
    .fetch_one(executor)
    .await
}

pub async fn set_title(db: &PgPool, id: i64, title: &str) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE worksheet_templates SET title = $2,
                updated_at = CASE WHEN title = $2 THEN updated_at ELSE now() END
          WHERE id = $1",
    )
    .bind(id)
    .bind(title)
    .execute(db)
    .await?;
    Ok(())
}

/// Hard delete: the template and all its versions. Worksheets made from it keep their content
/// and lose the link (`ON DELETE SET NULL`).
pub async fn delete(db: &PgPool, id: i64) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM worksheet_templates WHERE id = $1")
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

/// Extractions still open long after their calls must have timed out died with the server.
pub async fn expire_stale(db: &PgPool, teacher_id: i64, stale_secs: f64) -> sqlx::Result<()> {
    sqlx::query(&format!(
        "UPDATE worksheet_templates
            SET status = '{STATUS_FAILED}', error = $2, updated_at = now()
          WHERE teacher_id = $1 AND status = '{STATUS_EXTRACTING}'
            AND extract_started_at < now() - make_interval(secs => $3)"
    ))
    .bind(teacher_id)
    .bind(super::extract::MSG_STALE)
    .bind(stale_secs)
    .execute(db)
    .await?;
    Ok(())
}

// ---------------------------------------------------------------- what the API returns

#[derive(Debug, Clone, Serialize)]
pub struct SourceMaterial {
    pub id: i64,
    pub title: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub id: i64,
    pub title: String,
    /// `extracting` | `ready` | `failed`
    pub status: String,
    pub section_count: usize,
    pub source_material: Option<SourceMaterial>,
    pub version_no: Option<i32>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Detail {
    #[serde(flatten)]
    pub summary: Summary,
    pub error: Option<String>,
    pub content: Option<WsTemplate>,
}

#[derive(sqlx::FromRow)]
struct DetailRow {
    id: i64,
    title: String,
    status: String,
    error: Option<String>,
    updated_at: DateTime<Utc>,
    material_id: Option<i64>,
    material_title: Option<String>,
    version_no: Option<i32>,
    content: Option<serde_json::Value>,
}

impl From<DetailRow> for Detail {
    fn from(r: DetailRow) -> Self {
        let content: Option<WsTemplate> = r
            .content
            .map(|c| serde_json::from_value(c).unwrap_or_default());
        Self {
            summary: Summary {
                id: r.id,
                title: r.title,
                status: r.status,
                section_count: content.as_ref().map_or(0, |c| c.sections.len()),
                source_material: match (r.material_id, r.material_title) {
                    (Some(id), Some(title)) => Some(SourceMaterial { id, title }),
                    _ => None,
                },
                version_no: r.version_no,
                updated_at: r.updated_at,
            },
            error: r.error,
            content,
        }
    }
}

const DETAIL_SELECT: &str = "SELECT t.id, t.title, t.status, t.error, t.updated_at,
        m.id AS material_id, m.title AS material_title, v.version_no, v.content
   FROM worksheet_templates t
   LEFT JOIN materials m ON m.id = t.source_material_id
   LEFT JOIN LATERAL (SELECT version_no, content FROM worksheet_template_versions
                       WHERE template_id = t.id ORDER BY version_no DESC LIMIT 1) v ON TRUE";

/// The teacher's templates, newest first.
pub async fn list(db: &PgPool, teacher_id: i64) -> sqlx::Result<Vec<Summary>> {
    let rows: Vec<DetailRow> = sqlx::query_as(&format!(
        "{DETAIL_SELECT} WHERE t.teacher_id = $1 ORDER BY t.created_at DESC, t.id DESC"
    ))
    .bind(teacher_id)
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(|r| Detail::from(r).summary).collect())
}

pub async fn detail(db: &PgPool, id: i64) -> AppResult<Detail> {
    let row: Option<DetailRow> = sqlx::query_as(&format!("{DETAIL_SELECT} WHERE t.id = $1"))
        .bind(id)
        .fetch_optional(db)
        .await?;
    row.map(Detail::from)
        .ok_or_else(|| AppError::not_found(MSG_NOT_FOUND))
}

/// A template a worksheet may use: the teacher's own and `ready`. Anything else is a 400 (the
/// id came in a worksheet request).
pub async fn ready_for_use(
    db: &PgPool,
    teacher_id: i64,
    id: i64,
) -> AppResult<(TemplateRow, i32, WsTemplate)> {
    let row = find(db, id, teacher_id)
        .await?
        .ok_or_else(|| AppError::bad_request(MSG_NOT_FOUND))?;
    let not_ready =
        || AppError::bad_request("這個版型還沒有準備好（還在讀取或讀取失敗），請選其他版型。");
    if row.status != STATUS_READY {
        return Err(not_ready());
    }
    let (version_no, content) = latest_version(db, id).await?.ok_or_else(not_ready)?;
    Ok((row, version_no, content))
}
