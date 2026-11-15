//! Extraction of a template from one of the teacher's worksheets, as a background job (probe
//! v4): call A lists the outline (per-paragraph blocks once, sub-steps not separate) with the
//! header facts; call B writes exactly one section per outline item. Each call is retried once,
//! with the temperature raised by 0.4 (the gateway caches identical requests), when its output
//! ran to `max_tokens` or could not be read. The template row says how it went (`status`,
//! `error`); the browser polls `GET /api/worksheet-templates/:id`.
//!
//! Every call is an `ai_interactions` row. The worksheet's text is never logged: the snapshot
//! keeps its length and a hash, as for materials elsewhere.

use std::collections::HashMap;

use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use super::store::{self, TemplateRow, STATUS_EXTRACTING, STATUS_FAILED, STATUS_READY};
use super::{
    details_schema, from_extraction, outline_schema, RawDetails, RawOutline, WsTemplate,
    DETAILS_SCHEMA_NAME, MAX_SECTIONS, OUTLINE_SCHEMA_NAME,
};
use crate::ai::pipeline::InteractionMeta;
use crate::ai::LlmRequest;
use crate::checks::{self as base_checks, CheckResult};
use crate::error::{AppError, AppResult};
use crate::materials::store::{self as material_store, MaterialRow};
use crate::prompts;
use crate::routes::{log_usage, scaffold_id};
use crate::sop::ai::{attempt, finish_failure, finish_output, prompt, Failure};
use crate::AppState;

pub const KEY_OUTLINE: &str = "worksheet_template_outline";
pub const KEY_DETAILS: &str = "worksheet_template_details";
pub const SCAFFOLD_EXTRACT: &str = "SC-WS-TPL-EXTRACT";

/// Output budgets (probe: outline 2000, details 5000; a budget only matters on a runaway).
pub const OUTLINE_MAX_TOKENS: u32 = 2000;
pub const DETAILS_MAX_TOKENS: u32 = 5000;
const BASE_TEMPERATURE: f32 = 0.2;
const RETRY_TEMPERATURE_STEP: f32 = 0.4;
/// Longer than this is not one worksheet (the probe's longest was well under it).
pub const MAX_SOURCE_CHARS: usize = 30_000;

pub const MSG_NO_TEXT: &str =
    "這份檔案讀不到文字（可能是掃描檔）。請上傳 DOCX、文字型 PDF，或直接貼上文字。";
pub const MSG_NOT_A_DOCUMENT: &str =
    "這份教材沒有文字可以讀。請選一份學習單文件（DOCX、PDF 或貼上的文字）。";
pub const MSG_MATERIAL_NOT_FOUND: &str = "找不到這份教材。";
pub const MSG_STALE: &str = "這次讀取中斷了（可能是系統剛更新）。請按「重試」。";
const MSG_UNREADABLE: &str = "Manna 這次沒有讀出這份學習單的設計（輸出不完整）。請按「重試」再試一次；一直失敗的話，可能是檔案太長或版面太亂。";
const MSG_TIMEOUT: &str = "AI 目前沒有回應，這次沒有讀出版型。請稍後按「重試」。";
const MSG_ERROR: &str = "AI 暫時無法使用，這次沒有讀出版型。請稍後按「重試」。";
const MSG_DISABLED: &str = "這個環境沒有開啟 AI 協助，無法讀出版型。";
const MSG_INTERNAL: &str = "讀取版型時發生錯誤。請按「重試」。";

/// The source material for an extraction: one of the teacher's library materials with
/// extracted text. Anything else is a 400 with a message the teacher can act on.
pub async fn source(
    db: &sqlx::PgPool,
    teacher_id: i64,
    material_id: i64,
) -> AppResult<MaterialRow> {
    let row = material_store::owned(db, material_id, teacher_id)
        .await
        .map_err(|_| AppError::bad_request(MSG_MATERIAL_NOT_FOUND))?;
    if row.text_status == "not_applicable" {
        return Err(AppError::bad_request(MSG_NOT_A_DOCUMENT));
    }
    if row.text_status != "ready" || row.text.trim().is_empty() {
        return Err(AppError::bad_request(MSG_NO_TEXT));
    }
    if row.text.chars().count() > MAX_SOURCE_CHARS {
        return Err(AppError::bad_request(format!(
            "這份檔案的文字超過 {MAX_SOURCE_CHARS} 字，不像是一份學習單。請上傳單一份學習單。"
        )));
    }
    Ok(row)
}

/// Seconds after which an open extraction is reported as failed.
pub fn stale_after(state: &AppState) -> f64 {
    (state.config.ai_timeout_secs * 6 + 60) as f64
}

/// Starts the background job for `row` (already `extracting`) on `text`.
pub fn spawn(state: &AppState, row: &TemplateRow, text: String) {
    let state = state.clone();
    let (id, teacher_id, run) = (row.id, row.teacher_id, row.extract_run);
    tokio::spawn(async move {
        let work = {
            let state = state.clone();
            tokio::spawn(async move { extract(&state, id, teacher_id, run, &text).await })
        };
        let outcome = match work.await {
            Ok(Ok(result)) => result,
            Ok(Err(e)) => {
                tracing::error!(template_id = id, error = %e, "template extraction failed");
                Err(MSG_INTERNAL.to_string())
            }
            Err(e) => {
                tracing::error!(template_id = id, error = %e, "template extraction panicked");
                Err(MSG_INTERNAL.to_string())
            }
        };
        if let Err(e) = finish(&state, id, teacher_id, run, outcome).await {
            tracing::error!(template_id = id, error = %e, "could not close the template extraction");
        }
    });
}

/// Stores the result, unless the template was deleted or another run took over meanwhile.
async fn finish(
    state: &AppState,
    id: i64,
    teacher_id: i64,
    run: i32,
    outcome: Result<WsTemplate, String>,
) -> AppResult<()> {
    let db = &state.db;
    let mut tx = db.begin().await?;
    let current: Option<(String, i32)> = sqlx::query_as(
        "SELECT status, extract_run FROM worksheet_templates WHERE id = $1 FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?;
    if current != Some((STATUS_EXTRACTING.to_string(), run)) {
        return Ok(());
    }
    let (status, error, version_no) = match &outcome {
        Ok(content) => {
            let no = store::add_version(&mut *tx, id, content, "ai").await?;
            (STATUS_READY, None, Some(no))
        }
        Err(message) => (STATUS_FAILED, Some(message.as_str()), None),
    };
    sqlx::query(
        "UPDATE worksheet_templates SET status = $2, error = $3, updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(status)
    .bind(error)
    .execute(&mut *tx)
    .await?;
    log_usage(
        &mut *tx,
        teacher_id,
        "worksheet_template_extracted",
        json!({
            "template_id": id, "run": run, "status": status, "version_no": version_no,
            "sections": outcome.as_ref().map(|c| c.sections.len()).unwrap_or(0),
        }),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

/// The two calls. `Ok(Err(message))`: the extraction failed, `message` says why (Chinese).
pub async fn extract(
    state: &AppState,
    id: i64,
    teacher_id: i64,
    run: i32,
    text: &str,
) -> AppResult<Result<WsTemplate, String>> {
    let ctx = CallCtx {
        state,
        template_id: id,
        teacher_id,
        run,
    };
    // ---- call A: outline and header facts
    let outline: RawOutline = match ctx
        .call(
            Call {
                key: KEY_OUTLINE,
                schema_name: OUTLINE_SCHEMA_NAME,
                schema: outline_schema(),
                vars: HashMap::from([("worksheet_text", text.to_string())]),
                fake_context: json!({ "task": KEY_OUTLINE, "text": text }),
                max_tokens: OUTLINE_MAX_TOKENS,
            },
            |o: &RawOutline| {
                if o.outline.is_empty() {
                    Err("outline 是空的".to_string())
                } else {
                    Ok(())
                }
            },
        )
        .await?
    {
        Ok(o) => o,
        Err(message) => return Ok(Err(message)),
    };
    let mut outline = outline;
    outline.outline.truncate(MAX_SECTIONS);
    let n = outline.outline.len();

    // ---- call B: one section per outline item
    let lines: Vec<String> = outline
        .outline
        .iter()
        .enumerate()
        .map(|(i, o)| {
            format!(
                "{}. {}{}",
                i + 1,
                o.title_pattern,
                if o.per_paragraph {
                    "（逐段重複，只寫一個 section，text_scope 寫「逐段」）"
                } else {
                    ""
                }
            )
        })
        .collect();
    let details: RawDetails = match ctx
        .call(
            Call {
                key: KEY_DETAILS,
                schema_name: DETAILS_SCHEMA_NAME,
                schema: details_schema(n),
                vars: HashMap::from([
                    ("worksheet_text", text.to_string()),
                    ("outline_count", n.to_string()),
                    ("outline_lines", lines.join("\n")),
                ]),
                fake_context: json!({
                    "task": KEY_DETAILS,
                    "outline": outline.outline,
                    "text": text,
                }),
                max_tokens: DETAILS_MAX_TOKENS,
            },
            |d: &RawDetails| {
                if d.sections.len() == n {
                    Ok(())
                } else {
                    Err(format!(
                        "sections 有 {} 項，大綱有 {n} 項",
                        d.sections.len()
                    ))
                }
            },
        )
        .await?
    {
        Ok(d) => d,
        Err(message) => return Ok(Err(message)),
    };
    Ok(Ok(from_extraction(&outline, &details)))
}

struct CallCtx<'a> {
    state: &'a AppState,
    template_id: i64,
    teacher_id: i64,
    run: i32,
}

struct Call {
    key: &'static str,
    schema_name: &'static str,
    schema: Value,
    vars: HashMap<&'static str, String>,
    fake_context: Value,
    max_tokens: u32,
}

impl CallCtx<'_> {
    /// One extraction step: a call, and one retry with a higher temperature when the output
    /// ran away, could not be read or fails `accept`. `Ok(Err(message))` when nothing usable
    /// came back.
    async fn call<R: DeserializeOwned>(
        &self,
        call: Call,
        accept: impl Fn(&R) -> Result<(), String>,
    ) -> AppResult<Result<R, String>> {
        let state = self.state;
        let db = &state.db;
        if !state.ai.enabled() {
            return Ok(Err(MSG_DISABLED.to_string()));
        }
        let prompt = prompt(db, call.key).await?;
        let system = prompts::render(&prompt.system_text, &call.vars);
        let user = prompts::render(&prompt.user_template, &call.vars);
        // The teacher's worksheet stays deletable: the log keeps its length and a hash only.
        let mut logged = call.vars.clone();
        if let Some(text) = logged.get("worksheet_text").cloned() {
            logged.insert("worksheet_text", prompts::redacted("教師的學習單", &text));
        }
        let logged_user = prompts::render(&prompt.user_template, &logged);
        let meta = InteractionMeta {
            teacher_id: self.teacher_id,
            module: call.key,
            unit_id: None,
            lesson_id: None,
            scaffold_id: scaffold_id(db, SCAFFOLD_EXTRACT).await?,
            prompt_version_id: Some(prompt.id),
        };
        let mut retry_of: Option<i64> = None;
        for attempt_no in 1..=2 {
            let temperature = BASE_TEMPERATURE + RETRY_TEMPERATURE_STEP * (attempt_no - 1) as f32;
            let snapshot = json!({
                "inputs": logged,
                "template": { "id": self.template_id, "extract_run": self.run },
                "prompt": { "key": call.key, "version_no": prompt.version_no,
                            "system": system, "user": logged_user },
                "schema": call.schema_name,
                "request": { "temperature": temperature, "max_tokens": call.max_tokens },
                "retry_of": retry_of,
            });
            let req = LlmRequest {
                system: system.clone(),
                user: user.clone(),
                schema_name: call.schema_name,
                schema: call.schema.clone(),
                fake_context: call.fake_context.clone(),
                attempt: attempt_no,
                max_tokens: Some(call.max_tokens),
                temperature: Some(temperature),
            };
            let a = attempt(state, &meta, &snapshot, &req, retry_of).await?;
            retry_of = Some(a.id);
            match &a.response {
                Ok(resp) => {
                    let parsed = serde_json::from_value::<R>(resp.json.clone())
                        .map_err(|e| ("schema", e.to_string()))
                        .and_then(|r| accept(&r).map(|()| r).map_err(|e| ("shape", e)));
                    match parsed {
                        Ok(r) => {
                            let report = base_checks::report(&[
                                CheckResult::pass("schema"),
                                CheckResult::pass("shape"),
                            ]);
                            finish_output(db, &a, resp, "shown", report, Vec::new()).await?;
                            return Ok(Ok(r));
                        }
                        Err((check, detail)) => {
                            let report = base_checks::report(&[CheckResult::fail(check, detail)]);
                            finish_output(db, &a, resp, "blocked", report, Vec::new()).await?;
                        }
                    }
                }
                Err(e) => match finish_failure(db, &a, e).await? {
                    Failure::Timeout => return Ok(Err(MSG_TIMEOUT.to_string())),
                    Failure::Error => {
                        let message = if matches!(e, crate::ai::ProviderError::Disabled) {
                            MSG_DISABLED
                        } else {
                            MSG_ERROR
                        };
                        return Ok(Err(message.to_string()));
                    }
                    Failure::Retryable => {}
                },
            }
        }
        Ok(Err(MSG_UNREADABLE.to_string()))
    }
}
