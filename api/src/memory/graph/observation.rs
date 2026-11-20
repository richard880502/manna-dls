//! Read-only graph observation against the teacher’s canonical Postgres habits.
use super::super::Memory;
use super::client::SidecarClient;
use super::compatible_sidecar;
use crate::AppState;
use serde::Serialize;
use serde_json::{json, Value};

/// What the index said about one generation step. Metadata only: the prompt always carries
/// every active habit in confirmation order, whatever the index answers.
#[derive(Debug, Clone, Serialize, Default)]
pub struct GraphObservation {
    /// `not_configured`, `no_habits`, `unavailable`, `not_indexed`, `core_changed`,
    /// `missing_group`, `empty_group`, `error` or `ranked`.
    pub status: &'static str,
    /// Graphiti's ranking of the active habit ids (BM25), when it answered.
    pub ranked_ids: Vec<i64>,
    pub group_id: Option<String>,
    pub revision: Option<i64>,
    /// The core that built the searched group.
    pub graphiti_version: Option<String>,
    /// The teacher was queued for a rebuild because the group cannot be trusted.
    pub reindex_requested: bool,
}

fn observed(status: &'static str) -> GraphObservation {
    GraphObservation {
        status,
        ..GraphObservation::default()
    }
}

/// Queue a rebuild of the published group `group` (only if it is still the published one).
async fn request_reindex(state: &AppState, teacher_id: i64, group: &str) -> bool {
    sqlx::query(
        "UPDATE memory_indexes SET revision=revision+1,attempts=0,next_attempt_at=now(),
        updated_at=now() WHERE teacher_id=$1 AND group_id=$2 AND revision=indexed_revision",
    )
    .bind(teacher_id)
    .bind(group)
    .execute(&state.db)
    .await
    .is_ok_and(|r| r.rows_affected() > 0)
}

/// Search the published group with the canonical habits only (never raw conversation or
/// material). Graph results can never introduce a fact or another teacher's habit: ids are
/// intersected with the teacher's active ids. A group built by another core, or one that is
/// missing or empty while Postgres has habits, is never searched and the teacher is rebuilt.
pub async fn observe(state: &AppState, teacher_id: i64, active: &[Memory]) -> GraphObservation {
    if !state.config.memory.enabled() {
        return observed("not_configured");
    }
    if active.is_empty() {
        return observed("no_habits");
    }
    if !super::providers_ready(state) {
        return observed("unavailable");
    }
    let Some(sidecar) = compatible_sidecar(state).await else {
        return observed("unavailable");
    };
    let published: Option<(String, i64, Option<String>)> = sqlx::query_as(
        "SELECT group_id, revision, indexed_graphiti_version FROM memory_indexes
        WHERE teacher_id=$1 AND revision=indexed_revision AND group_id IS NOT NULL",
    )
    .bind(teacher_id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();
    let Some((group, revision, built_by)) = published else {
        return observed("not_indexed");
    };
    let mut out = GraphObservation {
        status: "error",
        group_id: Some(group.clone()),
        revision: Some(revision),
        graphiti_version: built_by.clone(),
        ..GraphObservation::default()
    };
    if built_by.as_deref() != Some(sidecar.graphiti_version.as_str()) {
        out.status = "core_changed";
        out.reindex_requested = request_reindex(state, teacher_id, &group).await;
        return out;
    }
    let response = SidecarClient::new(&state.config.memory)
        .search(&json!({"group_id":group,
            "query":active.iter().map(|m|m.statement.as_str()).collect::<Vec<_>>().join(" "),
            "memory_ids":active.iter().map(|m|m.id).collect::<Vec<_>>()}))
        .await;
    let Ok(v) = response else {
        return out;
    };
    match v["group"].as_str() {
        Some("missing") | Some("empty") => {
            out.status = if v["group"] == "missing" {
                "missing_group"
            } else {
                "empty_group"
            };
            out.reindex_requested = request_reindex(state, teacher_id, &group).await;
            return out;
        }
        Some("ok") => {}
        _ => return out,
    }
    for id in v["memory_ids"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_i64)
    {
        if active.iter().any(|m| m.id == id) && !out.ranked_ids.contains(&id) {
            out.ranked_ids.push(id);
        }
    }
    out.status = "ranked";
    out
}
