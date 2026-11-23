//! Which habits a generation step receives when a teacher has many.
//!
//! * Closed-kind habits (`choice_options`, …) always go to every step in `HABIT_STEPS`, as before.
//! * Free-form habits are filtered by their scope for the step, and when more than
//!   [`MAX_INJECTED_CUSTOM`] remain they are ranked by how close their statement is to the step's
//!   request (cosine similarity of pgvector embeddings, stored in Postgres) and the best are kept.
//!   Without embeddings (off, down, not yet computed) the most recently confirmed are kept.
//! * The habits that go in always keep confirmation order, so the prompt layout is stable.
//! * Every decision is returned as a record for `input_snapshot.teacher_memory.selection`.
//!
//! FalkorDB's full-text index is not used here: it splits on spaces only, which is useless for
//! Chinese (docs/TEACHER_MEMORY.md).
use std::collections::HashMap;
use std::time::Duration;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::{scope, Memory, MAX_INJECTED_CUSTOM};
use crate::embedding::{vector_literal, Embedder, QUERY_CHARS};
use crate::AppState;

/// The habits chosen for one step and the record of how.
pub struct Selection {
    pub included: Vec<Memory>,
    pub record: Value,
}

/// How a ranking was made. `recency` is the documented fallback.
struct Ranking {
    method: &'static str,
    fallback_reason: Option<&'static str>,
    /// Memory id → similarity (only for `embedding`).
    scores: HashMap<i64, f32>,
    /// Best first.
    order: Vec<i64>,
}

fn rounded(score: f32) -> f64 {
    (f64::from(score) * 10_000.0).round() / 10_000.0
}

fn newest_first(a: &Memory, b: &Memory) -> std::cmp::Ordering {
    b.valid_at.cmp(&a.valid_at).then(b.id.cmp(&a.id))
}

/// The most recently confirmed first.
fn recency(candidates: &[&Memory], reason: Option<&'static str>) -> Ranking {
    let mut sorted: Vec<&Memory> = candidates.to_vec();
    sorted.sort_by(|a, b| newest_first(a, b));
    Ranking {
        method: "recency",
        fallback_reason: reason,
        scores: HashMap::new(),
        order: sorted.iter().map(|m| m.id).collect(),
    }
}

fn content_hash(model: &str, dims: usize, statement: &str) -> String {
    hex::encode(Sha256::digest(
        format!("{model}\n{dims}\n{statement}").as_bytes(),
    ))
}

/// Computes and stores the vector of each habit that has none for the configured model.
/// `false` when they could not all be made (embeddings off, provider down).
async fn ensure_vectors(state: &AppState, candidates: &[&Memory]) -> bool {
    let embedder: &Embedder = &state.embedder;
    if !embedder.enabled() || candidates.is_empty() {
        return embedder.enabled();
    }
    let ids: Vec<i64> = candidates.iter().map(|m| m.id).collect();
    let have: Vec<i64> = sqlx::query_scalar(
        "SELECT memory_id FROM teacher_memory_embeddings
         WHERE memory_id = ANY($1) AND model = $2 AND dims = $3",
    )
    .bind(&ids)
    .bind(embedder.model())
    .bind(embedder.dims() as i32)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();
    let missing: Vec<&Memory> = candidates
        .iter()
        .copied()
        .filter(|m| !have.contains(&m.id))
        .collect();
    if missing.is_empty() {
        return true;
    }
    let texts: Vec<String> = missing.iter().map(|m| m.statement.clone()).collect();
    match embedder
        .embed(
            &texts,
            Duration::from_millis(embedder.config().query_timeout_ms),
        )
        .await
    {
        Ok(vectors) => {
            for (m, v) in missing.iter().zip(vectors) {
                if store_vector(state, m.id, &m.statement, &v).await.is_err() {
                    return false;
                }
            }
            true
        }
        Err(e) => {
            tracing::warn!(error = %e, "could not embed teacher habits; using recency");
            false
        }
    }
}

async fn store_vector(
    state: &AppState,
    memory_id: i64,
    statement: &str,
    vector: &[f32],
) -> sqlx::Result<()> {
    let e = &state.embedder;
    sqlx::query(
        "INSERT INTO teacher_memory_embeddings (memory_id, model, dims, content_hash, embedding)
         VALUES ($1, $2, $3, $4, $5::vector)
         ON CONFLICT (memory_id) DO UPDATE SET model = EXCLUDED.model, dims = EXCLUDED.dims,
             content_hash = EXCLUDED.content_hash, embedding = EXCLUDED.embedding,
             created_at = now()",
    )
    .bind(memory_id)
    .bind(e.model())
    .bind(e.dims() as i32)
    .bind(content_hash(e.model(), e.dims(), statement))
    .bind(vector_literal(vector))
    .execute(&state.db)
    .await?;
    Ok(())
}

/// Compute the vector of a habit right after it was confirmed (best effort, off the request
/// path: a habit without a vector is simply embedded the first time it has to be ranked).
pub fn embed_in_background(state: &AppState, memory_id: i64, statement: String) {
    if !state.embedder.enabled() {
        return;
    }
    let state = state.clone();
    tokio::spawn(async move {
        let embedder = state.embedder.clone();
        let text = vec![statement.clone()];
        let timeout = Duration::from_secs(embedder.config().batch_timeout_secs);
        match embedder.embed(&text, timeout).await {
            Ok(mut v) => {
                if let Some(vector) = v.pop() {
                    if let Err(e) = store_vector(&state, memory_id, &statement, &vector).await {
                        tracing::warn!(error = %e, memory_id, "could not store a habit embedding");
                    }
                }
            }
            Err(e) => tracing::warn!(error = %e, memory_id, "could not embed a new habit"),
        }
    });
}

/// Rank free-form habits by similarity to `query`, best first.
async fn rank(state: &AppState, candidates: &[&Memory], query: &str) -> Ranking {
    let embedder = &state.embedder;
    if !embedder.enabled() {
        return recency(candidates, Some("embeddings_disabled"));
    }
    if query.trim().is_empty() {
        return recency(candidates, Some("empty_query"));
    }
    if !ensure_vectors(state, candidates).await {
        return recency(candidates, Some("vectors_unavailable"));
    }
    let vector = match embedder.embed_query(query).await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, "could not embed the request; using recency");
            return recency(candidates, Some("query_embedding_failed"));
        }
    };
    let ids: Vec<i64> = candidates.iter().map(|m| m.id).collect();
    let rows: Result<Vec<(i64, f32)>, _> = sqlx::query_as(
        "SELECT memory_id, (1 - (embedding <=> $1::vector))::float4
           FROM teacher_memory_embeddings
          WHERE memory_id = ANY($2) AND model = $3 AND dims = $4",
    )
    .bind(vector_literal(&vector))
    .bind(&ids)
    .bind(embedder.model())
    .bind(embedder.dims() as i32)
    .fetch_all(&state.db)
    .await;
    let Ok(rows) = rows else {
        return recency(candidates, Some("similarity_failed"));
    };
    let scores: HashMap<i64, f32> = rows.into_iter().collect();
    if scores.len() != candidates.len() {
        return recency(candidates, Some("vectors_unavailable"));
    }
    let mut sorted: Vec<&Memory> = candidates.to_vec();
    sorted.sort_by(|a, b| {
        scores[&b.id]
            .partial_cmp(&scores[&a.id])
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| newest_first(a, b))
    });
    Ranking {
        method: "embedding",
        fallback_reason: None,
        order: sorted.iter().map(|m| m.id).collect(),
        scores,
    }
}

/// The habits that go to `module`, with the record of the decision.
pub async fn select(state: &AppState, all: &[Memory], module: &str, query: &str) -> Selection {
    select_with_limit(state, all, module, query, MAX_INJECTED_CUSTOM).await
}

pub async fn select_with_limit(
    state: &AppState,
    all: &[Memory],
    module: &str,
    query: &str,
    limit: usize,
) -> Selection {
    let query: String = query.trim().chars().take(QUERY_CHARS).collect();
    let mut included: Vec<&Memory> = all.iter().filter(|m| !m.is_custom()).collect();
    let (custom, other_scope): (Vec<&Memory>, Vec<&Memory>) =
        all.iter().filter(|m| m.is_custom()).partition(|m| {
            m.scope
                .as_deref()
                .is_some_and(|s| scope::applies(s, module))
        });
    let mut filtered: Vec<Value> = other_scope
        .iter()
        .map(|m| json!({"id": m.id, "kind": m.kind, "scope": m.scope, "reason": "scope"}))
        .collect();

    let (method, fallback_reason, scores) = if custom.len() <= limit {
        included.extend(custom.iter().copied());
        ("not_needed", None, HashMap::new())
    } else {
        let ranking = rank(state, &custom, &query).await;
        let keep: Vec<i64> = ranking.order.iter().copied().take(limit).collect();
        for m in &custom {
            if keep.contains(&m.id) {
                included.push(m);
            } else {
                let mut v = json!({"id": m.id, "kind": m.kind, "scope": m.scope,
                    "reason": "below_top_k"});
                if let Some(s) = ranking.scores.get(&m.id) {
                    v["score"] = json!(rounded(*s));
                }
                filtered.push(v);
            }
        }
        (ranking.method, ranking.fallback_reason, ranking.scores)
    };
    included.sort_by(|a, b| a.valid_at.cmp(&b.valid_at).then(a.id.cmp(&b.id)));

    let embedding = (method == "embedding")
        .then(|| json!({"model": state.embedder.model(), "dims": state.embedder.dims()}));
    let record = json!({
        "policy": "closed_all+custom_scope_top_k",
        "module": module,
        "k": limit,
        "ranking": method,
        "fallback_reason": fallback_reason,
        "embedding": embedding,
        "query": (method != "not_needed").then(|| json!({
            "chars": query.chars().count(),
            "sha256": hex::encode(Sha256::digest(query.as_bytes())),
        })),
        "included": included.iter().map(|m| {
            let mut v = json!({"id": m.id, "kind": m.kind, "scope": m.scope});
            if let Some(s) = scores.get(&m.id) {
                v["score"] = json!(rounded(*s));
            }
            v
        }).collect::<Vec<_>>(),
        "filtered_out": filtered,
    });
    Selection {
        included: included.into_iter().cloned().collect(),
        record,
    }
}

/// The habits shown to the extraction model so that it can say what a new habit replaces: all
/// of them up to `limit`, otherwise every closed habit and the free-form ones closest to `text`.
pub async fn for_extraction(
    state: &AppState,
    all: &[Memory],
    text: &str,
    limit: usize,
) -> (Vec<Memory>, Value) {
    if all.len() <= limit {
        return (
            all.to_vec(),
            json!({"total": all.len(), "shown": all.len(), "ranking": "not_needed"}),
        );
    }
    let closed: Vec<&Memory> = all.iter().filter(|m| !m.is_custom()).collect();
    let custom: Vec<&Memory> = all.iter().filter(|m| m.is_custom()).collect();
    let room = limit.saturating_sub(closed.len());
    let ranking = rank(state, &custom, text).await;
    let keep: Vec<i64> = ranking.order.iter().copied().take(room).collect();
    let mut shown: Vec<Memory> = closed.into_iter().cloned().collect();
    shown.extend(custom.into_iter().filter(|m| keep.contains(&m.id)).cloned());
    shown.sort_by(|a, b| a.valid_at.cmp(&b.valid_at).then(a.id.cmp(&b.id)));
    let record = json!({"total": all.len(), "shown": shown.len(), "ranking": ranking.method,
        "fallback_reason": ranking.fallback_reason,
        "shown_ids": shown.iter().map(|m| m.id).collect::<Vec<_>>()});
    (shown, record)
}
