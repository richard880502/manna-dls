//! Manna DLS backend (Phase 1: AI-assisted lesson preparation).
//!
//! `main.rs` is a thin wrapper: load [`config::Config`], connect + migrate the database,
//! create the bootstrap PI invite, then serve [`build_app`].

pub mod ai;
pub mod auth;
pub mod checks;
pub mod config;
pub mod core_questions;
pub mod curriculum;
pub mod db;
pub mod embedding;
pub mod error;
pub mod kb_embeddings;
pub mod materials;
pub mod preferences;
pub mod prep;
pub mod prompts;
pub mod record;
pub mod retrieval;
pub mod routes;
pub mod settings;
pub mod sop;
pub mod storage;
pub mod syllables;
pub mod vocabulary;
pub mod worksheet;

use std::path::PathBuf;
use std::sync::Arc;

use axum::handler::HandlerWithoutStateExt;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::http::{StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::Router;
use sqlx::PgPool;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

use crate::ai::AiClient;
use crate::config::Config;
use crate::embedding::Embedder;
use crate::kb_embeddings::EmbedJobs;
use crate::materials::access::LinkSigner;
use crate::storage::Storage;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub config: Arc<Config>,
    pub ai: Arc<AiClient>,
    /// Embedding model for hybrid retrieval; disabled unless `EMBEDDING_PROVIDER` is set.
    pub embedder: Arc<Embedder>,
    pub embed_jobs: Arc<EmbedJobs>,
    /// Where original material files live; `None` when uploads are off (`STORAGE_BACKEND=none`
    /// or a broken storage configuration).
    pub storage: Option<Arc<dyn Storage>>,
    /// Signs the short-lived file links for `<img>`, `<audio>` and `<video>`.
    pub links: Arc<LinkSigner>,
    /// Bounds how many uploads are held in memory at once.
    pub upload_slots: Arc<tokio::sync::Semaphore>,
    /// OCR of scanned files: the provider and the server-wide page and file slots.
    pub ocr: Arc<materials::ocr::Ocr>,
    /// Bounds how many preview images (知識庫) are rendered at once.
    pub thumb_slots: Arc<tokio::sync::Semaphore>,
    /// Pictures on worksheets: the provider and the server-wide request slots.
    pub images: Arc<worksheet::images::Images>,
}

impl AppState {
    pub fn new(db: PgPool, config: Config) -> Self {
        let ai = Arc::new(AiClient::from_config(&config));
        let embedder = Arc::new(Embedder::new(config.embedding.clone()));
        let storage = config.storage.build();
        let ocr = Arc::new(materials::ocr::Ocr::new(config.ocr.clone()));
        let images = Arc::new(worksheet::images::Images::new(config.images.clone()));
        Self {
            db,
            config: Arc::new(config),
            ai,
            embedder,
            embed_jobs: Arc::default(),
            storage,
            links: Arc::new(LinkSigner::new()),
            upload_slots: Arc::new(tokio::sync::Semaphore::new(
                materials::MAX_CONCURRENT_UPLOADS,
            )),
            ocr,
            thumb_slots: Arc::new(tokio::sync::Semaphore::new(
                materials::thumbnail::MAX_CONCURRENT_RENDERS,
            )),
            images,
        }
    }
}

/// `/api/*` routes plus the frontend: files from `STATIC_DIR` (e.g. `/assets/*`), and
/// `index.html` for other non-API paths (SPA routing). A missing static directory only yields
/// 404s.
pub fn build_app(state: AppState) -> Router {
    let static_dir = state.config.static_dir.clone();
    let index = static_dir.join("index.html");
    let spa_fallback = move |uri: Uri| spa_index(index.clone(), uri);
    let files = ServeDir::new(&static_dir).fallback(spa_fallback.into_service());
    Router::new()
        .nest("/api", routes::api_router().with_state(state))
        .fallback_service(files)
        .layer(TraceLayer::new_for_http())
}

/// True for paths that name a file (anything under `/assets/`, or a last segment with an
/// extension): a missing file must be a 404, not the SPA shell.
pub fn looks_like_file(path: &str) -> bool {
    path.starts_with("/assets/")
        || path
            .rsplit('/')
            .next()
            .is_some_and(|last| last.contains('.'))
}

async fn spa_index(index: PathBuf, uri: Uri) -> Response {
    if looks_like_file(uri.path()) {
        return StatusCode::NOT_FOUND.into_response();
    }
    match tokio::fs::read(&index).await {
        Ok(html) => (
            [
                (CONTENT_TYPE, "text/html; charset=utf-8"),
                (CACHE_CONTROL, "no-cache"),
            ],
            html,
        )
            .into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::looks_like_file;

    #[test]
    fn file_paths_never_fall_back_to_the_spa() {
        assert!(looks_like_file("/assets/index-abc.js"));
        assert!(looks_like_file("/assets/avatars/a"));
        assert!(looks_like_file("/favicon.ico"));
        assert!(!looks_like_file("/"));
        assert!(!looks_like_file("/units/12/core"));
        assert!(!looks_like_file("/lessons/3"));
    }
}
