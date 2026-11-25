//! Transport-level retry and quota protection shared by model calls.
use super::ProviderError;
use rand::Rng;
use sqlx::PgPool;
use std::{future::Future, sync::Mutex, sync::OnceLock, time::Duration};

pub struct Guard {
    until: Mutex<Option<tokio::time::Instant>>,
    retries: u32,
    cooldown: Duration,
    /// Where the one-time `ai_quota_exhausted` event is written; unset in unit tests.
    events: OnceLock<PgPool>,
}

impl Guard {
    pub fn new(retries: u32, cooldown: Duration) -> Self {
        Self {
            until: Mutex::new(None),
            retries,
            cooldown,
            events: OnceLock::new(),
        }
    }

    /// Lets the guard record `ai_quota_exhausted` (no content) when a cooldown starts.
    pub fn attach_db(&self, db: PgPool) {
        let _ = self.events.set(db);
    }

    /// When the quota cooldown ends, or `None` when calls are not blocked.
    pub fn blocked_until(&self) -> Option<tokio::time::Instant> {
        (*self.until.lock().unwrap_or_else(|e| e.into_inner()))
            .filter(|t| *t > tokio::time::Instant::now())
    }
    pub fn clear(&self) {
        *self.until.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    /// Starts the cooldown. True only for the call that actually entered it (not for calls
    /// already in flight that fail with the same quota error afterwards).
    fn enter_cooldown(&self) -> bool {
        let now = tokio::time::Instant::now();
        let mut until = self.until.lock().unwrap_or_else(|e| e.into_inner());
        let fresh = until.is_none_or(|t| t <= now);
        *until = Some(now + self.cooldown);
        fresh
    }

    pub async fn run<T, F, Fut>(&self, timeout: Duration, call: F) -> Result<T, ProviderError>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T, ProviderError>>,
    {
        self.run_counted(timeout, call).await.0
    }

    /// Like [`Guard::run`], also returning how many times the call was repeated.
    pub async fn run_counted<T, F, Fut>(
        &self,
        timeout: Duration,
        mut call: F,
    ) -> (Result<T, ProviderError>, u32)
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T, ProviderError>>,
    {
        if self
            .until
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some_and(|t| t > tokio::time::Instant::now())
        {
            return (
                Err(ProviderError::QuotaExceeded {
                    body: "quota cooldown".into(),
                }),
                0,
            );
        }
        let mut rate_retries = 0;
        let mut overload_retried = false;
        loop {
            let result = tokio::time::timeout(timeout, call())
                .await
                .unwrap_or(Err(ProviderError::Timeout));
            let retried = rate_retries + u32::from(overload_retried);
            match result {
                Err(ProviderError::QuotaExceeded { body }) => {
                    if self.enter_cooldown() {
                        if let Some(db) = self.events.get().cloned() {
                            tokio::spawn(async move {
                                let _ = sqlx::query(
                                    "INSERT INTO usage_logs (event, payload) VALUES ('ai_quota_exhausted', '{}')",
                                )
                                .execute(&db)
                                .await;
                            });
                        }
                    }
                    return (Err(ProviderError::QuotaExceeded { body }), retried);
                }
                Err(ProviderError::RateLimited { retry_after, .. })
                    if rate_retries < self.retries =>
                {
                    let fallback =
                        Duration::from_secs(2u64.saturating_pow(rate_retries.saturating_add(1)));
                    rate_retries += 1;
                    let jitter = Duration::from_millis(rand::thread_rng().gen_range(0..=500));
                    tokio::time::sleep(
                        retry_after
                            .unwrap_or(fallback)
                            .saturating_add(jitter)
                            .min(Duration::from_secs(20)),
                    )
                    .await;
                }
                Err(ProviderError::Overloaded { .. }) if !overload_retried => {
                    overload_retried = true;
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
                other => return (other, retried),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::time::Instant;

    fn rate(secs: u64) -> ProviderError {
        ProviderError::RateLimited {
            retry_after: Some(Duration::from_secs(secs)),
            body: String::new(),
        }
    }

    #[tokio::test]
    async fn quota_blocks_calls_until_cleared() {
        let guard = Guard::new(2, Duration::from_secs(600));
        let calls = AtomicUsize::new(0);
        let first: Result<(), _> = guard
            .run(Duration::from_secs(1), || async {
                calls.fetch_add(1, Ordering::SeqCst);
                Err(ProviderError::QuotaExceeded {
                    body: String::new(),
                })
            })
            .await;
        assert!(matches!(first, Err(ProviderError::QuotaExceeded { .. })));
        assert!(guard.blocked_until().is_some());
        let second = guard
            .run(Duration::from_secs(1), || async {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
            .await;
        assert!(matches!(second, Err(ProviderError::QuotaExceeded { .. })));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        guard.clear();
        assert!(guard.blocked_until().is_none());
        assert!(guard
            .run(Duration::from_secs(1), || async {
                Ok::<_, ProviderError>(())
            })
            .await
            .is_ok());
    }

    #[tokio::test(start_paused = true)]
    async fn rate_limit_is_retried_with_the_given_delay_then_returned() {
        let guard = Guard::new(2, Duration::from_secs(600));
        let calls = AtomicUsize::new(0);
        let start = Instant::now();
        let (result, retries): (Result<(), _>, _) = guard
            .run_counted(Duration::from_secs(60), || async {
                calls.fetch_add(1, Ordering::SeqCst);
                Err(rate(3))
            })
            .await;
        assert!(matches!(result, Err(ProviderError::RateLimited { .. })));
        assert_eq!(calls.load(Ordering::SeqCst), 3, "first call + 2 retries");
        assert_eq!(retries, 2);
        // Two waits of 3 s plus at most 0.5 s jitter each.
        let waited = start.elapsed();
        assert!(waited >= Duration::from_secs(6) && waited <= Duration::from_millis(7001));
    }

    #[tokio::test(start_paused = true)]
    async fn rate_limit_that_clears_succeeds() {
        let guard = Guard::new(2, Duration::from_secs(600));
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let (result, retries) = guard
            .run_counted(Duration::from_secs(60), move || {
                let n = c.fetch_add(1, Ordering::SeqCst);
                async move {
                    if n == 0 {
                        Err(rate(1))
                    } else {
                        Ok(7)
                    }
                }
            })
            .await;
        assert_eq!(result, Ok(7));
        assert_eq!(retries, 1);
    }

    #[tokio::test(start_paused = true)]
    async fn server_errors_are_retried_once() {
        let guard = Guard::new(2, Duration::from_secs(600));
        let calls = AtomicUsize::new(0);
        let start = Instant::now();
        let (result, retries): (Result<(), _>, _) = guard
            .run_counted(Duration::from_secs(60), || async {
                calls.fetch_add(1, Ordering::SeqCst);
                Err(ProviderError::Overloaded {
                    status: 503,
                    body: String::new(),
                })
            })
            .await;
        assert!(matches!(
            result,
            Err(ProviderError::Overloaded { status: 503, .. })
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(retries, 1);
        assert_eq!(start.elapsed(), Duration::from_secs(2));
    }

    #[tokio::test(start_paused = true)]
    async fn timeouts_are_not_retried() {
        let guard = Guard::new(2, Duration::from_secs(600));
        let calls = AtomicUsize::new(0);
        let (result, retries): (Result<(), _>, _) = guard
            .run_counted(Duration::from_secs(5), || async {
                calls.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_secs(60)).await;
                Ok(())
            })
            .await;
        assert_eq!(result, Err(ProviderError::Timeout));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(retries, 0);
    }

    #[tokio::test(start_paused = true)]
    async fn the_cooldown_is_entered_once() {
        let guard = Guard::new(2, Duration::from_secs(600));
        assert!(guard.enter_cooldown());
        assert!(!guard.enter_cooldown(), "already cooling down");
        guard.clear();
        assert!(guard.enter_cooldown());
        // Expired cooldowns can be entered again.
        tokio::time::advance(Duration::from_secs(601)).await;
        assert!(guard.enter_cooldown());
    }
}
