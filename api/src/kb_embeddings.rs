//! The semantic index of the knowledge base: which text of an item is embedded, and the
//! background job that keeps `knowledge_embeddings` in step with `knowledge_items`
//! (docs/API.md「Phase 2.4」).
//!
//! * **What is embedded** ([`embed_text`]): for a curriculum item, `科目｜學習表現 or 學習內容｜
//!   類別名稱：條文` — the official text with the names that say what kind of statement it is.
//!   The code itself is left out: it is an opaque label to an embedding model, and exact codes
//!   are already resolved by `retrieval::curriculum_by_codes`. Other items: `標題：內容`.
//!   docs/RETRIEVAL_EVAL.md compares the alternatives.
//! * **Idempotent**: a row is current when its model, dimensions and content hash (SHA-256 of
//!   the recipe version and the embedded text) match; only the others are embedded. Running
//!   the job twice does nothing the second time.
//! * **Resumable**: vectors are stored batch by batch. A run that stops (provider down,
//!   restart) leaves what it stored; the next run continues with what is still missing.
//! * **Never in the way**: the job runs in a background task after an import has committed, at
//!   startup, or when a manager asks. Its failures are recorded in `embedding_jobs` and logged;
//!   retrieval keeps working by keyword for items without a current vector.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::PgPool;

use crate::embedding::{vector_literal, EmbedError, Embedder};
use crate::AppState;

/// Part of the content hash: bump it when [`embed_text`] changes so every item is re-embedded.
const RECIPE_VERSION: &str = "manna-embed-v1";
/// Texts per embedding request.
pub const BATCH: usize = 32;
/// Pauses before the second and third attempt of a failed batch.
const RETRY_PAUSES_MS: [u64; 2] = [300, 1_500];
/// Longest text sent for one item (curriculum items are far shorter).
const ITEM_CHARS: usize = 2_000;

/// The text embedded for one knowledge item.
pub fn embed_text(
    title: &str,
    content: &str,
    subject: &str,
    item_kind: &str,
    strand_name: &str,
) -> String {
    let kind = match item_kind {
        "learning_performance" => "學習表現",
        "learning_content" => "學習內容",
        _ => "",
    };
    let head = [subject, kind, strand_name]
        .into_iter()
        .filter(|p| !p.trim().is_empty())
        .collect::<Vec<_>>()
        .join("｜");
    let head = if head.is_empty() { title.trim() } else { &head };
    let text = if head.is_empty() {
        content.trim().to_string()
    } else {
        format!("{head}：{}", content.trim())
    };
    text.chars().take(ITEM_CHARS).collect()
}

pub fn content_hash(text: &str) -> String {
    let mut h = Sha256::new();
    h.update(RECIPE_VERSION.as_bytes());
    h.update(b"\n");
    h.update(text.as_bytes());
    hex::encode(h.finalize())
}

/// In-process guard: one indexing run at a time (the app is a single instance).
#[derive(Default)]
pub struct EmbedJobs {
    running: AtomicBool,
    /// Set when a run was asked for while one was in progress: it scans once more at the end.
    rerun: AtomicBool,
}

impl EmbedJobs {
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

#[derive(sqlx::FromRow)]
struct ItemRow {
    id: i64,
    title: String,
    content: String,
    subject: String,
    item_kind: String,
    strand_name: String,
    model: Option<String>,
    dims: Option<i32>,
    content_hash: Option<String>,
}

struct Pending {
    id: i64,
    text: String,
    hash: String,
}

/// Active items whose stored vector is missing or was made with another model, size or text.
/// With `force`, every active item.
async fn pending(db: &PgPool, embedder: &Embedder, force: bool) -> sqlx::Result<Vec<Pending>> {
    let rows: Vec<ItemRow> = sqlx::query_as(
        "SELECT k.id, k.title, k.content, k.subject, k.item_kind, k.strand_name,
                e.model, e.dims, e.content_hash
           FROM knowledge_items k
           LEFT JOIN knowledge_embeddings e ON e.knowledge_item_id = k.id
          WHERE k.retired_at IS NULL
          ORDER BY k.id",
    )
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|r| {
            let text = embed_text(
                &r.title,
                &r.content,
                &r.subject,
                &r.item_kind,
                &r.strand_name,
            );
            let hash = content_hash(&text);
            let current = r.model.as_deref() == Some(embedder.model())
                && r.dims == Some(embedder.dims() as i32)
                && r.content_hash.as_deref() == Some(hash.as_str());
            (force || !current).then_some(Pending {
                id: r.id,
                text,
                hash,
            })
        })
        .collect())
}

async fn embed_with_retries(
    embedder: &Embedder,
    texts: &[String],
) -> Result<Vec<Vec<f32>>, EmbedError> {
    let mut attempt = 0;
    loop {
        match embedder.embed_documents(texts).await {
            Ok(v) => return Ok(v),
            Err(e) => match RETRY_PAUSES_MS.get(attempt) {
                Some(ms) => {
                    tracing::warn!(error = %e, attempt = attempt + 1, "embedding batch failed; retrying");
                    tokio::time::sleep(Duration::from_millis(*ms)).await;
                    attempt += 1;
                }
                None => return Err(e),
            },
        }
    }
}

async fn store_batch(
    db: &PgPool,
    embedder: &Embedder,
    job_id: i64,
    batch: &[Pending],
    vectors: &[Vec<f32>],
) -> sqlx::Result<()> {
    let ids: Vec<i64> = batch.iter().map(|p| p.id).collect();
    let hashes: Vec<&str> = batch.iter().map(|p| p.hash.as_str()).collect();
    let literals: Vec<String> = vectors.iter().map(|v| vector_literal(v)).collect();
    let mut tx = db.begin().await?;
    sqlx::query(
        "INSERT INTO knowledge_embeddings (knowledge_item_id, model, dims, content_hash, embedding)
         SELECT t.id, $2, $3, t.hash, t.embedding::vector
           FROM UNNEST($1::bigint[], $4::text[], $5::text[]) AS t(id, hash, embedding)
         ON CONFLICT (knowledge_item_id) DO UPDATE
            SET model = EXCLUDED.model, dims = EXCLUDED.dims,
                content_hash = EXCLUDED.content_hash, embedding = EXCLUDED.embedding,
                created_at = now()",
    )
    .bind(&ids)
    .bind(embedder.model())
    .bind(embedder.dims() as i32)
    .bind(&hashes)
    .bind(&literals)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE embedding_jobs SET embedded = embedded + $2 WHERE id = $1")
        .bind(job_id)
        .bind(batch.len() as i32)
        .execute(&mut *tx)
        .await?;
    tx.commit().await
}

async fn finish(db: &PgPool, job_id: i64, error: Option<&str>) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE embedding_jobs
            SET status = CASE WHEN $2::text IS NULL THEN 'done' ELSE 'failed' END,
                error = $2, finished_at = now()
          WHERE id = $1",
    )
    .bind(job_id)
    .bind(error)
    .execute(db)
    .await?;
    Ok(())
}

/// One indexing run. Returns the job id when a job row was written (nothing to do and not
/// asked for by hand: no row).
async fn run_once(
    db: &PgPool,
    embedder: &Embedder,
    trigger: &str,
    started_by: Option<i64>,
    force: bool,
) -> sqlx::Result<Option<i64>> {
    // Only one run exists at a time, so a row still marked running was cut off by a restart.
    sqlx::query(
        "UPDATE embedding_jobs SET status = 'failed', error = 'interrupted', finished_at = now()
          WHERE status = 'running'",
    )
    .execute(db)
    .await?;
    // Vectors of retired versions are never read.
    sqlx::query(
        "DELETE FROM knowledge_embeddings e USING knowledge_items k
          WHERE k.id = e.knowledge_item_id AND k.retired_at IS NOT NULL",
    )
    .execute(db)
    .await?;

    let todo = pending(db, embedder, force).await?;
    if todo.is_empty() && trigger != "manual" {
        return Ok(None);
    }
    let job_id: i64 = sqlx::query_scalar(
        "INSERT INTO embedding_jobs (trigger, model, dims, total, started_by)
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(trigger)
    .bind(embedder.model())
    .bind(embedder.dims() as i32)
    .bind(todo.len() as i32)
    .bind(started_by)
    .fetch_one(db)
    .await?;

    for batch in todo.chunks(BATCH) {
        let texts: Vec<String> = batch.iter().map(|p| p.text.clone()).collect();
        let stored = match embed_with_retries(embedder, &texts).await {
            Ok(vectors) => store_batch(db, embedder, job_id, batch, &vectors)
                .await
                .map_err(|e| format!("database error: {e}")),
            Err(e) => Err(e.to_string()),
        };
        if let Err(message) = stored {
            tracing::warn!(job_id, error = %message, "knowledge embedding job stopped");
            finish(db, job_id, Some(&message)).await?;
            return Ok(Some(job_id));
        }
    }
    finish(db, job_id, None).await?;
    tracing::info!(job_id, items = todo.len(), "knowledge embedding job done");
    Ok(Some(job_id))
}

/// Starts an indexing run in the background. Returns `false` when embeddings are off or a run
/// is already in progress (an `import` or `startup` request then makes that run scan once more
/// when it finishes, so nothing committed meanwhile is missed).
pub fn spawn_sync(
    state: &AppState,
    trigger: &'static str,
    started_by: Option<i64>,
    force: bool,
) -> bool {
    if !state.embedder.enabled() {
        return false;
    }
    if state.embed_jobs.running.swap(true, Ordering::SeqCst) {
        if trigger != "manual" {
            state.embed_jobs.rerun.store(true, Ordering::SeqCst);
        }
        return false;
    }
    let state = state.clone();
    tokio::spawn(async move {
        let (mut trigger, mut started_by, mut force) = (trigger, started_by, force);
        loop {
            if let Err(e) = run_once(&state.db, &state.embedder, trigger, started_by, force).await {
                tracing::warn!(error = %e, "knowledge embedding job failed");
            }
            state.embed_jobs.running.store(false, Ordering::SeqCst);
            let again = state.embed_jobs.rerun.swap(false, Ordering::SeqCst)
                && !state.embed_jobs.running.swap(true, Ordering::SeqCst);
            if !again {
                break;
            }
            (trigger, started_by, force) = ("import", None, false);
        }
    });
    true
}

#[derive(Serialize, sqlx::FromRow)]
pub struct JobRow {
    pub id: i64,
    pub trigger: String,
    pub model: String,
    pub dims: i32,
    pub status: String,
    pub total: i32,
    pub embedded: i32,
    pub error: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Coverage of the semantic index and the latest run (`GET /api/admin/knowledge/embeddings`).
pub async fn status(state: &AppState) -> sqlx::Result<Value> {
    let embedder = &state.embedder;
    let (active, embedded, other): (i64, i64, i64) = sqlx::query_as(
        "SELECT COUNT(*),
                COUNT(e.knowledge_item_id) FILTER (WHERE e.model = $1 AND e.dims = $2),
                COUNT(e.knowledge_item_id) FILTER (WHERE NOT (e.model = $1 AND e.dims = $2))
           FROM knowledge_items k
           LEFT JOIN knowledge_embeddings e ON e.knowledge_item_id = k.id
          WHERE k.retired_at IS NULL",
    )
    .bind(embedder.model())
    .bind(embedder.dims() as i32)
    .fetch_one(&state.db)
    .await?;
    let job: Option<JobRow> = sqlx::query_as(
        "SELECT id, trigger, model, dims, status, total, embedded, error, created_at, finished_at
           FROM embedding_jobs ORDER BY id DESC LIMIT 1",
    )
    .fetch_optional(&state.db)
    .await?;
    let enabled = embedder.enabled();
    Ok(json!({
        "enabled": enabled,
        "provider": embedder.provider().as_str(),
        "model": enabled.then(|| embedder.model()),
        "dims": enabled.then(|| embedder.dims()),
        "active_items": active,
        // Items with a vector of the current model; 0 while embeddings are off.
        "embedded": if enabled { embedded } else { 0 },
        // Vectors of another model or size, ignored by retrieval until re-embedded.
        "other_model": if enabled { other } else { embedded + other },
        "running": state.embed_jobs.is_running(),
        "job": job,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curriculum_items_are_embedded_with_their_names_not_their_code() {
        let t = embed_text(
            "3-Ⅴ-7 學習表現｜語言能力（讀）",
            " 能了解短文的內容。 ",
            "英語文",
            "learning_performance",
            "語言能力（讀）",
        );
        assert_eq!(t, "英語文｜學習表現｜語言能力（讀）：能了解短文的內容。");
        assert!(!t.contains("3-Ⅴ-7"));
    }

    #[test]
    fn other_items_are_embedded_with_their_title() {
        assert_eq!(
            embed_text("倒裝句", "Not only 置於句首", "", "", ""),
            "倒裝句：Not only 置於句首"
        );
        assert_eq!(embed_text("", "只有內容", "", "", ""), "只有內容");
        let long = "字".repeat(5_000);
        assert_eq!(embed_text("t", &long, "", "", "").chars().count(), 2_000);
    }

    #[test]
    fn hash_depends_on_the_text() {
        assert_eq!(content_hash("a"), content_hash("a"));
        assert_ne!(content_hash("a"), content_hash("b"));
        assert_eq!(content_hash("a").len(), 64);
    }
}
