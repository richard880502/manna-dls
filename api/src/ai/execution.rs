//! A provider attempt and its research trace, shared by feature-specific generation policies.

use std::time::{Duration, Instant};

use serde_json::{json, Value};
use sqlx::PgPool;

use super::pipeline::{self, Finish, InteractionMeta};
use super::{
    ActiveAi, ErrorKind, LlmRequest, LlmResponse, ProviderError, Runaway, RUNAWAY_EXCERPT_KEY,
};
use crate::checks::{self, CheckResult};
use crate::config::AiProviderKind;
use crate::error::AppResult;

// ---------------------------------------------------------------- one model call

pub(crate) struct Attempt {
    pub(crate) id: i64,
    pub(crate) latency_ms: i32,
    pub(crate) response: Result<LlmResponse, ProviderError>,
}

pub(crate) enum Failure {
    Timeout,
    Error(ErrorKind),
    /// Refusal, unreadable output or a runaway to `max_tokens`: worth one changed retry.
    Retryable,
}

pub(crate) async fn attempt(
    db: &PgPool,
    ai: &ActiveAi,
    meta: &InteractionMeta,
    snapshot: &Value,
    req: &LlmRequest,
    retry_of: Option<i64>,
) -> AppResult<Attempt> {
    let id = pipeline::insert_interaction(
        db,
        meta,
        ai.provider_name(),
        &ai.model,
        snapshot,
        req.attempt,
        retry_of,
    )
    .await?;
    if ai.kind == AiProviderKind::Fake && req.user.contains("FAKE_SLOW") {
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
    // A run whose progress a test watches step by step (docs/API.md「產生進度」).
    if ai.kind == AiProviderKind::Fake && req.user.contains("FAKE_PACED") {
        tokio::time::sleep(Duration::from_millis(400)).await;
    }
    let start = Instant::now();
    let (response, retries) = ai.complete_counted(req).await;
    let latency_ms = i32::try_from(start.elapsed().as_millis()).unwrap_or(i32::MAX);
    match &response {
        Ok(resp) => pipeline::record_usage_counted(db, id, resp.usage, retries).await?,
        Err(_) => pipeline::record_usage_counted(db, id, None, retries).await?,
    }
    Ok(Attempt {
        id,
        latency_ms,
        response,
    })
}

/// Records a call that returned JSON.
pub(crate) async fn finish_output(
    db: &PgPool,
    a: &Attempt,
    resp: &LlmResponse,
    status: &str,
    report: Value,
    citations: Vec<String>,
) -> sqlx::Result<()> {
    pipeline::finish_interaction(
        db,
        a.id,
        Finish {
            status,
            output: Some(resp.json.clone()),
            citations,
            check_results: Some(report),
            model_version: Some(resp.model_version.clone()),
            error_detail: None,
            latency_ms: a.latency_ms,
        },
    )
    .await
}

/// Records a call that did not return JSON and says what kind of failure it was.
pub(crate) async fn finish_failure(
    db: &PgPool,
    a: &Attempt,
    error: &ProviderError,
) -> sqlx::Result<Failure> {
    // What a runaway wrote is kept (shortened) so it can be looked at later; it is model output,
    // never the material itself, and its excerpt is scrubbed with the evidence when a material
    // is deleted (`materials::store::scrub_evidence`).
    let mut output = None;
    let (status, check, detail, kind) = match error {
        ProviderError::Timeout => (
            "timeout",
            None,
            Some("provider timeout".to_string()),
            Failure::Timeout,
        ),
        ProviderError::Refusal(d) => (
            "blocked",
            Some(CheckResult::fail("refusal", d.clone())),
            None,
            Failure::Retryable,
        ),
        ProviderError::InvalidOutput(d) => (
            "blocked",
            Some(CheckResult::fail("schema", d.clone())),
            None,
            Failure::Retryable,
        ),
        ProviderError::MaxTokens { partial } => {
            let runaway = Runaway::of(partial);
            output = Some(json!({
                "truncated_at_max_tokens": true,
                "runaway": runaway.kind,
                "chars": runaway.chars,
                "longest_whitespace": runaway.longest_whitespace,
                RUNAWAY_EXCERPT_KEY: runaway.excerpt,
            }));
            (
                "blocked",
                Some(CheckResult::fail("max_tokens", runaway.summary())),
                None,
                Failure::Retryable,
            )
        }
        e => (
            "error",
            None,
            Some(e.to_string()),
            Failure::Error(ErrorKind::of(e)),
        ),
    };
    if matches!(kind, Failure::Error(_)) {
        tracing::warn!(interaction_id = a.id, error = %error, "AI provider error");
    }
    pipeline::finish_interaction(
        db,
        a.id,
        Finish {
            status,
            output,
            citations: Vec::new(),
            check_results: check.map(|c| checks::report(&[c])),
            model_version: None,
            error_detail: detail,
            latency_ms: a.latency_ms,
        },
    )
    .await?;
    Ok(kind)
}

/// Marks the row the shown result is attributed to, with what the code repaired.
pub(crate) async fn set_final(
    db: &PgPool,
    id: i64,
    status: &str,
    report: Value,
) -> sqlx::Result<()> {
    sqlx::query("UPDATE ai_interactions SET status = $2, check_results = $3 WHERE id = $1")
        .bind(id)
        .bind(status)
        .bind(report)
        .execute(db)
        .await?;
    Ok(())
}

pub(crate) fn report_with(results: &[CheckResult], repairs: Value) -> Value {
    let mut report = checks::report(results);
    report["repairs"] = repairs;
    report
}
