//! Lessons (課例), lesson plan versions, CoRe links and SIOP selection (spec §4.2).

use std::collections::HashSet;

use axum::extract::State;
use axum::Json;
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};

use crate::ai::schemas::IcapLevel;
use crate::auth::AuthTeacher;
use crate::error::{ApiJson, ApiPath, ApiQuery, AppError, AppResult};
use crate::routes::{owned_lesson, owned_unit, resolved_profile, ProfileSource, ResolvedProfile};
use crate::AppState;

pub const MAX_CORE_LINKS: usize = 3;
/// Upper bound for 我的範例 (same as pasted unit materials).
pub const MAX_MY_EXAMPLE_CHARS: usize = 50_000;
/// Teacher-entered step durations above this are rejected as typos.
pub const MAX_STEP_MINUTES: i32 = 600;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Lesson {
    pub id: i64,
    pub unit_id: i64,
    pub title: String,
    pub lesson_date: Option<NaiveDate>,
    pub class_description: String,
    pub created_at: DateTime<Utc>,
    pub my_example: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct LessonStep {
    pub position: i32,
    #[serde(default)]
    pub activity_type: String,
    pub icap_level: Option<String>,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub duration_min: Option<i32>,
    #[serde(default)]
    pub support_note: String,
    #[serde(default)]
    pub extension_note: String,
}

#[derive(Debug, Serialize)]
pub struct LessonVersion {
    pub id: i64,
    pub version_no: i32,
    pub objective: String,
    pub steps: Vec<LessonStep>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct LessonDetail {
    #[serde(flatten)]
    pub lesson: Lesson,
    pub latest_version: Option<LessonVersion>,
    pub versions_count: i64,
    pub core_focus_ids: Vec<i64>,
    pub siop_feature_ids: Vec<i64>,
    pub previous_next_change: Option<String>,
    /// The class profile Manna would use: the lesson's class_description when non-empty,
    /// otherwise (with `?class_id=`) the class's profile, otherwise the unit's class_profile,
    /// otherwise (with `?class_id=`) the preference answer P6.
    pub effective_class_profile: String,
    /// Which of those it is: `lesson`, `class`, `unit`, `preference` or `none`.
    pub class_profile_source: ProfileSource,
}

/// `?class_id=`: the class the teacher is working in (optional).
#[derive(Debug, Default, Deserialize)]
pub struct ClassQuery {
    #[serde(default)]
    pub class_id: Option<i64>,
}

const LESSON_COLS: &str =
    "id, unit_id, title, lesson_date, class_description, created_at, my_example";

pub async fn list(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(unit_id): ApiPath<i64>,
) -> AppResult<Json<Vec<Lesson>>> {
    owned_unit(&state.db, unit_id, teacher.id()).await?;
    let lessons = sqlx::query_as(&format!(
        "SELECT {LESSON_COLS} FROM lessons WHERE unit_id = $1 AND deleted_at IS NULL
          ORDER BY lesson_date NULLS LAST, created_at, id"
    ))
    .bind(unit_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(lessons))
}

#[derive(Debug, Deserialize)]
pub struct CreateLesson {
    pub title: String,
    pub lesson_date: Option<String>,
    pub class_description: Option<String>,
    #[serde(default)]
    pub my_example: Option<String>,
}

fn parse_date(d: Option<&str>) -> AppResult<Option<NaiveDate>> {
    match d.map(str::trim) {
        None | Some("") => Ok(None),
        Some(d) => NaiveDate::parse_from_str(d, "%Y-%m-%d")
            .map(Some)
            .map_err(|_| AppError::bad_request("日期格式需為 YYYY-MM-DD。")),
    }
}

fn check_my_example(s: &str) -> AppResult<()> {
    if s.chars().count() > MAX_MY_EXAMPLE_CHARS {
        Err(AppError::bad_request(format!(
            "我的範例最多 {MAX_MY_EXAMPLE_CHARS} 字。"
        )))
    } else {
        Ok(())
    }
}

pub async fn create(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(unit_id): ApiPath<i64>,
    ApiJson(req): ApiJson<CreateLesson>,
) -> AppResult<Json<Lesson>> {
    owned_unit(&state.db, unit_id, teacher.id()).await?;
    let title = req.title.trim();
    if title.is_empty() {
        return Err(AppError::bad_request("請輸入課例名稱。"));
    }
    let lesson_date = parse_date(req.lesson_date.as_deref())?;
    let my_example = req.my_example.unwrap_or_default();
    check_my_example(&my_example)?;
    let lesson = sqlx::query_as(&format!(
        "INSERT INTO lessons (unit_id, title, lesson_date, class_description, my_example)
         VALUES ($1, $2, $3, $4, $5) RETURNING {LESSON_COLS}"
    ))
    .bind(unit_id)
    .bind(title)
    .bind(lesson_date)
    .bind(req.class_description.unwrap_or_default())
    .bind(&my_example)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(lesson))
}

#[derive(Debug, sqlx::FromRow)]
struct VersionRow {
    id: i64,
    version_no: i32,
    objective: String,
    created_at: DateTime<Utc>,
}

pub async fn latest_version(
    db: &sqlx::PgPool,
    lesson_id: i64,
) -> sqlx::Result<Option<LessonVersion>> {
    let row: Option<VersionRow> = sqlx::query_as(
        "SELECT id, version_no, objective, created_at FROM lesson_versions
          WHERE lesson_id = $1 ORDER BY version_no DESC LIMIT 1",
    )
    .bind(lesson_id)
    .fetch_optional(db)
    .await?;
    let Some(v) = row else { return Ok(None) };
    let steps = sqlx::query_as(
        "SELECT position, activity_type, icap_level, content, duration_min, support_note,
                extension_note
           FROM lesson_steps WHERE lesson_version_id = $1 ORDER BY position",
    )
    .bind(v.id)
    .fetch_all(db)
    .await?;
    Ok(Some(LessonVersion {
        id: v.id,
        version_no: v.version_no,
        objective: v.objective,
        steps,
        created_at: v.created_at,
    }))
}

pub async fn linked_focus_ids(db: &sqlx::PgPool, lesson_id: i64) -> sqlx::Result<Vec<i64>> {
    sqlx::query_scalar(
        "SELECT core_focus_id FROM lesson_core_links
          WHERE lesson_id = $1 AND removed_at IS NULL ORDER BY id",
    )
    .bind(lesson_id)
    .fetch_all(db)
    .await
}

pub async fn selected_siop_ids(db: &sqlx::PgPool, lesson_id: i64) -> sqlx::Result<Vec<i64>> {
    sqlx::query_scalar(
        "SELECT siop_feature_id FROM lesson_siop_focus
          WHERE lesson_id = $1 AND removed_at IS NULL ORDER BY id",
    )
    .bind(lesson_id)
    .fetch_all(db)
    .await
}

/// Loads the detail of a lesson whose ownership was already checked.
async fn load_detail(
    db: &sqlx::PgPool,
    id: i64,
    teacher_id: i64,
    class_id: Option<i64>,
    unit_class_profile: &str,
) -> AppResult<LessonDetail> {
    let lesson: Lesson =
        sqlx::query_as(&format!("SELECT {LESSON_COLS} FROM lessons WHERE id = $1"))
            .bind(id)
            .fetch_one(db)
            .await?;
    let versions_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM lesson_versions WHERE lesson_id = $1")
            .bind(id)
            .fetch_one(db)
            .await?;
    let ResolvedProfile { text, source, .. } = resolved_profile(
        db,
        teacher_id,
        class_id,
        &lesson.class_description,
        unit_class_profile,
    )
    .await?;
    Ok(LessonDetail {
        lesson,
        latest_version: latest_version(db, id).await?,
        versions_count,
        core_focus_ids: linked_focus_ids(db, id).await?,
        siop_feature_ids: selected_siop_ids(db, id).await?,
        // Phase 2 (「下次要改」 carry-forward).
        previous_next_change: None,
        effective_class_profile: text,
        class_profile_source: source,
    })
}

pub async fn detail(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    ApiQuery(q): ApiQuery<ClassQuery>,
) -> AppResult<Json<LessonDetail>> {
    let r = owned_lesson(&state.db, id, teacher.id()).await?;
    Ok(Json(
        load_detail(
            &state.db,
            id,
            teacher.id(),
            q.class_id,
            &r.unit_class_profile,
        )
        .await?,
    ))
}

/// `Some(None)` for an explicit JSON null, `None` when the field is absent.
fn nullable<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

#[derive(Debug, Deserialize)]
pub struct UpdateLesson {
    #[serde(default)]
    pub title: Option<String>,
    /// Absent: unchanged; null or "": cleared; "YYYY-MM-DD": set.
    #[serde(default, deserialize_with = "nullable")]
    pub lesson_date: Option<Option<String>>,
    #[serde(default)]
    pub class_description: Option<String>,
    /// Overwritten in place (not versioned): clearing it deletes the text (D5).
    #[serde(default)]
    pub my_example: Option<String>,
}

pub async fn update(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    ApiQuery(q): ApiQuery<ClassQuery>,
    ApiJson(req): ApiJson<UpdateLesson>,
) -> AppResult<Json<LessonDetail>> {
    let r = owned_lesson(&state.db, id, teacher.id()).await?;
    crate::routes::class_context(&state.db, q.class_id, teacher.id()).await?;
    let title = match req.title.as_deref().map(str::trim) {
        Some("") => return Err(AppError::bad_request("請輸入課例名稱。")),
        t => t.map(str::to_string),
    };
    let date = match &req.lesson_date {
        None => None,
        Some(d) => Some(parse_date(d.as_deref())?),
    };
    if let Some(e) = &req.my_example {
        check_my_example(e)?;
    }
    sqlx::query(
        "UPDATE lessons
            SET title = COALESCE($2, title),
                lesson_date = CASE WHEN $3 THEN $4 ELSE lesson_date END,
                class_description = COALESCE($5, class_description),
                my_example = COALESCE($6, my_example)
          WHERE id = $1",
    )
    .bind(id)
    .bind(&title)
    .bind(date.is_some())
    .bind(date.flatten())
    .bind(&req.class_description)
    .bind(&req.my_example)
    .execute(&state.db)
    .await?;
    Ok(Json(
        load_detail(
            &state.db,
            id,
            teacher.id(),
            q.class_id,
            &r.unit_class_profile,
        )
        .await?,
    ))
}

#[derive(Debug, Deserialize)]
pub struct CreateVersion {
    #[serde(default)]
    pub objective: String,
    #[serde(default)]
    pub steps: Vec<LessonStep>,
}

fn validate_steps(steps: &[LessonStep]) -> AppResult<()> {
    let mut positions = HashSet::new();
    for s in steps {
        if !positions.insert(s.position) {
            return Err(AppError::bad_request(format!(
                "步驟順序 {} 重複。",
                s.position
            )));
        }
        if let Some(level) = &s.icap_level {
            if serde_json::from_value::<IcapLevel>(Value::String(level.clone())).is_err() {
                return Err(AppError::bad_request(format!(
                    "ICAP 層次「{level}」不正確。"
                )));
            }
        }
        if let Some(d) = s.duration_min {
            if !(0..=MAX_STEP_MINUTES).contains(&d) {
                return Err(AppError::bad_request(format!(
                    "步驟 {} 的預估時間需介於 0 到 {MAX_STEP_MINUTES} 分鐘。",
                    s.position
                )));
            }
        }
    }
    Ok(())
}

pub async fn create_version(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    ApiJson(req): ApiJson<CreateVersion>,
) -> AppResult<Json<LessonVersion>> {
    owned_lesson(&state.db, id, teacher.id()).await?;
    let mut steps = req.steps;
    for s in &mut steps {
        if s.icap_level.as_deref().is_some_and(|l| l.trim().is_empty()) {
            s.icap_level = None;
        }
    }
    validate_steps(&steps)?;
    steps.sort_by_key(|s| s.position);

    let mut tx = state.db.begin().await?;
    // Serialize concurrent saves of the same lesson so version_no stays gapless and unique.
    sqlx::query("SELECT id FROM lessons WHERE id = $1 FOR UPDATE")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let v: VersionRow = sqlx::query_as(
        "INSERT INTO lesson_versions (lesson_id, version_no, objective)
         VALUES ($1, (SELECT COALESCE(MAX(version_no), 0) + 1 FROM lesson_versions WHERE lesson_id = $1), $2)
         RETURNING id, version_no, objective, created_at",
    )
    .bind(id)
    .bind(&req.objective)
    .fetch_one(&mut *tx)
    .await?;
    for s in &steps {
        sqlx::query(
            "INSERT INTO lesson_steps (lesson_version_id, position, activity_type, icap_level, content,
                                       duration_min, support_note, extension_note)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(v.id)
        .bind(s.position)
        .bind(&s.activity_type)
        .bind(&s.icap_level)
        .bind(&s.content)
        .bind(s.duration_min)
        .bind(&s.support_note)
        .bind(&s.extension_note)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(Json(LessonVersion {
        id: v.id,
        version_no: v.version_no,
        objective: v.objective,
        steps,
        created_at: v.created_at,
    }))
}

fn dedup(ids: &[i64]) -> Vec<i64> {
    let mut seen = HashSet::new();
    ids.iter().copied().filter(|i| seen.insert(*i)).collect()
}

#[derive(Debug, Deserialize)]
pub struct CoreLinks {
    pub focus_ids: Vec<i64>,
}

pub async fn set_core_links(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    ApiJson(req): ApiJson<CoreLinks>,
) -> AppResult<Json<Value>> {
    let lesson = owned_lesson(&state.db, id, teacher.id()).await?;
    let ids = dedup(&req.focus_ids);
    if ids.len() > MAX_CORE_LINKS {
        return Err(AppError::bad_request("每節課最多連結 3 個 CoRe 學習重點。"));
    }
    let valid: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM core_focuses f JOIN core_versions v ON v.id = f.core_version_id
          WHERE f.id = ANY($1) AND v.unit_id = $2 AND f.deleted_at IS NULL",
    )
    .bind(&ids)
    .bind(lesson.unit_id)
    .fetch_one(&state.db)
    .await?;
    if valid != ids.len() as i64 {
        return Err(AppError::bad_request(
            "只能連結同一個單元的 CoRe 學習重點。",
        ));
    }
    let mut tx = state.db.begin().await?;
    sqlx::query("SELECT id FROM lessons WHERE id = $1 FOR UPDATE")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "UPDATE lesson_core_links SET removed_at = now()
          WHERE lesson_id = $1 AND removed_at IS NULL AND NOT (core_focus_id = ANY($2))",
    )
    .bind(id)
    .bind(&ids)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO lesson_core_links (lesson_id, core_focus_id)
         SELECT $1, f FROM unnest($2::bigint[]) WITH ORDINALITY AS t(f, ord)
          WHERE NOT EXISTS (SELECT 1 FROM lesson_core_links
                             WHERE lesson_id = $1 AND core_focus_id = t.f AND removed_at IS NULL)
          ORDER BY ord",
    )
    .bind(id)
    .bind(&ids)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(json!({ "focus_ids": ids })))
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct SiopFeature {
    pub id: i64,
    pub internal_id: String,
    pub component: String,
    pub summary: String,
}

/// Approved SIOP features (latest approved version of each internal_id).
pub async fn list_siop(
    State(state): State<AppState>,
    _teacher: AuthTeacher,
) -> AppResult<Json<Vec<SiopFeature>>> {
    let rows = sqlx::query_as(
        "SELECT id, internal_id, component, summary FROM (
            SELECT DISTINCT ON (internal_id) id, internal_id, component, summary
              FROM siop_features WHERE approved_at IS NOT NULL
             ORDER BY internal_id, version DESC) s
          ORDER BY internal_id",
    )
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

#[derive(Debug, Deserialize)]
pub struct SiopSelection {
    pub siop_feature_ids: Vec<i64>,
}

pub async fn set_siop(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    ApiJson(req): ApiJson<SiopSelection>,
) -> AppResult<Json<Value>> {
    owned_lesson(&state.db, id, teacher.id()).await?;
    let ids = dedup(&req.siop_feature_ids);
    let valid: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM siop_features WHERE id = ANY($1) AND approved_at IS NOT NULL",
    )
    .bind(&ids)
    .fetch_one(&state.db)
    .await?;
    if valid != ids.len() as i64 {
        return Err(AppError::bad_request("只能選用已核准的 SIOP 提示。"));
    }
    let mut tx = state.db.begin().await?;
    sqlx::query("SELECT id FROM lessons WHERE id = $1 FOR UPDATE")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "UPDATE lesson_siop_focus SET removed_at = now()
          WHERE lesson_id = $1 AND removed_at IS NULL AND NOT (siop_feature_id = ANY($2))",
    )
    .bind(id)
    .bind(&ids)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO lesson_siop_focus (lesson_id, siop_feature_id)
         SELECT $1, f FROM unnest($2::bigint[]) WITH ORDINALITY AS t(f, ord)
          WHERE NOT EXISTS (SELECT 1 FROM lesson_siop_focus
                             WHERE lesson_id = $1 AND siop_feature_id = t.f AND removed_at IS NULL)
          ORDER BY ord",
    )
    .bind(id)
    .bind(&ids)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(json!({ "siop_feature_ids": ids })))
}
