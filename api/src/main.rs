use std::future::IntoFuture;
use std::net::SocketAddr;

use anyhow::Context;
use manna_api::config::{AiProviderKind, Config};
use manna_api::storage::s3::S3Storage;
use manna_api::storage::{StorageBackend, StorageConfig};
use manna_api::{
    auth, build_app, build_internal_app, db, kb_embeddings, materials, worksheet, AppState,
};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,tower_http=info")),
        )
        .init();
    if let Err(e) = run().await {
        tracing::error!("{e:#}");
        eprintln!("manna-api failed to start: {e:#}");
        std::process::exit(1);
    }
}

async fn run() -> anyhow::Result<()> {
    let config = Config::from_env().context("invalid configuration")?;
    tracing::info!(?config, "starting manna-api");
    if config.ai_provider == AiProviderKind::Anthropic {
        tracing::warn!(
            "AI_PROVIDER=anthropic (overseas API): demo environment only, no real teacher data"
        );
    }
    if let Some(warning) = &config.embedding.warning {
        tracing::warn!("{warning}; retrieval is keyword-only");
    }
    if let Some(warning) = &config.ocr.warning {
        tracing::warn!("{warning}; scanned files keep the text they were uploaded with");
    }
    if let Some(warning) = &config.images.warning {
        tracing::warn!("{warning}; worksheets get no generated pictures");
    }
    if let Some(warning) = &config.memory.warning {
        tracing::warn!("{warning}; teacher memory stays Postgres-only (no Graphiti index)");
    }
    if config.ai_provider == AiProviderKind::Fake {
        tracing::warn!("AI_PROVIDER=fake: AI outputs are deterministic test fixtures");
    }

    let pool = db::connect(&config.database_url)
        .await
        .context("cannot connect to DATABASE_URL")?;
    db::migrate(&pool)
        .await
        .context("database migration failed")?;
    auth::bootstrap_admin(&pool, config.admin_bootstrap_code.as_deref())
        .await
        .context("creating bootstrap invite failed")?;

    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    report_storage(&config.storage);
    let state = AppState::new(pool, config);
    manna_api::memory::graph::spawn_worker(state.clone());
    // Index whatever is not embedded yet (first deploy, model change, interrupted run). In the
    // background: a slow or failing embedding provider must not delay startup.
    kb_embeddings::spawn_sync(&state, "startup", None, false);
    // OCR runs the last process left open are taken over before requests are served (a read
    // could otherwise report an old one as stale first); their jobs run in the background.
    match materials::ocr::recover(&state).await {
        Ok(0) => {}
        Ok(n) => tracing::info!(
            resumed = n,
            "resumed OCR runs left open by the last process"
        ),
        Err(e) => tracing::error!(error = %e, "could not resume open OCR runs"),
    }
    // Worksheet pictures left open, the same way.
    match worksheet::images::recover(&state).await {
        Ok(0) => {}
        Ok(n) => tracing::info!(
            resumed = n,
            "resumed worksheet pictures left open by the last process"
        ),
        Err(e) => tracing::error!(error = %e, "could not resume open worksheet pictures"),
    }
    match materials::ocr::backfill_platform(&state).await {
        Ok(0) => {}
        Ok(n) => tracing::info!(
            started = n,
            "OCR started for knowledge-base files without text"
        ),
        Err(e) => tracing::error!(error = %e, "could not start OCR for knowledge-base files"),
    }
    // The memory index's model forwarder has its own port that the proxy never routes to.
    let internal = if state.config.memory.enabled() {
        let internal_addr = SocketAddr::from(([0, 0, 0, 0], state.config.internal_port));
        let listener = tokio::net::TcpListener::bind(internal_addr)
            .await
            .with_context(|| format!("cannot listen on {internal_addr}"))?;
        tracing::info!("memory index forwarder on internal http://{internal_addr}");
        Some((listener, build_internal_app(state.clone())))
    } else {
        None
    };
    let app = build_app(state);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("cannot listen on {addr}"))?;
    tracing::info!("listening on http://{addr}");
    let public = axum::serve(listener, app).with_graceful_shutdown(shutdown_signal());
    match internal {
        None => public.await?,
        Some((listener, app)) => {
            let internal = axum::serve(listener, app).with_graceful_shutdown(shutdown_signal());
            let (public, internal) = tokio::join!(public.into_future(), internal.into_future());
            public?;
            internal?;
        }
    }
    Ok(())
}

/// Says once, at startup, where material files go. Storage problems never stop the server:
/// uploads are refused with a clear message and everything else keeps working.
fn report_storage(storage: &StorageConfig) {
    if let Some(problem) = &storage.problem {
        tracing::error!("material storage is OFF: {problem}");
        return;
    }
    match storage.backend {
        StorageBackend::None => {
            tracing::warn!("STORAGE_BACKEND=none: file uploads are off (pasted text still works)")
        }
        StorageBackend::Fs => {
            tracing::info!(dir = %storage.dir.display(), "material files go to the local filesystem")
        }
        StorageBackend::S3 => {
            let Some(settings) = storage.s3.clone() else {
                return;
            };
            tracing::info!(endpoint = %settings.endpoint, bucket = %settings.bucket,
                "material files go to S3-compatible storage");
            tokio::spawn(async move {
                let s3 = match S3Storage::new(&settings) {
                    Ok(s3) => s3,
                    Err(e) => return tracing::error!("S3 settings are invalid: {e:#}"),
                };
                // MinIO may still be starting when the app comes up: retry for about a minute.
                for attempt in 1..=12 {
                    match s3.ensure_bucket().await {
                        Ok(state) => return tracing::info!(?state, "S3 bucket is ready"),
                        Err(e) if attempt < 12 => {
                            tracing::warn!(attempt, "S3 bucket not ready yet: {e:#}");
                            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        }
                        Err(e) => tracing::error!(
                            "S3 bucket check failed (uploads will fail until this is fixed): {e:#}"
                        ),
                    }
                }
            });
        }
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
