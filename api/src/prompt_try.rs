//! 試產生: the project lead tries an UNSAVED prompt draft on a pasted sample before it goes live
//! (後台管理 → 提示詞版本).
//!
//! What keeps it away from teachers and from the research data:
//!
//! * It never goes through `application::generation::call`, `application::ai::attempt` or any other path that
//!   writes `ai_interactions`, `worksheets`, jobs or progress rows. The provider is called
//!   directly with `AiClient::complete`; nothing a teacher can see is read or written, and no
//!   teacher data (classes, materials, preferences) is read: the only inputs are what the lead
//!   typed into the box, plus the lead's OWN confirmed teaching habits while the
//!   teacher-memory switch is on, appended exactly as a real run of that step appends them
//!   (`memory::habit_context`), so the try shows what teachers' runs will receive.
//! * The draft text is used for this one call only. It is rendered here, never stored as a
//!   `prompt_versions` row, and `prompts::latest` is not involved.
//! * The result is stored in its own table, `prompt_try_runs` (every row is a preview, enforced
//!   by a CHECK), so research exports, 我的作品 and the usage statistics, which read
//!   `ai_interactions` / `worksheets`, cannot contain it.
//! * The sample text is kept only as its length and a hash (the same rule as the logged copies
//!   of teacher materials); the model's output is kept so the lead can look at it again.
//!
//! Only prompts that are one single call are supported (see [`SUPPORTED`]); a step inside the
//! multi-step worksheet pipeline would need the whole pipeline to run with that one key
//! overridden, which cannot be done without creating teacher-visible rows (see the PR text).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::ai::{LlmRequest, ProviderError};
use crate::prompts::{self, PromptVersion};
use crate::AppState;

/// Keys that can be tried.
pub const SUPPORTED: [&str; 3] = ["prep_ask", "objective_draft", "worksheet_warmup"];

pub const MAX_PASSAGE_CHARS: usize = 12_000;
pub const MAX_QUESTION_CHARS: usize = 1_000;
/// Runs per lead per hour (each one is a real model call).
pub const MAX_RUNS_PER_HOUR: i64 = 30;

const BASE_TEMPERATURE: f32 = 0.2;
/// The title the fake provider writes into its outputs (English, as a real title would be).
const FAKE_TITLE: &str = "the sample passage";

pub fn supported(key: &str) -> bool {
    SUPPORTED.contains(&key)
}

/// What the lead types in.
#[derive(Debug, Clone, Deserialize)]
pub struct Sample {
    /// An English passage (paragraphs separated by blank lines).
    pub passage: String,
    /// `prep_ask`: the teacher's question.
    #[serde(default)]
    pub question: String,
    /// Optional class description, stands in for the 班級特性.
    #[serde(default)]
    pub class_profile: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CheckLine {
    pub name: String,
    pub passed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

fn line(name: &str, passed: bool, detail: impl Into<Option<String>>) -> CheckLine {
    CheckLine {
        name: name.to_string(),
        passed,
        detail: detail.into(),
    }
}

/// How a try ended: `ok` (the model answered), `disabled`, `timeout`, `refused`, `error`.
#[derive(Debug, Serialize)]
pub struct TryResult {
    pub status: &'static str,
    pub message: Option<String>,
    /// The exact messages that went to the model.
    pub system: String,
    pub user: String,
    /// The model's raw JSON, before any check or repair.
    pub output: Option<Value>,
    pub checks: Vec<CheckLine>,
    /// What the platform's own checks would have fixed or noted (not failures).
    pub notes: Vec<String>,
    /// The lead's own confirmed habits, appended exactly as a real run appends them while the
    /// teacher-memory switch is on (`input_snapshot.teacher_memory` of a real run). `None` for
    /// a key that never receives habits.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub teacher_memory: Option<Value>,
}

/// vars, `required` appended variables, schema name, schema, fake context, max tokens.
type Parts = (
    HashMap<&'static str, String>,
    Vec<(&'static str, &'static str)>,
    &'static str,
    Value,
    Value,
    u32,
);

struct Prepared {
    vars: HashMap<&'static str, String>,
    required: Vec<(&'static str, &'static str)>,
    schema_name: &'static str,
    schema: Value,
    fake_context: Value,
    max_tokens: u32,
    /// Candidates of `worksheet_warmup` (the real run takes them from the word list).
    candidates: Vec<crate::worksheet::checks::Candidate>,
    paragraphs: Vec<String>,
}

/// The paragraphs of a pasted passage (blank lines separate them).
pub fn paragraphs_of(text: &str) -> Vec<String> {
    crate::sop::split_paragraphs(text)
}

fn marked(paragraphs: &[String], label: impl Fn(usize) -> String) -> String {
    paragraphs
        .iter()
        .enumerate()
        .map(|(i, p)| format!("{} {p}", label(i + 1)))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn profile_text(sample: &Sample) -> String {
    crate::text::or_blank(sample.class_profile.trim())
}

/// A few words from the passage (long, not repeated) with the sentence they are in, standing
/// in for the candidates the real run takes from the vocabulary list.
fn sample_candidates(paragraphs: &[String]) -> Vec<crate::worksheet::checks::Candidate> {
    let mut out: Vec<crate::worksheet::checks::Candidate> = Vec::new();
    for (pi, p) in paragraphs.iter().enumerate() {
        for sentence in p.split_inclusive(['.', '!', '?']) {
            let sentence = sentence.trim();
            for raw in sentence.split(|c: char| !c.is_ascii_alphabetic()) {
                let word = raw.to_ascii_lowercase();
                if word.len() >= 7
                    && !out.iter().any(|c| c.word == word)
                    && out.len() < 8
                    && !sentence.is_empty()
                {
                    out.push(crate::worksheet::checks::Candidate {
                        word,
                        level: None,
                        paragraph_no: pi as i64 + 1,
                        sentence: sentence.to_string(),
                    });
                }
            }
        }
    }
    out
}

fn prepare(key: &str, sample: &Sample) -> Result<Prepared, String> {
    let paragraphs = paragraphs_of(&sample.passage);
    if paragraphs.is_empty() {
        return Err("請先貼上一段教材文字。".to_string());
    }
    let title = "（試產生的範例教材）";
    let mut candidates = Vec::new();
    let (vars, required, schema_name, schema, fake_context, max_tokens): Parts = match key {
        "prep_ask" => {
            let question = sample.question.trim();
            if question.is_empty() {
                return Err("請寫一個老師會問的問題。".to_string());
            }
            let passage = format!(
                "{title}（共 {} 段）\n\n{}",
                paragraphs.len(),
                marked(&paragraphs, |n| format!("[第 {n} 段]"))
            );
            (
                HashMap::from([
                    ("question", question.to_string()),
                    ("recent_turns", "（這是第一則訊息）".to_string()),
                    ("class_profile", profile_text(sample)),
                    ("curriculum_items", "（試產生：沒有附課綱條目）".to_string()),
                    ("passage_numbered", passage),
                ]),
                vec![
                    ("question", "教師的問題"),
                    ("class_profile", "班級特性"),
                    ("passage_numbered", "教材"),
                ],
                crate::prep::ASK_SCHEMA_NAME,
                crate::prep::ask_schema(),
                json!({
                    "task": "prep_ask", "question": question, "has_material": true,
                    "recent": Vec::<Value>::new(), "candidates": Vec::<String>::new(),
                }),
                3000,
            )
        }
        "objective_draft" => {
            let purpose = crate::objectives::draft::purpose("worksheet");
            (
                HashMap::from([
                    ("purpose", purpose.to_string()),
                    ("school_track", "普高".to_string()),
                    ("class_profile", profile_text(sample)),
                    ("teacher_request", crate::objectives::NONE.to_string()),
                    ("recent_categories", "（這是第一次草擬）".to_string()),
                    ("variety_note", "第一個語言目標先考慮學習策略。".to_string()),
                    ("avoid_note", String::new()),
                    (
                        "curriculum_items",
                        "（試產生：沒有附課綱條目；curriculum_codes 一律給空陣列）".to_string(),
                    ),
                    ("material_title", title.to_string()),
                    ("paragraph_count", paragraphs.len().to_string()),
                    (
                        "passage_numbered",
                        marked(&paragraphs, |n| format!("[第 {n} 段]")),
                    ),
                    ("redo_note", String::new()),
                ]),
                vec![
                    ("purpose", "這一課要做"),
                    ("class_profile", "班級特性"),
                    ("teacher_request", "教師的要求"),
                ],
                crate::objectives::DRAFT_SCHEMA_NAME,
                crate::objectives::draft_schema(),
                json!({
                    "task": "objective_draft", "kind": "worksheet", "material_title": FAKE_TITLE,
                    "paragraphs": paragraphs, "round": 1, "avoid": Vec::<String>::new(),
                    "strategy_first": false,
                }),
                crate::objectives::draft::DRAFT_MAX_TOKENS,
            )
        }
        "worksheet_warmup" => {
            candidates = sample_candidates(&paragraphs);
            let lines: Vec<String> = candidates
                .iter()
                .map(|c| format!("{}｜未對應到詞彙表｜P{}", c.word, c.paragraph_no))
                .collect();
            (
                HashMap::from([
                    ("school_track", "普高".to_string()),
                    ("class_profile", profile_text(sample)),
                    (
                        "objectives_summary",
                        "（試產生：沒有設定學習目標）".to_string(),
                    ),
                    (
                        "passage_numbered",
                        marked(&paragraphs, |n| format!("[P{n}]")),
                    ),
                    (
                        "student_task_labels",
                        crate::worksheet::student_tasks().join("、"),
                    ),
                    (
                        "answer_format_labels",
                        crate::worksheet::ANSWER_FORMATS.join("、"),
                    ),
                    ("teacher_notes_for_ai", String::new()),
                    ("vocab_count", "3–5".to_string()),
                    ("teacher_warmup", "（沒有）".to_string()),
                    ("teacher_vocabulary", "（沒有）".to_string()),
                    (
                        "tasks_summary",
                        "（試產生：沒有前一步的任務摘要）".to_string(),
                    ),
                    (
                        "vocab_candidates",
                        if lines.is_empty() {
                            "（沒有候選字彙；key_vocabulary 請給空陣列）".to_string()
                        } else {
                            lines.join("\n")
                        },
                    ),
                ]),
                vec![("class_profile", "班級特性")],
                crate::worksheet::schemas::WARMUP_SCHEMA_NAME,
                crate::worksheet::schemas::warmup_schema(),
                json!({
                    "task": "worksheet_warmup", "title": FAKE_TITLE, "vocab_count": 5,
                    "candidates": candidates.iter()
                        .map(|c| json!({ "word": c.word, "paragraph_no": c.paragraph_no, "sentence": c.sentence }))
                        .collect::<Vec<_>>(),
                }),
                2000,
            )
        }
        _ => return Err("這個提示詞還不能試產生。".to_string()),
    };
    Ok(Prepared {
        vars,
        required,
        schema_name,
        schema,
        fake_context,
        max_tokens,
        candidates,
        paragraphs,
    })
}

/// The draft as a prompt version that exists only for this call (id 0, never stored).
fn draft_version(key: &str, system_text: &str, user_template: &str) -> PromptVersion {
    PromptVersion {
        id: 0,
        prompt_key: key.to_string(),
        version_no: 0,
        system_text: system_text.to_string(),
        user_template: user_template.to_string(),
        created_at: chrono::Utc::now(),
    }
}

/// Runs one call with the draft text on the sample. `Err` is a message about the sample (400);
/// everything the model or the checks do is in the `TryResult`.
pub async fn run(
    state: &AppState,
    lead_id: i64,
    key: &str,
    system_text: &str,
    user_template: &str,
    sample: &Sample,
) -> Result<TryResult, String> {
    if sample.passage.chars().count() > MAX_PASSAGE_CHARS {
        return Err(format!("範例教材最多 {MAX_PASSAGE_CHARS} 字。"));
    }
    if sample.question.chars().count() > MAX_QUESTION_CHARS
        || sample.class_profile.chars().count() > MAX_QUESTION_CHARS
    {
        return Err(format!("問題與班級描述各最多 {MAX_QUESTION_CHARS} 字。"));
    }
    let p = prepare(key, sample)?;
    let draft = draft_version(key, system_text, user_template);
    let req = LlmRequest {
        system: prompts::render(&draft.system_text, &p.vars),
        user: prompts::render_user_with(&draft, &p.vars, &p.required),
        schema_name: p.schema_name,
        schema: p.schema.clone(),
        fake_context: p.fake_context.clone(),
        attempt: 1,
        max_tokens: Some(p.max_tokens),
        temperature: Some(BASE_TEMPERATURE),
        images: Vec::new(),
    };
    // The same habit context a real run of this step gets (only the lead's own habits; the
    // index is not consulted: its ranking is metadata only and never changes the prompt).
    let memory = crate::memory::habit_context(&state.db, lead_id, key)
        .await
        .map_err(|e| format!("無法讀取出題習慣：{e}"))?;
    let req = match &memory {
        Some(ctx) => crate::memory::apply(ctx, &req),
        None => req,
    };
    let mut result = TryResult {
        status: "ok",
        message: None,
        system: req.system.clone(),
        user: req.user.clone(),
        output: None,
        checks: Vec::new(),
        notes: Vec::new(),
        teacher_memory: memory
            .as_ref()
            .map(|ctx| crate::memory::snapshot_value(ctx, None)),
    };
    if !state.ai.enabled() {
        result.status = "disabled";
        result.message = Some("這個環境沒有開啟 AI，所以不能試產生。".to_string());
        return Ok(result);
    }
    match state.ai.active().complete(&req).await {
        Ok(resp) => {
            result.checks = evaluate(key, &resp.json, &p, &mut result.notes);
            result.output = Some(resp.json);
        }
        Err(e) => {
            let (status, message) = match e {
                ProviderError::Timeout => ("timeout", "AI 沒有在時間內回應。"),
                ProviderError::Refusal(_) => ("refused", "模型拒絕回答這一份輸入。"),
                ProviderError::MaxTokens { .. } => {
                    ("error", "輸出太長被截斷了（提示詞可能讓模型一直寫下去）。")
                }
                ProviderError::InvalidOutput(_) => {
                    ("error", "模型的輸出不是指定的 JSON 格式，沒辦法讀取。")
                }
                ProviderError::Disabled => ("disabled", "這個環境沒有開啟 AI，所以不能試產生。"),
                _ => ("error", "AI 暫時無法使用。"),
            };
            result.status = status;
            result.message = Some(message.to_string());
        }
    }
    Ok(result)
}

/// The platform's own checks on one output, without the repair call the real run would make.
fn evaluate(key: &str, output: &Value, p: &Prepared, notes: &mut Vec<String>) -> Vec<CheckLine> {
    match key {
        "prep_ask" => match serde_json::from_value::<crate::prep::RawAnswer>(output.clone()) {
            Err(e) => vec![line("schema", false, e.to_string())],
            Ok(raw) => {
                let mut out = vec![line("schema", true, None)];
                match crate::prep::clean_answer(&raw) {
                    Ok(_) => out.push(line("answer_usable", true, None)),
                    Err(problem) => out.push(line("answer_usable", false, problem)),
                }
                out
            }
        },
        "objective_draft" => {
            match serde_json::from_value::<crate::objectives::RawDrafts>(output.clone()) {
                Err(e) => vec![line("schema", false, e.to_string())],
                Ok(raw) => {
                    let rules = crate::objectives::check_rules(&raw, false);
                    let mut out = vec![line("schema", true, None)];
                    out.push(line(
                        "has_content_and_language",
                        rules.complete(),
                        (!rules.complete()).then(|| {
                            "內容目標與語言目標各至少要有 1 個通過檢查的目標。".to_string()
                        }),
                    ));
                    for dropped in &rules.dropped {
                        out.push(line("rules", false, dropped.clone()));
                    }
                    if rules.dropped.is_empty() {
                        out.push(line("rules", true, None));
                    }
                    notes.extend(rules.repairs.iter().cloned());
                    out
                }
            }
        }
        "worksheet_warmup" => {
            match serde_json::from_value::<crate::worksheet::schemas::RawWarmup>(output.clone()) {
                Err(e) => vec![line("schema", false, e.to_string())],
                Ok(raw) => {
                    let ctx = crate::worksheet::checks::WarmupCtx {
                        paragraphs: &p.paragraphs,
                        candidates: &p.candidates,
                        vocab_count: 5,
                    };
                    let eval = crate::worksheet::checks::evaluate_warmup(&raw, &ctx);
                    notes.extend(eval.repairs.iter().cloned());
                    let mut out = vec![line("schema", true, None)];
                    out.extend(
                        eval.results
                            .iter()
                            .map(|r| line(r.name, r.passed, r.detail.clone())),
                    );
                    out
                }
            }
        }
        _ => Vec::new(),
    }
}

/// A short, non-reversible summary of the sample for the stored row (never the text itself).
pub fn sample_summary(sample: &Sample) -> Value {
    json!({
        "passage_chars": sample.passage.chars().count(),
        "passage_sha256": crate::auth::sha256_hex(&sample.passage),
        "question_chars": sample.question.chars().count(),
        "class_profile_chars": sample.class_profile.chars().count(),
    })
}
