//! 108 curriculum items for the lesson SOP (docs/API.md「Phase 2.1」).
//!
//! Everything the SOP needs from the curriculum goes through the two functions here, which sit
//! on the knowledge base's own retrieval (`retrieval::retrieve_curriculum`, exact-code lookup
//! `retrieval::curriculum_by_codes`). The rule they serve: curriculum **text** always comes from
//! `knowledge_items` by code, never from the model. Only official items count (those carrying
//! the code printed in their source); hand-written notes filed under `curriculum` do not.

use std::collections::HashMap;

use sqlx::PgPool;

use crate::embedding::Embedder;
use crate::retrieval::{self, CurriculumFilter, KbItem, Trace};

/// A curriculum item as the knowledge base stores it.
pub type CurriculumItem = KbItem;

/// More than any track has: "all of them".
const ALL: usize = 100_000;

/// The items the model may link objectives to, in the default scope (senior high, English
/// subjects) of the teacher's track: all of them when they fit in `limit`, otherwise the best
/// matches for `query`, filled up in code order.
pub async fn items_for_track(
    db: &PgPool,
    track: &str,
    query: &str,
    limit: usize,
) -> sqlx::Result<Vec<CurriculumItem>> {
    let scope = CurriculumFilter::default();
    let all: Vec<KbItem> = retrieval::retrieve_curriculum(db, track, &scope, "", ALL)
        .await?
        .into_iter()
        .map(|s| s.item)
        .collect();
    if all.len() <= limit {
        return Ok(all);
    }
    let mut picked: Vec<KbItem> = retrieval::search_curriculum(all.clone(), &scope, query, limit)
        .into_iter()
        .map(|s| s.item)
        .collect();
    for item in all {
        if picked.len() >= limit {
            break;
        }
        if !picked.iter().any(|p| p.item_code == item.item_code) {
            picked.push(item);
        }
    }
    Ok(picked)
}

/// [`items_for_track`] with hybrid ranking when the track has more items than `limit` and an
/// embedding model is available; also says how the items were chosen (for `retrieval_logs`).
/// The ranked items come first, then the rest in code order up to `limit`.
pub async fn items_for_track_hybrid(
    db: &PgPool,
    embedder: &Embedder,
    track: &str,
    query: &str,
    limit: usize,
) -> sqlx::Result<(Vec<CurriculumItem>, Trace)> {
    let scope = CurriculumFilter::default();
    let all: Vec<KbItem> = retrieval::retrieve_curriculum(db, track, &scope, "", ALL)
        .await?
        .into_iter()
        .map(|s| s.item)
        .collect();
    if all.len() <= limit {
        let codes: Vec<String> = all.iter().map(|i| i.item_code.clone()).collect();
        return Ok((all, Trace::all(&codes)));
    }
    let found = retrieval::rank_with_embeddings(db, embedder, query, all.clone(), limit).await;
    let mut picked: Vec<KbItem> = found.items.into_iter().map(|s| s.item).collect();
    for item in all {
        if picked.len() >= limit {
            break;
        }
        if !picked.iter().any(|p| p.item_code == item.item_code) {
            picked.push(item);
        }
    }
    Ok((picked, found.trace))
}

/// Exact lookup by `item_code` for the links a teacher saves. Active items come from the
/// knowledge base's exact-code lookup; a code that was retired since is still resolved (latest
/// version), so a link saved earlier stays readable. Codes that match nothing are absent.
pub async fn by_codes(
    db: &PgPool,
    codes: &[String],
) -> sqlx::Result<HashMap<String, CurriculumItem>> {
    if codes.is_empty() {
        return Ok(HashMap::new());
    }
    let mut connection = db.acquire().await?;
    by_codes_in(&mut connection, codes).await
}

pub(crate) async fn by_codes_in(
    db: &mut sqlx::PgConnection,
    codes: &[String],
) -> sqlx::Result<HashMap<String, CurriculumItem>> {
    if codes.is_empty() {
        return Ok(HashMap::new());
    }
    let mut found: HashMap<String, CurriculumItem> =
        retrieval::curriculum_by_codes(&mut *db, None, codes)
            .await?
            .into_iter()
            // The lookup also matches official codes (shared between 領綱); a stored link names
            // one item, by its unique item_code.
            .filter(|i| codes.contains(&i.item_code))
            .map(|i| (i.item_code.clone(), i))
            .collect();
    let missing: Vec<&String> = codes.iter().filter(|c| !found.contains_key(*c)).collect();
    if !missing.is_empty() {
        let retired: Vec<KbItem> = sqlx::query_as(
            "SELECT DISTINCT ON (item_code)
                    item_code, version, category, school_track, title, content, source_doc,
                    source_version, source_page, official_code, item_kind, strand_name, stage,
                    subject, advanced, recurring
               FROM knowledge_items
              WHERE item_code = ANY($1) AND category = 'curriculum' AND official_code <> ''
              ORDER BY item_code, version DESC",
        )
        .bind(missing)
        .fetch_all(db)
        .await?;
        found.extend(retired.into_iter().map(|i| (i.item_code.clone(), i)));
    }
    Ok(found)
}
