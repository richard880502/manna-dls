//! Reading and writing templates. Versions are append-only; a template and its versions are
//! deleted for good (no soft delete).
//!
//! A template is personal (owner-only) or a platform template (平台範本, docs/API.md「Phase 2.9」):
//! every teacher reads and uses platform templates; only ManageKnowledge changes them.

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::types::Json;
use sqlx::PgPool;

use super::WsTemplate;
use crate::auth::{AuthTeacher, Capability};
use crate::error::{AppError, AppResult};
use crate::materials::catalog::{SCOPE_PERSONAL, SCOPE_PLATFORM};

pub const STATUS_EXTRACTING: &str = "extracting";
pub const STATUS_READY: &str = "ready";
pub const STATUS_FAILED: &str = "failed";

pub const MSG_NOT_FOUND: &str = "找不到這個版型。";
pub const MSG_PLATFORM_READ_ONLY: &str =
    "這是 Manna 的平台範本，只有計畫主持人或知識庫管理員可以修改。要調整的話，請先「另存成我的範本」。";

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
    /// `personal` or `platform`.
    pub scope: String,
    pub category: String,
    pub tags: Vec<String>,
}

impl TemplateRow {
    pub fn is_platform(&self) -> bool {
        self.scope == SCOPE_PLATFORM
    }
}

const COLS: &str = "id, teacher_id, title, status, error, source_material_id, extract_run,
    extract_started_at, created_at, updated_at, scope, category, tags";

/// A template the teacher may read and use: her own personal one or any platform one; another
/// teacher's personal template is a 404 (no existence leak).
pub async fn readable(db: &PgPool, id: i64, teacher_id: i64) -> AppResult<TemplateRow> {
    find(db, id, teacher_id)
        .await?
        .ok_or_else(|| AppError::not_found(MSG_NOT_FOUND))
}

/// A template the teacher may change, retry or delete: her own personal one, or a platform one
/// when she holds ManageKnowledge (403 otherwise).
pub async fn writable(db: &PgPool, id: i64, teacher: &AuthTeacher) -> AppResult<TemplateRow> {
    let row = readable(db, id, teacher.id()).await?;
    if row.is_platform() && !teacher.can(Capability::ManageKnowledge) {
        return Err(AppError::Forbidden(MSG_PLATFORM_READ_ONLY.into()));
    }
    Ok(row)
}

/// The template if the teacher may read it (see [`readable`]).
pub async fn find(db: &PgPool, id: i64, teacher_id: i64) -> sqlx::Result<Option<TemplateRow>> {
    sqlx::query_as(&format!(
        "SELECT {COLS} FROM worksheet_templates
          WHERE id = $1 AND (scope = '{SCOPE_PLATFORM}' OR teacher_id = $2)"
    ))
    .bind(id)
    .bind(teacher_id)
    .fetch_optional(db)
    .await
}

/// The platform template `id`, for the admin endpoints; anything else is a 404.
pub async fn platform(db: &PgPool, id: i64) -> AppResult<TemplateRow> {
    sqlx::query_as(&format!(
        "SELECT {COLS} FROM worksheet_templates WHERE id = $1 AND scope = '{SCOPE_PLATFORM}'"
    ))
    .bind(id)
    .fetch_optional(db)
    .await?
    .ok_or_else(|| AppError::not_found("找不到這個平台範本。"))
}

/// The template made from `material_id` that a new worksheet of `teacher_id` can follow: a
/// `ready` one first (her own, then a platform one), otherwise her newest one still
/// `extracting` (its extraction is already under way). Failed ones are not reused.
pub async fn reusable_for_material(
    db: &PgPool,
    teacher_id: i64,
    material_id: i64,
) -> sqlx::Result<Option<TemplateRow>> {
    sqlx::query_as(&format!(
        "SELECT {COLS} FROM worksheet_templates
          WHERE source_material_id = $2
            AND ((teacher_id = $1 AND scope = '{SCOPE_PERSONAL}'
                  AND status IN ('{STATUS_READY}', '{STATUS_EXTRACTING}'))
                 OR (scope = '{SCOPE_PLATFORM}' AND status = '{STATUS_READY}'))
          ORDER BY status = '{STATUS_READY}' DESC, scope = '{SCOPE_PERSONAL}' DESC,
                   created_at DESC, id DESC
          LIMIT 1"
    ))
    .bind(teacher_id)
    .bind(material_id)
    .fetch_optional(db)
    .await
}

/// The teacher's newest template made from `material_id` that is still `extracting` (for a
/// sample whose OCR is still running: a ready template was made from the old text).
pub async fn extracting_for_material(
    db: &PgPool,
    teacher_id: i64,
    material_id: i64,
) -> sqlx::Result<Option<TemplateRow>> {
    sqlx::query_as(&format!(
        "SELECT {COLS} FROM worksheet_templates
          WHERE teacher_id = $1 AND source_material_id = $2 AND status = '{STATUS_EXTRACTING}'
          ORDER BY created_at DESC, id DESC
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

/// A new personal template of `teacher_id`, extracting from `material_id`.
pub async fn insert(
    db: &PgPool,
    teacher_id: i64,
    title: &str,
    material_id: i64,
) -> sqlx::Result<TemplateRow> {
    insert_placed(
        db,
        NewTemplate {
            teacher_id,
            title,
            material_id,
            scope: SCOPE_PERSONAL,
            category: "",
            tags: &[],
        },
    )
    .await
}

pub struct NewTemplate<'a> {
    pub teacher_id: i64,
    pub title: &'a str,
    pub material_id: i64,
    pub scope: &'static str,
    /// From the sample list, or "" (suggested when the extraction finishes).
    pub category: &'a str,
    pub tags: &'a [String],
}

/// A new template, `extracting`. Every template made from a personal material, by any path,
/// moves that material to the teacher's samples (我的範本): it is no longer listed in 我的教材.
pub async fn insert_placed(db: &PgPool, new: NewTemplate<'_>) -> sqlx::Result<TemplateRow> {
    let mut tx = db.begin().await?;
    let row: TemplateRow = sqlx::query_as(&format!(
        "INSERT INTO worksheet_templates (teacher_id, title, status, source_material_id,
                                          extract_started_at, scope, category, tags)
         VALUES ($1, $2, '{STATUS_EXTRACTING}', $3, now(), $4, $5, $6) RETURNING {COLS}"
    ))
    .bind(new.teacher_id)
    .bind(new.title)
    .bind(new.material_id)
    .bind(new.scope)
    .bind(new.category)
    .bind(new.tags)
    .fetch_one(&mut *tx)
    .await?;
    crate::materials::store::mark_sample(&mut *tx, new.material_id).await?;
    tx.commit().await?;
    Ok(row)
}

/// A personal copy of `source` for `teacher_id` (「另存成我的範本」): `ready` at once, version 1 is
/// `content` with origin `teacher`. It keeps the source material (the original file), category
/// and tags.
pub async fn copy(
    db: &PgPool,
    source: &TemplateRow,
    teacher_id: i64,
    title: &str,
    content: &WsTemplate,
) -> sqlx::Result<TemplateRow> {
    let mut tx = db.begin().await?;
    let row: TemplateRow = sqlx::query_as(&format!(
        "INSERT INTO worksheet_templates (teacher_id, title, status, source_material_id,
                                          scope, category, tags)
         VALUES ($1, $2, '{STATUS_READY}', $3, '{SCOPE_PERSONAL}', $4, $5) RETURNING {COLS}"
    ))
    .bind(teacher_id)
    .bind(title)
    .bind(source.source_material_id)
    .bind(&source.category)
    .bind(&source.tags)
    .fetch_one(&mut *tx)
    .await?;
    add_version(&mut *tx, row.id, content, "teacher").await?;
    tx.commit().await?;
    Ok(row)
}

/// Category and tags (already validated).
pub async fn set_catalog(
    db: &PgPool,
    id: i64,
    category: &str,
    tags: &[String],
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE worksheet_templates
            SET category = $2, tags = $3,
                updated_at = CASE WHEN (category, tags) IS DISTINCT FROM ($2, $3)
                                  THEN now() ELSE updated_at END
          WHERE id = $1",
    )
    .bind(id)
    .bind(category)
    .bind(tags)
    .execute(db)
    .await?;
    Ok(())
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

/// Extractions still open long after their calls must have timed out died with the server: the
/// teacher's own, and the platform's (every teacher reads those).
pub async fn expire_stale(db: &PgPool, teacher_id: i64, stale_secs: f64) -> sqlx::Result<()> {
    sqlx::query(&format!(
        "UPDATE worksheet_templates
            SET status = '{STATUS_FAILED}', error = $2, updated_at = now()
          WHERE (teacher_id = $1 OR scope = '{SCOPE_PLATFORM}') AND status = '{STATUS_EXTRACTING}'
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
    /// `personal` or `platform` (a platform template is read-only for teachers: copy it first).
    pub scope: String,
    pub category: String,
    pub tags: Vec<String>,
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
    scope: String,
    category: String,
    tags: Vec<String>,
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
                scope: r.scope,
                category: r.category,
                tags: r.tags,
            },
            error: r.error,
            content,
        }
    }
}

const DETAIL_SELECT: &str = "SELECT t.id, t.title, t.status, t.error, t.updated_at, t.scope,
        t.category, t.tags,
        m.id AS material_id, m.title AS material_title, v.version_no, v.content
   FROM worksheet_templates t
   LEFT JOIN materials m ON m.id = t.source_material_id
   LEFT JOIN LATERAL (SELECT version_no, content FROM worksheet_template_versions
                       WHERE template_id = t.id ORDER BY version_no DESC LIMIT 1) v ON TRUE";

/// The teacher's own (personal) templates, newest first.
pub async fn list(db: &PgPool, teacher_id: i64) -> sqlx::Result<Vec<Summary>> {
    let rows: Vec<DetailRow> = sqlx::query_as(&format!(
        "{DETAIL_SELECT} WHERE t.teacher_id = $1 AND t.scope = '{SCOPE_PERSONAL}'
          ORDER BY t.created_at DESC, t.id DESC"
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

/// A template a worksheet may use: the teacher's own or a platform one, `ready`. Anything else
/// is a 400 (the id came in a worksheet request).
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
