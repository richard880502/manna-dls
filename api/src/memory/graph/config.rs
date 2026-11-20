//! Deployment wiring; live model capability is evaluated by the runtime.
use std::time::Duration;

/// How long a `/health` answer (compatible or not) is reused before asking again.
pub const HEALTH_TTL: Duration = Duration::from_secs(45);
#[derive(Clone)]
pub struct MemoryConfig {
    pub url: Option<String>,
    pub token: Option<String>,
    pub proxy_url: Option<String>,
    pub health_ttl: Duration,
    /// Why the index stays off although it was configured (said once at startup).
    pub warning: Option<String>,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            url: None,
            token: None,
            proxy_url: None,
            health_ttl: HEALTH_TTL,
            warning: None,
        }
    }
}

// Hand-written: the internal service secret never reaches logs.
impl std::fmt::Debug for MemoryConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryConfig")
            .field("url", &self.url)
            .field("token_set", &self.token.is_some())
            .field("proxy_url", &self.proxy_url)
            .field("warning", &self.warning)
            .finish()
    }
}

impl MemoryConfig {
    /// Validate deployment wiring only. The PI can switch providers after startup,
    /// so provider capability must never erase configured URLs here.
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Self {
        let nonempty = |k| get(k).filter(|v| !v.trim().is_empty());
        let mut c = Self {
            url: nonempty("GRAPHITI_URL"),
            token: nonempty("GRAPHITI_TOKEN"),
            proxy_url: nonempty("MEMORY_PROXY_URL"),
            ..Self::default()
        };
        if c.url.is_some() != c.token.is_some() {
            c.warning = Some("GRAPHITI_URL and GRAPHITI_TOKEN must both be set".into());
            c.url = None;
        } else if c.url.is_some() && !c.enabled() {
            c.warning = Some("MEMORY_PROXY_URL is not set".into());
            c.url = None;
        }
        c
    }
    pub fn enabled(&self) -> bool {
        self.url.is_some() && self.token.is_some() && self.proxy_url.is_some()
    }
    pub(super) fn base(&self) -> &str {
        self.url
            .as_deref()
            .unwrap_or_default()
            .trim_end_matches('/')
    }
    pub(super) fn secret(&self) -> &str {
        self.token.as_deref().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_complete_deployment_wiring_enables_the_index() {
        let all = |k: &str| match k {
            "GRAPHITI_URL" => Some("http://manna-memory:8000".to_string()),
            "GRAPHITI_TOKEN" => Some("secret".to_string()),
            "MEMORY_PROXY_URL" => Some("http://manna-api:8081/api/internal/memory/v1".to_string()),
            _ => None,
        };
        let on = MemoryConfig::from_lookup(all);
        assert!(on.enabled() && on.warning.is_none());
        assert!(!format!("{on:?}").contains("secret"));
        let partial =
            MemoryConfig::from_lookup(|k| (k == "GRAPHITI_URL").then(|| "http://x".to_string()));
        assert!(!partial.enabled() && partial.warning.is_some());
        let unset = MemoryConfig::from_lookup(|k| {
            (k == "MEMORY_PROXY_URL").then(|| "http://x".to_string())
        });
        assert!(!unset.enabled() && unset.warning.is_none());
    }
}
