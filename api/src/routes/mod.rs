//! HTTP routes (contract: docs/API.md).

pub mod admin;
pub mod browse;
pub mod classes;
pub mod core;
pub mod embeddings;
pub mod feedback;
pub mod knowledge;
pub mod lessons;
pub mod library;
pub mod library_admin;
pub mod materials;
pub mod objectives;
pub mod preferences;
pub mod prep;
pub mod remote;
pub mod runs;
pub mod sop;
pub mod suggestions;
pub mod templates;
pub mod units;
pub mod worksheet_revise;
pub mod worksheet_vocab;
pub mod worksheets;

use axum::extract::{DefaultBodyLimit, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{delete, get, patch, post, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
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
        // classes and the design library (Phase 1.4)
        .route("/classes", get(classes::list).post(classes::create))
        .route("/classes/order", put(classes::reorder))
        .route("/classes/{id}", get(classes::get_one).put(classes::update))
        .route("/classes/{id}/archive", post(classes::archive))
        .route("/classes/{id}/unarchive", post(classes::unarchive))
        .route("/classes/{id}/units", post(classes::add_unit))
        .route("/classes/{id}/units/order", put(classes::reorder_units))
        .route(
            "/classes/{id}/units/{unit_id}",
            put(classes::update_unit).delete(classes::remove_unit),
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
        .route("/lessons/{id}/sop", get(sop::get).put(sop::put))
        .route("/lessons/{id}/sop/material", put(sop::put_material))
        .route("/lessons/{id}/sop/versions", get(sop::versions))
        .route("/lessons/{id}/sop/versions/{version_no}", get(sop::version))
        .route("/lessons/{id}/sop/analysis", post(sop::start_analysis))
        .route("/lessons/{id}/sop/objectives", post(sop::start_objectives))
        .route("/sop-jobs/{id}", get(sop::job))
        // 學習單產生器 (Phase 2.3)
        .route(
            "/worksheets",
            get(worksheets::list).post(worksheets::create),
        )
        .route(
            "/worksheets/{id}",
            get(worksheets::get)
                .put(worksheets::save)
                .delete(worksheets::delete),
        )
        .route("/worksheets/{id}/material", put(worksheets::put_material))
        .route("/worksheets/{id}/generate", post(worksheets::generate))
        .route("/worksheets/{id}/jobs", get(worksheets::jobs))
        .route(
            "/worksheets/{id}/blocks/{block_id}/regenerate",
            post(worksheets::regenerate_block),
        )
        .route(
            "/worksheets/{id}/blocks/{block_id}/undo",
            post(worksheets::undo_block),
        )
        // 逐區修改與註記
        .route(
            "/worksheets/{id}/blocks/{block_id}/revise",
            post(worksheet_revise::revise_block),
        )
        .route(
            "/worksheets/{id}/comments",
            get(worksheet_revise::list_comments).post(worksheet_revise::create_comment),
        )
        .route(
            "/worksheets/{id}/comments/{comment_id}",
            patch(worksheet_revise::update_comment).delete(worksheet_revise::delete_comment),
        )
        .route(
            "/worksheets/{id}/proposals",
            get(worksheet_revise::list_proposals),
        )
        .route(
            "/worksheets/{id}/proposals/{proposal_id}",
            get(worksheet_revise::get_proposal),
        )
        .route(
            "/worksheets/{id}/proposals/{proposal_id}/accept",
            post(worksheet_revise::accept_proposal),
        )
        .route(
            "/worksheets/{id}/proposals/{proposal_id}/reject",
            post(worksheet_revise::reject_proposal),
        )
        .route("/worksheets/{id}/sheet", get(worksheets::sheet))
        .route("/worksheets/{id}/versions", get(worksheets::versions))
        .route(
            "/worksheets/{id}/sop-sync",
            get(worksheets::sync_preview).post(worksheets::sync_apply),
        )
        .route("/worksheets/{id}/link", put(worksheets::link))
        .route(
            "/worksheets/{id}/save-as-lesson",
            post(worksheets::save_as_lesson),
        )
        .route(
            "/worksheets/{id}/checks/vocabulary",
            post(worksheets::check_vocabulary),
        )
        .route("/worksheets/{id}/checks", get(worksheets::checks))
        // 詞彙級別檢查：處理超出級別的字
        .route(
            "/worksheets/{id}/checks/{check_id}/suggest",
            post(worksheet_vocab::suggest),
        )
        .route(
            "/worksheets/{id}/checks/{check_id}/apply",
            post(worksheet_vocab::apply),
        )
        .route("/worksheets/{id}/export", get(worksheets::export))
        .route("/worksheets/{id}/thumbnail", get(worksheets::thumbnail))
        // 學習單的圖片
        .route("/worksheets/{id}/images", get(worksheets::images_state))
        .route(
            "/worksheets/{id}/images/{image_id}",
            get(worksheets::image_file),
        )
        .route(
            "/worksheets/{id}/image-slots/{slot_id}/regenerate",
            post(worksheets::regenerate_image),
        )
        .route(
            "/worksheets/{id}/image-slots/{slot_id}/upload",
            post(worksheets::upload_image).layer(DefaultBodyLimit::max(
                (crate::materials::MAX_IMAGE_BYTES + 1024 * 1024) as usize,
            )),
        )
        .route(
            "/worksheets/{id}/image-slots/{slot_id}",
            delete(worksheets::remove_image),
        )
        // 我的學習單版型 (Phase 2.5)
        .route(
            "/worksheet-templates",
            get(templates::list).post(templates::create),
        )
        .route(
            "/worksheet-templates/{id}",
            get(templates::get)
                .put(templates::save)
                .patch(templates::save)
                .delete(templates::delete),
        )
        .route("/worksheet-templates/{id}/retry", post(templates::retry))
        .route("/worksheet-templates/{id}/copy", post(templates::copy))
        // 教材參考、我的教材、學習單範本 (Phase 2.9)
        .route("/browse/materials", get(browse::materials))
        .route("/browse/templates", get(browse::templates))
        // 備課對話 (Phase 2.6)
        .route("/prep-sessions", get(prep::list).post(prep::create))
        .route(
            "/prep-sessions/{id}",
            get(prep::get).patch(prep::patch).delete(prep::delete),
        )
        .route("/prep-sessions/{id}/turns", post(prep::create_turn))
        .route(
            "/prep-sessions/{id}/images",
            post(prep::upload_image).layer(DefaultBodyLimit::max(
                (crate::prep::images::MAX_BYTES + 1024 * 1024) as usize,
            )),
        )
        .route(
            "/prep-sessions/{id}/images/{image_id}",
            get(prep::image_file).delete(prep::delete_image),
        )
        .route("/prep-sessions/{id}/turns/{turn_id}/kit", get(prep::kit))
        .route(
            "/prep-sessions/{id}/turns/{turn_id}/siop-check",
            get(prep::siop_check).post(prep::rerun_siop_check),
        )
        // material library, original files and annotations (Phase 2.2)
        .route("/materials", get(library::list).post(library::create))
        .route(
            "/materials/upload",
            post(library::upload).layer(DefaultBodyLimit::max(
                crate::materials::MAX_UPLOAD_REQUEST_BYTES,
            )),
        )
        .route(
            "/materials/{id}",
            get(library::get_one)
                .put(library::update)
                .delete(library::remove),
        )
        .route("/materials/{id}/file", get(library::file))
        .route("/materials/{id}/file-link", post(library::file_link))
        .route("/materials/{id}/thumbnail", get(library::thumbnail))
        .route("/materials/{id}/ocr", post(library::run_ocr))
        .route(
            "/materials/{id}/annotations",
            get(library::list_annotations).post(library::create_annotation),
        )
        .route(
            "/materials/{id}/annotations/ai",
            get(library::ai_annotations),
        )
        .route(
            "/annotations/{id}",
            put(library::update_annotation).delete(library::delete_annotation),
        )
        .route(
            "/lessons/{id}/materials/{material_id}",
            put(library::link_lesson).delete(library::unlink_lesson),
        )
        .route("/ai/{interaction_id}/decision", post(feedback::decision))
        // 內容目標與語言目標的草擬（SIOP G2、G3）
        .route("/objective-drafts", post(objectives::draft))
        .route(
            "/objective-drafts/progress/{progress_id}",
            get(objectives::draft_progress),
        )
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
        // knowledge base and vocabulary (Phase 2.0)
        .route("/knowledge/curriculum", get(knowledge::search_curriculum))
        .route(
            "/knowledge/curriculum/by-code",
            get(knowledge::curriculum_by_code),
        )
        .route(
            "/vocabulary/analyze",
            post(knowledge::analyze_vocabulary).layer(DefaultBodyLimit::max(2 * 1024 * 1024)),
        )
        // knowledge-base management (PI or 知識庫管理員)
        .route(
            "/admin/knowledge/import",
            post(admin::import_knowledge).layer(DefaultBodyLimit::max(20 * 1024 * 1024)),
        )
        .route(
            "/admin/vocabulary/import",
            post(admin::import_vocabulary).layer(DefaultBodyLimit::max(20 * 1024 * 1024)),
        )
        .route("/admin/knowledge/summary", get(admin::knowledge_summary))
        .route("/admin/knowledge/embeddings", get(embeddings::status))
        .route(
            "/admin/knowledge/embeddings/rebuild",
            post(embeddings::rebuild),
        )
        .route(
            "/admin/knowledge/retire-demo",
            post(admin::retire_demo_knowledge),
        )
        .route("/admin/knowledge/items", get(knowledge::browse_items))
        // 教材與範本管理 (Phase 2.9; PI or 知識庫管理員)
        .route(
            "/admin/library",
            get(library_admin::list)
                .post(library_admin::upload)
                .layer(DefaultBodyLimit::max(
                    crate::materials::MAX_UPLOAD_REQUEST_BYTES,
                )),
        )
        .route(
            "/admin/library/templates",
            post(library_admin::upload_template).layer(DefaultBodyLimit::max(
                crate::materials::MAX_UPLOAD_REQUEST_BYTES,
            )),
        )
        .route(
            "/admin/library/templates/{id}",
            patch(library_admin::patch_template).delete(library_admin::delete_template),
        )
        .route(
            "/admin/library/templates/{id}/retry",
            post(library_admin::retry_template),
        )
        .route(
            "/admin/library/{id}",
            patch(library_admin::patch_material).delete(library_admin::delete_material),
        )
        .route("/admin/vocabulary/items", get(knowledge::browse_vocabulary))
        // PI
        .route("/admin/invites", post(admin::create_invite))
        .route("/admin/accounts", get(admin::list_accounts))
        .route("/admin/accounts/{id}", patch(admin::update_account_role))
        .route(
            "/admin/settings",
            get(admin::get_settings).put(admin::put_settings),
        )
        .route(
            "/admin/prompts",
            get(admin::list_prompts).post(admin::create_prompt),
        )
        .route(
            "/admin/prompts/{key}/versions",
            get(admin::list_prompt_versions),
        )
        .route("/admin/prompts/{key}/try", post(admin::try_prompt))
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

/// Git commit of the running build, so a deployment can be checked against what was pushed.
/// First `BUILD_COMMIT`, baked into the image from the `SOURCE_COMMIT` build argument (see the
/// Dockerfile and README「部署」), then `SOURCE_COMMIT` from the runtime environment.
fn running_commit() -> String {
    commit_from(&[
        std::env::var("BUILD_COMMIT").ok(),
        std::env::var("SOURCE_COMMIT").ok(),
    ])
}

/// The first value that is a commit hash, or "unknown". Placeholders a deployment may pass
/// instead (empty, "unknown", "HEAD") do not hide a later real value.
fn commit_from(candidates: &[Option<String>]) -> String {
    candidates
        .iter()
        .flatten()
        .map(|c| c.trim())
        .find(|c| (7..=64).contains(&c.len()) && c.bytes().all(|b| b.is_ascii_hexdigit()))
        .map_or_else(|| "unknown".to_string(), str::to_string)
}

async fn public_config(State(state): State<AppState>) -> AppResult<Json<serde_json::Value>> {
    let c = &state.config;
    Ok(Json(json!({
        "demo_mode": c.demo_mode,
        "ai_enabled": c.ai_enabled(),
        "ai_provider": c.ai_provider.as_str(),
        "ai_model": c.ai_model,
        "route_choice_enabled": crate::settings::route_choice_enabled(&state.db).await?,
        // Whether files can be uploaded to the material library (pasting always works).
        "uploads_enabled": state.storage.is_some(),
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
        "comprehensive" => "綜高",
        "both" => "各類型共用",
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

/// For optional JSON fields where `null` means something else than "not sent":
/// absent → `None`, `null` → `Some(None)`, a value → `Some(Some(value))`. Use with
/// `#[serde(default, deserialize_with = "…")]`.
pub fn double_option<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: serde::Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::<T>::deserialize(deserializer)?))
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
    fn the_baked_commit_comes_first_then_the_runtime_one() {
        let sha = "0123abcd0123abcd0123abcd0123abcd0123abcd";
        let some = |s: &str| Some(s.to_string());
        // Baked into the image at build time: wins over the runtime value.
        assert_eq!(commit_from(&[some(sha), some("89abcdef")]), sha);
        // No build argument (a local `docker build`): the runtime value.
        for baked in [None, some(""), some("  "), some("unknown"), some("HEAD")] {
            assert_eq!(commit_from(&[baked, some(&format!(" {sha}\n"))]), sha);
        }
        // Neither: "unknown".
        assert_eq!(commit_from(&[None, None]), "unknown");
        assert_eq!(commit_from(&[some(""), some("unknown")]), "unknown");
        assert_eq!(commit_from(&[some("HEAD"), some("main")]), "unknown");
        // Short hashes count; anything that is not hex does not.
        assert_eq!(commit_from(&[some("abc1234"), None]), "abc1234");
        assert_eq!(commit_from(&[some("abc123"), some("deadbeefz")]), "unknown");
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
