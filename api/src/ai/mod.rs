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
    #[error("output truncated at max_tokens")]
    MaxTokens,
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
