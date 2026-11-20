//! Cached health and model pins shared between worker and internal requests.
use super::SidecarInfo;
use std::time::Instant;

/// Process-wide state of the index client (cached health, last orphan sweep).
#[derive(Default)]
pub struct Runtime {
    pub(super) health: tokio::sync::Mutex<Option<(Instant, Option<SidecarInfo>)>>,
    pub(super) last_sweep: std::sync::Mutex<Option<Instant>>,
    /// The model settings of each leased index job (by token hash), pinned when the job was
    /// claimed: the forwarder answers the sidecar's calls in separate requests, so a task-local
    /// cannot reach it. One API instance only; a missing entry (restart) falls back to the active
    /// settings.
    pins: std::sync::Mutex<std::collections::HashMap<String, crate::ai::ActiveAi>>,
}

impl Runtime {
    pub(super) fn pin_job(&self, token: &str, ai: crate::ai::ActiveAi) -> PinGuard<'_> {
        let key = crate::auth::sha256_hex(token);
        self.pins
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(key.clone(), ai);
        PinGuard { runtime: self, key }
    }

    pub(super) fn job_model(&self, token: &str) -> Option<crate::ai::ActiveAi> {
        self.pins
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&crate::auth::sha256_hex(token))
            .cloned()
    }
}

/// Removes a job's pinned settings when the job ends, however it ends.
pub(super) struct PinGuard<'a> {
    runtime: &'a Runtime,
    key: String,
}

impl Drop for PinGuard<'_> {
    fn drop(&mut self) {
        self.runtime
            .pins
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.key);
    }
}

/// True for the settings in force for this call (including an existing job pin).
/// Deployment wiring stays intact while a provider is unavailable so a later PI
/// switch can resume the durable queue without restarting the process.
pub fn providers_ready(state: &crate::AppState) -> bool {
    let chat = state.ai.provider_configured(state.ai.active().kind);
    chat && state.config.embedding.provider
        == crate::embedding::EmbeddingProviderKind::OpenAiCompatible
        && state.config.embedding.base_url.is_some()
}
