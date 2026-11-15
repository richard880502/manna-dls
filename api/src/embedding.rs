//! Text embeddings for hybrid retrieval (docs/API.md「Phase 2.4」).
//!
//! * `EMBEDDING_PROVIDER=openai_compatible` calls `POST {EMBEDDING_BASE_URL}/embeddings` (the
//!   NCHC gateway; its embeddings key is separate from the chat key). `fake` is a deterministic
//!   offline embedder for tests; `none` (default) turns embeddings off.
//! * Embeddings are an optional improvement, never a dependency: a bad setting disables them
//!   with a warning instead of stopping the server, and every failure at request time makes the
//!   caller fall back to keyword retrieval (`retrieval.rs`).
//! * The API key is never logged: [`EmbeddingConfig`]'s `Debug` leaves it out and error
//!   messages carry the HTTP status but never the response body or the URL.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::retrieval::tokenize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddingProviderKind {
    None,
    Fake,
    OpenAiCompatible,
}

impl EmbeddingProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Fake => "fake",
            Self::OpenAiCompatible => "openai_compatible",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "none" => Some(Self::None),
            "fake" => Some(Self::Fake),
            "openai_compatible" | "openai-compatible" => Some(Self::OpenAiCompatible),
            _ => None,
        }
    }
}

/// The model the retrieval evaluation was run with (docs/RETRIEVAL_EVAL.md).
pub const DEFAULT_MODEL: &str = "embeddinggemma-2";
pub const DEFAULT_DIMS: usize = 768;
/// pgvector's limit for a `vector` value.
pub const MAX_DIMS: usize = 16_000;
const DEFAULT_QUERY_TIMEOUT_MS: u64 = 2_500;
const DEFAULT_BATCH_TIMEOUT_SECS: u64 = 60;
const FAKE_MODEL: &str = "fake-embedding";
const FAKE_DIMS: usize = 64;

#[derive(Clone, PartialEq, Eq)]
pub struct EmbeddingConfig {
    pub provider: EmbeddingProviderKind,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub model: String,
    pub dims: usize,
    /// Budget for embedding one query inside a request; past it retrieval is keyword-only.
    pub query_timeout_ms: u64,
    /// Budget for one batch of the background indexing job.
    pub batch_timeout_secs: u64,
    /// Why embeddings were turned off although `EMBEDDING_PROVIDER` asked for them.
    pub warning: Option<String>,
}

// Hand-written so the API key never reaches logs.
impl std::fmt::Debug for EmbeddingConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EmbeddingConfig")
            .field("provider", &self.provider)
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("dims", &self.dims)
            .field("query_timeout_ms", &self.query_timeout_ms)
            .field("warning", &self.warning)
            .finish_non_exhaustive()
    }
}

impl EmbeddingConfig {
    pub fn disabled() -> Self {
        Self {
            provider: EmbeddingProviderKind::None,
            base_url: None,
            api_key: None,
            model: String::new(),
            dims: DEFAULT_DIMS,
            query_timeout_ms: DEFAULT_QUERY_TIMEOUT_MS,
            batch_timeout_secs: DEFAULT_BATCH_TIMEOUT_SECS,
            warning: None,
        }
    }

    /// The deterministic test embedder.
    pub fn fake() -> Self {
        Self {
            provider: EmbeddingProviderKind::Fake,
            model: FAKE_MODEL.into(),
            dims: FAKE_DIMS,
            ..Self::disabled()
        }
    }

    fn off(warning: String) -> Self {
        Self {
            warning: Some(warning),
            ..Self::disabled()
        }
    }

    /// Reads `EMBEDDING_*`. Never fails: an invalid or incomplete setting yields a disabled
    /// config carrying a warning, so a typo cannot keep the server from starting.
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Self {
        let get = |k: &str| {
            get(k)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        let raw = get("EMBEDDING_PROVIDER").unwrap_or_default();
        let Some(provider) = EmbeddingProviderKind::parse(&raw) else {
            return Self::off(format!(
                "EMBEDDING_PROVIDER={raw:?} is not openai_compatible, fake or none; embeddings are off"
            ));
        };
        let number =
            |var: &str, default: u64| match get(var) {
                None => Ok(default),
                Some(v) => v.parse::<u64>().ok().filter(|n| *n > 0).ok_or_else(|| {
                    format!("{var}={v:?} is not a positive number; embeddings are off")
                }),
            };
        let numbers = (|| {
            Ok::<_, String>((
                number("EMBEDDING_DIMS", DEFAULT_DIMS as u64)?,
                number("EMBEDDING_QUERY_TIMEOUT_MS", DEFAULT_QUERY_TIMEOUT_MS)?,
                number("EMBEDDING_TIMEOUT_SECS", DEFAULT_BATCH_TIMEOUT_SECS)?,
            ))
        })();
        match provider {
            EmbeddingProviderKind::None => Self::disabled(),
            EmbeddingProviderKind::Fake => Self {
                model: get("EMBEDDING_MODEL").unwrap_or_else(|| FAKE_MODEL.into()),
                ..Self::fake()
            },
            EmbeddingProviderKind::OpenAiCompatible => {
                let (dims, query_timeout_ms, batch_timeout_secs) = match numbers {
                    Ok(n) => n,
                    Err(warning) => return Self::off(warning),
                };
                if dims as usize > MAX_DIMS {
                    return Self::off(format!(
                        "EMBEDDING_DIMS={dims} is above {MAX_DIMS}; embeddings are off"
                    ));
                }
                let Some(base_url) = get("EMBEDDING_BASE_URL") else {
                    return Self::off(
                        "EMBEDDING_PROVIDER=openai_compatible needs EMBEDDING_BASE_URL; embeddings are off"
                            .into(),
                    );
                };
                Self {
                    provider,
                    base_url: Some(base_url),
                    api_key: get("EMBEDDING_API_KEY"),
                    model: get("EMBEDDING_MODEL").unwrap_or_else(|| DEFAULT_MODEL.into()),
                    dims: dims as usize,
                    query_timeout_ms,
                    batch_timeout_secs,
                    warning: None,
                }
            }
        }
    }
}

#[derive(Debug, Clone, thiserror::Error, PartialEq)]
pub enum EmbedError {
    #[error("embeddings are disabled")]
    Disabled,
    #[error("embedding request timed out")]
    Timeout,
    /// The response body is deliberately not kept: a gateway may echo credentials in it.
    #[error("embedding request failed: HTTP {status}")]
    Http { status: u16 },
    #[error("embedding request failed: {0}")]
    Network(String),
    #[error("unexpected embedding response: {0}")]
    Invalid(String),
}

impl EmbedError {
    /// Short code stored in `retrieval_logs.fallback_reason`.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Timeout => "embedding_timeout",
            Self::Http { .. } | Self::Network(_) | Self::Invalid(_) => "embedding_failed",
        }
    }
}

/// Queries longer than this are cut before embedding (the model reads about 2,000 tokens).
pub const QUERY_CHARS: usize = 2_000;
const QUERY_CACHE_ENTRIES: usize = 64;

#[derive(Default)]
struct QueryCache {
    by_hash: HashMap<[u8; 32], Arc<Vec<f32>>>,
    order: VecDeque<[u8; 32]>,
}

pub struct Embedder {
    config: EmbeddingConfig,
    http: reqwest::Client,
    /// Query vectors by hash of the text: one request embeds a query once however many
    /// rankings use it, and a retry of the same request does not pay again. Small and bounded.
    cache: Mutex<QueryCache>,
}

impl Embedder {
    pub fn new(config: EmbeddingConfig) -> Self {
        Self {
            config,
            http: reqwest::Client::new(),
            cache: Mutex::default(),
        }
    }

    pub fn enabled(&self) -> bool {
        self.config.provider != EmbeddingProviderKind::None
    }

    pub fn provider(&self) -> EmbeddingProviderKind {
        self.config.provider
    }

    pub fn model(&self) -> &str {
        &self.config.model
    }

    pub fn dims(&self) -> usize {
        self.config.dims
    }

    pub fn config(&self) -> &EmbeddingConfig {
        &self.config
    }

    /// One vector per text, in order, each of [`Self::dims`] dimensions.
    pub async fn embed(
        &self,
        texts: &[String],
        timeout: Duration,
    ) -> Result<Vec<Vec<f32>>, EmbedError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        match self.config.provider {
            EmbeddingProviderKind::None => Err(EmbedError::Disabled),
            EmbeddingProviderKind::Fake => Ok(texts
                .iter()
                .map(|t| fake_embedding(t, self.config.dims))
                .collect()),
            EmbeddingProviderKind::OpenAiCompatible => {
                // The outer timeout also bounds connection setup and a stalled body.
                match tokio::time::timeout(timeout, self.call(texts, timeout)).await {
                    Ok(result) => result,
                    Err(_) => Err(EmbedError::Timeout),
                }
            }
        }
    }

    /// Batch embedding for the indexing job.
    pub async fn embed_documents(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbedError> {
        self.embed(texts, Duration::from_secs(self.config.batch_timeout_secs))
            .await
    }

    /// The vector of one query, within the short request-time budget; cached.
    pub async fn embed_query(&self, query: &str) -> Result<Arc<Vec<f32>>, EmbedError> {
        if !self.enabled() {
            return Err(EmbedError::Disabled);
        }
        let text: String = query.trim().chars().take(QUERY_CHARS).collect();
        let key: [u8; 32] = Sha256::digest(text.as_bytes()).into();
        if let Some(hit) = self
            .cache
            .lock()
            .ok()
            .and_then(|c| c.by_hash.get(&key).cloned())
        {
            return Ok(hit);
        }
        let timeout = Duration::from_millis(self.config.query_timeout_ms);
        let vector = self
            .embed(std::slice::from_ref(&text), timeout)
            .await?
            .pop()
            .ok_or_else(|| EmbedError::Invalid("no vector returned".into()))?;
        let vector = Arc::new(vector);
        if let Ok(mut cache) = self.cache.lock() {
            if cache.by_hash.insert(key, vector.clone()).is_none() {
                cache.order.push_back(key);
            }
            while cache.order.len() > QUERY_CACHE_ENTRIES {
                if let Some(old) = cache.order.pop_front() {
                    cache.by_hash.remove(&old);
                }
            }
        }
        Ok(vector)
    }

    async fn call(&self, texts: &[String], timeout: Duration) -> Result<Vec<Vec<f32>>, EmbedError> {
        let base = self.config.base_url.as_deref().unwrap_or_default();
        let mut request = self
            .http
            .post(format!("{}/embeddings", base.trim_end_matches('/')))
            .timeout(timeout)
            .json(&json!({ "model": self.config.model, "input": texts }));
        if let Some(key) = &self.config.api_key {
            request = request.bearer_auth(key);
        }
        let from_reqwest = |e: reqwest::Error| {
            if e.is_timeout() {
                EmbedError::Timeout
            } else {
                // `without_url`: the message must not depend on what the URL contains.
                EmbedError::Network(e.without_url().to_string())
            }
        };
        let response = request.send().await.map_err(from_reqwest)?;
        let status = response.status().as_u16();
        let body = response.text().await.map_err(from_reqwest)?;
        parse_response(status, &body, texts.len(), self.config.dims)
    }
}

/// Reads an OpenAI-style embeddings response: `data[i].embedding`, ordered by `data[i].index`.
pub fn parse_response(
    status: u16,
    body: &str,
    expected: usize,
    dims: usize,
) -> Result<Vec<Vec<f32>>, EmbedError> {
    if status == 408 || status == 504 {
        return Err(EmbedError::Timeout);
    }
    if !(200..300).contains(&status) {
        return Err(EmbedError::Http { status });
    }
    let value: Value = serde_json::from_str(body)
        .map_err(|e| EmbedError::Invalid(format!("response is not JSON: {e}")))?;
    let data = value["data"]
        .as_array()
        .ok_or_else(|| EmbedError::Invalid("no data array".into()))?;
    if data.len() != expected {
        return Err(EmbedError::Invalid(format!(
            "{} vectors for {expected} inputs",
            data.len()
        )));
    }
    let mut out: Vec<Option<Vec<f32>>> = vec![None; expected];
    for (position, row) in data.iter().enumerate() {
        let index = row["index"].as_u64().map_or(position, |i| i as usize);
        let vector: Vec<f32> = row["embedding"]
            .as_array()
            .ok_or_else(|| EmbedError::Invalid("embedding is not an array".into()))?
            .iter()
            .map(|x| x.as_f64().map(|f| f as f32))
            .collect::<Option<_>>()
            .ok_or_else(|| EmbedError::Invalid("embedding holds a non-number".into()))?;
        if vector.len() != dims {
            return Err(EmbedError::Invalid(format!(
                "vector has {} dimensions, EMBEDDING_DIMS is {dims}",
                vector.len()
            )));
        }
        if !vector.iter().all(|x| x.is_finite()) || vector.iter().all(|x| *x == 0.0) {
            return Err(EmbedError::Invalid("vector is zero or not finite".into()));
        }
        match out.get_mut(index) {
            Some(slot @ None) => *slot = Some(vector),
            _ => return Err(EmbedError::Invalid(format!("bad index {index}"))),
        }
    }
    out.into_iter()
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| EmbedError::Invalid("missing index".into()))
}

/// Deterministic stand-in for a model: each token of the retrieval tokenizer adds one to a
/// bucket chosen by its hash, then the vector is normalised. Texts sharing words are close,
/// unrelated texts are not; nothing about meaning is modelled.
pub fn fake_embedding(text: &str, dims: usize) -> Vec<f32> {
    let dims = dims.max(1);
    let mut v = vec![0f32; dims];
    for token in tokenize(text) {
        let h = Sha256::digest(token.as_bytes());
        let bucket = u64::from_le_bytes(h[..8].try_into().unwrap_or_default()) as usize % dims;
        v[bucket] += 1.0;
    }
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm == 0.0 {
        // No usable token: a fixed unit vector, so the value is still a valid embedding.
        v[0] = 1.0;
    } else {
        v.iter_mut().for_each(|x| *x /= norm);
    }
    v
}

/// pgvector's text form, bound as a string and cast with `::vector`.
pub fn vector_literal(v: &[f32]) -> String {
    let mut s = String::with_capacity(v.len() * 10 + 2);
    s.push('[');
    for (i, x) in v.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&x.to_string());
    }
    s.push(']');
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(pairs: &[(&str, &str)]) -> EmbeddingConfig {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        EmbeddingConfig::from_lookup(|k| map.get(k).cloned())
    }

    fn cosine(a: &[f32], b: &[f32]) -> f32 {
        a.iter().zip(b).map(|(x, y)| x * y).sum()
    }

    #[test]
    fn default_is_off_without_warning() {
        let c = cfg(&[]);
        assert_eq!(c.provider, EmbeddingProviderKind::None);
        assert!(c.warning.is_none());
        assert!(!Embedder::new(c).enabled());
    }

    #[test]
    fn openai_compatible_defaults_to_the_evaluated_model() {
        let c = cfg(&[
            ("EMBEDDING_PROVIDER", "openai_compatible"),
            ("EMBEDDING_BASE_URL", "http://x/v1"),
            ("EMBEDDING_API_KEY", "secret-key"),
        ]);
        assert_eq!(c.provider, EmbeddingProviderKind::OpenAiCompatible);
        assert_eq!((c.model.as_str(), c.dims), (DEFAULT_MODEL, DEFAULT_DIMS));
        assert_eq!(c.query_timeout_ms, 2_500);
        assert!(
            !format!("{c:?}").contains("secret-key"),
            "the key must not be printed"
        );
    }

    #[test]
    fn bad_settings_disable_embeddings_instead_of_failing() {
        for pairs in [
            vec![("EMBEDDING_PROVIDER", "gemma")],
            vec![("EMBEDDING_PROVIDER", "openai_compatible")],
            vec![
                ("EMBEDDING_PROVIDER", "openai_compatible"),
                ("EMBEDDING_BASE_URL", "http://x/v1"),
                ("EMBEDDING_DIMS", "lots"),
            ],
            vec![
                ("EMBEDDING_PROVIDER", "openai_compatible"),
                ("EMBEDDING_BASE_URL", "http://x/v1"),
                ("EMBEDDING_DIMS", "99999"),
            ],
            vec![
                ("EMBEDDING_PROVIDER", "openai_compatible"),
                ("EMBEDDING_BASE_URL", "http://x/v1"),
                ("EMBEDDING_QUERY_TIMEOUT_MS", "0"),
            ],
        ] {
            let c = cfg(&pairs);
            assert_eq!(c.provider, EmbeddingProviderKind::None, "{pairs:?}");
            assert!(c.warning.is_some(), "{pairs:?}");
        }
    }

    #[test]
    fn fake_embedding_is_deterministic_normalised_and_word_based() {
        let a = fake_embedding("能讀懂短文並找出主旨", 64);
        assert_eq!(a, fake_embedding("能讀懂短文並找出主旨", 64));
        assert!((cosine(&a, &a) - 1.0).abs() < 1e-5);
        let near = fake_embedding("讀懂短文的主旨", 64);
        let far = fake_embedding("telephone etiquette", 64);
        assert!(cosine(&a, &near) > cosine(&a, &far) + 0.2);
        assert_eq!(fake_embedding("", 8)[0], 1.0);
    }

    #[test]
    fn response_is_ordered_by_index_and_checked() {
        let body = r#"{"data":[{"index":1,"embedding":[0,1]},{"index":0,"embedding":[1,0]}]}"#;
        assert_eq!(
            parse_response(200, body, 2, 2).unwrap(),
            vec![vec![1.0, 0.0], vec![0.0, 1.0]]
        );
        assert!(matches!(
            parse_response(200, body, 2, 3),
            Err(EmbedError::Invalid(_))
        ));
        assert!(matches!(
            parse_response(200, body, 3, 2),
            Err(EmbedError::Invalid(_))
        ));
        let zero = r#"{"data":[{"index":0,"embedding":[0,0]}]}"#;
        assert!(matches!(
            parse_response(200, zero, 1, 2),
            Err(EmbedError::Invalid(_))
        ));
        let twice = r#"{"data":[{"index":0,"embedding":[1,0]},{"index":0,"embedding":[1,0]}]}"#;
        assert!(parse_response(200, twice, 2, 2).is_err());
        assert_eq!(parse_response(504, "", 1, 2), Err(EmbedError::Timeout));
        assert_eq!(
            parse_response(401, "no", 1, 2).unwrap_err().code(),
            "embedding_failed"
        );
        assert!(parse_response(200, "<html>", 1, 2).is_err());
    }

    #[test]
    fn vector_literal_is_pgvector_text() {
        assert_eq!(vector_literal(&[1.0, -0.5, 0.25]), "[1,-0.5,0.25]");
    }

    #[tokio::test]
    async fn disabled_embedder_refuses_and_fake_caches_queries() {
        let off = Embedder::new(EmbeddingConfig::disabled());
        assert_eq!(off.embed_query("x").await, Err(EmbedError::Disabled));
        let fake = Embedder::new(EmbeddingConfig::fake());
        let a = fake.embed_query("短文 閱讀").await.unwrap();
        let b = fake.embed_query("  短文 閱讀  ").await.unwrap();
        assert!(Arc::ptr_eq(&a, &b), "same text is embedded once");
        assert_eq!(a.len(), fake.dims());
    }
}
