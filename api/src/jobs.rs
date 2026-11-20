//! Shared isolation for background work. The caller owns persistence and finalization:
//! OCR/images resume guarded runs, Graphiti retries durable leases, and text jobs close
//! their existing result rows. This module does not silently replay model calls.
use std::future::Future;

use crate::error::{AppError, AppResult};

/// Join an isolated task without letting its panic skip the caller's finalization.
/// Model pins must be captured before spawning and explicitly scoped around `work`.
/// A typed application error is preserved so a feature can retain its public message.
pub(crate) async fn run<T, F>(kind: &'static str, id: i64, work: F) -> AppResult<T>
where
    T: Send + 'static,
    F: Future<Output = AppResult<T>> + Send + 'static,
{
    let result = match tokio::spawn(work).await {
        Ok(result) => result,
        Err(error) => Err(AppError::Internal(anyhow::anyhow!(error))),
    };
    if let Err(error) = &result {
        tracing::error!(job_kind = kind, job_id = id, error = %error, "background work failed");
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::{AiClient, AiSource};
    use crate::config::{AiProviderKind, Config};

    #[tokio::test]
    async fn failures_and_panics_return_to_the_owner_for_finalization() {
        assert_eq!(run("test", 1, async { Ok(7) }).await.unwrap(), 7);
        let bad = run::<(), _>("test", 2, async { Err(AppError::bad_request("retry")) }).await;
        assert!(matches!(bad, Err(AppError::BadRequest(message)) if message == "retry"));
        let panic = run::<(), _>("test", 3, async { panic!("provider task panicked") }).await;
        assert!(matches!(panic, Err(AppError::Internal(_))));
        // The owner still runs after either failure, with the original panic isolated.
        assert_eq!(run("test", 4, async { Ok(8) }).await.unwrap(), 8);
    }

    #[tokio::test]
    async fn a_job_uses_its_pin_even_when_the_global_provider_is_disabled() {
        let ai = std::sync::Arc::new(AiClient::from_config(&Config::for_tests("unused")));
        let pin = ai.active();
        let (mut off, _) = ai.active_settings();
        off.kind = AiProviderKind::None;
        ai.set_active(off, AiSource::Admin);
        assert!(!ai.enabled());
        let work_ai = ai.clone();
        let enabled = run("test", 1, pin.scope(async move { Ok(work_ai.enabled()) }))
            .await
            .unwrap();
        assert!(enabled);
        assert!(!ai.enabled());
    }
}
