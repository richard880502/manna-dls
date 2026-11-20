//! Teacher-owned teaching context and visible class-profile precedence, shared by application flows.

use serde::Serialize;
use serde_json::{json, Value};
use sqlx::PgPool;

use crate::error::{AppError, AppResult};

// ---------------------------------------------------------------- shared lookups

#[derive(Debug, sqlx::FromRow)]
pub struct UnitRef {
    pub id: i64,
    pub title: String,
    pub class_profile: String,
}

/// The unit if it exists and belongs to `teacher_id`; otherwise 404 (no existence leak).
pub async fn owned_unit(
    db: impl sqlx::PgExecutor<'_>,
    unit_id: i64,
    teacher_id: i64,
) -> AppResult<UnitRef> {
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

pub async fn owned_lesson(
    db: impl sqlx::PgExecutor<'_>,
    lesson_id: i64,
    teacher_id: i64,
) -> AppResult<LessonRef> {
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

// ---------------------------------------------------------------- class context

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ClassRef {
    pub id: i64,
    pub name: String,
    pub class_profile: String,
    pub archived: bool,
}

/// The class if it exists and belongs to `teacher_id` (archived or not); otherwise 404.
pub async fn owned_class(db: &PgPool, class_id: i64, teacher_id: i64) -> AppResult<ClassRef> {
    sqlx::query_as(
        "SELECT id, name, class_profile, (archived_at IS NOT NULL) AS archived FROM classes
          WHERE id = $1 AND teacher_id = $2",
    )
    .bind(class_id)
    .bind(teacher_id)
    .fetch_optional(db)
    .await?
    .ok_or_else(|| AppError::not_found("找不到這個班級。"))
}

/// The class a request works in (`class_id` is optional everywhere); someone else's class → 404.
pub async fn class_context(
    db: &PgPool,
    class_id: Option<i64>,
    teacher_id: i64,
) -> AppResult<Option<ClassRef>> {
    match class_id {
        Some(id) => Ok(Some(owned_class(db, id, teacher_id).await?)),
        None => Ok(None),
    }
}

/// Which text became the class profile sent to the model (docs/API.md Phase 1.4).
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProfileSource {
    /// The lesson's own class description (覆寫).
    Lesson,
    /// The profile of the class the teacher is working in.
    Class,
    /// The unit's legacy class profile.
    Unit,
    /// The preference answer P6 (only in a class context).
    Preference,
    None,
}

impl ProfileSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lesson => "lesson",
            Self::Class => "class",
            Self::Unit => "unit",
            Self::Preference => "preference",
            Self::None => "none",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedProfile {
    pub text: String,
    pub source: ProfileSource,
    /// The class context of the request, whether or not its profile was the one used.
    pub class: Option<ClassRef>,
}

impl ResolvedProfile {
    /// Label of the `AiResult.context` item. Without a class context it stays 「班級特性」; in a
    /// class context it says which profile was used.
    pub fn context_label(&self) -> String {
        let Some(class) = &self.class else {
            return "班級特性".to_string();
        };
        match self.source {
            ProfileSource::Class => format!("班級特性（{}）", class.name),
            ProfileSource::Lesson => "班級特性（本課覆寫）".to_string(),
            ProfileSource::Unit => "班級特性（單元設定）".to_string(),
            ProfileSource::Preference => "班級特性（備課偏好）".to_string(),
            ProfileSource::None => "班級特性".to_string(),
        }
    }

    /// For the logged input snapshot of an AI interaction.
    pub fn snapshot(&self) -> Value {
        json!({
            "class_id": self.class.as_ref().map(|c| c.id),
            "class_profile_source": self.source.as_str(),
        })
    }
}

/// Precedence: class profile > unit `class_profile` (legacy). The class profile only takes part
/// in a class context (`class` is Some).
///
/// A lesson-level override and preference answer P6 used to come first and last. Teachers can no
/// longer see or edit either (班級特性 lives on the class page only), so they are ignored: the
/// AI must not read a profile the teacher cannot see. Their stored values are kept, and the
/// parameters stay so callers do not change.
pub fn resolve_class_profile(
    _lesson_description: &str,
    class: Option<&ClassRef>,
    unit_profile: &str,
    _preference_profile: &str,
) -> ResolvedProfile {
    let candidates = [
        (
            ProfileSource::Class,
            class.map(|c| c.class_profile.as_str()).unwrap_or_default(),
            class.is_some(),
        ),
        (ProfileSource::Unit, unit_profile, true),
    ];
    let (source, text) = candidates
        .iter()
        .find(|(_, text, applies)| *applies && !text.trim().is_empty())
        .map(|(source, text, _)| (*source, text.trim().to_string()))
        .unwrap_or((ProfileSource::None, String::new()));
    ResolvedProfile {
        text,
        source,
        class: class.cloned(),
    }
}

/// [`resolve_class_profile`] for a request: loads the class (404 when not the teacher's).
pub async fn resolved_profile(
    db: &PgPool,
    teacher_id: i64,
    class_id: Option<i64>,
    lesson_description: &str,
    unit_profile: &str,
) -> AppResult<ResolvedProfile> {
    let class = class_context(db, class_id, teacher_id).await?;
    Ok(resolve_class_profile(
        lesson_description,
        class.as_ref(),
        unit_profile,
        "",
    ))
}

pub fn track_label(track: &str) -> &'static str {
    match track {
        "vocational" => "技高",
        "comprehensive" => "綜高",
        "both" => "各類型共用",
        _ => "普高",
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

    fn class(profile: &str) -> ClassRef {
        ClassRef {
            id: 3,
            name: "高一 3 班".into(),
            class_profile: profile.into(),
            archived: false,
        }
    }

    #[test]
    fn class_profile_precedence() {
        let c = class("三班：活潑");
        // An old lesson override no longer wins.
        let r = resolve_class_profile("今天半班", Some(&c), "單元舊描述", "問卷");
        assert_eq!(
            (r.source, r.text.as_str()),
            (ProfileSource::Class, "三班：活潑")
        );
        assert_eq!(r.context_label(), "班級特性（高一 3 班）");
        let empty = class("  ");
        let r = resolve_class_profile("", Some(&empty), "單元舊描述", "問卷");
        assert_eq!(r.source, ProfileSource::Unit);
        // P6 is no longer a fallback.
        let r = resolve_class_profile("", Some(&empty), "", " 問卷 ");
        assert_eq!(r.source, ProfileSource::None);
    }

    #[test]
    fn without_a_class_context_the_profile_is_lesson_then_unit_only() {
        let r = resolve_class_profile("", None, "單元舊描述", "問卷");
        assert_eq!(
            (r.source, r.text.as_str()),
            (ProfileSource::Unit, "單元舊描述")
        );
        assert_eq!(r.context_label(), "班級特性");
        let r = resolve_class_profile("", None, "", "問卷");
        assert_eq!((r.source, r.text.as_str()), (ProfileSource::None, ""));
    }
}
