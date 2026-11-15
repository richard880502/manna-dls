use std::net::SocketAddr;

use anyhow::Context;
use manna_api::config::{AiProviderKind, Config};
use manna_api::storage::s3::S3Storage;
use manna_api::storage::{StorageBackend, StorageConfig};
use manna_api::{auth, build_app, db, AppState};
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
    let app = build_app(AppState::new(pool, config));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("cannot listen on {addr}"))?;
    tracing::info!("listening on http://{addr}");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
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
                match S3Storage::new(&settings) {
                    Ok(s3) => match s3.check().await {
                        Ok(()) => tracing::info!("S3 bucket is reachable"),
                        Err(e) => tracing::error!(
                            "S3 bucket check failed (uploads will fail until this is fixed): {e:#}"
                        ),
                    },
                    Err(e) => tracing::error!("S3 settings are invalid: {e:#}"),
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
