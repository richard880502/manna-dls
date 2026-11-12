//! The shared AI call flow (spec §5.2):
//!
//! 1. insert `ai_interactions` (status `pending`, full input snapshot, prompt version,
//!    scaffold, provider, model);
//! 2. call the provider with a timeout;
//! 3. run the automatic checks; on failure regenerate once — attempt 2 is its own
//!    `ai_interactions` row (`attempt = 2`, `retry_of` = attempt 1), so the blocked
//!    output of attempt 1 stays recorded;
//! 4. still failing → `blocked`; passing → `shown`; timeout → `timeout`; other provider
//!    failures → `error`. No other model is ever tried.

use std::time::Instant;

use serde::Serialize;
use serde_json::{json, Value};
use sqlx::PgPool;

use super::{LlmRequest, ProviderError};
use crate::checks::{self, CheckResult};
use crate::error::AppResult;
use crate::AppState;

pub const MSG_BLOCKED: &str = "目前暫無建議，你可以繼續手動完成。";
pub const MSG_TIMEOUT: &str = "AI 目前沒有回應，這項協助先暫停；你可以繼續手動完成。";
pub const MSG_ERROR: &str = "AI 暫時無法使用，這項協助先暫停；你可以繼續手動完成。";
pub const MSG_DISABLED: &str = "這個環境沒有開啟 AI 協助，你可以繼續手動完成。";

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Source {
    pub item_code: String,
    pub title: String,
    pub category: String,
    pub source_doc: String,
    pub source_page: String,
}

/// One piece of information Manna injected into an AI call (「Manna 已帶入的資訊」).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AiContextItem {
    pub label: String,
    pub preview: String,
}

/// Longest preview shown for a context item, in characters (before the ellipsis).
pub const CONTEXT_PREVIEW_CHARS: usize = 60;

/// Collapses whitespace and clips to [`CONTEXT_PREVIEW_CHARS`] characters.
pub fn preview(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= CONTEXT_PREVIEW_CHARS {
        flat
    } else {
        let mut out: String = flat.chars().take(CONTEXT_PREVIEW_CHARS).collect();
        out.push('…');
        out
    }
}

impl AiContextItem {
    pub fn new(label: impl Into<String>, text: &str) -> Self {
        Self {
            label: label.into(),
            preview: preview(text),
        }
    }
}

/// Builds a context list, skipping items whose text is blank.
#[derive(Debug, Default)]
pub struct ContextList(Vec<AiContextItem>);

impl ContextList {
    pub fn add(&mut self, label: &str, text: &str) -> &mut Self {
        if !text.trim().is_empty() {
            self.0.push(AiContextItem::new(label, text));
        }
        self
    }

    pub fn into_vec(self) -> Vec<AiContextItem> {
        self.0
    }
}

/// `AiResult<T>` from docs/API.md.
#[derive(Debug, Serialize)]
pub struct AiResult<T: Serialize> {
    pub interaction_id: Option<i64>,
    pub status: &'static str,
    pub message: Option<String>,
    pub output: Option<T>,
    pub sources: Vec<Source>,
    /// What Manna injected into the call (empty when AI is disabled and nothing was sent).
    pub context: Vec<AiContextItem>,
}

impl<T: Serialize> AiResult<T> {
    pub fn disabled() -> Self {
        Self::not_shown(None, "disabled", MSG_DISABLED)
    }

    fn not_shown(interaction_id: Option<i64>, status: &'static str, msg: &str) -> Self {
        Self {
            interaction_id,
            status,
            message: Some(msg.to_string()),
            output: None,
            sources: Vec::new(),
            context: Vec::new(),
        }
    }

    pub fn shown(interaction_id: i64, output: T, sources: Vec<Source>) -> Self {
        Self {
            interaction_id: Some(interaction_id),
            status: "shown",
            message: None,
            output: Some(output),
            sources,
            context: Vec::new(),
        }
    }

    pub fn with_context(mut self, context: Vec<AiContextItem>) -> Self {
        self.context = context;
        self
    }
}

pub struct InteractionMeta {
    pub teacher_id: i64,
    pub module: &'static str,
    pub unit_id: Option<i64>,
    pub lesson_id: Option<i64>,
    pub scaffold_id: Option<i64>,
    pub prompt_version_id: Option<i64>,
}

pub enum Outcome<T> {
    Shown { interaction_id: i64, output: T },
    Blocked { interaction_id: i64 },
    Timeout { interaction_id: i64 },
    Error { interaction_id: i64 },
}

impl<T: Serialize> Outcome<T> {
    /// Converts to the API result; `sources` is called only for shown outputs.
    pub fn into_result(self, sources: impl FnOnce(&T) -> Vec<Source>) -> AiResult<T> {
        match self {
            Outcome::Shown {
                interaction_id,
                output,
            } => {
                let s = sources(&output);
                AiResult::shown(interaction_id, output, s)
            }
            Outcome::Blocked { interaction_id } => {
                AiResult::not_shown(Some(interaction_id), "blocked", MSG_BLOCKED)
            }
            Outcome::Timeout { interaction_id } => {
                AiResult::not_shown(Some(interaction_id), "timeout", MSG_TIMEOUT)
            }
            Outcome::Error { interaction_id } => {
                AiResult::not_shown(Some(interaction_id), "error", MSG_ERROR)
            }
        }
    }

    pub fn interaction_id(&self) -> i64 {
        match self {
            Outcome::Shown { interaction_id, .. }
            | Outcome::Blocked { interaction_id }
            | Outcome::Timeout { interaction_id }
            | Outcome::Error { interaction_id } => *interaction_id,
        }
    }
}

/// Result of parsing + checking one raw model output.
pub struct Checked<T> {
    /// `None` when the output did not match the expected shape.
    pub output: Option<T>,
    pub results: Vec<CheckResult>,
    pub citations: Vec<String>,
}

/// Parses `raw` into `T`; a shape mismatch becomes a failed `schema` check.
pub fn parse_and_check<T: serde::de::DeserializeOwned>(
    raw: &Value,
    check: impl FnOnce(&T) -> (Vec<CheckResult>, Vec<String>),
) -> Checked<T> {
    match serde_json::from_value::<T>(raw.clone()) {
        Ok(out) => {
            let (mut results, citations) = check(&out);
            results.insert(0, CheckResult::pass("schema"));
            Checked {
                output: Some(out),
                results,
                citations,
            }
        }
        Err(e) => Checked {
            output: None,
            results: vec![CheckResult::fail("schema", e.to_string())],
            citations: Vec::new(),
        },
    }
}

pub async fn insert_interaction(
    db: &PgPool,
    meta: &InteractionMeta,
    provider: &str,
    model: &str,
    snapshot: &Value,
    attempt: i32,
    retry_of: Option<i64>,
) -> sqlx::Result<i64> {
    sqlx::query_scalar(
        "INSERT INTO ai_interactions
           (teacher_id, module, unit_id, lesson_id, scaffold_id, prompt_version_id,
            input_snapshot, provider, model, attempt, retry_of, status)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, 'pending')
         RETURNING id",
    )
    .bind(meta.teacher_id)
    .bind(meta.module)
    .bind(meta.unit_id)
    .bind(meta.lesson_id)
    .bind(meta.scaffold_id)
    .bind(meta.prompt_version_id)
    .bind(snapshot)
    .bind(provider)
    .bind(model)
    .bind(attempt)
    .bind(retry_of)
    .fetch_one(db)
    .await
}

pub struct Finish<'a> {
    pub status: &'a str,
    pub output: Option<Value>,
    pub citations: Vec<String>,
    pub check_results: Option<Value>,
    pub model_version: Option<String>,
    pub error_detail: Option<String>,
    pub latency_ms: i32,
}

pub async fn finish_interaction(db: &PgPool, id: i64, f: Finish<'_>) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE ai_interactions
            SET status = $2, output = $3, citations = $4, check_results = $5,
                model_version = $6, error_detail = $7, latency_ms = $8
          WHERE id = $1",
    )
    .bind(id)
    .bind(f.status)
    .bind(f.output)
    .bind(f.citations)
    .bind(f.check_results)
    .bind(f.model_version)
    .bind(f.error_detail)
    .bind(f.latency_ms)
    .execute(db)
    .await?;
    Ok(())
}

fn elapsed_ms(start: Instant) -> i32 {
    i32::try_from(start.elapsed().as_millis()).unwrap_or(i32::MAX)
}

/// Runs attempt 1 (row `first_id`, already inserted) and at most one regeneration.
pub async fn run<T, F>(
    state: &AppState,
    meta: &InteractionMeta,
    first_id: i64,
    snapshot: &Value,
    mut req: LlmRequest,
    check: F,
) -> AppResult<Outcome<T>>
where
    T: Serialize,
    F: Fn(&Value) -> Checked<T>,
{
    let ai = &state.ai;
    let mut interaction_id = first_id;
    for attempt in 1..=2 {
        if attempt > 1 {
            let mut snap = snapshot.clone();
            snap["retry_of"] = json!(first_id);
            interaction_id = insert_interaction(
                &state.db,
                meta,
                ai.provider_name(),
                &ai.model,
                &snap,
                attempt,
                Some(first_id),
            )
            .await?;
        }
        req.attempt = attempt;
        let start = Instant::now();
        let result = ai.complete(&req).await;
        let latency_ms = elapsed_ms(start);

        let (failure_results, raw_output, model_version) = match result {
            Ok(resp) => {
                let checked = check(&resp.json);
                let passed = checks::all_passed(&checked.results);
                let report = checks::report(&checked.results);
                let status = if passed { "shown" } else { "blocked" };
                finish_interaction(
                    &state.db,
                    interaction_id,
                    Finish {
                        status,
                        output: Some(resp.json.clone()),
                        citations: checked.citations.clone(),
                        check_results: Some(report),
                        model_version: Some(resp.model_version.clone()),
                        error_detail: None,
                        latency_ms,
                    },
                )
                .await?;
                match (passed, checked.output) {
                    (true, Some(output)) => {
                        return Ok(Outcome::Shown {
                            interaction_id,
                            output,
                        })
                    }
                    _ => {
                        tracing::info!(interaction_id, attempt, "AI output failed checks");
                        continue;
                    }
                }
            }
            Err(ProviderError::Refusal(detail)) => (
                vec![CheckResult::fail("refusal", detail)],
                None,
                None::<String>,
            ),
            Err(ProviderError::InvalidOutput(detail)) => {
                (vec![CheckResult::fail("schema", detail)], None, None)
            }
            Err(ProviderError::Timeout) => {
                finish_interaction(
                    &state.db,
                    interaction_id,
                    Finish {
                        status: "timeout",
                        output: None,
                        citations: Vec::new(),
                        check_results: None,
                        model_version: None,
                        error_detail: Some("provider timeout".into()),
                        latency_ms,
                    },
                )
                .await?;
                return Ok(Outcome::Timeout { interaction_id });
            }
            Err(e) => {
                tracing::warn!(interaction_id, error = %e, "AI provider error");
                finish_interaction(
                    &state.db,
                    interaction_id,
                    Finish {
                        status: "error",
                        output: None,
                        citations: Vec::new(),
                        check_results: None,
                        model_version: None,
                        error_detail: Some(e.to_string()),
                        latency_ms,
                    },
                )
                .await?;
                return Ok(Outcome::Error { interaction_id });
            }
        };
        // Refusal or unparseable output: recorded as a failed check, then regenerate once.
        finish_interaction(
            &state.db,
            interaction_id,
            Finish {
                status: "blocked",
                output: raw_output,
                citations: Vec::new(),
                check_results: Some(checks::report(&failure_results)),
                model_version,
                error_detail: None,
                latency_ms,
            },
        )
        .await?;
    }
    Ok(Outcome::Blocked { interaction_id })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_flattens_and_clips() {
        assert_eq!(preview("  高二\n普通班  30 人 "), "高二 普通班 30 人");
        let long = "字".repeat(80);
        let p = preview(&long);
        assert_eq!(p.chars().count(), CONTEXT_PREVIEW_CHARS + 1);
        assert!(p.ends_with('…'));
    }

    #[test]
    fn context_list_skips_blank_items() {
        let mut c = ContextList::default();
        c.add("班級特性", "  ").add("我的範例", "舊學習單");
        let v = c.into_vec();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].label, "我的範例");
        let r: AiResult<()> = AiResult::disabled();
        assert!(r.context.is_empty());
        let json = serde_json::to_value(r.with_context(v)).unwrap();
        assert_eq!(json["context"][0]["preview"], "舊學習單");
    }
}
