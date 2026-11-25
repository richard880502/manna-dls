//! HTTP routes (contract: docs/API.md).

pub mod admin;
pub mod admin_ai_traffic;
pub mod browse;
pub mod classes;
pub mod community;
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
pub mod worksheet_history;
pub mod worksheet_revise;
pub mod worksheet_snapshots;
pub mod worksheet_vocab;
pub mod worksheets;

use axum::extract::{DefaultBodyLimit, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{delete, get, patch, post, put};
use axum::{Json, Router};
use serde_json::json;

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
        .route(
            "/admin/memory-settings",
            get(crate::memory::routes::settings_get).put(crate::memory::routes::settings_put),
        )
        .route(
            "/admin/community-settings",
            get(community::settings_get).put(community::settings_put),
        )
        // 社群 (docs/COMMUNITY.md)
        .route("/community/worksheets", get(community::explore))
        .route("/community/mine", get(community::mine))
        .route("/community/saves", get(community::saved))
        .route("/community/worksheets/{id}", get(community::preview))
        .route(
            "/community/worksheets/{id}/images/{image_id}",
            get(community::picture),
        )
        .route(
            "/community/worksheets/{id}/save",
            post(community::save).delete(community::unsave),
        )
        .route("/community/worksheets/{id}/remix", post(community::remix))
        .route("/worksheets/{id}/share", put(community::share))
        .route("/me", get(auth::me))
        .route(
            "/me/memories",
            get(crate::memory::routes::get).post(crate::memory::routes::confirm),
        )
        .route("/me/memories/preview", post(crate::memory::routes::preview))
        .route(
            "/me/memories/candidates",
            get(crate::memory::routes::candidates_list),
        )
        .route(
            "/me/memories/candidates/{id}/shown",
            post(crate::memory::routes::candidate_shown),
        )
        .route("/me/memories/{id}", delete(crate::memory::routes::remove))
        .route(
            "/admin/memory-index",
            get(crate::memory::routes::index_status),
        )
        .route(
            "/admin/memory-index/rebuild",
            post(crate::memory::routes::rebuild),
        )
        // The model forwarder of the memory index is NOT here: it is served only on the
        // internal listener (`INTERNAL_PORT`, `crate::build_internal_app`).
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
        .route("/stepwise-lessons", get(lessons::list_stepwise))
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
        .route("/lessons/{id}/sop/goals", post(sop::start_goals))
        .route("/lessons/{id}/sop/tasks", post(sop::start_tasks))
        .route("/lessons/{id}/sop/warmup", post(sop::start_warmup))
        .route(
            "/lessons/{id}/sop/exit-ticket",
            post(sop::start_exit_ticket),
        )
        .route(
            "/lessons/{id}/sop/objective-english",
            post(sop::objective_english),
        )
        .route(
            "/lessons/{id}/sop/procedural-steps",
            post(sop::procedural_steps),
        )
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
        .route(
            "/worksheets/{id}/material-selection",
            put(worksheets::put_material_selection),
        )
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
        .route(
            "/worksheets/{id}/blocks/{block_id}/restore-word-table",
            post(worksheets::restore_word_table),
        )
        .route(
            "/worksheets/{id}/vocabulary-meanings",
            post(worksheets::fill_vocabulary_meanings),
        )
        // 修改歷程
        .route(
            "/worksheets/{id}/blocks/{block_id}/history",
            get(worksheet_history::block_history),
        )
        .route(
            "/worksheets/{id}/blocks/{block_id}/adopt",
            post(worksheet_history::adopt_version),
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
            "/worksheets/{id}/versions/{version_no}/sheet",
            get(worksheet_history::version_sheet),
        )
        .route(
            "/worksheets/{id}/versions/{version_no}/restore",
            post(worksheet_history::restore_version),
        )
        .route(
            "/worksheets/{id}/snapshots",
            post(worksheet_snapshots::create),
        )
        .route(
            "/worksheets/{id}/snapshots/{snapshot_id}",
            patch(worksheet_snapshots::rename).delete(worksheet_snapshots::remove),
        )
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
            "/prep-sessions/{id}/material-selection",
            put(prep::put_material_selection),
        )
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
            "/prep-sessions/{id}/turns/{turn_id}/undo",
            post(prep::undo_turn),
        )
        .route(
            "/prep-sessions/{id}/turns/{turn_id}/siop-check",
            get(prep::siop_check).post(prep::rerun_siop_check),
        )
        // material library, original files and annotations (Phase 2.2)
        .route("/materials", get(library::list).post(library::create))
        .route("/materials/split-preview", post(library::split_preview))
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
        .route("/ai-jobs/{id}", delete(crate::queue::routes::cancel))
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
        .route("/admin/ai-traffic", get(admin_ai_traffic::get))
        .route(
            "/admin/ai-model",
            get(crate::ai::admin::get)
                .put(crate::ai::admin::put)
                .delete(crate::ai::admin::reset),
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
    let ai = state.ai.active();
    Ok(Json(json!({
        "demo_mode": c.demo_mode,
        "ai_enabled": ai.enabled(),
        "ai_provider": ai.provider_name(),
        "ai_model": ai.model,
        "route_choice_enabled": crate::settings::route_choice_enabled(&state.db).await?,
        // 社群: the PI's switch (off: the web shows nothing about it).
        "community_enabled": crate::settings::community_enabled(&state.db).await?,
        // Whether files can be uploaded to the material library (pasting always works).
        "uploads_enabled": state.storage.is_some(),
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
