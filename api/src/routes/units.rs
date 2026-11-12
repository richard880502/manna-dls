//! Units (單元).

use axum::extract::State;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::auth::AuthTeacher;
use crate::error::{ApiJson, ApiPath, AppError, AppResult};
use crate::AppState;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Unit {
    pub id: i64,
    pub title: String,
    pub created_at: DateTime<Utc>,
    pub core_status: String,
    pub lesson_count: i64,
}

const UNIT_SELECT: &str = "
    SELECT u.id, u.title, u.created_at,
           COALESCE((SELECT CASE WHEN cv.completed_at IS NOT NULL THEN 'completed' ELSE 'draft' END
                       FROM core_versions cv
                      WHERE cv.unit_id = u.id AND cv.phase = 'pre'
                      ORDER BY cv.version_no DESC LIMIT 1), 'none') AS core_status,
           (SELECT COUNT(*) FROM lessons l WHERE l.unit_id = u.id AND l.deleted_at IS NULL) AS lesson_count
      FROM units u
     WHERE u.teacher_id = $1 AND u.deleted_at IS NULL";

pub async fn list(
    State(state): State<AppState>,
    teacher: AuthTeacher,
) -> AppResult<Json<Vec<Unit>>> {
    let units = sqlx::query_as(&format!(
        "{UNIT_SELECT} ORDER BY u.created_at DESC, u.id DESC"
    ))
    .bind(teacher.id())
    .fetch_all(&state.db)
    .await?;
    Ok(Json(units))
}

async fn load(state: &AppState, teacher_id: i64, unit_id: i64) -> AppResult<Unit> {
    sqlx::query_as(&format!("{UNIT_SELECT} AND u.id = $2"))
        .bind(teacher_id)
        .bind(unit_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::not_found("找不到這個單元。"))
}

pub async fn get_one(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<Json<Unit>> {
    Ok(Json(load(&state, teacher.id(), id).await?))
}

#[derive(Deserialize)]
pub struct CreateUnit {
    pub title: String,
}

pub async fn create(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiJson(req): ApiJson<CreateUnit>,
) -> AppResult<Json<Unit>> {
    let title = req.title.trim();
    if title.is_empty() {
        return Err(AppError::bad_request("請輸入單元名稱。"));
    }
    let id: i64 =
        sqlx::query_scalar("INSERT INTO units (teacher_id, title) VALUES ($1, $2) RETURNING id")
            .bind(teacher.id())
            .bind(title)
            .fetch_one(&state.db)
            .await?;
    Ok(Json(load(&state, teacher.id(), id).await?))
}
