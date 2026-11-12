//! HTTP routes (contract: docs/API.md).

pub mod admin;
pub mod core;
pub mod feedback;
pub mod lessons;
pub mod suggestions;
pub mod units;

use axum::extract::{DefaultBodyLimit, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde_json::json;
use sqlx::PgPool;

use crate::auth;
use crate::error::{AppError, AppResult};
use crate::AppState;

pub fn api_router() -> Router<AppState> {
    Router::new()
        // public
        .route("/health", get(health))
        .route("/config", get(public_config))
        .route("/auth/login", post(auth::login))
        .route("/auth/logout", post(auth::logout))
        // teacher
        .route("/me", get(auth::me))
        .route("/units", get(units::list).post(units::create))
        .route("/units/{id}", get(units::get_one))
        .route("/core/questions", get(core::questions))
        .route("/units/{id}/core", get(core::get_core))
        .route("/units/{id}/core/draft", put(core::save_draft))
        .route("/units/{id}/core/complete", post(core::complete))
        .route("/core/assist", post(core::assist))
        .route(
            "/units/{id}/lessons",
            get(lessons::list).post(lessons::create),
        )
        .route("/lessons/{id}", get(lessons::detail))
        .route("/lessons/{id}/versions", post(lessons::create_version))
        .route("/lessons/{id}/core-links", put(lessons::set_core_links))
        .route("/siop", get(lessons::list_siop))
        .route("/lessons/{id}/siop", put(lessons::set_siop))
        .route("/lessons/{id}/suggestions", post(suggestions::suggest))
        .route("/ai/{interaction_id}/decision", post(feedback::decision))
        .route("/scaffold-events", post(feedback::scaffold_event))
        .route("/teacher-cases", get(feedback::teacher_cases))
        // PI
        .route("/admin/invites", post(admin::create_invite))
        .route(
            "/admin/prompts",
            get(admin::list_prompts).post(admin::create_prompt),
        )
        .route(
            "/admin/knowledge/import",
            post(admin::import_knowledge).layer(DefaultBodyLimit::max(20 * 1024 * 1024)),
        )
        .route(
            "/admin/teacher-cases/import",
            post(admin::import_teacher_cases).layer(DefaultBodyLimit::max(20 * 1024 * 1024)),
        )
        .fallback(api_not_found)
}

async fn api_not_found() -> AppError {
    AppError::not_found("找不到這個 API。")
}

async fn health(State(state): State<AppState>) -> impl IntoResponse {
    let db = crate::db::ping(&state.db).await;
    let status = if db {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (status, Json(json!({ "ok": db, "db": db })))
}

async fn public_config(State(state): State<AppState>) -> impl IntoResponse {
    let c = &state.config;
    Json(json!({
        "demo_mode": c.demo_mode,
        "ai_enabled": c.ai_enabled(),
        "ai_provider": c.ai_provider.as_str(),
        "ai_model": c.ai_model,
    }))
}

// ---------------------------------------------------------------- shared lookups

#[derive(Debug, sqlx::FromRow)]
pub struct UnitRef {
    pub id: i64,
    pub title: String,
}

/// The unit if it exists and belongs to `teacher_id`; otherwise 404 (no existence leak).
pub async fn owned_unit(db: &PgPool, unit_id: i64, teacher_id: i64) -> AppResult<UnitRef> {
    sqlx::query_as(
        "SELECT id, title FROM units WHERE id = $1 AND teacher_id = $2 AND deleted_at IS NULL",
    )
    .bind(unit_id)
    .bind(teacher_id)
    .fetch_optional(db)
    .await?
    .ok_or_else(|| AppError::not_found("找不到這個單元。"))
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct LessonRef {
    pub id: i64,
    pub unit_id: i64,
    pub title: String,
    pub unit_title: String,
    pub class_description: String,
}

pub async fn owned_lesson(db: &PgPool, lesson_id: i64, teacher_id: i64) -> AppResult<LessonRef> {
    sqlx::query_as(
        "SELECT l.id, l.unit_id, l.title, u.title AS unit_title, l.class_description
           FROM lessons l JOIN units u ON u.id = l.unit_id
          WHERE l.id = $1 AND u.teacher_id = $2 AND l.deleted_at IS NULL AND u.deleted_at IS NULL",
    )
    .bind(lesson_id)
    .bind(teacher_id)
    .fetch_optional(db)
    .await?
    .ok_or_else(|| AppError::not_found("找不到這個課例。"))
}

/// Latest version of a scaffold by code.
pub async fn scaffold_id(db: &PgPool, code: &str) -> sqlx::Result<Option<i64>> {
    sqlx::query_scalar(
        "SELECT id FROM scaffolds WHERE scaffold_code = $1 ORDER BY version DESC LIMIT 1",
    )
    .bind(code)
    .fetch_optional(db)
    .await
}

pub fn track_label(track: &str) -> &'static str {
    match track {
        "vocational" => "技高",
        "both" => "普高／技高共用",
        _ => "普高",
    }
}
