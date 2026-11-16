//! 我的學習單版型 endpoints (docs/API.md「Phase 2.5：我的學習單版型」): a teacher's templates,
//! their extraction as a background job, edits (append-only versions) and the hard delete.
//! Every template belongs to one teacher; anyone else's id is a 404.

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::auth::AuthTeacher;
use crate::error::{ApiJson, ApiPath, AppError, AppResult};
use crate::routes::log_usage;
use crate::worksheet::template::extract::{self, stale_after};
use crate::worksheet::template::store::{
    self, Detail, Summary, STATUS_EXTRACTING, STATUS_FAILED, STATUS_READY,
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
    let material = extract::source(db, teacher.id(), req.material_id).await?;
    let title = given.unwrap_or_else(|| {
        let base = material.title.trim();
        let base = if base.is_empty() {
            "我的學習單"
        } else {
            base
        };
        // Room for 「版型」 within the limit.
        let base: String = base.chars().take(MAX_TITLE_CHARS - 2).collect();
        format!("{}版型", base.trim_end())
    });
    let row = store::insert(db, teacher.id(), &title, material.id).await?;
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
    store::owned(&state.db, id, teacher.id()).await?;
    Ok(Json(store::detail(&state.db, id).await?))
}

#[derive(Debug, Deserialize)]
pub struct SaveTemplate {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub content: Option<WsTemplate>,
}

pub async fn save(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    ApiJson(req): ApiJson<SaveTemplate>,
) -> AppResult<Json<Detail>> {
    let db = &state.db;
    store::expire_stale(db, teacher.id(), stale_after(&state)).await?;
    let row = store::owned(db, id, teacher.id()).await?;
    if row.status == STATUS_EXTRACTING {
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
    let row = store::owned(db, id, teacher.id()).await?;
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
    let material = extract::source(db, teacher.id(), material_id).await?;
    let Some(row) = store::restart(db, id).await? else {
        return Err(AppError::Conflict(
            "只有讀取失敗的版型可以重試。".to_string(),
        ));
    };
    let detail = store::detail(db, id).await?;
    extract::spawn(&state, &row, material.text);
    log_usage(
        db,
        teacher.id(),
        "worksheet_template_retried",
        json!({ "template_id": id, "run": row.extract_run }),
    )
    .await?;
    Ok((StatusCode::ACCEPTED, Json(detail)))
}

pub async fn delete(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<Json<serde_json::Value>> {
    let db = &state.db;
    store::owned(db, id, teacher.id()).await?;
    store::delete(db, id).await?;
    log_usage(
        db,
        teacher.id(),
        "worksheet_template_deleted",
        json!({ "template_id": id }),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}
