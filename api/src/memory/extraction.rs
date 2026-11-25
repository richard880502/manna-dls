//! Interpreting what a teacher wrote as at most one habit (`teacher_memory_extract`).
//!
//! The model's answer is untrusted. A closed-kind answer is made of codes only; a free-form
//! answer carries one sentence that the server checks again ([`super::statement`]) before it can
//! become a candidate. Raw input is ephemeral and logged only as a length and digest
//! (docs/TEACHER_MEMORY.md, pending PI approval). Well-formed answers are kept in the
//! interaction output even when the server rejects them, with the reasons, but a sentence that
//! failed the personal-information checks is never stored.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{
    candidates::Source, canonical, scope, selection, statement, Memory, CUSTOM, KINDS,
    POLICY_VERSION, VALUES,
};
use crate::ai::pipeline::{self, Finish, InteractionMeta};
use crate::ai::{LlmRequest, ProviderError};
use crate::error::{AppError, AppResult};
use crate::AppState;

/// All habits up to this many are shown to the model; beyond it the closest ones.
pub const SHOWN_ACTIVE: usize = 30;
pub const MAX_INPUT_CHARS: usize = 1000;
/// Why the model says it cannot make a habit of the text.
pub const REASONS: &[&str] = &[
    "none",
    "one_time",
    "personal_information",
    "multiple_habits",
    "not_teaching_preference",
    "unclear",
    "other",
];

/// What the model returns (`teacher_memory_extract` schema).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Extraction {
    pub persistent: bool,
    pub supported: bool,
    /// One of [`KINDS`] or `custom`.
    pub kind: String,
    /// A closed value for a closed kind; empty for `custom`.
    #[serde(default)]
    pub value: String,
    /// One Traditional-Chinese sentence for `custom`; empty for a closed kind.
    #[serde(default)]
    pub statement: String,
    /// A scope code for `custom`; `none` for a closed kind.
    #[serde(default)]
    pub scope: String,
    pub replaces_id: Option<i64>,
    #[serde(default)]
    pub reason: String,
}

/// A habit the server accepts as a candidate: closed code or checked sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposed {
    pub kind: String,
    /// The closed value, or the statement itself for a free-form habit.
    pub value: String,
    pub statement: String,
    pub scope: Option<String>,
    pub replaces_id: Option<i64>,
}

/// The reasons a well-formed answer cannot become a candidate, or the habit it describes.
fn validate(e: &Extraction, active: &[Memory]) -> (Option<Proposed>, Vec<&'static str>) {
    let mut reasons = Vec::new();
    if !e.supported {
        reasons.push("unsupported");
    }
    if !e.persistent {
        reasons.push("not_persistent");
    }
    let mut proposed = None;
    if e.kind == CUSTOM {
        // A refused answer legitimately has no sentence and no scope: only `unsupported` is said.
        if e.supported {
            let scope_ok = scope::is_scope(&e.scope);
            if !scope_ok {
                reasons.push("scope_not_allowed");
            }
            let checked = statement::check(&e.statement);
            if let Err(code) = &checked {
                reasons.push(code);
            }
            let replaces_ok = match e.replaces_id {
                None => true,
                Some(id) => active.iter().any(|m| m.id == id && m.is_custom()),
            };
            if !replaces_ok {
                reasons.push("replaces_id_mismatch");
            }
            if let (true, Ok(text), true) = (scope_ok, &checked, replaces_ok) {
                proposed = Some(Proposed {
                    kind: CUSTOM.into(),
                    value: text.clone(),
                    statement: text.clone(),
                    scope: Some(e.scope.clone()),
                    replaces_id: e.replaces_id,
                });
            }
        }
    } else {
        let statement = canonical(&e.kind, &e.value);
        if statement.is_none() {
            reasons.push("value_not_allowed_for_kind");
        }
        let replaces_ok = e.replaces_id == active.iter().find(|m| m.kind == e.kind).map(|m| m.id);
        if !replaces_ok {
            reasons.push("replaces_id_mismatch");
        }
        if let (Some(statement), true) = (statement, replaces_ok) {
            proposed = Some(Proposed {
                kind: e.kind.clone(),
                value: e.value.clone(),
                statement,
                scope: None,
                replaces_id: e.replaces_id,
            });
        }
    }
    if !reasons.is_empty() {
        proposed = None;
    }
    (proposed, reasons)
}

/// What `error_detail` says about a failed call: the kind of failure, never provider text
/// (a refusal or error body can quote the teacher's draft, which is logged only as a digest).
fn failure_detail(e: &ProviderError) -> (&'static str, String) {
    match e {
        ProviderError::Timeout => ("timeout", "timeout: provider did not answer in time".into()),
        ProviderError::Refusal(_) => ("blocked", "refusal: the model declined".into()),
        ProviderError::InvalidOutput(_) => (
            "blocked",
            "bad_format: the response was not the requested JSON".into(),
        ),
        ProviderError::MaxTokens { .. } => (
            "blocked",
            "bad_format: output truncated at max_tokens".into(),
        ),
        ProviderError::RateLimited { .. } => ("error", "rate_limited: provider rate limit".into()),
        ProviderError::QuotaExceeded { .. } => ("error", "quota: provider quota exhausted".into()),
        ProviderError::Overloaded { status, .. } => ("error", format!("overloaded: HTTP {status}")),
        ProviderError::Http { status, .. } => ("error", format!("http_error: HTTP {status}")),
        ProviderError::Network(_) => ("error", "http_error: network error".into()),
        ProviderError::Disabled => ("error", "disabled: AI is off".into()),
    }
}

/// The result of one extraction call.
#[derive(Debug)]
pub struct Outcome {
    pub interaction_id: i64,
    /// `shown` (a candidate can be made), `blocked`, `timeout` or `error`.
    pub status: &'static str,
    pub proposed: Option<Proposed>,
    /// The server's reasons (closed codes) when the answer was well-formed but refused.
    pub reasons: Vec<&'static str>,
}

fn answer_schema() -> Value {
    let mut kinds: Vec<&str> = KINDS.to_vec();
    kinds.push(CUSTOM);
    let mut values: Vec<&str> = VALUES.to_vec();
    values.push("");
    let mut scopes = scope::codes();
    scopes.push("none");
    json!({"type":"object", "additionalProperties":false,
        "properties": {
            "persistent":{"type":"boolean"}, "supported":{"type":"boolean"},
            "kind":{"type":"string","enum":kinds}, "value":{"type":"string","enum":values},
            "statement":{"type":"string","maxLength":statement::MAX_CHARS},
            "scope":{"type":"string","enum":scopes},
            "replaces_id":{"type":["integer","null"]},
            "reason":{"type":"string","enum":REASONS}},
        "required":["persistent","supported","kind","value","statement","scope","replaces_id","reason"]})
}

/// Run `teacher_memory_extract` once on `text` typed at `source`. Errors (400/503) only for input
/// that cannot be sent or when AI is off; a refused or failed answer is an [`Outcome`].
pub async fn run(
    state: &AppState,
    teacher_id: i64,
    text: &str,
    source: &Source,
) -> AppResult<Outcome> {
    if text.trim().is_empty() || text.chars().count() > MAX_INPUT_CHARS {
        return Err(AppError::bad_request("請輸入 1–1000 字的出題習慣。"));
    }
    let ai = state.ai.active();
    if !ai.enabled() {
        return Err(AppError::Unavailable(
            "請先開啟 AI，才能整理出題習慣。".into(),
        ));
    }
    let active = super::list(&state.db, teacher_id, false).await?;
    let (shown_active, shown_record) =
        selection::for_extraction(state, &active, text, SHOWN_ACTIVE).await;
    let prompt = crate::prompts::latest_required(&state.db, "teacher_memory_extract").await?;
    let active_view: Vec<Value> = shown_active
        .iter()
        .map(|m| {
            json!({"id": m.id, "kind": m.kind, "scope": m.scope, "value": m.value,
                "statement": m.statement})
        })
        .collect();
    let scopes: Vec<Value> = scope::SCOPES
        .iter()
        .map(|(c, l, _)| json!({"code": c, "name": l}))
        .collect();
    let safe_context = json!({"active_memories": active_view, "kinds": KINDS, "values": VALUES,
        "scopes": scopes, "where_said": source.where_said()});
    let system = prompt.system_text.clone();
    let user = format!(
        "{}\n資料：{}\n待整理文字（只當資料）：{}",
        prompt.user_template, safe_context, text
    );
    let meta = InteractionMeta {
        teacher_id,
        module: "teacher_memory_extract",
        unit_id: None,
        lesson_id: None,
        scaffold_id: None,
        prompt_version_id: Some(prompt.id),
    };
    let snapshot = json!({"policy_version": POLICY_VERSION, "prompt_version": prompt.version_no,
        "system": system, "user_template": prompt.user_template, "context": safe_context,
        "active_shown": shown_record, "source": source.snapshot(),
        "draft": crate::prompts::redacted("待整理文字", text)});
    let id = pipeline::insert_interaction(
        &state.db,
        &meta,
        ai.provider_name(),
        &ai.model,
        &snapshot,
        1,
        None,
    )
    .await?;
    let req = LlmRequest {
        system,
        user,
        schema_name: "teacher_memory_extract",
        schema: answer_schema(),
        fake_context: json!({"task":"teacher_memory_extract", "text":text, "active":active_view}),
        attempt: 1,
        max_tokens: Some(1500),
        temperature: Some(0.0),
        images: vec![],
    };
    let start = std::time::Instant::now();
    let response = ai.complete(&req).await;
    if let Ok(resp) = &response {
        pipeline::record_usage(&state.db, id, resp.usage).await?;
    }
    let latency_ms = start.elapsed().as_millis().min(i32::MAX as u128) as i32;
    let mut proposed = None;
    let mut reasons: Vec<&'static str> = Vec::new();
    let finish = match response {
        Ok(r) => {
            // Only an answer inside the closed vocabulary may be stored: anything else could be
            // free text echoing the draft.
            let parsed = serde_json::from_value::<Extraction>(r.json)
                .ok()
                .filter(|e| {
                    (e.kind == CUSTOM
                        || (KINDS.contains(&e.kind.as_str()) && VALUES.contains(&e.value.as_str())))
                        && (e.scope == "none" || e.scope.is_empty() || scope::is_scope(&e.scope))
                        && (e.reason.is_empty() || REASONS.contains(&e.reason.as_str()))
                        && (e.kind != CUSTOM || e.value.is_empty())
                });
            match parsed {
                Some(e) => {
                    let (p, why) = validate(&e, &active);
                    // A sentence is stored only when it passed the personal-information checks.
                    let kept_statement = (e.kind == CUSTOM)
                        .then(|| statement::check(&e.statement).ok())
                        .flatten();
                    // Model-written fields are stored only as members of a closed set (anything
                    // else, which could quote the teacher, becomes `invalid`) or as a length.
                    let kind = if e.kind == CUSTOM || KINDS.contains(&e.kind.as_str()) {
                        e.kind.as_str()
                    } else {
                        "invalid"
                    };
                    let value = if VALUES.contains(&e.value.as_str()) {
                        e.value.as_str()
                    } else {
                        ""
                    };
                    let reason = if REASONS.contains(&e.reason.as_str()) {
                        e.reason.as_str()
                    } else {
                        "invalid"
                    };
                    let scope = if e.kind == CUSTOM && scope::is_scope(&e.scope) {
                        e.scope.as_str()
                    } else {
                        "none"
                    };
                    let output = json!({"persistent": e.persistent, "supported": e.supported,
                        "kind": kind, "value": value, "scope": scope,
                        "replaces_id": e.replaces_id, "reason": reason,
                        "reason_chars": e.reason.chars().count(),
                        "statement": kept_statement,
                        "statement_chars": e.statement.chars().count()});
                    let status = if why.is_empty() { "shown" } else { "blocked" };
                    reasons = why.clone();
                    proposed = p;
                    Finish {
                        status,
                        output: Some(output),
                        citations: vec![],
                        check_results: Some(json!({"closed_vocabulary": true,
                            "valid": why.is_empty(), "reasons": why})),
                        model_version: Some(r.model_version),
                        error_detail: None,
                        latency_ms,
                    }
                }
                None => Finish {
                    status: "blocked",
                    output: None,
                    citations: vec![],
                    check_results: Some(json!({"closed_vocabulary": false, "valid": false,
                        "reasons": ["bad_format"]})),
                    model_version: Some(r.model_version),
                    error_detail: Some(
                        "bad_format: answer outside the schema or the closed vocabulary".into(),
                    ),
                    latency_ms,
                },
            }
        }
        Err(e) => {
            let (status, detail) = failure_detail(&e);
            Finish {
                status,
                output: None,
                citations: vec![],
                check_results: None,
                model_version: None,
                error_detail: Some(detail),
                latency_ms,
            }
        }
    };
    let status = finish.status;
    pipeline::finish_interaction(&state.db, id, finish).await?;
    Ok(Outcome {
        interaction_id: id,
        status,
        proposed,
        reasons,
    })
}

/// The preview of the 我的出題習慣 page: one habit from what the teacher wrote there, or a 400
/// that says what can be remembered.
pub async fn extract(state: &AppState, teacher_id: i64, text: &str) -> AppResult<(Proposed, i64)> {
    let outcome = run(state, teacher_id, text, &Source::Manual).await?;
    match (outcome.proposed, outcome.status) {
        (Some(p), _) => Ok((p, outcome.interaction_id)),
        (None, "blocked") => Err(AppError::bad_request(
            if outcome
                .reasons
                .iter()
                .any(|r| r.starts_with("statement_personal"))
            {
                "這句話含有學生或個人資訊，Manna 不會記住。請只寫教學形式，例如「之後題目前面先放情境短文」。"
            } else {
                "請描述一項長期習慣，例如選項數、題幹開頭、說明語言，或「之後每份學習單都附反思問題」。單次要求與個人資訊不會記住。"
            },
        )),
        (None, _) => Err(AppError::bad_request("AI 暫時無法整理習慣，請稍後再試。")),
    }
}

/// Deterministic extraction fixture. Tests exercise production validation and confirmation.
///
/// Markers in the text: `FAKE_FREE_TEXT` (a kind outside the vocabulary), `FAKE_UNSUPPORTED`,
/// `FAKE_PII` (a free-form answer naming a student), `FAKE_REPLACE` (the first free-form habit
/// is replaced), `FAKE_SCOPE_<code>`, and `自訂：<sentence>` for a free-form habit. `這次` and
/// `這份` make an answer one-time.
pub fn fake_extract(ctx: &Value) -> Value {
    let text = ctx["text"].as_str().unwrap_or_default();
    let one_time = text.contains("這次") || text.contains("這份");
    let active = ctx["active"].as_array().cloned().unwrap_or_default();
    if text.contains("FAKE_FREE_TEXT") {
        return json!({"persistent":true,"supported":true,"kind":"王小明有氣喘","value":"3",
            "statement":"","scope":"none","replaces_id":null,"reason":"none"});
    }
    if text.contains("FAKE_LEAKY_REASON") {
        // A refusal reason that quotes the teacher: must never be stored.
        return json!({"persistent":true,"supported":false,"kind":"custom","value":"",
            "statement":"","scope":"none","replaces_id":null,"reason":"提到學生王小明的氣喘"});
    }
    if text.contains("FAKE_PII") {
        return json!({"persistent":true,"supported":true,"kind":"custom","value":"",
            "statement":"王小明有氣喘，題目要簡單。","scope":"content","replaces_id":null,
            "reason":"none"});
    }
    if let Some((_, sentence)) = text.split_once("自訂：") {
        let scope = scope::codes()
            .into_iter()
            .find(|c| text.contains(&format!("FAKE_SCOPE_{c}")))
            .unwrap_or("content");
        let sentence = sentence.split("FAKE_").next().unwrap_or_default().trim();
        let replaces_id = text
            .contains("FAKE_REPLACE")
            .then(|| active.iter().find(|m| m["kind"] == CUSTOM))
            .flatten()
            .and_then(|m| m["id"].as_i64());
        let supported = !text.contains("FAKE_UNSUPPORTED");
        return json!({"persistent": !one_time, "supported": supported, "kind": "custom",
            "value": "", "statement": sentence, "scope": scope, "replaces_id": replaces_id,
            "reason": if !supported { "multiple_habits" } else if one_time { "one_time" } else { "none" }});
    }
    let (kind, value) = if text.contains("配對") {
        (
            "matching_pairs",
            if text.contains('六') || text.contains('6') {
                "6"
            } else if text.contains('二') || text.contains('兩') || text.contains('2') {
                "2"
            } else {
                "5"
            },
        )
    } else if text.contains("Which") {
        ("avoid_opening", "which_of_the_following")
    } else {
        (
            "choice_options",
            if text.contains('四') || text.contains('4') {
                "4"
            } else if text.contains('六') || text.contains('6') {
                // Well-formed codes, but not a supported option count: rejected by the server.
                "6"
            } else {
                "3"
            },
        )
    };
    let replaces_id = active
        .iter()
        .find(|m| m["kind"] == kind)
        .and_then(|m| m["id"].as_i64());
    let supported = !text.contains("FAKE_UNSUPPORTED");
    json!({"persistent": !one_time, "supported": supported, "kind": kind, "value": value,
        "statement": "", "scope": "none", "replaces_id": replaces_id,
        "reason": if !supported { "multiple_habits" } else if one_time { "one_time" } else { "none" }})
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn active(id: i64, kind: &str, scope: Option<&str>) -> Memory {
        Memory {
            id,
            kind: kind.into(),
            value: "3".into(),
            statement: "x".into(),
            scope: scope.map(str::to_string),
            valid_at: Utc::now(),
            invalid_at: None,
            superseded_by: None,
            invalidation_reason: None,
            interaction_id: 1,
        }
    }

    fn custom(statement: &str, scope: &str, replaces: Option<i64>) -> Extraction {
        Extraction {
            persistent: true,
            supported: true,
            kind: CUSTOM.into(),
            value: String::new(),
            statement: statement.into(),
            scope: scope.into(),
            replaces_id: replaces,
            reason: "none".into(),
        }
    }

    #[test]
    fn a_free_form_answer_needs_scope_statement_and_a_valid_replacement() {
        let ok = custom("每份學習單最後附一題反思問題。", "content", None);
        let (p, why) = validate(&ok, &[]);
        assert!(why.is_empty(), "{why:?}");
        let p = p.unwrap();
        assert_eq!(
            (p.kind.as_str(), p.scope.as_deref()),
            ("custom", Some("content"))
        );
        assert_eq!(p.value, p.statement);

        assert_eq!(
            validate(&custom("好", "content", None), &[]).1,
            ["statement_not_chinese"]
        );
        assert_eq!(
            validate(&custom("題目要短。", "elsewhere", None), &[]).1,
            ["scope_not_allowed"]
        );
        assert_eq!(
            validate(&custom("王小明有氣喘，題目要簡單。", "content", None), &[]).1,
            ["statement_personal_information"]
        );
        // replaces_id must name one of the teacher's active free-form habits.
        let closed = active(7, "choice_options", None);
        let free = active(8, CUSTOM, Some("layout"));
        let active_set = [closed, free];
        assert_eq!(
            validate(&custom("題目要短。", "content", Some(7)), &active_set).1,
            ["replaces_id_mismatch"]
        );
        assert_eq!(
            validate(&custom("題目要短。", "content", Some(99)), &active_set).1,
            ["replaces_id_mismatch"]
        );
        assert!(
            validate(&custom("題目要短。", "content", Some(8)), &active_set)
                .1
                .is_empty()
        );
    }

    #[test]
    fn a_one_time_or_unsupported_answer_is_never_proposed() {
        let mut one_time = custom("這次改成三題。", "content", None);
        one_time.persistent = false;
        let (p, why) = validate(&one_time, &[]);
        assert!(p.is_none());
        assert_eq!(why, ["not_persistent"]);
        let mut refused = custom("", "none", None);
        refused.supported = false;
        refused.reason = "personal_information".into();
        let (p, why) = validate(&refused, &[]);
        assert!(p.is_none());
        assert_eq!(why, ["unsupported"]);
    }

    #[test]
    fn closed_kinds_keep_their_rules() {
        let mut e = custom("", "none", None);
        e.kind = "choice_options".into();
        e.value = "3".into();
        assert!(validate(&e, &[]).0.is_some());
        e.value = "6".into();
        assert_eq!(validate(&e, &[]).1, ["value_not_allowed_for_kind"]);
        e.value = "3".into();
        e.replaces_id = Some(5);
        assert_eq!(validate(&e, &[]).1, ["replaces_id_mismatch"]);
        assert!(validate(&e, &[active(5, "choice_options", None)])
            .0
            .is_some());
    }
}
