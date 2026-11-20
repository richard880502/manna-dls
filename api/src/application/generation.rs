//! Checked structured generation with one repair attempt and a usable-output fallback.
//!
//! Features supply required-context rules and content validators. This runner owns
//! prompt rendering/redaction, attempt tracing, repair requests and progress updates.

use std::collections::HashMap;
use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use super::ai::{attempt, finish_failure, finish_output, set_final, Failure};
use crate::ai::pipeline::InteractionMeta;
use crate::ai::LlmRequest;
use crate::audit::scaffold_id;
use crate::checks::{self as base_checks, CheckResult};
use crate::error::AppResult;
use crate::progress::{self, Tracker};
use crate::prompts::{self, latest_required as prompt};
use crate::text::clip_chars;
use crate::AppState;

const RETRY_TEMPERATURE_STEP: f32 = 0.4;
pub(crate) const MSG_UNREADABLE: &str =
    "Manna 這次沒有交出可以用的內容（輸出不完整或沒有通過檢查）。請按「再試一次」。";
pub(crate) const MSG_TIMEOUT: &str = "AI 目前沒有回應，這次沒有完成。請稍後按「再試一次」。";
pub(crate) const MSG_ERROR: &str = "AI 暫時無法使用，這次沒有完成。請稍後按「再試一次」。";
pub(crate) const MSG_DISABLED: &str = "這個環境沒有開啟 AI 協助。";

// ---------------------------------------------------------------- one model call, retried once

pub(crate) struct Call {
    pub(crate) key: &'static str,
    pub(crate) scaffold: &'static str,
    pub(crate) schema_name: &'static str,
    pub(crate) schema: Value,
    pub(crate) vars: HashMap<&'static str, String>,
    /// Placeholders that must reach the model even with a prompt version that lacks them.
    pub(crate) required: Vec<(&'static str, &'static str)>,
    /// Variables logged only as their length and a hash.
    pub(crate) redact: Vec<(&'static str, &'static str)>,
    pub(crate) fake_context: Value,
    pub(crate) max_tokens: u32,
    /// The first attempt's temperature (the retry adds 0.4).
    pub(crate) temperature: f32,
    /// Logged with every attempt (ids, never text).
    pub(crate) trace: Value,
    /// The step of a run's progress the call is (its retry is said there as 「重試中」).
    pub(crate) progress: Option<(Arc<Tracker>, &'static str)>,
    /// Pictures the teacher attached; sent after the user message's text.
    pub(crate) images: Vec<crate::ai::LlmImage>,
}

/// The output of an attempt after the server's checks: what can be used, and what to tell the
/// model in the retry.
pub(crate) struct Checked<T> {
    pub(crate) output: Option<T>,
    pub(crate) problems: Vec<String>,
}

/// One call and, when the output ran away, could not be read or has problems, one retry with
/// a higher temperature and the problems listed. The retry's usable output wins; when it has
/// none, the first attempt's usable output (if any) is used. `Ok(Err(message))`: nothing usable.
pub(crate) async fn call<R, T>(
    state: &AppState,
    teacher_id: i64,
    c: Call,
    context_requirements: impl FnOnce(&prompts::PromptVersion) -> Vec<(&'static str, &'static str)>,
    check: impl Fn(&R) -> Checked<T>,
    on_first: impl AsyncFnOnce(i64) -> AppResult<()>,
) -> AppResult<Result<(T, i64), String>>
where
    R: DeserializeOwned,
{
    call_in(
        state,
        teacher_id,
        (None, None),
        c,
        context_requirements,
        check,
        on_first,
    )
    .await
}

/// [`call`] for work that belongs to a unit and lesson (the 課例 SOP steps): `scope` is
/// `(unit_id, lesson_id)` of the logged interactions.
pub(crate) async fn call_in<R, T>(
    state: &AppState,
    teacher_id: i64,
    scope: (Option<i64>, Option<i64>),
    c: Call,
    context_requirements: impl FnOnce(&prompts::PromptVersion) -> Vec<(&'static str, &'static str)>,
    check: impl Fn(&R) -> Checked<T>,
    on_first: impl AsyncFnOnce(i64) -> AppResult<()>,
) -> AppResult<Result<(T, i64), String>>
where
    R: DeserializeOwned,
{
    // Keep all attempts of this generation on the same model. `pinned` preserves an
    // enclosing job's pin, and otherwise snapshots the settings at this entry point.
    state
        .ai
        .pinned(async move {
            let db = &state.db;
            if !state.ai.enabled() {
                return Ok(Err(MSG_DISABLED.to_string()));
            }
            let prompt = prompt(db, c.key).await?;
            let mut required = c.required.clone();
            required.extend(context_requirements(&prompt));
            let system = prompts::render(&prompt.system_text, &c.vars);
            let user = prompts::render_user_with(&prompt, &c.vars, &required);
            let mut logged = c.vars.clone();
            for (name, label) in &c.redact {
                if let Some(v) = logged.get(name).cloned() {
                    logged.insert(name, prompts::redacted(label, &v));
                }
            }
            let logged_system = prompts::render(&prompt.system_text, &logged);
            let logged_user = prompts::render_user_with(&prompt, &logged, &required);
            let meta = InteractionMeta {
                teacher_id,
                module: c.key,
                unit_id: scope.0,
                lesson_id: scope.1,
                scaffold_id: scaffold_id(db, c.scaffold).await?,
                prompt_version_id: Some(prompt.id),
            };
            let mut retry_of: Option<i64> = None;
            let mut note = String::new();
            let mut first: Option<(T, i64, Value)> = None;
            let mut on_first = Some(on_first);
            for attempt_no in 1..=2 {
                if attempt_no == 2 {
                    if let Some((tracker, key)) = &c.progress {
                        tracker.note(key, Some(progress::NOTE_RETRY)).await;
                    }
                }
                let temperature = c.temperature + RETRY_TEMPERATURE_STEP * (attempt_no - 1) as f32;
                let snapshot = json!({
                    "inputs": logged,
                    "prep": c.trace,
                    "prompt": { "key": c.key, "version_no": prompt.version_no,
                                "system": logged_system, "user": format!("{logged_user}{note}") },
                    "schema": c.schema_name,
                    "request": { "temperature": temperature, "max_tokens": c.max_tokens },
                    "retry_of": retry_of,
                });
                let req = LlmRequest {
                    system: system.clone(),
                    user: format!("{user}{note}"),
                    schema_name: c.schema_name,
                    schema: c.schema.clone(),
                    fake_context: c.fake_context.clone(),
                    attempt: attempt_no,
                    max_tokens: Some(c.max_tokens),
                    temperature: Some(temperature),
                    images: c.images.clone(),
                };
                let a = attempt(state, &meta, &snapshot, &req, retry_of).await?;
                if let Some(f) = on_first.take() {
                    f(a.id).await?;
                }
                retry_of = Some(a.id);
                let problems: Vec<String> = match &a.response {
                    Ok(resp) => match serde_json::from_value::<R>(resp.json.clone()) {
                        Ok(raw) => {
                            let checked = check(&raw);
                            let passed = checked.problems.is_empty();
                            let results = if passed {
                                vec![CheckResult::pass("schema"), CheckResult::pass("checks")]
                            } else {
                                vec![
                                    CheckResult::pass("schema"),
                                    CheckResult::fail("checks", checked.problems.join("；")),
                                ]
                            };
                            let report = base_checks::report(&results);
                            match checked.output {
                                // A usable output from the retry is what the teacher gets, problems and
                                // all (the parts that failed were taken out by the check).
                                Some(output) if passed || attempt_no == 2 => {
                                    finish_output(db, &a, resp, "shown", report, Vec::new()).await?;
                                    return Ok(Ok((output, a.id)));
                                }
                                Some(output) => {
                                    finish_output(db, &a, resp, "blocked", report.clone(), Vec::new())
                                        .await?;
                                    first = Some((output, a.id, report));
                                }
                                None => {
                                    finish_output(db, &a, resp, "blocked", report, Vec::new()).await?;
                                }
                            }
                            if checked.problems.is_empty() {
                                vec!["上一次沒有交出可用的內容。".to_string()]
                            } else {
                                checked.problems
                            }
                        }
                        Err(e) => {
                            let report = base_checks::report(&[CheckResult::fail("schema", e.to_string())]);
                            finish_output(db, &a, resp, "blocked", report, Vec::new()).await?;
                            vec!["上一次的輸出不符合指定的 JSON 格式。".to_string()]
                        }
                    },
                    Err(e) => match finish_failure(db, &a, e).await? {
                        // The retry did not answer: what was usable in the first attempt is kept.
                        Failure::Timeout | Failure::Error if first.is_some() => break,
                        Failure::Timeout => return Ok(Err(MSG_TIMEOUT.to_string())),
                        Failure::Error => {
                            let message = if matches!(e, crate::ai::ProviderError::Disabled) {
                                MSG_DISABLED
                            } else {
                                MSG_ERROR
                            };
                            return Ok(Err(message.to_string()));
                        }
                        Failure::Retryable => vec!["上一次沒有交出完整的輸出。".to_string()],
                    },
                };
                note = format!(
                    "\n\n【重要：這是第二次請求】上一次的輸出沒有通過平台的檢查。請修正下列問題後，重新輸出完整的 JSON：\n{}",
                    problems
                        .iter()
                        .take(12)
                        .map(|p| format!("- {}", clip_chars(p, 300)))
                        .collect::<Vec<_>>()
                        .join("\n")
                );
            }
            if let Some((output, id, report)) = first {
                set_final(db, id, "shown", report).await?;
                return Ok(Ok((output, id)));
            }
            Ok(Err(MSG_UNREADABLE.to_string()))
        })
        .await
}

pub(crate) async fn nothing(_: i64) -> AppResult<()> {
    Ok(())
}
