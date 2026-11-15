//! LLM provider abstraction.
//!
//! Every provider takes the same [`LlmRequest`] (rendered system/user text + JSON schema) and
//! returns parsed JSON plus the model version the provider reported. There is no automatic
//! fallback to another model: when the configured provider fails, AI is paused (spec §2, §5.2).

pub mod anthropic;
pub mod fake;
pub mod openai;
pub mod pipeline;
pub mod schemas;

use std::time::Duration;

use serde_json::Value;

use crate::config::{AiProviderKind, Config};

#[derive(Debug, Clone)]
pub struct LlmRequest {
    pub system: String,
    pub user: String,
    pub schema_name: &'static str,
    pub schema: Value,
    /// Structured copy of the task inputs. Only the fake provider reads it.
    pub fake_context: Value,
    /// 1 for the first call, 2 for the single regeneration.
    pub attempt: i32,
    /// Upper bound for the output. The OpenAI-compatible gateway hangs on a runaway output
    /// when none is set (docs/LLM_VALIDATION.md section 2); `None` keeps the provider's default.
    pub max_tokens: Option<u32>,
    /// OpenAI-compatible provider only. A retry must differ from the first request (the
    /// gateway caches identical ones), so the SOP stages raise it on the repair call.
    pub temperature: Option<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LlmResponse {
    pub json: Value,
    /// Model id echoed by the provider (`response.model`).
    pub model_version: String,
}

#[derive(Debug, Clone, thiserror::Error, PartialEq)]
pub enum ProviderError {
    #[error("AI provider timed out")]
    Timeout,
    /// The model declined (Anthropic `stop_reason: "refusal"`, OpenAI `refusal`/`content_filter`).
    #[error("model refused: {0}")]
    Refusal(String),
    /// The output ran to `max_tokens`. `partial` is what the model wrote until then, kept for
    /// diagnosis only (see [`Runaway`]); it is never parsed.
    #[error("output truncated at max_tokens")]
    MaxTokens { partial: String },
    #[error("HTTP {status}: {body}")]
    Http { status: u16, body: String },
    #[error("network error: {0}")]
    Network(String),
    /// The response could not be read as the requested JSON.
    #[error("invalid output: {0}")]
    InvalidOutput(String),
    #[error("AI is disabled")]
    Disabled,
}

impl ProviderError {
    pub(crate) fn from_reqwest(e: reqwest::Error) -> Self {
        if e.is_timeout() {
            Self::Timeout
        } else {
            Self::Network(e.to_string())
        }
    }
}

/// Truncates long provider bodies before they are stored in `error_detail`.
pub(crate) fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max).collect();
        out.push('…');
        out
    }
}

/// How an output that ran to `max_tokens` went wrong, for `ai_interactions`
/// (docs/LLM_VALIDATION.md section 2: gemma loops in whitespace or repeats one value).
#[derive(Debug, Clone, PartialEq)]
pub struct Runaway {
    /// `whitespace` (a run of blank characters it never left), `repetition` (the same text
    /// over and over at the end) or `long` (neither: the output was simply too long).
    pub kind: &'static str,
    /// Characters the model wrote.
    pub chars: usize,
    pub longest_whitespace: usize,
    /// The output with long whitespace runs collapsed; head and tail only.
    pub excerpt: String,
}

/// Key of the runaway excerpt in a stored `ai_interactions.output`. It may quote the material,
/// so deleting a material scrubs it like `evidence`.
pub const RUNAWAY_EXCERPT_KEY: &str = "runaway_excerpt";

/// A whitespace run at least this long is a runaway, not formatting.
const RUNAWAY_WHITESPACE: usize = 200;
/// Characters of the collapsed output kept at each end of the excerpt.
const RUNAWAY_EXCERPT: usize = 1500;

impl Runaway {
    pub fn of(partial: &str) -> Self {
        let mut collapsed = String::new();
        let mut longest = 0;
        let mut run = String::new();
        let flush = |run: &mut String, out: &mut String, longest: &mut usize| {
            let n = run.chars().count();
            *longest = (*longest).max(n);
            if n > 8 {
                out.push_str(&format!("⟨{n} whitespace⟩"));
            } else {
                out.push_str(run);
            }
            run.clear();
        };
        for c in partial.chars() {
            if c.is_whitespace() {
                run.push(c);
            } else {
                flush(&mut run, &mut collapsed, &mut longest);
                collapsed.push(c);
            }
        }
        flush(&mut run, &mut collapsed, &mut longest);
        let kind = if longest >= RUNAWAY_WHITESPACE {
            "whitespace"
        } else if repeats_at_end(&collapsed) {
            "repetition"
        } else {
            "long"
        };
        let n = collapsed.chars().count();
        let excerpt = if n <= 2 * RUNAWAY_EXCERPT {
            collapsed
        } else {
            let head: String = collapsed.chars().take(RUNAWAY_EXCERPT).collect();
            let tail: String = collapsed.chars().skip(n - RUNAWAY_EXCERPT).collect();
            format!("{head} …[{} chars]… {tail}", n - 2 * RUNAWAY_EXCERPT)
        };
        Self {
            kind,
            chars: partial.chars().count(),
            longest_whitespace: longest,
            excerpt,
        }
    }

    /// One line for `check_results`.
    pub fn summary(&self) -> String {
        format!(
            "output ran to max_tokens: {} runaway ({} chars, longest whitespace run {})",
            self.kind, self.chars, self.longest_whitespace
        )
    }
}

/// True when the last 40 characters occur at least three times in the last 3000.
fn repeats_at_end(s: &str) -> bool {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() < 120 {
        return false;
    }
    let window: String = chars[chars.len() - 40..].iter().collect();
    let recent: String = chars[chars.len().saturating_sub(3000)..].iter().collect();
    recent.matches(window.as_str()).count() >= 3
}

pub struct AiClient {
    pub kind: AiProviderKind,
    pub model: String,
    pub effort: String,
    pub timeout: Duration,
    http: reqwest::Client,
    anthropic_api_key: Option<String>,
    openai_base_url: Option<String>,
    openai_api_key: Option<String>,
}

impl AiClient {
    pub fn from_config(cfg: &Config) -> Self {
        let timeout = Duration::from_secs(cfg.ai_timeout_secs);
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("reqwest client");
        Self {
            kind: cfg.ai_provider,
            model: cfg.ai_model.clone(),
            effort: cfg.ai_effort.clone(),
            timeout,
            http,
            anthropic_api_key: cfg.anthropic_api_key.clone(),
            openai_base_url: cfg.openai_compat_base_url.clone(),
            openai_api_key: cfg.openai_compat_api_key.clone(),
        }
    }

    pub fn enabled(&self) -> bool {
        self.kind != AiProviderKind::None
    }

    pub fn provider_name(&self) -> &'static str {
        self.kind.as_str()
    }

    /// One call to the configured provider, bounded by `AI_TIMEOUT_SECS`. Never retries on
    /// another model.
    pub async fn complete(&self, req: &LlmRequest) -> Result<LlmResponse, ProviderError> {
        let call = async {
            match self.kind {
                AiProviderKind::None => Err(ProviderError::Disabled),
                AiProviderKind::Fake => fake::complete(req, &self.model),
                AiProviderKind::Anthropic => {
                    let key = self.anthropic_api_key.as_deref().unwrap_or_default();
                    anthropic::call(&self.http, key, &self.model, &self.effort, req).await
                }
                AiProviderKind::OpenAiCompatible => {
                    let base = self.openai_base_url.as_deref().unwrap_or_default();
                    openai::call(
                        &self.http,
                        base,
                        self.openai_api_key.as_deref(),
                        &self.model,
                        req,
                    )
                    .await
                }
            }
        };
        match tokio::time::timeout(self.timeout, call).await {
            Ok(r) => r,
            Err(_) => Err(ProviderError::Timeout),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every structured output the server asks for.
    fn all_schemas() -> Vec<(&'static str, Value)> {
        vec![
            ("core_assist", schemas::core_assist_schema()),
            ("plan_suggest", schemas::plan_suggest_schema()),
            ("material_focuses", schemas::material_focuses_schema()),
            ("sop_analysis", crate::sop::schemas::analysis_schema()),
            ("sop_objectives", crate::sop::schemas::objectives_schema()),
            ("worksheet_tasks", crate::worksheet::schemas::tasks_schema()),
            (
                "worksheet_warmup",
                crate::worksheet::schemas::warmup_schema(),
            ),
            (
                "worksheet_scaffolds",
                crate::worksheet::schemas::scaffolds_schema(),
            ),
            ("worksheet_exit", crate::worksheet::schemas::exit_schema()),
        ]
    }

    fn check_order(path: &str, v: &Value, problems: &mut Vec<String>) {
        match v {
            Value::Object(o) => {
                if let (Some(Value::Object(props)), Some(Value::Array(required))) =
                    (o.get("properties"), o.get("required"))
                {
                    let keys: Vec<&str> = props.keys().map(String::as_str).collect();
                    let req: Vec<&str> = required.iter().filter_map(Value::as_str).collect();
                    if keys != req {
                        problems.push(format!("{path}: properties {keys:?} vs required {req:?}"));
                    }
                }
                for (k, x) in o {
                    check_order(&format!("{path}.{k}"), x, problems);
                }
            }
            Value::Array(a) => {
                for (i, x) in a.iter().enumerate() {
                    check_order(&format!("{path}[{i}]"), x, problems);
                }
            }
            _ => {}
        }
    }

    /// The gateway's constrained decoding makes the model write properties in the order the
    /// schema lists them. Sorted alphabetically (serde_json without `preserve_order`), the tasks
    /// schema asked for `answer_en` before `prompt_en` and put `title_en` after `items`; the
    /// model then wanted to close the object early and, with only whitespace allowed, wrote
    /// whitespace until `max_tokens` (docs/llm-eval/worksheet-real). The schemas must reach the
    /// provider in the order they are written: the order of `required`.
    #[test]
    fn schemas_keep_their_written_property_order() {
        let mut problems = Vec::new();
        for (name, schema) in all_schemas() {
            check_order(name, &schema, &mut problems);
            // And the serialised body keeps it too.
            let s = schema.to_string();
            let back: Value = serde_json::from_str(&s).unwrap();
            check_order(&format!("{name} (reparsed)"), &back, &mut problems);
        }
        assert!(problems.is_empty(), "{problems:#?}");
        let tasks = crate::worksheet::schemas::tasks_schema().to_string();
        let at = |k: &str| tasks.find(&format!("\"{k}\"")).unwrap();
        assert!(at("prompt_en") < at("answer_en"));
        assert!(at("title_en") < at("instructions_en"));
        assert!(at("instructions_en") < at("design_reason_zh"));
    }

    #[test]
    fn runaway_whitespace_is_named_and_collapsed() {
        let partial = format!(
            "{{\n  \"background_activities\": [{{\"student_tasks\": [\"討論／分享\"]{}",
            "    \n".repeat(2000)
        );
        let r = Runaway::of(&partial);
        assert_eq!(r.kind, "whitespace");
        assert_eq!(r.longest_whitespace, 10_000);
        assert!(r.excerpt.contains("⟨10000 whitespace⟩"), "{}", r.excerpt);
        assert!(r.excerpt.len() < 400);
        assert!(r.summary().contains("whitespace runaway"));
    }

    #[test]
    fn runaway_repetition_and_plain_long_output() {
        let looped = format!("{{\"student_tasks\": [{}", "\"理解內容\", ".repeat(800));
        let r = Runaway::of(&looped);
        assert_eq!(r.kind, "repetition");
        // Head and tail only.
        assert!(r.excerpt.contains("chars]…"));
        assert!(r.excerpt.chars().count() < 3 * RUNAWAY_EXCERPT);
        let long: String = (0..400).map(|i| format!("word{i} ")).collect();
        assert_eq!(Runaway::of(&long).kind, "long");
        assert_eq!(Runaway::of("").kind, "long");
    }
}
