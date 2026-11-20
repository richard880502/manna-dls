//! Teacher decisions on AI output, scaffold events, and "其他老師怎麼教" (teacher cases).

use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::audit::scaffold_id;
use crate::auth::AuthTeacher;
use crate::error::{ApiJson, ApiPath, ApiQuery, AppError, AppResult};
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct DecisionRequest {
    pub item_index: Option<i32>,
    pub decision: String,
    pub revised_content: Option<String>,
    pub reason: Option<String>,
    pub reason_note: Option<String>,
    pub thumbs: Option<String>,
}

pub async fn decision(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(interaction_id): ApiPath<i64>,
    ApiJson(req): ApiJson<DecisionRequest>,
) -> AppResult<Json<Value>> {
    if !matches!(
        req.decision.as_str(),
        "adopt" | "revise" | "reject" | "answer" | "skip"
    ) {
        return Err(AppError::bad_request("判斷類型不正確。"));
    }
    let reason = req.reason.filter(|r| !r.is_empty());
    if let Some(r) = &reason {
        if !matches!(
            r.as_str(),
            "source_problem"
                | "curriculum_mismatch"
                | "not_feasible"
                | "unclear"
                | "better_idea"
                | "other"
        ) {
            return Err(AppError::bad_request("理由選項不正確。"));
        }
    }
    let thumbs = req.thumbs.filter(|t| !t.is_empty());
    if let Some(t) = &thumbs {
        if !matches!(t.as_str(), "up" | "down") {
            return Err(AppError::bad_request("👍／👎 的值不正確。"));
        }
    }
    let item_index = req.item_index.unwrap_or(0);
    if item_index < 0 {
        return Err(AppError::bad_request("item_index 不可為負數。"));
    }
    let owned: Option<i64> =
        sqlx::query_scalar("SELECT id FROM ai_interactions WHERE id = $1 AND teacher_id = $2")
            .bind(interaction_id)
            .bind(teacher.id())
            .fetch_optional(&state.db)
            .await?;
    if owned.is_none() {
        return Err(AppError::not_found("找不到這筆 AI 建議。"));
    }
    sqlx::query(
        "INSERT INTO teacher_decisions
           (ai_interaction_id, teacher_id, item_index, decision, revised_content, reason, reason_note, thumbs)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(interaction_id)
    .bind(teacher.id())
    .bind(item_index)
    .bind(&req.decision)
    .bind(&req.revised_content)
    .bind(&reason)
    .bind(&req.reason_note)
    .bind(&thumbs)
    .execute(&state.db)
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Debug, Deserialize)]
pub struct ScaffoldEventRequest {
    pub scaffold_code: String,
    pub action: String,
    pub trigger: Option<String>,
    pub dwell_ms: Option<i64>,
}

pub async fn scaffold_event(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiJson(req): ApiJson<ScaffoldEventRequest>,
) -> AppResult<Json<Value>> {
    if !matches!(req.action.as_str(), "show" | "expand" | "use" | "skip") {
        return Err(AppError::bad_request("鷹架動作不正確。"));
    }
    let dwell_ms = match req.dwell_ms {
        None => None,
        Some(ms) if (0..=i64::from(i32::MAX)).contains(&ms) => Some(ms as i32),
        Some(_) => return Err(AppError::bad_request("dwell_ms 超出範圍。")),
    };
    let scaffold = scaffold_id(&state.db, &req.scaffold_code)
        .await?
        .ok_or_else(|| AppError::bad_request(format!("鷹架編號 {} 不存在。", req.scaffold_code)))?;
    sqlx::query(
        "INSERT INTO scaffold_events (teacher_id, scaffold_id, trigger, action, dwell_ms)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(teacher.id())
    .bind(scaffold)
    .bind(req.trigger.unwrap_or_default())
    .bind(&req.action)
    .bind(dwell_ms)
    .execute(&state.db)
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Debug, Deserialize)]
pub struct CaseQuery {
    pub q: Option<String>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct TeacherCase {
    pub case_code: String,
    pub school_track: String,
    pub grade: String,
    pub topic: String,
    pub summary: String,
    pub details: String,
}

pub(crate) fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// Approved cases for the teacher's track (or `both`), optionally filtered by keyword.
pub async fn teacher_cases(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiQuery(query): ApiQuery<CaseQuery>,
) -> AppResult<Json<Vec<TeacherCase>>> {
    let pattern = query
        .q
        .map(|q| q.trim().to_string())
        .filter(|q| !q.is_empty())
        .map(|q| format!("%{}%", escape_like(&q)));
    let rows = sqlx::query_as(
        "SELECT case_code, school_track, grade, topic, summary, details
           FROM teacher_cases
          WHERE approved AND school_track IN ($1, 'both')
            AND ($2::text IS NULL
                 OR topic ILIKE $2 OR summary ILIKE $2 OR details ILIKE $2 OR grade ILIKE $2
                 OR case_code ILIKE $2)
          ORDER BY case_code",
    )
    .bind(teacher.track())
    .bind(pattern)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

#[cfg(test)]
mod tests {
    use super::escape_like;

    #[test]
    fn like_wildcards_are_escaped() {
        assert_eq!(escape_like("50%_a\\b"), "50\\%\\_a\\\\b");
    }
}
