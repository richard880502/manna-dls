//! 我的學習單版型 endpoints (docs/API.md「Phase 2.5：我的學習單版型」, 「Phase 2.9」): a teacher's
//! templates, their extraction as a background job, edits (append-only versions), the hard
//! delete, and the copy of a platform template. A teacher reaches her own templates and reads
//! every platform template (平台範本); another teacher's id is a 404. Platform templates are
//! changed only by ManageKnowledge (403 for everyone else); teachers copy them instead.

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::auth::AuthTeacher;
use crate::error::{ApiJson, ApiPath, AppError, AppResult};
use crate::materials::catalog::{self, PURPOSE_SAMPLE, SAMPLE_CATEGORIES, SCOPE_PLATFORM};
use crate::materials::store as material_store;
use crate::routes::log_usage;
use crate::worksheet::template::extract::{self, stale_after};
use crate::worksheet::template::store::{
    self, Detail, Summary, TemplateRow, STATUS_EXTRACTING, STATUS_FAILED, STATUS_READY,
};
use crate::worksheet::template::{WsTemplate, MAX_TITLE_CHARS};
use crate::AppState;

#[derive(Debug, Serialize)]
pub struct TemplateList {
    pub templates: Vec<Summary>,
}

pub async fn list(
    State(state): State<AppState>,
    teacher: AuthTeacher,
) -> AppResult<Json<TemplateList>> {
    store::expire_stale(&state.db, teacher.id(), stale_after(&state)).await?;
    Ok(Json(TemplateList {
        templates: store::list(&state.db, teacher.id()).await?,
    }))
}

fn check_title(title: &str) -> AppResult<String> {
    let title = title.trim();
    if title.is_empty() {
        return Err(AppError::bad_request("版型名稱不能是空的。"));
    }
    if title.chars().count() > MAX_TITLE_CHARS {
        return Err(AppError::bad_request(format!(
            "版型名稱最多 {MAX_TITLE_CHARS} 字。"
        )));
    }
    Ok(title.to_string())
}

#[derive(Debug, Deserialize)]
pub struct CreateTemplate {
    pub material_id: i64,
    #[serde(default)]
    pub title: Option<String>,
    /// From the sample list; blank or absent: suggested when the extraction finishes.
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
}

/// Tags sent either as a list or as one comma-separated string.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum TagsInput {
    List(Vec<String>),
    Text(String),
}

impl TagsInput {
    pub fn cleaned(&self) -> AppResult<Vec<String>> {
        match self {
            Self::List(list) => catalog::clean_tags(list),
            Self::Text(text) => catalog::split_tags(text),
        }
    }
}

pub async fn create(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiJson(req): ApiJson<CreateTemplate>,
) -> AppResult<(StatusCode, Json<Detail>)> {
    let db = &state.db;
    let given = match req.title.as_deref().map(str::trim) {
        Some(t) if !t.is_empty() => Some(check_title(t)?),
        _ => None,
    };
    let category = catalog::check_category(
        &SAMPLE_CATEGORIES,
        req.category.as_deref().unwrap_or_default(),
        true,
    )?;
    let tags = catalog::clean_tags(req.tags.as_deref().unwrap_or_default())?;
    let material = extract::source(db, teacher.id(), req.material_id).await?;
    let title = given.unwrap_or_else(|| extract::default_title(&material.title));
    let row = store::insert_placed(
        db,
        store::NewTemplate {
            teacher_id: teacher.id(),
            title: &title,
            material_id: material.id,
            scope: catalog::SCOPE_PERSONAL,
            category: &category,
            tags: &tags,
        },
    )
    .await?;
    // Read the answer before the job starts: a fast extraction could otherwise finish first and
    // the 202 would say "ready" (or "failed") instead of "extracting".
    let detail = store::detail(db, row.id).await?;
    extract::spawn(&state, &row, material.text);
    log_usage(
        db,
        teacher.id(),
        "worksheet_template_created",
        json!({ "template_id": row.id, "material_id": row.source_material_id }),
    )
    .await?;
    Ok((StatusCode::ACCEPTED, Json(detail)))
}

pub async fn get(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<Json<Detail>> {
    store::expire_stale(&state.db, teacher.id(), stale_after(&state)).await?;
    store::readable(&state.db, id, teacher.id()).await?;
    Ok(Json(store::detail(&state.db, id).await?))
}

#[derive(Debug, Deserialize)]
pub struct SaveTemplate {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub content: Option<WsTemplate>,
    /// From the sample list, or "" (none).
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub tags: Option<TagsInput>,
}

pub async fn save(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    ApiJson(req): ApiJson<SaveTemplate>,
) -> AppResult<Json<Detail>> {
    let db = &state.db;
    store::expire_stale(db, teacher.id(), stale_after(&state)).await?;
    let row = store::writable(db, id, &teacher).await?;
    let category = req
        .category
        .as_deref()
        .map(|c| catalog::check_category(&SAMPLE_CATEGORIES, c, true))
        .transpose()?;
    let tags = req.tags.as_ref().map(TagsInput::cleaned).transpose()?;
    // Category and tags can be set while the extraction runs; title and content cannot.
    if row.status == STATUS_EXTRACTING && (req.title.is_some() || req.content.is_some()) {
        return Err(AppError::Conflict(
            "Manna 還在讀這份學習單的設計，好了之後再改。".to_string(),
        ));
    }
    let title = req.title.as_deref().map(check_title).transpose()?;
    let content = match req.content {
        Some(mut c) => {
            if row.status != STATUS_READY {
                return Err(AppError::Conflict(
                    "這個版型還沒有讀取成功，請先按「重試」。".to_string(),
                ));
            }
            c.validate().map_err(AppError::bad_request)?;
            Some(c)
        }
        None => None,
    };
    if category.is_some() || tags.is_some() {
        store::set_catalog(
            db,
            id,
            category.as_deref().unwrap_or(&row.category),
            tags.as_deref().unwrap_or(&row.tags),
        )
        .await?;
    }
    if let Some(t) = &title {
        store::set_title(db, id, t).await?;
    }
    if let Some(c) = &content {
        let unchanged = store::latest_version(db, id)
            .await?
            .is_some_and(|(_, current)| current == *c);
        if !unchanged {
            let version_no = store::add_version(db, id, c, "teacher").await?;
            log_usage(
                db,
                teacher.id(),
                "worksheet_template_saved",
                json!({ "template_id": id, "version_no": version_no, "sections": c.sections.len() }),
            )
            .await?;
        }
    }
    Ok(Json(store::detail(db, id).await?))
}

pub async fn retry(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<(StatusCode, Json<Detail>)> {
    let db = &state.db;
    store::expire_stale(db, teacher.id(), stale_after(&state)).await?;
    let row = store::writable(db, id, &teacher).await?;
    let (row, text) = begin_retry(&state, &row).await?;
    // Read the answer before the job starts: a fast extraction could otherwise finish first.
    let detail = store::detail(db, id).await?;
    spawn_retry(&state, teacher.id(), &row, text).await?;
    Ok((StatusCode::ACCEPTED, Json(detail)))
}

/// Opens another extraction of a failed template (`POST …/retry`, also for the admin's platform
/// templates): 409 unless it is `failed`, 400 when its sample is gone. Returns the template
/// (now `extracting`) and the sample's text; the caller reads its answer, then
/// [`spawn_retry`]s.
pub async fn begin_retry(state: &AppState, row: &TemplateRow) -> AppResult<(TemplateRow, String)> {
    let db = &state.db;
    if row.status != STATUS_FAILED {
        return Err(AppError::Conflict(
            "只有讀取失敗的版型可以重試。".to_string(),
        ));
    }
    let Some(material_id) = row.source_material_id else {
        return Err(AppError::bad_request(
            "原本的學習單檔案已經刪除，無法重試；請重新上傳。",
        ));
    };
    // The template's owner reads its sample (a platform template's sample is a platform row).
    let material = extract::source(db, row.teacher_id, material_id).await?;
    let Some(row) = store::restart(db, row.id).await? else {
        return Err(AppError::Conflict(
            "只有讀取失敗的版型可以重試。".to_string(),
        ));
    };
    Ok((row, material.text))
}

/// Starts the extraction [`begin_retry`] opened and logs it.
pub async fn spawn_retry(
    state: &AppState,
    actor_id: i64,
    row: &TemplateRow,
    text: String,
) -> AppResult<()> {
    extract::spawn(state, row, text);
    log_usage(
        &state.db,
        actor_id,
        "worksheet_template_retried",
        json!({ "template_id": row.id, "run": row.extract_run }),
    )
    .await?;
    Ok(())
}

pub async fn delete(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<Json<serde_json::Value>> {
    let row = store::writable(&state.db, id, &teacher).await?;
    remove(&state, teacher.id(), &row).await?;
    Ok(Json(json!({ "ok": true })))
}

/// Hard delete of a template. A platform template takes its platform sample along (the file the
/// admin uploaded for it), unless another template still uses that sample; the sample's file is
/// removed first, so a storage failure deletes nothing.
pub async fn remove(state: &AppState, actor_id: i64, row: &TemplateRow) -> AppResult<()> {
    let db = &state.db;
    if row.scope == SCOPE_PLATFORM {
        if let Some(material_id) = row.source_material_id {
            let others: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM worksheet_templates
                                 WHERE source_material_id = $1 AND id <> $2)",
            )
            .bind(material_id)
            .bind(row.id)
            .fetch_one(db)
            .await?;
            let sample = material_store::by_id(db, material_id).await?;
            if let Some(sample) = sample
                .filter(|m| !others && m.scope == SCOPE_PLATFORM && m.purpose == PURPOSE_SAMPLE)
            {
                material_store::delete(state, actor_id, sample).await?;
            }
        }
    }
    store::delete(db, row.id).await?;
    log_usage(
        db,
        actor_id,
        "worksheet_template_deleted",
        json!({ "template_id": row.id, "scope": row.scope }),
    )
    .await?;
    Ok(())
}

/// `POST /api/worksheet-templates/:id/copy`: 「另存成我的範本」. A personal copy of a template the
/// teacher can read (a platform one, or her own): `ready`, version 1 = the current content with
/// origin `teacher`, titled 「<title>（我的版本）」. 409 while the template has no content yet.
pub async fn copy(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<(StatusCode, Json<Detail>)> {
    let db = &state.db;
    store::expire_stale(db, teacher.id(), stale_after(&state)).await?;
    let source = store::readable(db, id, teacher.id()).await?;
    let content = match store::latest_version(db, id).await? {
        Some((_, content)) if source.status == STATUS_READY => content,
        _ => {
            return Err(AppError::Conflict(
                "這個版型還沒有準備好（還在讀取或讀取失敗），還不能另存。".to_string(),
            ))
        }
    };
    let title = copy_title(&source.title);
    let row = store::copy(db, &source, teacher.id(), &title, &content).await?;
    log_usage(
        db,
        teacher.id(),
        "worksheet_template_copied",
        json!({ "template_id": row.id, "from_template_id": id, "from_scope": source.scope }),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(store::detail(db, row.id).await?)))
}

/// 「<title>（我的版本）」 within the title limit.
pub fn copy_title(title: &str) -> String {
    const SUFFIX: &str = "（我的版本）";
    let room = MAX_TITLE_CHARS - SUFFIX.chars().count();
    let base: String = title.trim().chars().take(room).collect();
    format!("{}{SUFFIX}", base.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_are_named_my_version_within_the_limit() {
        assert_eq!(copy_title(" 閱讀理解版型 "), "閱讀理解版型（我的版本）");
        let long = "字".repeat(MAX_TITLE_CHARS);
        let t = copy_title(&long);
        assert_eq!(t.chars().count(), MAX_TITLE_CHARS);
        assert!(t.ends_with("（我的版本）"));
    }
}
