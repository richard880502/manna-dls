//! The OCR model: `chandra-ocr-2` on the NCHC gateway (OpenAI-compatible chat completions with
//! one page image per request), or a deterministic fake for tests.
//!
//! * `OCR_PROVIDER=openai_compatible` posts to `{OCR_BASE_URL}/chat/completions`; the base URL
//!   and key default to `EMBEDDING_BASE_URL` / `EMBEDDING_API_KEY` (the same NCHC gateway).
//!   `fake` answers with fixed HTML per page; `none` (default) turns OCR off. When `AI_PROVIDER`
//!   is `fake` and `OCR_PROVIDER` is not set, OCR is `fake` too. The deployment
//!   (`docker-compose.yml`) passes `openai_compatible` unless Coolify sets `OCR_PROVIDER`.
//! * Like embeddings, OCR is optional: a bad setting turns it off with a warning instead of
//!   stopping the server.
//! * The API key never reaches logs: [`OcrConfig`]'s `Debug` leaves it out, and errors carry
//!   the HTTP status but never the response body or the URL.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use serde_json::{json, Value};
use tokio::sync::Semaphore;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OcrProviderKind {
    None,
    Fake,
    OpenAiCompatible,
}

impl OcrProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Fake => "fake",
            Self::OpenAiCompatible => "openai_compatible",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "none" => Some(Self::None),
            "fake" => Some(Self::Fake),
            "openai_compatible" | "openai-compatible" => Some(Self::OpenAiCompatible),
            _ => None,
        }
    }
}

pub const DEFAULT_MODEL: &str = "chandra-ocr-2";
pub const DEFAULT_TIMEOUT_SECS: u64 = 120;
pub const DEFAULT_MAX_PAGES: usize = 40;
pub const DEFAULT_CONCURRENCY: usize = 2;
/// Output budget of one page (a dense A4 page of the probe used well under half of it).
pub const MAX_TOKENS: u32 = 4000;
/// The instruction sent with every page (what the probe used).
pub const PROMPT: &str = "OCR this page to markdown.";
const FAKE_MODEL: &str = "fake-ocr";

#[derive(Clone, PartialEq, Eq)]
pub struct OcrConfig {
    pub provider: OcrProviderKind,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub model: String,
    /// Budget of one page request.
    pub timeout_secs: u64,
    /// Pages read per file; the rest is skipped with a note.
    pub max_pages: usize,
    /// Pages being read at once across the whole server.
    pub concurrency: usize,
    /// Why OCR is off although `OCR_PROVIDER` asked for it.
    pub warning: Option<String>,
}

// Hand-written so the API key never reaches logs.
impl std::fmt::Debug for OcrConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OcrConfig")
            .field("provider", &self.provider)
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("timeout_secs", &self.timeout_secs)
            .field("max_pages", &self.max_pages)
            .field("concurrency", &self.concurrency)
            .field("warning", &self.warning)
            .finish_non_exhaustive()
    }
}

impl OcrConfig {
    pub fn disabled() -> Self {
        Self {
            provider: OcrProviderKind::None,
            base_url: None,
            api_key: None,
            model: String::new(),
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            max_pages: DEFAULT_MAX_PAGES,
            concurrency: DEFAULT_CONCURRENCY,
            warning: None,
        }
    }

    /// The deterministic test provider.
    pub fn fake() -> Self {
        Self {
            provider: OcrProviderKind::Fake,
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

    /// Reads `OCR_*` (empty values count as unset). `ai_is_fake`: `AI_PROVIDER=fake`, which
    /// makes `fake` the default. Never fails: an invalid setting yields a disabled config with a
    /// warning.
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>, ai_is_fake: bool) -> Self {
        let get = |k: &str| {
            get(k)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        let provider = match get("OCR_PROVIDER") {
            None if ai_is_fake => OcrProviderKind::Fake,
            None => OcrProviderKind::None,
            Some(raw) => match OcrProviderKind::parse(&raw) {
                Some(p) => p,
                None => {
                    return Self::off(format!(
                        "OCR_PROVIDER={raw:?} is not openai_compatible, fake or none; OCR is off"
                    ))
                }
            },
        };
        if provider == OcrProviderKind::None {
            return Self::disabled();
        }
        let number = |var: &str, default: u64| match get(var) {
            None => Ok(default),
            Some(v) => v
                .parse::<u64>()
                .ok()
                .filter(|n| *n > 0)
                .ok_or_else(|| format!("{var}={v:?} is not a positive number; OCR is off")),
        };
        let numbers = (|| {
            Ok::<_, String>((
                number("OCR_TIMEOUT_SECS", DEFAULT_TIMEOUT_SECS)?,
                number("OCR_MAX_PAGES", DEFAULT_MAX_PAGES as u64)?,
                number("OCR_CONCURRENCY", DEFAULT_CONCURRENCY as u64)?,
            ))
        })();
        let (timeout_secs, max_pages, concurrency) = match numbers {
            Ok(n) => n,
            Err(warning) => return Self::off(warning),
        };
        let base = Self {
            provider,
            timeout_secs,
            max_pages: max_pages as usize,
            concurrency: concurrency as usize,
            ..Self::disabled()
        };
        match provider {
            OcrProviderKind::None => Self::disabled(),
            OcrProviderKind::Fake => Self {
                model: get("OCR_MODEL").unwrap_or_else(|| FAKE_MODEL.into()),
                ..base
            },
            OcrProviderKind::OpenAiCompatible => {
                let Some(base_url) = get("OCR_BASE_URL").or_else(|| get("EMBEDDING_BASE_URL"))
                else {
                    return Self::off(
                        "OCR_PROVIDER=openai_compatible needs OCR_BASE_URL (or EMBEDDING_BASE_URL); OCR is off"
                            .into(),
                    );
                };
                Self {
                    base_url: Some(base_url),
                    api_key: get("OCR_API_KEY").or_else(|| get("EMBEDDING_API_KEY")),
                    model: get("OCR_MODEL").unwrap_or_else(|| DEFAULT_MODEL.into()),
                    ..base
                }
            }
        }
    }
}

#[derive(Debug, Clone, thiserror::Error, PartialEq)]
pub enum OcrError {
    #[error("OCR is disabled")]
    Disabled,
    #[error("OCR request timed out")]
    Timeout,
    /// The response body is deliberately not kept: a gateway may echo credentials in it.
    #[error("OCR request failed: HTTP {status}")]
    Http { status: u16 },
    #[error("OCR request failed: {0}")]
    Network(String),
    #[error("unexpected OCR response: {0}")]
    Invalid(String),
}

impl OcrError {
    /// A timeout, a 5xx or a dropped connection is tried once more.
    pub fn retryable(&self) -> bool {
        match self {
            Self::Timeout | Self::Network(_) => true,
            Self::Http { status } => *status >= 500,
            Self::Disabled | Self::Invalid(_) => false,
        }
    }

    /// `ai_interactions.status` for this failure.
    pub fn interaction_status(&self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            _ => "error",
        }
    }

    /// What the page records (Chinese, shown to the teacher in the OCR note).
    pub fn page_message(&self) -> String {
        match self {
            Self::Disabled => "這個環境沒有開啟文字辨識".into(),
            Self::Timeout => "辨識逾時".into(),
            Self::Http { status } => format!("辨識服務暫時無法使用（HTTP {status}）"),
            Self::Network(_) => "連不上辨識服務".into(),
            Self::Invalid(_) => "辨識結果讀不出來".into(),
        }
    }
}

/// One page as it is sent: PNG or JPEG bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageImage {
    pub bytes: Vec<u8>,
    pub mime: &'static str,
}

/// What the fake provider looks at (the real one only sees the image).
#[derive(Debug, Clone, Default)]
pub struct FakeHints {
    pub filename: String,
    pub page: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OcrReply {
    /// Layout-labelled HTML (`<div data-bbox=… data-label=…>`).
    pub html: String,
    pub model_version: Option<String>,
}

/// The OCR provider plus the server-wide limits: [`Ocr::pages`] bounds the pages being read at
/// once (`OCR_CONCURRENCY`), [`Ocr::files`] the files being worked on at once (each holds its
/// file or rendered pages), and [`Ocr::live`] knows which materials have a job in this process.
pub struct Ocr {
    config: OcrConfig,
    http: reqwest::Client,
    pub pages: Arc<Semaphore>,
    pub files: Arc<Semaphore>,
    live: Arc<Mutex<HashMap<i64, usize>>>,
}

/// Registers a job of this process for a material while it is held.
pub struct LiveJob {
    live: Arc<Mutex<HashMap<i64, usize>>>,
    material_id: i64,
}

impl Drop for LiveJob {
    fn drop(&mut self) {
        if let Ok(mut live) = self.live.lock() {
            if let Some(n) = live.get_mut(&self.material_id) {
                *n -= 1;
                if *n == 0 {
                    live.remove(&self.material_id);
                }
            }
        }
    }
}

impl Ocr {
    pub fn new(config: OcrConfig) -> Self {
        let slots = config.concurrency.max(1);
        Self {
            config,
            http: reqwest::Client::new(),
            pages: Arc::new(Semaphore::new(slots)),
            files: Arc::new(Semaphore::new(slots)),
            live: Arc::default(),
        }
    }

    pub fn enabled(&self) -> bool {
        self.config.provider != OcrProviderKind::None
    }

    pub fn config(&self) -> &OcrConfig {
        &self.config
    }

    pub fn provider_name(&self) -> &'static str {
        self.config.provider.as_str()
    }

    pub fn model(&self) -> &str {
        &self.config.model
    }

    pub fn track(&self, material_id: i64) -> LiveJob {
        if let Ok(mut live) = self.live.lock() {
            *live.entry(material_id).or_insert(0) += 1;
        }
        LiveJob {
            live: self.live.clone(),
            material_id,
        }
    }

    /// Materials with a job in this process.
    pub fn live_ids(&self) -> Vec<i64> {
        self.live
            .lock()
            .map(|l| l.keys().copied().collect())
            .unwrap_or_default()
    }

    /// Reads one page. One request, no retry (the job retries and logs each attempt).
    pub async fn read_page(
        &self,
        image: &PageImage,
        hints: &FakeHints,
    ) -> Result<OcrReply, OcrError> {
        match self.config.provider {
            OcrProviderKind::None => Err(OcrError::Disabled),
            OcrProviderKind::Fake => fake_reply(hints).await,
            OcrProviderKind::OpenAiCompatible => {
                let timeout = Duration::from_secs(self.config.timeout_secs);
                match tokio::time::timeout(timeout, self.call(image, timeout)).await {
                    Ok(result) => result,
                    Err(_) => Err(OcrError::Timeout),
                }
            }
        }
    }

    async fn call(&self, image: &PageImage, timeout: Duration) -> Result<OcrReply, OcrError> {
        let base = self.config.base_url.as_deref().unwrap_or_default();
        let mut request = self
            .http
            .post(format!("{}/chat/completions", base.trim_end_matches('/')))
            .timeout(timeout)
            .json(&request_body(&self.config.model, image));
        if let Some(key) = &self.config.api_key {
            request = request.bearer_auth(key);
        }
        let from_reqwest = |e: reqwest::Error| {
            if e.is_timeout() {
                OcrError::Timeout
            } else {
                OcrError::Network(e.without_url().to_string())
            }
        };
        let response = request.send().await.map_err(from_reqwest)?;
        let status = response.status().as_u16();
        let body = response.text().await.map_err(from_reqwest)?;
        parse_response(status, &body)
    }
}

/// The chat-completions body: the page as a data URL, then the instruction.
pub fn request_body(model: &str, image: &PageImage) -> Value {
    let data = base64::engine::general_purpose::STANDARD.encode(&image.bytes);
    json!({
        "model": model,
        "max_tokens": MAX_TOKENS,
        "messages": [{
            "role": "user",
            "content": [
                { "type": "image_url", "image_url": { "url": format!("data:{};base64,{data}", image.mime) } },
                { "type": "text", "text": PROMPT },
            ],
        }],
    })
}

/// `choices[0].message.content` of a chat-completions response. An output cut at
/// `max_tokens` is still used: the HTML read so far is worth keeping.
pub fn parse_response(status: u16, body: &str) -> Result<OcrReply, OcrError> {
    if status == 408 || status == 504 {
        return Err(OcrError::Timeout);
    }
    if !(200..300).contains(&status) {
        return Err(OcrError::Http { status });
    }
    let v: Value = serde_json::from_str(body)
        .map_err(|e| OcrError::Invalid(format!("response is not JSON: {e}")))?;
    let html = v["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| OcrError::Invalid("no message content".into()))?;
    Ok(OcrReply {
        html: html.to_string(),
        model_version: v["model"].as_str().map(str::to_string),
    })
}

// ---------------------------------------------------------------- fake

/// Upper-case words of the file name (`scan LESSON7.pdf` → `LESSON7`): the fake writes them into
/// the page text so a test can find its file's text. `FAKE_OCR_*` switches are left out.
pub fn fake_markers(filename: &str) -> Vec<String> {
    let stem = filename.rsplit_once('.').map_or(filename, |(stem, _)| stem);
    stem.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|w| {
            w.len() >= 3
                && w.chars().any(|c| c.is_ascii_uppercase())
                && !w.chars().any(|c| c.is_ascii_lowercase())
                && !w.starts_with("FAKE_OCR")
        })
        .map(str::to_string)
        .collect()
}

/// Seconds the fake takes per page of a `FAKE_OCR_SLOW` file.
pub const FAKE_SLOW_SECS: u64 = 2;

/// Deterministic HTML for one page: a Section-Header 「Page n」, two Text blocks (the first with
/// the file's markers), a Picture and a Page-Footer. Switches in the file name:
/// `FAKE_OCR_FAIL` (page 2 fails, both attempts), `FAKE_OCR_DOWN` (every page fails),
/// `FAKE_OCR_SLOW` (each page takes [`FAKE_SLOW_SECS`] seconds).
pub async fn fake_reply(hints: &FakeHints) -> Result<OcrReply, OcrError> {
    let name = &hints.filename;
    if name.contains("FAKE_OCR_SLOW") {
        tokio::time::sleep(Duration::from_secs(FAKE_SLOW_SECS)).await;
    }
    if name.contains("FAKE_OCR_DOWN") || (name.contains("FAKE_OCR_FAIL") && hints.page == 2) {
        return Err(OcrError::Http { status: 503 });
    }
    let n = hints.page;
    let markers = fake_markers(name);
    let marker = if markers.is_empty() {
        String::new()
    } else {
        format!(" {}", markers.join(" "))
    };
    Ok(OcrReply {
        html: format!(
            "<div data-bbox=\"60 40 940 90\" data-label=\"Section-Header\"><h2>Page {n}</h2></div>\n\
             <div data-bbox=\"60 110 940 220\" data-label=\"Text\"><p>Scanned text on page {n}{marker}.</p></div>\n\
             <div data-bbox=\"60 240 940 330\" data-label=\"Text\"><p>第 {n} 頁的第二段：Tom &amp; Jerry &lt;ran&gt;.</p></div>\n\
             <div data-bbox=\"300 350 700 600\" data-label=\"Picture\"><img alt=\"a drawing\"/></div>\n\
             <div data-bbox=\"450 960 550 990\" data-label=\"Page-Footer\">{n}</div>"
        ),
        model_version: Some(FAKE_MODEL.to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(pairs: &[(&str, &str)], ai_fake: bool) -> OcrConfig {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        OcrConfig::from_lookup(|k| map.get(k).cloned(), ai_fake)
    }

    #[test]
    fn ocr_is_off_by_default_and_fake_with_the_fake_ai() {
        assert_eq!(cfg(&[], false).provider, OcrProviderKind::None);
        assert_eq!(cfg(&[], true).provider, OcrProviderKind::Fake);
        assert_eq!(
            cfg(&[("OCR_PROVIDER", "none")], true).provider,
            OcrProviderKind::None
        );
        let c = cfg(&[("OCR_PROVIDER", " ")], true);
        assert_eq!(c.provider, OcrProviderKind::Fake, "empty counts as unset");
        assert_eq!(c.timeout_secs, 120);
        assert_eq!(c.max_pages, 40);
        assert_eq!(c.concurrency, 2);
    }

    #[test]
    fn the_gateway_settings_default_to_the_embedding_ones() {
        let c = cfg(
            &[
                ("OCR_PROVIDER", "openai_compatible"),
                ("EMBEDDING_BASE_URL", "http://gw/v1"),
                ("EMBEDDING_API_KEY", "secret-key"),
            ],
            false,
        );
        assert_eq!(c.provider, OcrProviderKind::OpenAiCompatible);
        assert_eq!(c.base_url.as_deref(), Some("http://gw/v1"));
        assert_eq!(c.api_key.as_deref(), Some("secret-key"));
        assert_eq!(c.model, "chandra-ocr-2");
        assert!(!format!("{c:?}").contains("secret-key"), "key not logged");
        let own = cfg(
            &[
                ("OCR_PROVIDER", "openai_compatible"),
                ("EMBEDDING_BASE_URL", "http://gw/v1"),
                ("OCR_BASE_URL", "http://ocr/v1"),
                ("OCR_API_KEY", "k2"),
                ("OCR_MODEL", "other"),
                ("OCR_TIMEOUT_SECS", "30"),
                ("OCR_MAX_PAGES", "5"),
                ("OCR_CONCURRENCY", "4"),
            ],
            false,
        );
        assert_eq!(own.base_url.as_deref(), Some("http://ocr/v1"));
        assert_eq!(own.api_key.as_deref(), Some("k2"));
        assert_eq!(own.model, "other");
        assert_eq!(
            (own.timeout_secs, own.max_pages, own.concurrency),
            (30, 5, 4)
        );
    }

    #[test]
    fn bad_settings_turn_ocr_off_with_a_warning() {
        for pairs in [
            vec![("OCR_PROVIDER", "tesseract")],
            vec![("OCR_PROVIDER", "openai_compatible")],
            vec![("OCR_PROVIDER", "fake"), ("OCR_MAX_PAGES", "0")],
            vec![("OCR_PROVIDER", "fake"), ("OCR_CONCURRENCY", "two")],
        ] {
            let c = cfg(&pairs, false);
            assert_eq!(c.provider, OcrProviderKind::None, "{pairs:?}");
            assert!(c.warning.is_some(), "{pairs:?}");
        }
    }

    #[test]
    fn the_request_carries_the_page_as_a_data_url() {
        let image = PageImage {
            bytes: b"\x89PNG".to_vec(),
            mime: "image/png",
        };
        let body = request_body("chandra-ocr-2", &image);
        assert_eq!(body["model"], "chandra-ocr-2");
        assert_eq!(body["max_tokens"], 4000);
        let content = &body["messages"][0]["content"];
        assert_eq!(
            content[0]["image_url"]["url"],
            "data:image/png;base64,iVBORw=="
        );
        assert_eq!(content[1]["text"], "OCR this page to markdown.");
    }

    #[test]
    fn responses_and_errors() {
        let ok = json!({ "model": "chandra-ocr-2-0901",
            "choices": [{ "finish_reason": "stop", "message": { "content": "<div>x</div>" } }] })
        .to_string();
        let r = parse_response(200, &ok).unwrap();
        assert_eq!(r.html, "<div>x</div>");
        assert_eq!(r.model_version.as_deref(), Some("chandra-ocr-2-0901"));
        assert_eq!(parse_response(504, ""), Err(OcrError::Timeout));
        assert_eq!(
            parse_response(503, "secret echo"),
            Err(OcrError::Http { status: 503 })
        );
        assert!(matches!(
            parse_response(200, "{}"),
            Err(OcrError::Invalid(_))
        ));
        assert!(OcrError::Timeout.retryable());
        assert!(OcrError::Http { status: 502 }.retryable());
        assert!(!OcrError::Http { status: 400 }.retryable());
        assert!(!OcrError::Invalid("x".into()).retryable());
    }

    #[test]
    fn fake_markers_come_from_the_file_name() {
        assert_eq!(fake_markers("scan LESSON7 p2.pdf"), ["LESSON7"]);
        assert_eq!(fake_markers("WS_ONE FAKE_OCR_FAIL.png"), ["WS_ONE"]);
        assert!(fake_markers("photo.png").is_empty());
    }

    #[tokio::test]
    async fn the_fake_fails_page_two_on_request() {
        let hints = |page| FakeHints {
            filename: "x FAKE_OCR_FAIL.pdf".into(),
            page,
        };
        assert!(fake_reply(&hints(1)).await.is_ok());
        assert_eq!(
            fake_reply(&hints(2)).await,
            Err(OcrError::Http { status: 503 })
        );
        let html = fake_reply(&FakeHints {
            filename: "MARKER1.png".into(),
            page: 3,
        })
        .await
        .unwrap()
        .html;
        assert!(
            html.contains("Page 3") && html.contains("MARKER1"),
            "{html}"
        );
    }
}
