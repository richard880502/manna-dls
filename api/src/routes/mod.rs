//! HTTP routes (contract: docs/API.md).

pub mod admin;
pub mod core;
pub mod feedback;
pub mod lessons;
pub mod materials;
pub mod preferences;
pub mod remote;
pub mod runs;
pub mod suggestions;
pub mod units;

use axum::extract::{DefaultBodyLimit, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{delete, get, post, put};
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
        .route("/preferences/questions", get(preferences::questions))
        .route(
            "/me/preferences",
            get(preferences::get_mine).put(preferences::put_mine),
        )
        .route("/units", get(units::list).post(units::create))
        .route(
            "/units/class-profile-default",
            get(units::get_class_profile_default),
        )
        .route("/units/{id}", get(units::get_one).put(units::update))
        .route(
            "/units/{id}/materials",
            get(materials::list).post(materials::create),
        )
        .route("/units/{id}/materials/{mid}", delete(materials::remove))
        .route(
            "/units/{id}/material-focuses",
            post(materials::material_focuses),
        )
        .route("/core/questions", get(core::questions))
        .route("/units/{id}/core", get(core::get_core))
        .route("/units/{id}/core/draft", put(core::save_draft))
        .route("/units/{id}/core/complete", post(core::complete))
        .route("/core/assist", post(core::assist))
        .route(
            "/units/{id}/lessons",
            get(lessons::list).post(lessons::create),
        )
        .route("/lessons/{id}", get(lessons::detail).put(lessons::update))
        .route("/lessons/{id}/versions", post(lessons::create_version))
        .route("/lessons/{id}/core-links", put(lessons::set_core_links))
        .route("/siop", get(lessons::list_siop))
        .route("/lessons/{id}/siop", put(lessons::set_siop))
        .route("/lessons/{id}/suggestions", post(suggestions::suggest))
        .route("/ai/{interaction_id}/decision", post(feedback::decision))
        .route("/scaffold-events", post(feedback::scaffold_event))
        .route("/teacher-cases", get(feedback::teacher_cases))
        // Teach & Record (Phase 1.2)
        .route("/runs", get(runs::list_mine))
        .route(
            "/lessons/{id}/runs",
            get(runs::list_for_lesson).post(runs::start),
        )
        .route("/runs/{id}", get(runs::detail))
        .route("/runs/{id}/end", post(runs::end))
        .route("/runs/{id}/events/sync", post(runs::sync))
        .route(
            "/runs/{id}/events/{device_event_id}/review",
            put(runs::review),
        )
        .route("/runs/{id}/review/complete", post(runs::complete_review))
        .route(
            "/runs/{id}/pairing",
            post(runs::create_pairing).delete(runs::revoke_pairing),
        )
        // phone remote (run-scoped token, no teacher login)
        .route("/remote/pair", post(remote::pair))
        .route("/remote/run", get(remote::get_run))
        .route("/remote/events/sync", post(remote::sync))
        // PI
        .route("/admin/invites", post(admin::create_invite))
        .route(
            "/admin/settings",
            get(admin::get_settings).put(admin::put_settings),
        )
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
    (
        status,
        Json(json!({ "ok": db, "db": db, "commit": running_commit() })),
    )
}

/// Git commit of the running build. Coolify provides `SOURCE_COMMIT` at runtime;
/// CI polls this to confirm a deployment actually went live.
fn running_commit() -> String {
    std::env::var("SOURCE_COMMIT")
        .ok()
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

async fn public_config(State(state): State<AppState>) -> AppResult<Json<serde_json::Value>> {
    let c = &state.config;
    Ok(Json(json!({
        "demo_mode": c.demo_mode,
        "ai_enabled": c.ai_enabled(),
        "ai_provider": c.ai_provider.as_str(),
        "ai_model": c.ai_model,
        "route_choice_enabled": crate::settings::route_choice_enabled(&state.db).await?,
    })))
}

// ---------------------------------------------------------------- shared lookups

#[derive(Debug, sqlx::FromRow)]
pub struct UnitRef {
    pub id: i64,
    pub title: String,
    pub class_profile: String,
}

/// The unit if it exists and belongs to `teacher_id`; otherwise 404 (no existence leak).
pub async fn owned_unit(db: &PgPool, unit_id: i64, teacher_id: i64) -> AppResult<UnitRef> {
    sqlx::query_as(
        "SELECT id, title, class_profile FROM units
          WHERE id = $1 AND teacher_id = $2 AND deleted_at IS NULL",
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
    pub unit_class_profile: String,
    pub my_example: String,
}

impl LessonRef {
    pub fn effective_class_profile(&self) -> String {
        effective_class_profile(&self.class_description, &self.unit_class_profile)
    }
}

/// The lesson's class description when it has one, otherwise the unit's class profile.
pub fn effective_class_profile(class_description: &str, unit_class_profile: &str) -> String {
    if class_description.trim().is_empty() {
        unit_class_profile.trim().to_string()
    } else {
        class_description.trim().to_string()
    }
}

pub async fn owned_lesson(db: &PgPool, lesson_id: i64, teacher_id: i64) -> AppResult<LessonRef> {
    sqlx::query_as(
        "SELECT l.id, l.unit_id, l.title, u.title AS unit_title, l.class_description,
                u.class_profile AS unit_class_profile, l.my_example
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

/// Appends a `usage_logs` row.
pub async fn log_usage<'e, E>(
    executor: E,
    teacher_id: i64,
    event: &str,
    payload: serde_json::Value,
) -> sqlx::Result<()>
where
    E: sqlx::PgExecutor<'e>,
{
    sqlx::query("INSERT INTO usage_logs (teacher_id, event, payload) VALUES ($1, $2, $3)")
        .bind(teacher_id)
        .bind(event)
        .bind(payload)
        .execute(executor)
        .await?;
    Ok(())
}

/// Text shown to the model for an optional teacher field.
pub fn or_blank(s: &str) -> String {
    if s.trim().is_empty() {
        "（尚未填寫）".to_string()
    } else {
        s.to_string()
    }
}

/// Clips to `max` characters, adding an ellipsis when something was cut.
pub fn clip_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_class_profile_prefers_the_lesson() {
        assert_eq!(effective_class_profile("  ", "高二普通班"), "高二普通班");
        assert_eq!(
            effective_class_profile("今天只有半班", "高二普通班"),
            "今天只有半班"
        );
        assert_eq!(effective_class_profile("", ""), "");
    }
}
