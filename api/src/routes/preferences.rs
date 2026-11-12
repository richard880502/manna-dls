//! 初次使用設定與「我的備課偏好」(docs/ONBOARDING.md §2, §5).

use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::auth::AuthTeacher;
use crate::error::{ApiJson, AppError, AppResult};
use crate::preferences::{
    self, PreferenceAnswers, PreferenceQuestion, Preferences, MAX_CLASS_PROFILE_CHARS,
    PREFERENCE_QUESTIONS, QUESTION_SET_VERSION,
};
use crate::AppState;

#[derive(Serialize)]
pub struct QuestionSet {
    pub question_set_version: i32,
    pub questions: &'static [PreferenceQuestion],
}

pub async fn questions() -> Json<QuestionSet> {
    Json(QuestionSet {
        question_set_version: QUESTION_SET_VERSION,
        questions: &PREFERENCE_QUESTIONS,
    })
}

/// `Preferences | null` — null means onboarding has not been done (or skipped) yet.
pub async fn get_mine(
    State(state): State<AppState>,
    teacher: AuthTeacher,
) -> AppResult<Json<Option<Preferences>>> {
    Ok(Json(preferences::latest(&state.db, teacher.id()).await?))
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PrefSource {
    Onboarding,
    Settings,
}

impl PrefSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::Onboarding => "onboarding",
            Self::Settings => "settings",
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct PutPreferences {
    pub answers: PreferenceAnswers,
    pub skipped: bool,
    pub source: PrefSource,
}

/// Saves a new version (never overwrites older ones).
pub async fn put_mine(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiJson(req): ApiJson<PutPreferences>,
) -> AppResult<Json<Preferences>> {
    let answers = req.answers.normalized();
    if answers.class_profile.chars().count() > MAX_CLASS_PROFILE_CHARS {
        return Err(AppError::bad_request(format!(
            "班級特性最多 {MAX_CLASS_PROFILE_CHARS} 字。"
        )));
    }
    let mut tx = state.db.begin().await?;
    let saved = preferences::insert_version(
        &mut tx,
        teacher.id(),
        &answers,
        req.skipped,
        req.source.as_str(),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(saved))
}

/// Validates an optional class profile from a unit request.
pub fn check_class_profile(s: &str) -> AppResult<()> {
    if s.chars().count() > MAX_CLASS_PROFILE_CHARS {
        Err(AppError::bad_request(format!(
            "班級特性最多 {MAX_CLASS_PROFILE_CHARS} 字。"
        )))
    } else {
        Ok(())
    }
}

/// Summary of the preference fields that change AI behaviour, for the `context` list.
pub fn preference_summary(a: &PreferenceAnswers, extra: &[&str]) -> String {
    let mut parts = vec![format!("建議長度：{}", preferences::detail_label(a.detail))];
    parts.extend(extra.iter().map(|s| s.to_string()));
    parts.join("；")
}

/// JSON copy of the answers used in AI input snapshots.
pub fn snapshot(a: &PreferenceAnswers) -> Value {
    json!({ "detail": a.detail, "hardest": a.hardest })
}
