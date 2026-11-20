//! LLM provider abstraction.
//!
//! Every provider takes the same [`LlmRequest`] (rendered system/user text + JSON schema) and
//! returns parsed JSON plus the model version the provider reported. There is no automatic
//! fallback to another model: when the configured provider fails, AI is paused (spec §2, §5.2).

pub mod admin;
pub mod anthropic;
pub(crate) mod execution;
pub mod fake;
pub mod openai;
pub mod pipeline;
pub mod schemas;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;

use crate::config::{AiProviderKind, Config};

/// A picture sent with the user message (a screenshot or photo the teacher attached). Already
/// downscaled and re-encoded by the server; never logged (only its size is).
#[derive(Clone, PartialEq, Eq)]
pub struct LlmImage {
    /// `image/jpeg`.
    pub media_type: &'static str,
    pub data: bytes::Bytes,
}

impl std::fmt::Debug for LlmImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "LlmImage({}, {} bytes)",
            self.media_type,
            self.data.len()
        )
    }
}

impl LlmImage {
    /// `data:image/jpeg;base64,…` (OpenAI `image_url`).
    pub fn data_url(&self) -> String {
        use base64::Engine;
        format!(
            "data:{};base64,{}",
            self.media_type,
            base64::engine::general_purpose::STANDARD.encode(&self.data)
        )
    }
}

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
    /// Pictures that go with the user message (after its text). Empty for almost every call.
    /// The OpenAI-compatible provider sends them as `image_url` parts, Anthropic as `image`
    /// blocks; the fake provider records that they arrived.
    pub images: Vec<LlmImage>,
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

/// Drops NUL characters from every string (and key) of a model's output: PostgreSQL refuses them
/// in JSONB and text, and they never carry meaning.
pub fn strip_nul(value: &mut serde_json::Value) {
    use serde_json::Value;
    match value {
        Value::String(s) if s.contains('\0') => s.retain(|c| c != '\0'),
        Value::Array(items) => items.iter_mut().for_each(strip_nul),
        Value::Object(map) => {
            if map.keys().any(|k| k.contains('\0')) {
                let entries: Vec<_> = std::mem::take(map).into_iter().collect();
                for (k, v) in entries {
                    map.insert(k.replace('\0', ""), v);
                }
            }
            map.values_mut().for_each(strip_nul);
        }
        _ => {}
    }
}

/// How long a prep turn's pinned settings are kept when nothing forgets them (a turn and its
/// background steps finish long before this).
const TURN_PIN_TTL: Duration = Duration::from_secs(12 * 60 * 60);

tokio::task_local! {
    /// The settings a background job runs with (see [`AiClient::pinned`]).
    static PINNED: ActiveAi;
}

/// The text model's settings: what the PI may switch (`app_settings.ai_text_model`), what the
/// environment (`AI_PROVIDER`, `AI_MODEL`, `AI_EFFORT`, `AI_TIMEOUT_SECS`) provides by default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiSettings {
    pub kind: AiProviderKind,
    pub model: String,
    pub effort: String,
    pub timeout_secs: u64,
}

impl AiSettings {
    pub fn from_config(cfg: &Config) -> Self {
        Self {
            kind: cfg.ai_provider,
            model: cfg.ai_model.clone(),
            effort: cfg.ai_effort.clone(),
            timeout_secs: cfg.ai_timeout_secs,
        }
    }

    /// `{provider, model, effort, timeout_secs}`, the shape stored in `app_settings`, shown on
    /// the admin page and written to `usage_logs`.
    pub fn to_json(&self) -> Value {
        serde_json::json!({
            "provider": self.kind.as_str(),
            "model": self.model,
            "effort": self.effort,
            "timeout_secs": self.timeout_secs,
        })
    }
}

/// Where the active settings come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiSource {
    /// The environment variables.
    Env,
    /// The PI's choice in 後台管理.
    Admin,
}

impl AiSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Env => "env",
            Self::Admin => "admin",
        }
    }
}

/// Credentials and addresses of the providers, fixed at startup (they come from the environment
/// only; the PI switches provider and model, never keys).
struct Endpoints {
    anthropic_api_key: Option<String>,
    openai_base_url: Option<String>,
    openai_api_key: Option<String>,
    /// `OPENAI_API_KEY`, for the `openai` provider.
    openai_platform_key: Option<String>,
}

/// The text model client. The provider, model, effort and timeout can be switched while the
/// server runs ([`AiClient::set_active`], used by the PI's admin page); every AI call takes ONE
/// [`ActiveAi`] snapshot with [`AiClient::active`] and uses it for the request, the timeout and
/// what it records in `ai_interactions`, so a call never mixes settings and a call in flight
/// finishes with what it started with.
///
/// The switch lives in the memory of this process. Only one API instance runs; with several,
/// each would have to reload `app_settings` after a change.
pub struct AiClient {
    active: std::sync::RwLock<(Arc<AiSettings>, AiSource)>,
    env_default: AiSettings,
    /// The longest call timeout in effect since the process started, for the stale-job checks
    /// (a job started under a long timeout must not look stale after the PI shortens it).
    longest_timeout_secs: AtomicU64,
    /// Settings remembered per prep turn (see [`AiClient::remember_turn`]).
    turn_pins: std::sync::Mutex<std::collections::HashMap<i64, (std::time::Instant, ActiveAi)>>,
    /// Serialises the PI's changes (test call, save, activate).
    pub(crate) change_lock: tokio::sync::Mutex<()>,
    http: reqwest::Client,
    endpoints: Arc<Endpoints>,
}

/// One consistent view of the active settings, taken at the start of a call.
#[derive(Clone)]
pub struct ActiveAi {
    pub kind: AiProviderKind,
    pub model: String,
    pub effort: String,
    pub timeout: Duration,
    http: reqwest::Client,
    endpoints: Arc<Endpoints>,
}

impl AiClient {
    pub fn from_config(cfg: &Config) -> Self {
        // No overall `timeout` here: it differs per call (the snapshot's) and is enforced in
        // `ActiveAi::complete`.
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("reqwest client");
        let env_default = AiSettings::from_config(cfg);
        Self {
            active: std::sync::RwLock::new((Arc::new(env_default.clone()), AiSource::Env)),
            longest_timeout_secs: AtomicU64::new(env_default.timeout_secs),
            turn_pins: std::sync::Mutex::default(),
            env_default,
            change_lock: tokio::sync::Mutex::new(()),
            http,
            endpoints: Arc::new(Endpoints {
                anthropic_api_key: cfg.anthropic_api_key.clone(),
                openai_base_url: cfg.openai_compat_base_url.clone(),
                openai_api_key: cfg.openai_compat_api_key.clone(),
                openai_platform_key: cfg.openai_api_key.clone(),
            }),
        }
    }

    fn snapshot_of(&self, s: &AiSettings) -> ActiveAi {
        ActiveAi {
            kind: s.kind,
            model: s.model.clone(),
            effort: s.effort.clone(),
            timeout: Duration::from_secs(s.timeout_secs),
            http: self.http.clone(),
            endpoints: self.endpoints.clone(),
        }
    }

    /// The settings in force right now, or the job's when called inside [`AiClient::pinned`].
    /// Take it ONCE per AI call and use it for everything about that call, including what is
    /// recorded.
    pub fn active(&self) -> ActiveAi {
        if let Ok(pinned) = PINNED.try_with(|a| a.clone()) {
            return pinned;
        }
        let guard = self.active.read().unwrap_or_else(|e| e.into_inner());
        self.snapshot_of(&guard.0)
    }

    /// Pins the settings for a whole background job: every [`AiClient::active`] inside `fut`
    /// returns the settings in force NOW (when this is called, i.e. when the job is started),
    /// whatever the PI switches meanwhile. Inside an already pinned scope the outer pin is
    /// kept, so a job that starts another job passes its model on.
    ///
    /// Task-locals do not cross `tokio::spawn`: wrap the future handed to every `spawn` that
    /// leads to text-model calls.
    pub fn pinned<F: std::future::Future>(
        &self,
        fut: F,
    ) -> impl std::future::Future<Output = F::Output> {
        PINNED.scope(self.active(), fut)
    }

    /// Remembers the settings a prep turn starts with. A turn's work is several jobs, some
    /// started later by a poll of the page (the SIOP self-check after its worksheet): they all
    /// run with this pin, so the turn never mixes models. In memory of this process, like the
    /// switch itself.
    pub fn remember_turn(&self, turn_id: i64) {
        let pin = self.active();
        let mut pins = self.turn_pins.lock().unwrap_or_else(|e| e.into_inner());
        // A turn that ends without a self-check is never forgotten explicitly: drop pins far
        // older than any turn can run, never the ones of turns that may still be running.
        let now = std::time::Instant::now();
        pins.retain(|_, (at, _)| now.duration_since(*at) < TURN_PIN_TTL);
        pins.insert(turn_id, (now, pin));
    }

    pub fn forget_turn(&self, turn_id: i64) {
        self.turn_pins
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&turn_id);
    }

    fn turn_pin(&self, turn_id: i64) -> ActiveAi {
        let found = self
            .turn_pins
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&turn_id)
            .map(|(_, pin)| pin.clone());
        found.unwrap_or_else(|| self.active())
    }

    /// Like [`AiClient::pinned`], with the settings the turn started with.
    pub fn pinned_turn<F: std::future::Future>(
        &self,
        turn_id: i64,
        fut: F,
    ) -> impl std::future::Future<Output = F::Output> {
        PINNED.scope(self.turn_pin(turn_id), fut)
    }

    /// Runs `f` (which starts a job with [`AiClient::pinned`]) with the turn's settings.
    pub fn in_turn<R>(&self, turn_id: i64, f: impl FnOnce() -> R) -> R {
        PINNED.sync_scope(self.turn_pin(turn_id), f)
    }

    /// A client for settings that are not active (the admin test call).
    pub fn with_settings(&self, settings: &AiSettings) -> ActiveAi {
        self.snapshot_of(settings)
    }

    pub fn active_settings(&self) -> (AiSettings, AiSource) {
        let guard = self.active.read().unwrap_or_else(|e| e.into_inner());
        ((*guard.0).clone(), guard.1)
    }

    pub fn env_default(&self) -> &AiSettings {
        &self.env_default
    }

    /// Makes `settings` the ones every new call uses (calls in flight keep theirs).
    pub fn set_active(&self, settings: AiSettings, source: AiSource) {
        self.longest_timeout_secs
            .fetch_max(settings.timeout_secs, Ordering::Relaxed);
        *self.active.write().unwrap_or_else(|e| e.into_inner()) = (Arc::new(settings), source);
    }

    /// The longest timeout used since startup: the basis of the stale-job thresholds.
    pub fn longest_timeout_secs(&self) -> u64 {
        self.longest_timeout_secs.load(Ordering::Relaxed)
    }

    pub fn enabled(&self) -> bool {
        self.active().enabled()
    }

    /// Whether the environment holds what `kind` needs (a key or an address), i.e. whether the
    /// PI may pick it.
    pub fn provider_configured(&self, kind: AiProviderKind) -> bool {
        match kind {
            AiProviderKind::OpenAi => self.endpoints.openai_platform_key.is_some(),
            AiProviderKind::OpenAiCompatible => self.endpoints.openai_base_url.is_some(),
            _ => false,
        }
    }
}

impl ActiveAi {
    /// Runs `fut` with these settings pinned (see [`AiClient::pinned`]); for a snapshot taken
    /// earlier and carried into a spawned task.
    pub fn scope<F: std::future::Future>(
        self,
        fut: F,
    ) -> impl std::future::Future<Output = F::Output> {
        PINNED.scope(self, fut)
    }

    pub fn enabled(&self) -> bool {
        self.kind != AiProviderKind::None
    }

    pub fn provider_name(&self) -> &'static str {
        self.kind.as_str()
    }

    /// One call to the provider of this snapshot, bounded by its timeout. Never retries on
    /// another model.
    pub async fn complete(&self, req: &LlmRequest) -> Result<LlmResponse, ProviderError> {
        let e = &self.endpoints;
        let call = async {
            match self.kind {
                AiProviderKind::None => Err(ProviderError::Disabled),
                AiProviderKind::Fake => fake::complete(req, &self.model),
                AiProviderKind::Anthropic => {
                    let key = e.anthropic_api_key.as_deref().unwrap_or_default();
                    anthropic::call(&self.http, key, &self.model, &self.effort, req).await
                }
                AiProviderKind::OpenAiCompatible => {
                    let base = e.openai_base_url.as_deref().unwrap_or_default();
                    openai::call(
                        &self.http,
                        base,
                        e.openai_api_key.as_deref(),
                        &self.model,
                        None,
                        req,
                    )
                    .await
                }
                AiProviderKind::OpenAi => {
                    openai::call(
                        &self.http,
                        openai::OPENAI_BASE_URL,
                        e.openai_platform_key.as_deref(),
                        &self.model,
                        Some(&self.effort),
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

    fn settings(model: &str, timeout_secs: u64) -> AiSettings {
        AiSettings {
            kind: AiProviderKind::Fake,
            model: model.into(),
            effort: "low".into(),
            timeout_secs,
        }
    }

    #[test]
    fn a_snapshot_keeps_its_settings_when_the_active_ones_change() {
        let client = AiClient::from_config(&Config::for_tests("unused"));
        let before = client.active();
        assert_eq!(before.model, "fake-model");
        client.set_active(settings("next-model", 600), AiSource::Admin);
        // The call that started earlier is unchanged; the next one is the new settings.
        assert_eq!(before.model, "fake-model");
        assert_eq!(before.timeout, Duration::from_secs(5));
        let after = client.active();
        assert_eq!(after.model, "next-model");
        assert_eq!(after.timeout, Duration::from_secs(600));
        assert_eq!(client.active_settings().1, AiSource::Admin);
        // The stale-job checks keep the longest timeout they may have to cover.
        client.set_active(settings("short", 30), AiSource::Admin);
        assert_eq!(client.longest_timeout_secs(), 600);
        client.set_active(client.env_default().clone(), AiSource::Env);
        assert_eq!(client.active().model, "fake-model");
    }

    #[tokio::test]
    async fn a_pinned_scope_ignores_a_later_switch_and_nested_scopes_keep_the_outer_pin() {
        let client = AiClient::from_config(&Config::for_tests("unused"));
        let seen = client
            .pinned(async {
                client.set_active(settings("switched", 600), AiSource::Admin);
                let inside = client.active();
                // A nested job inherits the outer pin, not the switched settings.
                let nested = client.pinned(async { client.active().model }).await;
                (inside.model, nested)
            })
            .await;
        assert_eq!(seen, ("fake-model".to_string(), "fake-model".to_string()));
        // Outside the scope the switched settings apply.
        assert_eq!(client.active().model, "switched");
        // Pinned when called (the job is started), not when first polled.
        let later = client.pinned(async { client.active().model });
        client.set_active(settings("again", 60), AiSource::Admin);
        assert_eq!(later.await, "switched");
    }

    #[test]
    fn only_configured_real_providers_are_selectable() {
        let mut cfg = Config::for_tests("unused");
        let client = AiClient::from_config(&cfg);
        for kind in [
            AiProviderKind::OpenAi,
            AiProviderKind::OpenAiCompatible,
            AiProviderKind::Anthropic,
            AiProviderKind::Fake,
            AiProviderKind::None,
        ] {
            assert!(!client.provider_configured(kind), "{kind:?}");
        }
        cfg.openai_api_key = Some("k".into());
        cfg.anthropic_api_key = Some("k".into());
        let client = AiClient::from_config(&cfg);
        assert!(client.provider_configured(AiProviderKind::OpenAi));
        assert!(!client.provider_configured(AiProviderKind::OpenAiCompatible));
        // A configured Anthropic key never makes it selectable.
        assert!(!client.provider_configured(AiProviderKind::Anthropic));
        assert!(admin::validate(&client, "anthropic", "m", None, 60).is_err());
        let ok = admin::validate(&client, "openai", " gpt-5.6-sol ", Some("high"), 60).unwrap();
        assert_eq!(
            (ok.model.as_str(), ok.effort.as_str()),
            ("gpt-5.6-sol", "high")
        );
        assert!(admin::validate(&client, "openai", "gpt-5.6-sol", Some("max"), 900).is_ok());
        assert!(admin::validate(&client, "openai", "gpt-5.6-sol", None, 30).is_ok());
        assert!(admin::validate(&client, "openai", "gpt-5.6-sol", None, 29).is_err());
    }

    /// The HTTP client has no overall timeout of its own: the snapshot's bounds the call.
    #[tokio::test]
    async fn the_call_timeout_comes_from_the_snapshot() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut cfg = Config::for_tests("unused");
        cfg.openai_compat_base_url = Some(format!("http://{}/v1", listener.local_addr().unwrap()));
        // A server that accepts and never answers.
        tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((sock, _)) = listener.accept().await {
                held.push(sock);
            }
        });
        let client = AiClient::from_config(&cfg);
        let snap = client.with_settings(&AiSettings {
            kind: AiProviderKind::OpenAiCompatible,
            model: "m".into(),
            effort: "low".into(),
            timeout_secs: 1,
        });
        let req = LlmRequest {
            system: String::new(),
            user: String::new(),
            schema_name: "t",
            schema: Value::Null,
            fake_context: Value::Null,
            attempt: 1,
            max_tokens: None,
            temperature: None,
            images: Vec::new(),
        };
        let start = std::time::Instant::now();
        assert_eq!(snap.complete(&req).await, Err(ProviderError::Timeout));
        assert!(start.elapsed() < Duration::from_secs(5));
    }

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
