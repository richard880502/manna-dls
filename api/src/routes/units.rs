//! Units (單元), including the prep route (備課菜單) and the unit-level class profile.

use axum::extract::State;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::auth::AuthTeacher;
use crate::error::{ApiJson, ApiPath, AppError, AppResult};
use crate::preferences::{self, PrepRoute};
use crate::routes::log_usage;
use crate::routes::preferences::check_class_profile;
use crate::settings;
use crate::AppState;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Unit {
    pub id: i64,
    pub title: String,
    pub created_at: DateTime<Utc>,
    pub core_status: String,
    pub lesson_count: i64,
    pub prep_route: String,
    pub class_profile: String,
    /// Where the class profile came from when the unit was created (`teacher`, `previous_unit`,
    /// `preference`, `none`); NULL for units created before this was recorded.
    pub class_profile_source: Option<String>,
}

const UNIT_SELECT: &str = "
    SELECT u.id, u.title, u.created_at,
           COALESCE((SELECT CASE WHEN cv.completed_at IS NOT NULL THEN 'completed' ELSE 'draft' END
                       FROM core_versions cv
                      WHERE cv.unit_id = u.id AND cv.phase = 'pre'
                      ORDER BY cv.version_no DESC LIMIT 1), 'none') AS core_status,
           (SELECT COUNT(*) FROM lessons l WHERE l.unit_id = u.id AND l.deleted_at IS NULL) AS lesson_count,
           u.prep_route, u.class_profile, u.class_profile_source
      FROM units u
     WHERE u.teacher_id = $1 AND u.deleted_at IS NULL";

pub const MSG_ROUTE_LOCKED: &str = "目前平台採固定的備課順序，無法切換備課菜單。";

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
    #[serde(default)]
    pub prep_route: Option<PrepRoute>,
    #[serde(default)]
    pub class_profile: Option<String>,
}

/// Where a new unit's class profile came from (docs/API.md「班級特性沿用」).
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClassProfileSource {
    /// Sent explicitly on create: the teacher typed or edited it.
    Teacher,
    /// Copied from the teacher's most recent unit with a non-empty profile.
    PreviousUnit,
    /// Copied from the latest preference answers (P6).
    Preference,
    /// No default was available; the profile starts empty.
    None,
}

impl ClassProfileSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Teacher => "teacher",
            Self::PreviousUnit => "previous_unit",
            Self::Preference => "preference",
            Self::None => "none",
        }
    }
}

#[derive(Debug, Serialize)]
pub struct SourceUnit {
    pub id: i64,
    pub title: String,
}

/// The class profile a new unit gets when the create request does not carry one.
#[derive(Debug, Serialize)]
pub struct ClassProfileDefault {
    pub class_profile: String,
    /// `previous_unit`, `preference` or `none` (never `teacher`).
    pub source: ClassProfileSource,
    /// The unit the profile is copied from (only with `previous_unit`).
    pub source_unit: Option<SourceUnit>,
}

/// R3 / D2: the teacher's most recently created, non-deleted unit with a non-empty class
/// profile; only a teacher without one falls back to the preference answer P6.
pub async fn class_profile_default(
    db: &sqlx::PgPool,
    teacher_id: i64,
) -> AppResult<ClassProfileDefault> {
    let previous: Option<(i64, String, String)> = sqlx::query_as(
        "SELECT id, title, class_profile FROM units
          WHERE teacher_id = $1 AND deleted_at IS NULL AND btrim(class_profile) <> ''
          ORDER BY created_at DESC, id DESC
          LIMIT 1",
    )
    .bind(teacher_id)
    .fetch_optional(db)
    .await?;
    if let Some((id, title, class_profile)) = previous {
        return Ok(ClassProfileDefault {
            class_profile: class_profile.trim().to_string(),
            source: ClassProfileSource::PreviousUnit,
            source_unit: Some(SourceUnit { id, title }),
        });
    }
    let prefs = preferences::latest_answers(db, teacher_id).await?;
    let from_prefs = prefs.class_profile.trim();
    Ok(ClassProfileDefault {
        class_profile: from_prefs.to_string(),
        source: if from_prefs.is_empty() {
            ClassProfileSource::None
        } else {
            ClassProfileSource::Preference
        },
        source_unit: None,
    })
}

/// `GET /api/units/class-profile-default`: what [`create`] uses when `class_profile` is omitted,
/// so the create form can show it, with its source, as an editable prefill.
pub async fn get_class_profile_default(
    State(state): State<AppState>,
    teacher: AuthTeacher,
) -> AppResult<Json<ClassProfileDefault>> {
    Ok(Json(class_profile_default(&state.db, teacher.id()).await?))
}

/// Defaults (docs/API.md): prep_route from the preference `start_from` (else goal); class_profile
/// from [`class_profile_default`]. An explicit value wins, an empty string too. With route
/// choice disabled the route is always goal. The profile's source is stored with the unit.
pub async fn create(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiJson(req): ApiJson<CreateUnit>,
) -> AppResult<Json<Unit>> {
    let title = req.title.trim();
    if title.is_empty() {
        return Err(AppError::bad_request("請輸入單元名稱。"));
    }
    let prefs = preferences::latest_answers(&state.db, teacher.id()).await?;
    let route = if settings::route_choice_enabled(&state.db).await? {
        req.prep_route
            .or(prefs.start_from)
            .unwrap_or(PrepRoute::Goal)
    } else {
        PrepRoute::Goal
    };
    let (class_profile, source, source_unit_id) = match req.class_profile {
        Some(c) => (c.trim().to_string(), ClassProfileSource::Teacher, None),
        None => {
            let d = class_profile_default(&state.db, teacher.id()).await?;
            (d.class_profile, d.source, d.source_unit.map(|u| u.id))
        }
    };
    check_class_profile(&class_profile)?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO units (teacher_id, title, prep_route, class_profile,
                            class_profile_source, class_profile_source_unit_id)
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
    )
    .bind(teacher.id())
    .bind(title)
    .bind(route.as_str())
    .bind(&class_profile)
    .bind(source.as_str())
    .bind(source_unit_id)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(load(&state, teacher.id(), id).await?))
}

#[derive(Deserialize)]
pub struct UpdateUnit {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub prep_route: Option<PrepRoute>,
    #[serde(default)]
    pub class_profile: Option<String>,
}

/// Updates the given fields. A prep-route change is logged to `usage_logs`
/// (`prep_route_changed`, `{unit_id, from, to}`) and refused with 409 while route choice is
/// disabled; re-sending the current route is not a change.
pub async fn update(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    ApiJson(req): ApiJson<UpdateUnit>,
) -> AppResult<Json<Unit>> {
    let title = match req.title.as_deref().map(str::trim) {
        Some("") => return Err(AppError::bad_request("請輸入單元名稱。")),
        t => t.map(str::to_string),
    };
    let class_profile = req.class_profile.as_deref().map(str::trim);
    if let Some(c) = class_profile {
        check_class_profile(c)?;
    }
    let route_enabled = settings::route_choice_enabled(&state.db).await?;

    let mut tx = state.db.begin().await?;
    let current: String = sqlx::query_scalar(
        "SELECT prep_route FROM units
          WHERE id = $1 AND teacher_id = $2 AND deleted_at IS NULL FOR UPDATE",
    )
    .bind(id)
    .bind(teacher.id())
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::not_found("找不到這個單元。"))?;
    let new_route = req
        .prep_route
        .map(PrepRoute::as_str)
        .filter(|r| *r != current);
    if new_route.is_some() && !route_enabled {
        return Err(AppError::Conflict(MSG_ROUTE_LOCKED.into()));
    }
    sqlx::query(
        "UPDATE units SET title = COALESCE($2, title), prep_route = COALESCE($3, prep_route),
                          class_profile = COALESCE($4, class_profile)
          WHERE id = $1",
    )
    .bind(id)
    .bind(&title)
    .bind(new_route)
    .bind(class_profile)
    .execute(&mut *tx)
    .await?;
    if let Some(to) = new_route {
        log_usage(
            &mut *tx,
            teacher.id(),
            "prep_route_changed",
            json!({ "unit_id": id, "from": current, "to": to }),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(Json(load(&state, teacher.id(), id).await?))
}
