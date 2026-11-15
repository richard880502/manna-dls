//! Runtime configuration from environment variables.

use std::path::PathBuf;

use crate::embedding::EmbeddingConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiProviderKind {
    /// AI endpoints answer with status `disabled`.
    None,
    /// Deterministic offline provider for tests and CI.
    Fake,
    /// Anthropic Messages API. Overseas: demo environment only.
    Anthropic,
    /// OpenAI-compatible chat completions (the NCHC model).
    OpenAiCompatible,
}

impl AiProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Fake => "fake",
            Self::Anthropic => "anthropic",
            Self::OpenAiCompatible => "openai_compatible",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "none" => Some(Self::None),
            "fake" => Some(Self::Fake),
            "anthropic" => Some(Self::Anthropic),
            "openai_compatible" | "openai-compatible" => Some(Self::OpenAiCompatible),
            _ => None,
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("environment variable {0} is required")]
    Missing(&'static str),
    #[error("environment variable {var} has an invalid value {value:?}: {reason}")]
    Invalid {
        var: &'static str,
        value: String,
        reason: &'static str,
    },
    #[error(
        "AI_PROVIDER=anthropic is refused because DEMO_MODE is not true. Anthropic is an overseas \
         API and the spec (section 2) forbids sending real teacher data to it; it may only be used in \
         the demo environment. Set DEMO_MODE=true for the demo deployment, or use \
         AI_PROVIDER=openai_compatible (NCHC) / none."
    )]
    AnthropicRequiresDemoMode,
}

pub const VALID_EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub port: u16,
    pub static_dir: PathBuf,
    pub ai_provider: AiProviderKind,
    pub ai_model: String,
    pub ai_effort: String,
    pub ai_timeout_secs: u64,
    pub anthropic_api_key: Option<String>,
    pub openai_compat_base_url: Option<String>,
    pub openai_compat_api_key: Option<String>,
    pub demo_mode: bool,
    pub admin_bootstrap_code: Option<String>,
    pub rag_top_k: usize,
    /// Embeddings for hybrid retrieval. Never fails: a bad setting disables embeddings.
    pub embedding: EmbeddingConfig,
}

// Hand-written so API keys and the bootstrap code never reach logs.
impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("port", &self.port)
            .field("static_dir", &self.static_dir)
            .field("ai_provider", &self.ai_provider)
            .field("ai_model", &self.ai_model)
            .field("ai_effort", &self.ai_effort)
            .field("ai_timeout_secs", &self.ai_timeout_secs)
            .field("openai_compat_base_url", &self.openai_compat_base_url)
            .field("demo_mode", &self.demo_mode)
            .field("rag_top_k", &self.rag_top_k)
            .field("embedding", &self.embedding)
            .finish_non_exhaustive()
    }
}

fn parse_bool(var: &'static str, v: Option<String>) -> Result<bool, ConfigError> {
    match v.as_deref().map(|s| s.trim().to_ascii_lowercase()) {
        None => Ok(false),
        Some(s) => match s.as_str() {
            "" | "0" | "false" | "no" | "off" => Ok(false),
            "1" | "true" | "yes" | "on" => Ok(true),
            _ => Err(ConfigError::Invalid {
                var,
                value: s,
                reason: "expected true or false",
            }),
        },
    }
}

fn parse_num<T: std::str::FromStr>(
    var: &'static str,
    v: Option<String>,
    default: T,
) -> Result<T, ConfigError> {
    match v {
        None => Ok(default),
        Some(s) => s.trim().parse().map_err(|_| ConfigError::Invalid {
            var,
            value: s,
            reason: "expected a positive number",
        }),
    }
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|k| std::env::var(k).ok())
    }

    /// Builds the config from any key lookup (unit tests pass a map). Empty values count as unset.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let get = |k: &str| lookup(k).filter(|v| !v.trim().is_empty());

        let database_url = get("DATABASE_URL").ok_or(ConfigError::Missing("DATABASE_URL"))?;
        let port = parse_num("PORT", get("PORT"), 8080u16)?;
        let static_dir = PathBuf::from(get("STATIC_DIR").unwrap_or_else(|| "./web-dist".into()));

        let provider_raw = get("AI_PROVIDER").unwrap_or_default();
        let ai_provider =
            AiProviderKind::parse(&provider_raw).ok_or_else(|| ConfigError::Invalid {
                var: "AI_PROVIDER",
                value: provider_raw.clone(),
                reason: "expected anthropic, openai_compatible, fake or none",
            })?;
        let demo_mode = parse_bool("DEMO_MODE", get("DEMO_MODE"))?;

        let ai_model = match (get("AI_MODEL"), ai_provider) {
            (Some(m), _) => m.trim().to_string(),
            (None, AiProviderKind::Anthropic) => "claude-opus-5-5".into(),
            (None, AiProviderKind::Fake) => "fake-model".into(),
            (None, AiProviderKind::None) => String::new(),
            (None, AiProviderKind::OpenAiCompatible) => {
                return Err(ConfigError::Missing("AI_MODEL"));
            }
        };
        let ai_effort = get("AI_EFFORT")
            .map(|e| e.trim().to_ascii_lowercase())
            .unwrap_or_else(|| "medium".into());
        if !VALID_EFFORTS.contains(&ai_effort.as_str()) {
            return Err(ConfigError::Invalid {
                var: "AI_EFFORT",
                value: ai_effort,
                reason: "expected low, medium, high, xhigh or max",
            });
        }
        let ai_timeout_secs = parse_num("AI_TIMEOUT_SECS", get("AI_TIMEOUT_SECS"), 60u64)?;
        if ai_timeout_secs == 0 {
            return Err(ConfigError::Invalid {
                var: "AI_TIMEOUT_SECS",
                value: "0".into(),
                reason: "must be greater than zero",
            });
        }
        let rag_top_k = parse_num("RAG_TOP_K", get("RAG_TOP_K"), 8usize)?;
        if rag_top_k == 0 {
            return Err(ConfigError::Invalid {
                var: "RAG_TOP_K",
                value: "0".into(),
                reason: "must be greater than zero",
            });
        }

        let anthropic_api_key = get("ANTHROPIC_API_KEY");
        let openai_compat_base_url = get("OPENAI_COMPAT_BASE_URL");
        let openai_compat_api_key = get("OPENAI_COMPAT_API_KEY");

        match ai_provider {
            AiProviderKind::Anthropic => {
                // Hard rule (spec §2): real teacher data must never reach an overseas API.
                if !demo_mode {
                    return Err(ConfigError::AnthropicRequiresDemoMode);
                }
                if anthropic_api_key.is_none() {
                    return Err(ConfigError::Missing("ANTHROPIC_API_KEY"));
                }
            }
            AiProviderKind::OpenAiCompatible => {
                if openai_compat_base_url.is_none() {
                    return Err(ConfigError::Missing("OPENAI_COMPAT_BASE_URL"));
                }
            }
            AiProviderKind::Fake | AiProviderKind::None => {}
        }

        Ok(Self {
            database_url,
            port,
            static_dir,
            ai_provider,
            ai_model,
            ai_effort,
            ai_timeout_secs,
            anthropic_api_key,
            openai_compat_base_url,
            openai_compat_api_key,
            demo_mode,
            admin_bootstrap_code: get("ADMIN_BOOTSTRAP_CODE").map(|c| c.trim().to_string()),
            rag_top_k,
            embedding: EmbeddingConfig::from_lookup(get),
        })
    }

    /// Config for tests: fake AI provider, no static dir, given database URL.
    pub fn for_tests(database_url: impl Into<String>) -> Self {
        Self {
            database_url: database_url.into(),
            port: 0,
            static_dir: PathBuf::from("./does-not-exist-web-dist"),
            ai_provider: AiProviderKind::Fake,
            ai_model: "fake-model".into(),
            ai_effort: "medium".into(),
            ai_timeout_secs: 5,
            anthropic_api_key: None,
            openai_compat_base_url: None,
            openai_compat_api_key: None,
            demo_mode: false,
            admin_bootstrap_code: None,
            rag_top_k: 8,
            embedding: EmbeddingConfig::disabled(),
        }
    }

    pub fn ai_enabled(&self) -> bool {
        self.ai_provider != AiProviderKind::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn cfg(pairs: &[(&str, &str)]) -> Result<Config, ConfigError> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Config::from_lookup(|k| map.get(k).cloned())
    }

    const DB: (&str, &str) = ("DATABASE_URL", "postgres://u:p@localhost/db");

    #[test]
    fn database_url_is_required() {
        assert_eq!(cfg(&[]).err(), Some(ConfigError::Missing("DATABASE_URL")));
    }

    #[test]
    fn defaults() {
        let c = cfg(&[DB]).unwrap();
        assert_eq!(c.port, 8080);
        assert_eq!(c.ai_provider, AiProviderKind::None);
        assert!(!c.ai_enabled());
        assert_eq!(c.ai_effort, "medium");
        assert_eq!(c.ai_timeout_secs, 60);
        assert_eq!(c.rag_top_k, 8);
        assert!(!c.demo_mode);
        assert_eq!(c.static_dir, PathBuf::from("./web-dist"));
    }

    #[test]
    fn anthropic_without_demo_mode_is_refused() {
        let r = cfg(&[DB, ("AI_PROVIDER", "anthropic"), ("ANTHROPIC_API_KEY", "k")]);
        assert_eq!(r.err(), Some(ConfigError::AnthropicRequiresDemoMode));
        let r = cfg(&[
            DB,
            ("AI_PROVIDER", "anthropic"),
            ("ANTHROPIC_API_KEY", "k"),
            ("DEMO_MODE", "false"),
        ]);
        assert_eq!(r.err(), Some(ConfigError::AnthropicRequiresDemoMode));
    }

    #[test]
    fn anthropic_in_demo_mode_is_allowed_and_defaults_model() {
        let c = cfg(&[
            DB,
            ("AI_PROVIDER", "anthropic"),
            ("ANTHROPIC_API_KEY", "k"),
            ("DEMO_MODE", "true"),
        ])
        .unwrap();
        assert_eq!(c.ai_model, "claude-opus-5-5");
        assert!(c.demo_mode);
        let dbg = format!("{c:?}");
        assert!(!dbg.contains("\"k\""), "api key must not be printed: {dbg}");
    }

    #[test]
    fn anthropic_requires_key() {
        let r = cfg(&[DB, ("AI_PROVIDER", "anthropic"), ("DEMO_MODE", "1")]);
        assert_eq!(r.err(), Some(ConfigError::Missing("ANTHROPIC_API_KEY")));
    }

    #[test]
    fn openai_compatible_requires_base_url_and_model() {
        let r = cfg(&[DB, ("AI_PROVIDER", "openai_compatible"), ("AI_MODEL", "m")]);
        assert_eq!(
            r.err(),
            Some(ConfigError::Missing("OPENAI_COMPAT_BASE_URL"))
        );
        let r = cfg(&[
            DB,
            ("AI_PROVIDER", "openai_compatible"),
            ("OPENAI_COMPAT_BASE_URL", "http://x/v1"),
        ]);
        assert_eq!(r.err(), Some(ConfigError::Missing("AI_MODEL")));
        let c = cfg(&[
            DB,
            ("AI_PROVIDER", "openai_compatible"),
            ("OPENAI_COMPAT_BASE_URL", "http://x/v1"),
            ("AI_MODEL", "nchc-model"),
        ])
        .unwrap();
        assert_eq!(c.ai_provider, AiProviderKind::OpenAiCompatible);
        assert!(!c.demo_mode, "openai_compatible does not need demo mode");
    }

    #[test]
    fn invalid_values_are_rejected() {
        assert!(cfg(&[DB, ("AI_PROVIDER", "gpt")]).is_err());
        assert!(cfg(&[DB, ("AI_EFFORT", "extreme")]).is_err());
        assert!(cfg(&[DB, ("PORT", "eighty")]).is_err());
        assert!(cfg(&[DB, ("DEMO_MODE", "maybe")]).is_err());
        assert!(cfg(&[DB, ("RAG_TOP_K", "0")]).is_err());
        assert!(cfg(&[DB, ("AI_TIMEOUT_SECS", "0")]).is_err());
    }

    #[test]
    fn empty_values_count_as_unset() {
        let c = cfg(&[
            DB,
            ("AI_PROVIDER", ""),
            ("PORT", " "),
            ("ADMIN_BOOTSTRAP_CODE", ""),
        ])
        .unwrap();
        assert_eq!(c.ai_provider, AiProviderKind::None);
        assert_eq!(c.port, 8080);
        assert!(c.admin_bootstrap_code.is_none());
    }
}
