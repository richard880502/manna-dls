//! The picture model: OpenAI's Image API (`POST {IMAGE_BASE_URL}/images/generations` with a GPT
//! Image model) or a deterministic fake for tests.
//!
//! * `IMAGE_PROVIDER=openai` needs `OPENAI_API_KEY`; `IMAGE_BASE_URL` defaults to
//!   `https://api.openai.com/v1`. `fake` answers with a small PNG; `none` (default) turns
//!   pictures off. When `AI_PROVIDER` is `fake` and `IMAGE_PROVIDER` is not set, pictures are
//!   `fake` too (as OCR). The deployment (`docker-compose.yml`) passes `none` unless Coolify sets
//!   `IMAGE_PROVIDER`.
//! * Like OCR, pictures are optional: a bad setting turns them off with a warning instead of
//!   stopping the server.
//! * The API key never reaches logs: [`ImageConfig`]'s `Debug` leaves it out, and errors carry
//!   the HTTP status (and, for a refusal, OpenAI's fixed error code) but never the response body
//!   or the URL.
//!
//! The request follows the Image API reference (checked 2026-11-17): GPT Image models always
//! answer with base64 in `data[0].b64_json` (`response_format` and `url` are not supported for
//! them); `size` is `auto` or `WIDTHxHEIGHT` (2.x models: both edges multiples of 16, at most
//! 3840, aspect between 1:3 and 3:1); `quality` is `low`, `medium`, `high`, `auto`, and for the
//! 2.5 / 2.6 models also `xhigh` and `max`; `output_format` `png` | `webp` | `jpeg`, with
//! `output_compression` 0–100 for webp and jpeg; `moderation` `auto` (default) or `low`;
//! `safety_identifier` ≤ 128 characters. A request refused by the safety system answers
//! `error.code = "moderation_blocked"`; it is not retried.

use std::collections::HashSet;
use std::io::Cursor;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use serde_json::{json, Value};
use tokio::sync::Semaphore;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageProviderKind {
    None,
    Fake,
    OpenAi,
}

impl ImageProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Fake => "fake",
            Self::OpenAi => "openai",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "none" => Some(Self::None),
            "fake" => Some(Self::Fake),
            "openai" => Some(Self::OpenAi),
            _ => None,
        }
    }
}

pub const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";
pub const DEFAULT_MODEL: &str = "gpt-image-2.5-flare";
pub const DEFAULT_QUALITY: &str = "medium";
/// Landscape: the shape of a picture across a worksheet column.
pub const DEFAULT_SIZE: &str = "1536x1024";
/// OpenAI says a complex prompt may take up to two minutes; a timed-out call may still be
/// billed, so the budget is generous.
pub const DEFAULT_TIMEOUT_SECS: u64 = 180;
pub const DEFAULT_CONCURRENCY: usize = 2;
/// Pictures made by themselves for one worksheet (its slots, in worksheet order).
pub const DEFAULT_MAX_PER_WORKSHEET: u32 = 6;
/// Teacher-requested 「重畫」 per worksheet, all slots together.
pub const DEFAULT_MAX_REGENERATIONS: u32 = 10;
pub const QUALITIES: [&str; 6] = ["low", "medium", "high", "xhigh", "max", "auto"];
/// Always JPEG: Word on school PCs and the PDF export both take it, and it is the faster
/// output. The server re-encodes every picture anyway (see `super::normalize`).
pub const OUTPUT_FORMAT: &str = "jpeg";
pub const OUTPUT_COMPRESSION: u8 = 90;
/// Default filtering, which keeps the output suitable for minors (never `low`).
pub const MODERATION: &str = "auto";
/// The decoded picture may not be larger than this.
pub const MAX_IMAGE_BYTES: usize = 20 * 1024 * 1024;
const FAKE_MODEL: &str = "fake-image";

#[derive(Clone, PartialEq, Eq)]
pub struct ImageConfig {
    pub provider: ImageProviderKind,
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
    pub quality: String,
    pub size: String,
    /// Budget of one request.
    pub timeout_secs: u64,
    /// Requests running at once across the whole server.
    pub concurrency: usize,
    pub max_per_worksheet: u32,
    pub max_regenerations: u32,
    /// Why pictures are off although `IMAGE_PROVIDER` asked for them.
    pub warning: Option<String>,
}

// Hand-written so the API key never reaches logs.
impl std::fmt::Debug for ImageConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageConfig")
            .field("provider", &self.provider)
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("quality", &self.quality)
            .field("size", &self.size)
            .field("timeout_secs", &self.timeout_secs)
            .field("concurrency", &self.concurrency)
            .field("max_per_worksheet", &self.max_per_worksheet)
            .field("max_regenerations", &self.max_regenerations)
            .field("warning", &self.warning)
            .finish_non_exhaustive()
    }
}

/// `auto`, or `WIDTHxHEIGHT` with both edges multiples of 16 between 16 and 3840 and an aspect
/// ratio between 1:3 and 3:1 (the 2.x models' rule; 1024x1024, 1536x1024 and 1024x1536 suit
/// every GPT Image model).
pub fn valid_size(size: &str) -> bool {
    if size == "auto" {
        return true;
    }
    let Some((w, h)) = size.split_once('x') else {
        return false;
    };
    let (Ok(w), Ok(h)) = (w.parse::<u32>(), h.parse::<u32>()) else {
        return false;
    };
    let edge = |n: u32| (16..=3840).contains(&n) && n.is_multiple_of(16);
    edge(w) && edge(h) && w.max(h) <= 3 * w.min(h)
}

impl ImageConfig {
    pub fn disabled() -> Self {
        Self {
            provider: ImageProviderKind::None,
            base_url: DEFAULT_BASE_URL.into(),
            api_key: None,
            model: String::new(),
            quality: DEFAULT_QUALITY.into(),
            size: DEFAULT_SIZE.into(),
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            concurrency: DEFAULT_CONCURRENCY,
            max_per_worksheet: DEFAULT_MAX_PER_WORKSHEET,
            max_regenerations: DEFAULT_MAX_REGENERATIONS,
            warning: None,
        }
    }

    /// The deterministic test provider.
    pub fn fake() -> Self {
        Self {
            provider: ImageProviderKind::Fake,
            model: FAKE_MODEL.into(),
            ..Self::disabled()
        }
    }

    fn off(warning: String) -> Self {
        Self {
            warning: Some(warning),
            ..Self::disabled()
        }
    }

    pub fn enabled(&self) -> bool {
        self.provider != ImageProviderKind::None
    }

    /// Reads `IMAGE_*` and `OPENAI_API_KEY` (empty values count as unset). `ai_is_fake`:
    /// `AI_PROVIDER=fake`, which makes `fake` the default. Never fails: an invalid setting
    /// yields a disabled config with a warning.
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>, ai_is_fake: bool) -> Self {
        let get = |k: &str| {
            get(k)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        let provider = match get("IMAGE_PROVIDER") {
            None if ai_is_fake => ImageProviderKind::Fake,
            None => ImageProviderKind::None,
            Some(raw) => match ImageProviderKind::parse(&raw) {
                Some(p) => p,
                None => {
                    return Self::off(format!(
                        "IMAGE_PROVIDER={raw:?} is not openai, fake or none; pictures are off"
                    ))
                }
            },
        };
        if provider == ImageProviderKind::None {
            return Self::disabled();
        }
        let number = |var: &str, default: u64, zero_ok: bool| match get(var) {
            None => Ok(default),
            Some(v) => v
                .parse::<u64>()
                .ok()
                .filter(|n| zero_ok || *n > 0)
                .filter(|n| *n <= 100_000)
                .ok_or_else(|| format!("{var}={v:?} is not a valid number; pictures are off")),
        };
        let numbers = (|| {
            Ok::<_, String>((
                number("IMAGE_TIMEOUT_SECS", DEFAULT_TIMEOUT_SECS, false)?,
                number("IMAGE_CONCURRENCY", DEFAULT_CONCURRENCY as u64, false)?,
                number(
                    "IMAGE_MAX_PER_WORKSHEET",
                    u64::from(DEFAULT_MAX_PER_WORKSHEET),
                    true,
                )?,
                number(
                    "IMAGE_MAX_REGENERATIONS",
                    u64::from(DEFAULT_MAX_REGENERATIONS),
                    true,
                )?,
            ))
        })();
        let (timeout_secs, concurrency, max_per_worksheet, max_regenerations) = match numbers {
            Ok(n) => n,
            Err(warning) => return Self::off(warning),
        };
        let quality = get("IMAGE_QUALITY")
            .map(|q| q.to_ascii_lowercase())
            .unwrap_or_else(|| DEFAULT_QUALITY.into());
        if !QUALITIES.contains(&quality.as_str()) {
            return Self::off(format!(
                "IMAGE_QUALITY={quality:?} is not low, medium, high, xhigh, max or auto; pictures are off"
            ));
        }
        let size = get("IMAGE_SIZE")
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_else(|| DEFAULT_SIZE.into());
        if !valid_size(&size) {
            return Self::off(format!(
                "IMAGE_SIZE={size:?} is not auto or WIDTHxHEIGHT (multiples of 16, at most 3840, aspect within 3:1); pictures are off"
            ));
        }
        let base = Self {
            provider,
            quality,
            size,
            timeout_secs,
            concurrency: concurrency as usize,
            max_per_worksheet: max_per_worksheet as u32,
            max_regenerations: max_regenerations as u32,
            ..Self::disabled()
        };
        match provider {
            ImageProviderKind::None => Self::disabled(),
            ImageProviderKind::Fake => Self {
                model: get("IMAGE_MODEL").unwrap_or_else(|| FAKE_MODEL.into()),
                ..base
            },
            ImageProviderKind::OpenAi => {
                let Some(api_key) = get("OPENAI_API_KEY") else {
                    return Self::off(
                        "IMAGE_PROVIDER=openai needs OPENAI_API_KEY; pictures are off".into(),
                    );
                };
                let base_url = get("IMAGE_BASE_URL").unwrap_or_else(|| DEFAULT_BASE_URL.into());
                if !(base_url.starts_with("https://") || base_url.starts_with("http://")) {
                    return Self::off(format!(
                        "IMAGE_BASE_URL={base_url:?} is not an http(s) URL; pictures are off"
                    ));
                }
                Self {
                    base_url,
                    api_key: Some(api_key),
                    model: get("IMAGE_MODEL").unwrap_or_else(|| DEFAULT_MODEL.into()),
                    ..base
                }
            }
        }
    }
}

#[derive(Debug, Clone, thiserror::Error, PartialEq)]
pub enum ImageError {
    #[error("picture generation is disabled")]
    Disabled,
    #[error("picture request timed out")]
    Timeout,
    /// The response body is deliberately not kept: a gateway may echo credentials in it.
    #[error("picture request failed: HTTP {status}")]
    Http { status: u16 },
    /// OpenAI's safety system refused the prompt or the picture (`moderation_blocked`).
    #[error("picture request refused by the provider's safety system (HTTP {status})")]
    Moderated { status: u16 },
    #[error("picture request failed: {0}")]
    Network(String),
    #[error("unexpected picture response: {0}")]
    Invalid(String),
}

impl ImageError {
    /// A rate limit, a 5xx or a dropped connection is tried once more. A timeout is not: the
    /// picture may have been made (and billed) anyway. A refusal never is.
    pub fn retryable(&self) -> bool {
        match self {
            Self::Network(_) => true,
            Self::Http { status } => *status == 429 || *status >= 500,
            Self::Disabled | Self::Timeout | Self::Moderated { .. } | Self::Invalid(_) => false,
        }
    }

    /// `ai_interactions.status` for this failure.
    pub fn interaction_status(&self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::Moderated { .. } => "blocked",
            _ => "error",
        }
    }

    /// Why the slot has no picture (Chinese, shown to the teacher). Generic on purpose: the
    /// provider's moderation details only go to the log.
    pub fn slot_message(&self) -> String {
        match self {
            Self::Disabled => "這個環境沒有開啟 AI 產圖；請自己放一張圖。".into(),
            Self::Timeout => "產圖逾時；可以按「重畫」再試一次，或上傳自己的圖。".into(),
            Self::Http { status: 429 } => {
                "產圖服務目前太忙；請稍後按「重畫」，或上傳自己的圖。".into()
            }
            Self::Http { status } => {
                format!(
                    "產圖服務暫時無法使用（HTTP {status}）；可以稍後按「重畫」，或上傳自己的圖。"
                )
            }
            Self::Moderated { .. } => {
                "這張圖的描述沒有通過產圖服務的內容檢查；請換個說法重畫，或上傳自己的圖。".into()
            }
            Self::Network(_) => "連不上產圖服務；可以稍後按「重畫」，或上傳自己的圖。".into(),
            Self::Invalid(_) => "產圖服務回傳的圖讀不出來；可以按「重畫」再試一次。".into(),
        }
    }
}

/// Token counts of one picture, for cost tracking (no text).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct ImageUsage {
    pub input_tokens: i64,
    pub input_text_tokens: i64,
    pub input_image_tokens: i64,
    pub output_tokens: i64,
    pub output_image_tokens: i64,
    pub total_tokens: i64,
}

impl ImageUsage {
    fn of(v: &Value) -> Option<Self> {
        let n = |x: &Value| x.as_i64().unwrap_or(0);
        v.is_object().then(|| Self {
            input_tokens: n(&v["input_tokens"]),
            input_text_tokens: n(&v["input_tokens_details"]["text_tokens"]),
            input_image_tokens: n(&v["input_tokens_details"]["image_tokens"]),
            output_tokens: n(&v["output_tokens"]),
            output_image_tokens: n(&v["output_tokens_details"]["image_tokens"]),
            total_tokens: n(&v["total_tokens"]),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImageReply {
    /// The decoded picture as the provider sent it (PNG, JPEG or WebP).
    pub bytes: Vec<u8>,
    pub usage: Option<ImageUsage>,
}

/// The provider plus the server-wide limits: [`Images::slots`] bounds the requests running at
/// once (`IMAGE_CONCURRENCY`), and [`Images::live`] knows which picture rows have a job in this
/// process.
pub struct Images {
    config: ImageConfig,
    http: reqwest::Client,
    pub slots: Arc<Semaphore>,
    live: Arc<Mutex<HashSet<i64>>>,
}

/// Registers a job of this process for a picture row while it is held.
pub struct LiveImage {
    live: Arc<Mutex<HashSet<i64>>>,
    id: i64,
}

impl Drop for LiveImage {
    fn drop(&mut self) {
        if let Ok(mut live) = self.live.lock() {
            live.remove(&self.id);
        }
    }
}

impl Images {
    pub fn new(config: ImageConfig) -> Self {
        let slots = config.concurrency.max(1);
        Self {
            config,
            http: reqwest::Client::new(),
            slots: Arc::new(Semaphore::new(slots)),
            live: Arc::default(),
        }
    }

    pub fn enabled(&self) -> bool {
        self.config.enabled()
    }

    pub fn config(&self) -> &ImageConfig {
        &self.config
    }

    pub fn provider_name(&self) -> &'static str {
        self.config.provider.as_str()
    }

    pub fn model(&self) -> &str {
        &self.config.model
    }

    pub fn track(&self, id: i64) -> LiveImage {
        if let Ok(mut live) = self.live.lock() {
            live.insert(id);
        }
        LiveImage {
            live: self.live.clone(),
            id,
        }
    }

    /// Picture rows with a job in this process.
    pub fn live_ids(&self) -> Vec<i64> {
        self.live
            .lock()
            .map(|l| l.iter().copied().collect())
            .unwrap_or_default()
    }

    /// Makes one picture. One request, no retry (the job retries and logs each attempt).
    pub async fn generate(
        &self,
        prompt: &str,
        safety_identifier: &str,
    ) -> Result<ImageReply, ImageError> {
        match self.config.provider {
            ImageProviderKind::None => Err(ImageError::Disabled),
            ImageProviderKind::Fake => fake_reply(prompt).await,
            ImageProviderKind::OpenAi => {
                let timeout = Duration::from_secs(self.config.timeout_secs);
                match tokio::time::timeout(timeout, self.call(prompt, safety_identifier, timeout))
                    .await
                {
                    Ok(result) => result,
                    Err(_) => Err(ImageError::Timeout),
                }
            }
        }
    }

    async fn call(
        &self,
        prompt: &str,
        safety_identifier: &str,
        timeout: Duration,
    ) -> Result<ImageReply, ImageError> {
        let mut request = self
            .http
            .post(format!(
                "{}/images/generations",
                self.config.base_url.trim_end_matches('/')
            ))
            .timeout(timeout)
            .json(&request_body(&self.config, prompt, safety_identifier));
        if let Some(key) = &self.config.api_key {
            request = request.bearer_auth(key);
        }
        let from_reqwest = |e: reqwest::Error| {
            if e.is_timeout() {
                ImageError::Timeout
            } else {
                ImageError::Network(e.without_url().to_string())
            }
        };
        let response = request.send().await.map_err(from_reqwest)?;
        let status = response.status().as_u16();
        let body = response.text().await.map_err(from_reqwest)?;
        parse_response(status, &body)
    }
}

/// The `images/generations` body: one picture, the configured size and quality, JPEG, default
/// moderation, and a hashed per-teacher `safety_identifier`.
pub fn request_body(config: &ImageConfig, prompt: &str, safety_identifier: &str) -> Value {
    let mut body = json!({
        "model": config.model,
        "prompt": prompt,
        "n": 1,
        "size": config.size,
        "quality": config.quality,
        "output_format": OUTPUT_FORMAT,
        "output_compression": OUTPUT_COMPRESSION,
        "moderation": MODERATION,
    });
    if !safety_identifier.is_empty() {
        body["safety_identifier"] = json!(safety_identifier.chars().take(128).collect::<String>());
    }
    body
}

/// `data[0].b64_json` of an Image API response, decoded, with the token counts. A refusal
/// (`error.code = "moderation_blocked"`) is told apart from other errors by its code alone;
/// nothing else of an error body is kept.
pub fn parse_response(status: u16, body: &str) -> Result<ImageReply, ImageError> {
    if status == 408 || status == 504 {
        return Err(ImageError::Timeout);
    }
    if !(200..300).contains(&status) {
        let code = serde_json::from_str::<Value>(body)
            .ok()
            .and_then(|v| v["error"]["code"].as_str().map(str::to_string));
        if code.as_deref() == Some("moderation_blocked") {
            return Err(ImageError::Moderated { status });
        }
        return Err(ImageError::Http { status });
    }
    let v: Value = serde_json::from_str(body)
        .map_err(|e| ImageError::Invalid(format!("response is not JSON: {e}")))?;
    let b64 = v["data"][0]["b64_json"]
        .as_str()
        .ok_or_else(|| ImageError::Invalid("no data[0].b64_json".into()))?;
    if b64.len() > MAX_IMAGE_BYTES / 3 * 4 + 4 {
        return Err(ImageError::Invalid("picture too large".into()));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .map_err(|_| ImageError::Invalid("b64_json is not base64".into()))?;
    if bytes.is_empty() {
        return Err(ImageError::Invalid("empty picture".into()));
    }
    Ok(ImageReply {
        bytes,
        usage: ImageUsage::of(&v["usage"]),
    })
}

// ---------------------------------------------------------------- fake

/// Seconds the fake takes for a `FAKE_IMG_SLOW` prompt.
pub const FAKE_SLOW_SECS: u64 = 2;

/// A small PNG whose colour follows the prompt (the same prompt gives the same picture).
pub fn fake_png(prompt: &str) -> Vec<u8> {
    let hash = crate::auth::sha256_hex(prompt);
    let byte = |i: usize| u8::from_str_radix(&hash[i * 2..i * 2 + 2], 16).unwrap_or(128);
    let (r, g, b) = (byte(0), byte(1), byte(2));
    let image = image::RgbImage::from_fn(48, 32, |x, y| {
        if x == 0 || y == 0 || x == 47 || y == 31 {
            image::Rgb([20, 20, 20])
        } else {
            image::Rgb([r, g, b])
        }
    });
    let mut out = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image)
        .write_to(&mut out, image::ImageFormat::Png)
        .expect("a PNG encodes");
    out.into_inner()
}

/// The response the Image API sends for one picture (shape as observed on the real API:
/// `data[0]` has `b64_json` and `generation_id`, and `usage` has the token counts).
pub fn fake_response_body(png: &[u8]) -> String {
    json!({
        "created": 1_763_337_600,
        "data": [{
            "b64_json": base64::engine::general_purpose::STANDARD.encode(png),
            "generation_id": "gen_fake",
        }],
        "usage": {
            "input_tokens": 20,
            "input_tokens_details": { "image_tokens": 0, "text_tokens": 20 },
            "output_tokens": 196,
            "output_tokens_details": { "image_tokens": 196, "text_tokens": 0 },
            "total_tokens": 216,
        },
    })
    .to_string()
}

/// The fake provider: the response of [`fake_response_body`] read by [`parse_response`]. Markers
/// in the prompt (the slot's description or the teacher's hint): `FAKE_IMG_FAIL` (HTTP 503,
/// every attempt), `FAKE_IMG_BLOCKED` (refused by moderation), `FAKE_IMG_TIMEOUT` (timeout),
/// `FAKE_IMG_SLOW` (takes [`FAKE_SLOW_SECS`] seconds).
pub async fn fake_reply(prompt: &str) -> Result<ImageReply, ImageError> {
    if prompt.contains("FAKE_IMG_SLOW") {
        tokio::time::sleep(Duration::from_secs(FAKE_SLOW_SECS)).await;
    }
    if prompt.contains("FAKE_IMG_FAIL") {
        return parse_response(503, "{\"error\":{\"message\":\"secret echo\"}}");
    }
    if prompt.contains("FAKE_IMG_TIMEOUT") {
        return Err(ImageError::Timeout);
    }
    if prompt.contains("FAKE_IMG_BLOCKED") {
        return parse_response(
            400,
            &json!({ "error": { "type": "image_generation_user_error",
                                "code": "moderation_blocked", "message": "x" } })
            .to_string(),
        );
    }
    parse_response(200, &fake_response_body(&fake_png(prompt)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn cfg(pairs: &[(&str, &str)], ai_fake: bool) -> ImageConfig {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        ImageConfig::from_lookup(|k| map.get(k).cloned(), ai_fake)
    }

    #[test]
    fn pictures_are_off_by_default_and_fake_with_the_fake_ai() {
        assert_eq!(cfg(&[], false).provider, ImageProviderKind::None);
        assert!(cfg(&[], false).warning.is_none());
        assert_eq!(cfg(&[], true).provider, ImageProviderKind::Fake);
        assert_eq!(
            cfg(&[("IMAGE_PROVIDER", "none")], true).provider,
            ImageProviderKind::None
        );
        let c = cfg(&[("IMAGE_PROVIDER", " ")], true);
        assert_eq!(c.provider, ImageProviderKind::Fake, "empty counts as unset");
        assert_eq!(
            (
                c.timeout_secs,
                c.concurrency,
                c.quality.as_str(),
                c.size.as_str()
            ),
            (180, 2, "medium", "1536x1024")
        );
        assert_eq!((c.max_per_worksheet, c.max_regenerations), (6, 10));
    }

    #[test]
    fn openai_needs_a_key_and_reads_every_setting() {
        let c = cfg(
            &[
                ("IMAGE_PROVIDER", "openai"),
                ("OPENAI_API_KEY", "sk-secret-key"),
            ],
            false,
        );
        assert_eq!(c.provider, ImageProviderKind::OpenAi);
        assert_eq!(c.base_url, "https://api.openai.com/v1");
        assert_eq!(c.model, "gpt-image-2.5-flare");
        assert_eq!(c.api_key.as_deref(), Some("sk-secret-key"));
        let shown = format!("{c:?}");
        assert!(!shown.contains("sk-secret-key"), "key not logged: {shown}");
        assert!(shown.contains("gpt-image-2.5-flare"));
        let own = cfg(
            &[
                ("IMAGE_PROVIDER", "OpenAI"),
                ("OPENAI_API_KEY", "k2"),
                ("IMAGE_BASE_URL", "http://proxy.test/v1/"),
                ("IMAGE_MODEL", "gpt-image-2.6-flare"),
                ("IMAGE_QUALITY", "LOW"),
                ("IMAGE_SIZE", "1024x1536"),
                ("IMAGE_TIMEOUT_SECS", "60"),
                ("IMAGE_CONCURRENCY", "4"),
                ("IMAGE_MAX_PER_WORKSHEET", "0"),
                ("IMAGE_MAX_REGENERATIONS", "3"),
            ],
            false,
        );
        assert_eq!(own.base_url, "http://proxy.test/v1/");
        assert_eq!(own.model, "gpt-image-2.6-flare");
        assert_eq!(
            (own.quality.as_str(), own.size.as_str()),
            ("low", "1024x1536")
        );
        assert_eq!((own.timeout_secs, own.concurrency), (60, 4));
        assert_eq!((own.max_per_worksheet, own.max_regenerations), (0, 3));
    }

    #[test]
    fn bad_settings_turn_pictures_off_with_a_warning() {
        for pairs in [
            vec![("IMAGE_PROVIDER", "dalle")],
            vec![("IMAGE_PROVIDER", "openai")],
            vec![
                ("IMAGE_PROVIDER", "openai"),
                ("OPENAI_API_KEY", "k"),
                ("IMAGE_BASE_URL", "ftp://x"),
            ],
            vec![("IMAGE_PROVIDER", "fake"), ("IMAGE_QUALITY", "ultra")],
            vec![("IMAGE_PROVIDER", "fake"), ("IMAGE_SIZE", "1000x1000")],
            vec![("IMAGE_PROVIDER", "fake"), ("IMAGE_SIZE", "4096x1024")],
            vec![("IMAGE_PROVIDER", "fake"), ("IMAGE_SIZE", "big")],
            vec![("IMAGE_PROVIDER", "fake"), ("IMAGE_CONCURRENCY", "0")],
            vec![("IMAGE_PROVIDER", "fake"), ("IMAGE_TIMEOUT_SECS", "soon")],
            vec![
                ("IMAGE_PROVIDER", "fake"),
                ("IMAGE_MAX_REGENERATIONS", "-1"),
            ],
        ] {
            let c = cfg(&pairs, false);
            assert_eq!(c.provider, ImageProviderKind::None, "{pairs:?}");
            assert!(c.warning.is_some(), "{pairs:?}");
            assert!(!c.warning.unwrap().contains("sk-"), "{pairs:?}");
        }
    }

    #[test]
    fn sizes() {
        for ok in [
            "auto",
            "1024x1024",
            "1536x1024",
            "1024x1536",
            "1536x864",
            "3840x2160",
        ] {
            assert!(valid_size(ok), "{ok}");
        }
        for bad in [
            "",
            "1024",
            "1000x1000",
            "3856x2160",
            "1024x4096",
            "0x0",
            "1024x-16",
        ] {
            assert!(!valid_size(bad), "{bad}");
        }
    }

    #[test]
    fn the_request_follows_the_image_api() {
        let mut c = ImageConfig::fake();
        c.model = "gpt-image-2.5-flare".into();
        let body = request_body(&c, "A cat.", &"h".repeat(200));
        assert_eq!(
            body,
            json!({
                "model": "gpt-image-2.5-flare", "prompt": "A cat.", "n": 1, "size": "1536x1024",
                "quality": "medium", "output_format": "jpeg", "output_compression": 90,
                "moderation": "auto", "safety_identifier": "h".repeat(128),
            })
        );
        // No `response_format` (GPT Image models always answer with base64) and no `user`.
        assert!(body.get("response_format").is_none() && body.get("user").is_none());
        assert!(request_body(&c, "A cat.", "")
            .get("safety_identifier")
            .is_none());
    }

    #[test]
    fn responses_and_errors() {
        let png = fake_png("x");
        let ok = parse_response(200, &fake_response_body(&png)).unwrap();
        assert_eq!(ok.bytes, png);
        assert_eq!(
            ok.usage,
            Some(ImageUsage {
                input_tokens: 20,
                input_text_tokens: 20,
                input_image_tokens: 0,
                output_tokens: 196,
                output_image_tokens: 196,
                total_tokens: 216,
            })
        );
        // Without usage it still reads.
        let bare = json!({ "data": [{ "b64_json": "iVBORw==" }] }).to_string();
        assert_eq!(parse_response(200, &bare).unwrap().usage, None);
        assert_eq!(parse_response(504, ""), Err(ImageError::Timeout));
        let echo = parse_response(503, "{\"error\":{\"message\":\"sk-secret echo\"}}").unwrap_err();
        assert_eq!(echo, ImageError::Http { status: 503 });
        assert!(!echo.to_string().contains("secret"));
        assert_eq!(
            parse_response(
                400,
                r#"{"error":{"type":"image_generation_user_error","code":"moderation_blocked","message":"m","moderation_details":{"moderation_stage":"input","categories":["violence"]}}}"#
            ),
            Err(ImageError::Moderated { status: 400 })
        );
        assert_eq!(
            parse_response(403, "not json"),
            Err(ImageError::Http { status: 403 })
        );
        for bad in [
            "{}",
            r#"{"data":[{"url":"https://x"}]}"#,
            r#"{"data":[{"b64_json":"!!"}]}"#,
        ] {
            assert!(
                matches!(parse_response(200, bad), Err(ImageError::Invalid(_))),
                "{bad}"
            );
        }
        assert!(ImageError::Http { status: 429 }.retryable());
        assert!(ImageError::Http { status: 502 }.retryable());
        assert!(ImageError::Network("x".into()).retryable());
        assert!(!ImageError::Timeout.retryable(), "may have been billed");
        assert!(!ImageError::Moderated { status: 400 }.retryable());
        assert!(!ImageError::Http { status: 401 }.retryable());
        assert_eq!(
            ImageError::Moderated { status: 400 }.interaction_status(),
            "blocked"
        );
    }

    #[tokio::test]
    async fn the_fake_follows_its_markers() {
        let a = fake_reply("A red umbrella").await.unwrap();
        assert_eq!(
            a,
            fake_reply("A red umbrella").await.unwrap(),
            "deterministic"
        );
        assert!(a.bytes.starts_with(b"\x89PNG"));
        assert_eq!(a.usage.unwrap().total_tokens, 216);
        assert_eq!(
            fake_reply("x FAKE_IMG_FAIL").await,
            Err(ImageError::Http { status: 503 })
        );
        assert_eq!(
            fake_reply("x FAKE_IMG_BLOCKED").await,
            Err(ImageError::Moderated { status: 400 })
        );
        assert_eq!(
            fake_reply("x FAKE_IMG_TIMEOUT").await,
            Err(ImageError::Timeout)
        );
    }
}
