//! Sidecar transport and compatibility cache; no database or queue policy.
use super::super::{POLICY_VERSION, SIDECAR_API_VERSION};
use super::MemoryConfig;
use crate::AppState;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

pub(super) fn client(timeout: u64) -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout))
        .build()
        .expect("TLS client")
}

/// The HTTP contract is versioned independently of the Python engine. A core
/// upgrade with the same adapter contract never requires a Manna redeployment.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SidecarInfo {
    pub ok: bool,
    pub api_version: u32,
    pub graphiti_version: String,
    pub policy_version: String,
}
impl SidecarInfo {
    pub(super) fn compatible(&self) -> bool {
        self.ok
            && self.api_version == SIDECAR_API_VERSION
            && self.policy_version == POLICY_VERSION
            && !self.graphiti_version.trim().is_empty()
            && self.graphiti_version.len() <= 64
    }
}

/// The only sidecar HTTP entry point. Deadlines, paths and service authentication
/// live here; queue publication and interpretation of rankings remain with callers.
pub(super) struct SidecarClient<'a> {
    cfg: &'a MemoryConfig,
}

impl<'a> SidecarClient<'a> {
    pub(super) fn new(cfg: &'a MemoryConfig) -> Self {
        Self { cfg }
    }

    fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        timeout: u64,
    ) -> reqwest::RequestBuilder {
        client(timeout)
            .request(method, format!("{}{}", self.cfg.base(), path))
            .bearer_auth(self.cfg.secret())
    }

    async fn health(&self) -> Option<SidecarInfo> {
        let response = self
            .request(reqwest::Method::GET, "/health", 2)
            .send()
            .await
            .ok()?;
        success(response)
            .ok()?
            .json::<SidecarInfo>()
            .await
            .ok()
            .filter(SidecarInfo::compatible)
    }

    pub(super) async fn index(
        &self,
        payload: &serde_json::Value,
    ) -> reqwest::Result<reqwest::Response> {
        self.request(reqwest::Method::POST, "/index", 480)
            .json(payload)
            .send()
            .await
    }

    pub(super) async fn search(
        &self,
        payload: &serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        let response = self
            .request(reqwest::Method::POST, "/search", 2)
            .json(payload)
            .send()
            .await?;
        Ok(success(response)?.json().await?)
    }

    pub(super) async fn groups(&self) -> anyhow::Result<serde_json::Value> {
        let response = self
            .request(reqwest::Method::GET, "/groups", 5)
            .send()
            .await?;
        Ok(success(response)?.json().await?)
    }

    pub(super) async fn delete_group(&self, group: &str) -> anyhow::Result<()> {
        let response = self
            .request(reqwest::Method::DELETE, &format!("/groups/{group}"), 5)
            .send()
            .await?;
        success(response)?;
        Ok(())
    }
}

// `error_for_status` alone accepts 3xx. Only 2xx acknowledges sidecar work,
// especially a deletion: otherwise the durable garbage row must remain queued.
fn success(response: reqwest::Response) -> anyhow::Result<reqwest::Response> {
    anyhow::ensure!(
        response.status().is_success(),
        "sidecar HTTP {}",
        response.status()
    );
    Ok(response)
}

/// The sidecar's health, reused for [`MemoryConfig::health_ttl`]: searches and the worker do
/// not ask before every call. `None` when offline or incompatible (Postgres only).
pub async fn compatible_sidecar(state: &AppState) -> Option<SidecarInfo> {
    let cfg = &state.config.memory;
    if !cfg.enabled() {
        return None;
    }
    let mut cache = state.memory.health.lock().await;
    if let Some((at, info)) = cache.as_ref() {
        if at.elapsed() < cfg.health_ttl {
            return info.clone();
        }
    }
    let info = SidecarClient::new(cfg).health().await;
    *cache = Some((Instant::now(), info.clone()));
    info
}

/// Ask again on the next call (after a version mismatch or a failed index call).
pub async fn forget_health(state: &AppState) {
    *state.memory.health.lock().await = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_two_hundred_responses_acknowledge_sidecar_work() {
        for status in [200, 204, 304, 404, 503] {
            let response = axum::http::Response::builder()
                .status(status)
                .body(Vec::<u8>::new())
                .unwrap();
            assert_eq!(
                success(response.into()).is_ok(),
                (200..300).contains(&status)
            );
        }
    }
}
