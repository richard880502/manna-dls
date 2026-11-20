//! Durable queue status and current runtime capability; HTTP authorization lives in routes.
use super::{compatible_sidecar, SidecarInfo};
use crate::error::AppResult;
use crate::AppState;
use serde::Serialize;

#[derive(Serialize)]
pub struct IndexStatus {
    pub pending: i64,
    pub failed: i64,
    pub graph_enabled: bool,
    pub providers_ready: bool,
    pub compatible: bool,
    pub sidecar: Option<SidecarInfo>,
}

/// PI view. Without a configured index there is nothing to synchronize: both counters are 0.
pub async fn index_status(state: &AppState) -> AppResult<IndexStatus> {
    if !state.config.memory.enabled() {
        return Ok(IndexStatus {
            pending: 0,
            failed: 0,
            graph_enabled: false,
            providers_ready: false,
            compatible: false,
            sidecar: None,
        });
    }
    let (pending, failed): (i64, i64) = sqlx::query_as(
        "SELECT count(*) FILTER(WHERE revision>indexed_revision),
        count(*) FILTER(WHERE last_error IS NOT NULL) FROM memory_indexes",
    )
    .fetch_one(&state.db)
    .await?;
    let sidecar = compatible_sidecar(state).await;
    Ok(IndexStatus {
        pending,
        failed,
        graph_enabled: true,
        providers_ready: super::providers_ready(state),
        compatible: sidecar.is_some(),
        sidecar,
    })
}
