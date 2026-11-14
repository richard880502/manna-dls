//! Classes (班級) and what each class takes from the design library (備課庫).
//! Contract: docs/API.md「Phase 1.4：班級與備課庫」.
//!
//! A class holds no student data: a name, a few labels and a free-text profile. A unit design
//! lives once in the teacher's library and can be assigned to several classes; teaching records
//! (lesson runs) belong to one class. Classes are archived, never deleted; removing a unit from a
//! class is a soft removal and keeps the runs taught there.

use axum::extract::State;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;

use crate::auth::{valid_track, AuthTeacher};
use crate::error::{ApiJson, ApiPath, ApiQuery, AppError, AppResult};
use crate::preferences;
use crate::routes::preferences::check_class_profile;
use crate::routes::units::{self, Unit};
use crate::routes::{log_usage, owned_class, owned_unit};
use crate::AppState;

pub const MAX_CLASS_NAME_CHARS: usize = 60;
pub const MAX_CLASS_LABEL_CHARS: usize = 60;
pub const MAX_CLASS_SHORT_CHARS: usize = 20;
pub const MAX_CLASS_NOTE_CHARS: usize = 500;

pub const MSG_CLASS_ARCHIVED: &str = "這個班級已經封存，請先取消封存。";

// ---------------------------------------------------------------- shapes

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ClassRow {
    pub id: i64,
    pub name: String,
    pub grade: String,
    pub school_track: String,
    pub course_label: String,
    pub class_profile: String,
    /// `teacher`, `preference` (P6, first class only), `migration` (legacy unit) or `none`.
    pub class_profile_source: String,
    pub school_year: String,
    pub term: String,
    pub position: i32,
    pub created_at: DateTime<Utc>,
    pub archived_at: Option<DateTime<Utc>>,
    /// Units currently assigned to the class.
    pub unit_count: i64,
}

/// What the home page offers as the class's next step.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NextAction {
    /// A lesson is open in this class: go back to its record console.
    ResumeRun {
        run_id: i64,
        lesson_id: i64,
        lesson_title: String,
        unit_id: i64,
        unit_title: String,
    },
    /// The first lesson (in unit order) this class has not been taught yet.
    StartLesson {
        lesson_id: i64,
        lesson_title: String,
        unit_id: i64,
        unit_title: String,
    },
    /// The next unit has no lessons yet: plan it.
    PlanUnit { unit_id: i64, unit_title: String },
    /// The class has no units yet.
    AddUnit,
}

#[derive(Debug, Serialize)]
pub struct SchoolClass {
    #[serde(flatten)]
    pub row: ClassRow,
    /// None: every lesson of every assigned unit has been taught in this class.
    pub next_action: Option<NextAction>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct LessonRefOut {
    pub id: i64,
    pub title: String,
}

/// A unit design as one class uses it.
#[derive(Debug, Serialize)]
pub struct ClassUnit {
    pub unit: Unit,
    pub position: i32,
    pub note: String,
    pub assigned_at: DateTime<Utc>,
    pub lessons_total: i64,
    /// Lessons with at least one run in this class.
    pub lessons_taught: i64,
    pub last_run_at: Option<DateTime<Utc>>,
    /// The first lesson not taught in this class yet.
    pub next_lesson: Option<LessonRefOut>,
}

#[derive(Debug, Serialize)]
pub struct ClassDetail {
    #[serde(flatten)]
    pub class: SchoolClass,
    pub units: Vec<ClassUnit>,
}

const CLASS_SELECT: &str = "
    SELECT c.id, c.name, c.grade, c.school_track, c.course_label, c.class_profile,
           c.class_profile_source, c.school_year, c.term, c.position, c.created_at, c.archived_at,
           (SELECT COUNT(*) FROM class_units cu JOIN units u ON u.id = cu.unit_id
             WHERE cu.class_id = c.id AND cu.removed_at IS NULL AND u.deleted_at IS NULL) AS unit_count
      FROM classes c
     WHERE c.teacher_id = $1";

// ---------------------------------------------------------------- loading

#[derive(sqlx::FromRow)]
struct OpenRunRow {
    class_id: i64,
    run_id: i64,
    lesson_id: i64,
    lesson_title: String,
    unit_id: i64,
    unit_title: String,
}

#[derive(sqlx::FromRow)]
struct NextRow {
    class_id: i64,
    unit_id: i64,
    unit_title: String,
    lesson_id: Option<i64>,
    lesson_title: Option<String>,
}

/// Adds each class's next action (two queries for the whole list).
async fn with_next_actions(db: &PgPool, rows: Vec<ClassRow>) -> AppResult<Vec<SchoolClass>> {
    let ids: Vec<i64> = rows.iter().map(|r| r.id).collect();
    let open: Vec<OpenRunRow> = sqlx::query_as(
        "SELECT DISTINCT ON (r.class_id)
                r.class_id, r.id AS run_id, l.id AS lesson_id, l.title AS lesson_title,
                u.id AS unit_id, u.title AS unit_title
           FROM lesson_runs r
           JOIN lessons l ON l.id = r.lesson_id
           JOIN units u ON u.id = l.unit_id
          WHERE r.class_id = ANY($1) AND r.ended_at IS NULL
            AND l.deleted_at IS NULL AND u.deleted_at IS NULL
          ORDER BY r.class_id, r.started_at DESC, r.id DESC",
    )
    .bind(&ids)
    .fetch_all(db)
    .await?;
    // First unit (in class order) that is not finished in this class: its first lesson without
    // a run here, or the unit itself when it has no lessons yet.
    let next: Vec<NextRow> = sqlx::query_as(
        "SELECT DISTINCT ON (cu.class_id)
                cu.class_id, u.id AS unit_id, u.title AS unit_title,
                l.id AS lesson_id, l.title AS lesson_title
           FROM class_units cu
           JOIN units u ON u.id = cu.unit_id AND u.deleted_at IS NULL
           LEFT JOIN lessons l ON l.unit_id = u.id AND l.deleted_at IS NULL
          WHERE cu.class_id = ANY($1) AND cu.removed_at IS NULL
            AND (l.id IS NULL OR NOT EXISTS (
                    SELECT 1 FROM lesson_runs r
                     WHERE r.lesson_id = l.id AND r.class_id = cu.class_id))
          ORDER BY cu.class_id, cu.position, cu.id, l.lesson_date NULLS LAST, l.created_at, l.id",
    )
    .bind(&ids)
    .fetch_all(db)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| {
            let next_action = if let Some(o) = open.iter().find(|o| o.class_id == row.id) {
                Some(NextAction::ResumeRun {
                    run_id: o.run_id,
                    lesson_id: o.lesson_id,
                    lesson_title: o.lesson_title.clone(),
                    unit_id: o.unit_id,
                    unit_title: o.unit_title.clone(),
                })
            } else if row.unit_count == 0 {
                Some(NextAction::AddUnit)
            } else {
                next.iter().find(|n| n.class_id == row.id).map(|n| {
                    match (n.lesson_id, &n.lesson_title) {
                        (Some(lesson_id), Some(lesson_title)) => NextAction::StartLesson {
                            lesson_id,
                            lesson_title: lesson_title.clone(),
                            unit_id: n.unit_id,
                            unit_title: n.unit_title.clone(),
                        },
                        _ => NextAction::PlanUnit {
                            unit_id: n.unit_id,
                            unit_title: n.unit_title.clone(),
                        },
                    }
                })
            };
            SchoolClass { row, next_action }
        })
        .collect())
}

async fn load_class(db: &PgPool, teacher_id: i64, class_id: i64) -> AppResult<SchoolClass> {
    let row: ClassRow = sqlx::query_as(&format!("{CLASS_SELECT} AND c.id = $2"))
        .bind(teacher_id)
        .bind(class_id)
        .fetch_optional(db)
        .await?
        .ok_or_else(|| AppError::not_found("找不到這個班級。"))?;
    Ok(with_next_actions(db, vec![row])
        .await?
        .pop()
        .expect("one class in, one class out"))
}

#[derive(sqlx::FromRow)]
struct ClassUnitRow {
    unit_id: i64,
    position: i32,
    note: String,
    assigned_at: DateTime<Utc>,
    lessons_total: i64,
    lessons_taught: i64,
    last_run_at: Option<DateTime<Utc>>,
    next_lesson_id: Option<i64>,
    next_lesson_title: Option<String>,
}

async fn load_detail(db: &PgPool, teacher_id: i64, class_id: i64) -> AppResult<ClassDetail> {
    let class = load_class(db, teacher_id, class_id).await?;
    let rows: Vec<ClassUnitRow> = sqlx::query_as(
        "SELECT cu.unit_id, cu.position, cu.note, cu.assigned_at,
                (SELECT COUNT(*) FROM lessons l
                  WHERE l.unit_id = cu.unit_id AND l.deleted_at IS NULL) AS lessons_total,
                (SELECT COUNT(DISTINCT r.lesson_id) FROM lesson_runs r
                   JOIN lessons l ON l.id = r.lesson_id
                  WHERE l.unit_id = cu.unit_id AND l.deleted_at IS NULL
                    AND r.class_id = cu.class_id) AS lessons_taught,
                (SELECT MAX(r.started_at) FROM lesson_runs r
                   JOIN lessons l ON l.id = r.lesson_id
                  WHERE l.unit_id = cu.unit_id AND l.deleted_at IS NULL
                    AND r.class_id = cu.class_id) AS last_run_at,
                nl.id AS next_lesson_id, nl.title AS next_lesson_title
           FROM class_units cu
           JOIN units u ON u.id = cu.unit_id AND u.deleted_at IS NULL
           LEFT JOIN LATERAL (
                SELECT l.id, l.title FROM lessons l
                 WHERE l.unit_id = cu.unit_id AND l.deleted_at IS NULL
                   AND NOT EXISTS (SELECT 1 FROM lesson_runs r
                                    WHERE r.lesson_id = l.id AND r.class_id = cu.class_id)
                 ORDER BY l.lesson_date NULLS LAST, l.created_at, l.id
                 LIMIT 1) nl ON true
          WHERE cu.class_id = $1 AND cu.removed_at IS NULL
          ORDER BY cu.position, cu.id",
    )
    .bind(class_id)
    .fetch_all(db)
    .await?;
    let unit_ids: Vec<i64> = rows.iter().map(|r| r.unit_id).collect();
    let mut loaded = units::load_many(db, teacher_id, &unit_ids).await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let Some(at) = loaded.iter().position(|u| u.id == r.unit_id) else {
            continue;
        };
        out.push(ClassUnit {
            unit: loaded.swap_remove(at),
            position: r.position,
            note: r.note,
            assigned_at: r.assigned_at,
            lessons_total: r.lessons_total,
            lessons_taught: r.lessons_taught,
            last_run_at: r.last_run_at,
            next_lesson: match (r.next_lesson_id, r.next_lesson_title) {
                (Some(id), Some(title)) => Some(LessonRefOut { id, title }),
                _ => None,
            },
        });
    }
    Ok(ClassDetail { class, units: out })
}

// ---------------------------------------------------------------- validation

fn check_len(value: &str, max: usize, what: &str) -> AppResult<()> {
    if value.chars().count() > max {
        Err(AppError::bad_request(format!("{what}最多 {max} 字。")))
    } else {
        Ok(())
    }
}

fn check_name(name: &str) -> AppResult<()> {
    if name.is_empty() {
        return Err(AppError::bad_request("請輸入班級名稱。"));
    }
    check_len(name, MAX_CLASS_NAME_CHARS, "班級名稱")
}

fn check_track(track: &str) -> AppResult<()> {
    if valid_track(track) {
        Ok(())
    } else {
        Err(AppError::bad_request("學校類型必須是普高、技高或綜高。"))
    }
}

pub fn check_note(note: &str) -> AppResult<()> {
    check_len(note, MAX_CLASS_NOTE_CHARS, "這個班級的備註")
}

// ---------------------------------------------------------------- classes

#[derive(Debug, Default, Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    pub include_archived: Option<bool>,
}

/// Active classes in the teacher's order; with `include_archived=true`, archived ones follow.
pub async fn list(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiQuery(q): ApiQuery<ListQuery>,
) -> AppResult<Json<Vec<SchoolClass>>> {
    let rows: Vec<ClassRow> = sqlx::query_as(&format!(
        "{CLASS_SELECT} AND ($2 OR c.archived_at IS NULL)
          ORDER BY (c.archived_at IS NOT NULL), c.position, c.id"
    ))
    .bind(teacher.id())
    .bind(q.include_archived.unwrap_or(false))
    .fetch_all(&state.db)
    .await?;
    Ok(Json(with_next_actions(&state.db, rows).await?))
}

#[derive(Debug, Deserialize)]
pub struct CreateClass {
    pub name: String,
    #[serde(default)]
    pub grade: Option<String>,
    #[serde(default)]
    pub school_track: Option<String>,
    #[serde(default)]
    pub course_label: Option<String>,
    #[serde(default)]
    pub class_profile: Option<String>,
    #[serde(default)]
    pub school_year: Option<String>,
    #[serde(default)]
    pub term: Option<String>,
}

/// Defaults: school_track from the teacher; class_profile, when omitted, from the preference
/// answer P6 for the teacher's **first** class only (later classes start empty).
pub async fn create(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiJson(req): ApiJson<CreateClass>,
) -> AppResult<Json<SchoolClass>> {
    let name = req.name.trim();
    check_name(name)?;
    let grade = req.grade.as_deref().unwrap_or_default().trim();
    let course_label = req.course_label.as_deref().unwrap_or_default().trim();
    let school_year = req.school_year.as_deref().unwrap_or_default().trim();
    let term = req.term.as_deref().unwrap_or_default().trim();
    check_len(grade, MAX_CLASS_SHORT_CHARS, "年級")?;
    check_len(course_label, MAX_CLASS_LABEL_CHARS, "課程名稱")?;
    check_len(school_year, MAX_CLASS_SHORT_CHARS, "學年度")?;
    check_len(term, MAX_CLASS_SHORT_CHARS, "學期")?;
    let track = req
        .school_track
        .unwrap_or_else(|| teacher.track().to_string());
    check_track(&track)?;

    let mut tx = state.db.begin().await?;
    // Serialize this teacher's class creation (first-class default, position).
    sqlx::query("SELECT id FROM teachers WHERE id = $1 FOR UPDATE")
        .bind(teacher.id())
        .execute(&mut *tx)
        .await?;
    let (existing, max_position): (i64, Option<i32>) =
        sqlx::query_as("SELECT COUNT(*), MAX(position) FROM classes WHERE teacher_id = $1")
            .bind(teacher.id())
            .fetch_one(&mut *tx)
            .await?;
    let (class_profile, source) = match req.class_profile {
        Some(p) => (p.trim().to_string(), "teacher"),
        None if existing == 0 => {
            let p6 = preferences::latest_answers(&state.db, teacher.id())
                .await?
                .class_profile
                .trim()
                .to_string();
            let source = if p6.is_empty() { "none" } else { "preference" };
            (p6, source)
        }
        None => (String::new(), "none"),
    };
    check_class_profile(&class_profile)?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO classes (teacher_id, name, grade, school_track, course_label, class_profile,
                              class_profile_source, school_year, term, position)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) RETURNING id",
    )
    .bind(teacher.id())
    .bind(name)
    .bind(grade)
    .bind(&track)
    .bind(course_label)
    .bind(&class_profile)
    .bind(source)
    .bind(school_year)
    .bind(term)
    .bind(max_position.unwrap_or(0) + 1)
    .fetch_one(&mut *tx)
    .await?;
    log_usage(
        &mut *tx,
        teacher.id(),
        "class_created",
        json!({ "class_id": id, "class_profile_source": source }),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(load_class(&state.db, teacher.id(), id).await?))
}

pub async fn get_one(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<Json<ClassDetail>> {
    Ok(Json(load_detail(&state.db, teacher.id(), id).await?))
}

#[derive(Debug, Deserialize)]
pub struct UpdateClass {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub grade: Option<String>,
    #[serde(default)]
    pub school_track: Option<String>,
    #[serde(default)]
    pub course_label: Option<String>,
    #[serde(default)]
    pub class_profile: Option<String>,
    #[serde(default)]
    pub school_year: Option<String>,
    #[serde(default)]
    pub term: Option<String>,
}

/// Updates the given fields. Which fields changed is logged to `usage_logs` (`class_updated`).
pub async fn update(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    ApiJson(req): ApiJson<UpdateClass>,
) -> AppResult<Json<SchoolClass>> {
    let trim = |s: &Option<String>| s.as_deref().map(|v| v.trim().to_string());
    let name = trim(&req.name);
    let grade = trim(&req.grade);
    let course_label = trim(&req.course_label);
    let class_profile = trim(&req.class_profile);
    let school_year = trim(&req.school_year);
    let term = trim(&req.term);
    if let Some(n) = &name {
        check_name(n)?;
    }
    if let Some(v) = &grade {
        check_len(v, MAX_CLASS_SHORT_CHARS, "年級")?;
    }
    if let Some(v) = &course_label {
        check_len(v, MAX_CLASS_LABEL_CHARS, "課程名稱")?;
    }
    if let Some(v) = &school_year {
        check_len(v, MAX_CLASS_SHORT_CHARS, "學年度")?;
    }
    if let Some(v) = &term {
        check_len(v, MAX_CLASS_SHORT_CHARS, "學期")?;
    }
    if let Some(t) = &req.school_track {
        check_track(t)?;
    }
    if let Some(p) = &class_profile {
        check_class_profile(p)?;
    }

    let mut tx = state.db.begin().await?;
    let current: Option<(String, String, String, String, String, String, String)> = sqlx::query_as(
        "SELECT name, grade, school_track, course_label, class_profile, school_year, term
           FROM classes WHERE id = $1 AND teacher_id = $2 FOR UPDATE",
    )
    .bind(id)
    .bind(teacher.id())
    .fetch_optional(&mut *tx)
    .await?;
    let Some(cur) = current else {
        return Err(AppError::not_found("找不到這個班級。"));
    };
    let mut changed: Vec<&str> = Vec::new();
    let mut note = |field: &'static str, new: &Option<String>, old: &str| {
        if new.as_deref().is_some_and(|n| n != old) {
            changed.push(field);
        }
    };
    note("name", &name, &cur.0);
    note("grade", &grade, &cur.1);
    note("school_track", &req.school_track, &cur.2);
    note("course_label", &course_label, &cur.3);
    note("class_profile", &class_profile, &cur.4);
    note("school_year", &school_year, &cur.5);
    note("term", &term, &cur.6);
    if !changed.is_empty() {
        sqlx::query(
            "UPDATE classes
                SET name = COALESCE($2, name), grade = COALESCE($3, grade),
                    school_track = COALESCE($4, school_track),
                    course_label = COALESCE($5, course_label),
                    class_profile = COALESCE($6, class_profile),
                    school_year = COALESCE($7, school_year), term = COALESCE($8, term),
                    updated_at = now()
              WHERE id = $1",
        )
        .bind(id)
        .bind(&name)
        .bind(&grade)
        .bind(&req.school_track)
        .bind(&course_label)
        .bind(&class_profile)
        .bind(&school_year)
        .bind(&term)
        .execute(&mut *tx)
        .await?;
        log_usage(
            &mut *tx,
            teacher.id(),
            "class_updated",
            json!({ "class_id": id, "fields": changed }),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(Json(load_class(&state.db, teacher.id(), id).await?))
}

async fn set_archived(
    state: &AppState,
    teacher_id: i64,
    id: i64,
    archived: bool,
) -> AppResult<Json<SchoolClass>> {
    owned_class(&state.db, id, teacher_id).await?;
    let mut tx = state.db.begin().await?;
    // Idempotent: a retry keeps the first archive time.
    let changed = sqlx::query(
        "UPDATE classes
            SET archived_at = CASE WHEN $2 THEN COALESCE(archived_at, now()) END, updated_at = now()
          WHERE id = $1 AND (archived_at IS NOT NULL) <> $2",
    )
    .bind(id)
    .bind(archived)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if changed > 0 {
        log_usage(
            &mut *tx,
            teacher_id,
            if archived {
                "class_archived"
            } else {
                "class_unarchived"
            },
            json!({ "class_id": id }),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(Json(load_class(&state.db, teacher_id, id).await?))
}

/// 封存班級: hidden from the lists, nothing is deleted; its runs stay readable.
pub async fn archive(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<Json<SchoolClass>> {
    set_archived(&state, teacher.id(), id, true).await
}

pub async fn unarchive(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<Json<SchoolClass>> {
    set_archived(&state, teacher.id(), id, false).await
}

fn distinct(ids: &[i64]) -> bool {
    let mut seen = std::collections::HashSet::new();
    ids.iter().all(|id| seen.insert(*id))
}

#[derive(Debug, Deserialize)]
pub struct ReorderClasses {
    pub class_ids: Vec<i64>,
}

/// Puts the listed classes first, in the given order; classes not listed keep their relative
/// order after them.
pub async fn reorder(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiJson(req): ApiJson<ReorderClasses>,
) -> AppResult<Json<Vec<SchoolClass>>> {
    if !distinct(&req.class_ids) {
        return Err(AppError::bad_request("班級順序裡有重複的班級。"));
    }
    let mut tx = state.db.begin().await?;
    let owned: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM classes WHERE teacher_id = $1 ORDER BY position, id FOR UPDATE",
    )
    .bind(teacher.id())
    .fetch_all(&mut *tx)
    .await?;
    if req.class_ids.iter().any(|id| !owned.contains(id)) {
        return Err(AppError::not_found("找不到這個班級。"));
    }
    let rest = owned.iter().filter(|id| !req.class_ids.contains(id));
    for (n, id) in req.class_ids.iter().chain(rest).enumerate() {
        sqlx::query("UPDATE classes SET position = $2 WHERE id = $1")
            .bind(id)
            .bind(n as i32 + 1)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    let rows: Vec<ClassRow> = sqlx::query_as(&format!(
        "{CLASS_SELECT} AND c.archived_at IS NULL ORDER BY c.position, c.id"
    ))
    .bind(teacher.id())
    .fetch_all(&state.db)
    .await?;
    Ok(Json(with_next_actions(&state.db, rows).await?))
}

// ---------------------------------------------------------------- 取用: units in a class

/// Assigns `unit_id` to `class_id` at the end of the class's order, inside `tx`. Already
/// assigned: nothing changes, except the note when one is given. Ownership is the caller's job.
pub async fn assign_unit(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    teacher_id: i64,
    class_id: i64,
    unit_id: i64,
    note: Option<&str>,
) -> AppResult<()> {
    // Serialize assignments to this class (position, the partial unique index).
    sqlx::query("SELECT id FROM classes WHERE id = $1 FOR UPDATE")
        .bind(class_id)
        .execute(&mut **tx)
        .await?;
    let existing: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM class_units WHERE class_id = $1 AND unit_id = $2 AND removed_at IS NULL",
    )
    .bind(class_id)
    .bind(unit_id)
    .fetch_optional(&mut **tx)
    .await?;
    if let Some(row_id) = existing {
        if let Some(note) = note {
            sqlx::query("UPDATE class_units SET note = $2 WHERE id = $1")
                .bind(row_id)
                .bind(note)
                .execute(&mut **tx)
                .await?;
        }
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO class_units (class_id, unit_id, note, position)
         VALUES ($1, $2, $3, COALESCE((SELECT MAX(position) FROM class_units
                                        WHERE class_id = $1 AND removed_at IS NULL), 0) + 1)",
    )
    .bind(class_id)
    .bind(unit_id)
    .bind(note.unwrap_or_default())
    .execute(&mut **tx)
    .await?;
    log_usage(
        &mut **tx,
        teacher_id,
        "class_unit_assigned",
        json!({ "class_id": class_id, "unit_id": unit_id }),
    )
    .await?;
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct AssignUnit {
    pub unit_id: i64,
    #[serde(default)]
    pub note: Option<String>,
}

/// 從備課庫加入單元. Idempotent.
pub async fn add_unit(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(class_id): ApiPath<i64>,
    ApiJson(req): ApiJson<AssignUnit>,
) -> AppResult<Json<ClassDetail>> {
    let class = owned_class(&state.db, class_id, teacher.id()).await?;
    if class.archived {
        return Err(AppError::Conflict(MSG_CLASS_ARCHIVED.into()));
    }
    owned_unit(&state.db, req.unit_id, teacher.id()).await?;
    let note = req.note.as_deref().map(str::trim);
    if let Some(n) = note {
        check_note(n)?;
    }
    let mut tx = state.db.begin().await?;
    assign_unit(&mut tx, teacher.id(), class_id, req.unit_id, note).await?;
    tx.commit().await?;
    Ok(Json(load_detail(&state.db, teacher.id(), class_id).await?))
}

#[derive(Debug, Deserialize)]
pub struct UpdateClassUnit {
    pub note: String,
}

/// The per-class note of an assigned unit (e.g. 這班用基礎版).
pub async fn update_unit(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath((class_id, unit_id)): ApiPath<(i64, i64)>,
    ApiJson(req): ApiJson<UpdateClassUnit>,
) -> AppResult<Json<ClassDetail>> {
    owned_class(&state.db, class_id, teacher.id()).await?;
    let note = req.note.trim();
    check_note(note)?;
    let updated = sqlx::query(
        "UPDATE class_units SET note = $3
          WHERE class_id = $1 AND unit_id = $2 AND removed_at IS NULL",
    )
    .bind(class_id)
    .bind(unit_id)
    .bind(note)
    .execute(&state.db)
    .await?
    .rows_affected();
    if updated == 0 {
        return Err(AppError::not_found("這個班級沒有取用這個單元。"));
    }
    Ok(Json(load_detail(&state.db, teacher.id(), class_id).await?))
}

/// 從這個班級移除單元: a soft removal. The unit design stays in the library and the runs taught
/// in this class are kept. Removing a unit that is not assigned is a no-op.
pub async fn remove_unit(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath((class_id, unit_id)): ApiPath<(i64, i64)>,
) -> AppResult<Json<ClassDetail>> {
    owned_class(&state.db, class_id, teacher.id()).await?;
    let mut tx = state.db.begin().await?;
    let removed = sqlx::query(
        "UPDATE class_units SET removed_at = now()
          WHERE class_id = $1 AND unit_id = $2 AND removed_at IS NULL",
    )
    .bind(class_id)
    .bind(unit_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if removed > 0 {
        log_usage(
            &mut *tx,
            teacher.id(),
            "class_unit_removed",
            json!({ "class_id": class_id, "unit_id": unit_id }),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(Json(load_detail(&state.db, teacher.id(), class_id).await?))
}

#[derive(Debug, Deserialize)]
pub struct ReorderUnits {
    pub unit_ids: Vec<i64>,
}

/// Puts the listed units first, in the given order; assigned units not listed follow.
pub async fn reorder_units(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(class_id): ApiPath<i64>,
    ApiJson(req): ApiJson<ReorderUnits>,
) -> AppResult<Json<ClassDetail>> {
    owned_class(&state.db, class_id, teacher.id()).await?;
    if !distinct(&req.unit_ids) {
        return Err(AppError::bad_request("單元順序裡有重複的單元。"));
    }
    let mut tx = state.db.begin().await?;
    let assigned: Vec<i64> = sqlx::query_scalar(
        "SELECT unit_id FROM class_units WHERE class_id = $1 AND removed_at IS NULL
          ORDER BY position, id FOR UPDATE",
    )
    .bind(class_id)
    .fetch_all(&mut *tx)
    .await?;
    if req.unit_ids.iter().any(|id| !assigned.contains(id)) {
        return Err(AppError::not_found("這個班級沒有取用這個單元。"));
    }
    let rest = assigned.iter().filter(|id| !req.unit_ids.contains(id));
    for (n, unit_id) in req.unit_ids.iter().chain(rest).enumerate() {
        sqlx::query(
            "UPDATE class_units SET position = $3
              WHERE class_id = $1 AND unit_id = $2 AND removed_at IS NULL",
        )
        .bind(class_id)
        .bind(unit_id)
        .bind(n as i32 + 1)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(Json(load_detail(&state.db, teacher.id(), class_id).await?))
}
