//! `DELETE /api/ai-jobs/{id}`: leave the queue (docs/API.md「AI 工作佇列」).

use axum::extract::State;
use axum::http::StatusCode;

use crate::auth::AuthTeacher;
use crate::error::{ApiPath, AppResult};
use crate::AppState;

/// Cancels a job of the caller that has not started. 404 for another teacher's job, 409 once
/// it runs or ended. What the job was for is closed: a prep turn fails with 「已取消排隊。」, a
/// worksheet run stops as a run that failed before starting.
pub async fn cancel(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<StatusCode> {
    let job = super::cancel(&state.db, id, teacher.id()).await?;
    super::handlers::cancelled(&state, &job).await;
    Ok(StatusCode::NO_CONTENT)
}
