//! 108 curriculum items for the lesson SOP (docs/API.md「Phase 2.1」).
//!
//! Everything that needs curriculum items goes through the three functions here, so a better
//! knowledge base (more items, better retrieval, fetch by code) only has to change this file.
//! The rule they serve: curriculum **text** always comes from `knowledge_items` by code, never
//! from the model.

use std::collections::HashMap;

use serde::Serialize;
use sqlx::PgPool;

use crate::retrieval::{self, KbItem};

pub const CATEGORY: &str = "curriculum";

#[derive(Debug, Clone, Serialize, PartialEq, sqlx::FromRow)]
pub struct CurriculumItem {
    pub item_code: String,
    pub version: i32,
    pub category: String,
    pub school_track: String,
    pub title: String,
    pub content: String,
    pub source_doc: String,
    pub source_version: String,
    pub source_page: String,
}

const COLS: &str = "item_code, version, category, school_track, title, content, source_doc, \
                    source_version, source_page";

/// Every current curriculum item for a school track, in code order.
async fn all_for_track(db: &PgPool, track: &str) -> sqlx::Result<Vec<CurriculumItem>> {
    sqlx::query_as(&format!(
        "SELECT * FROM (
            SELECT DISTINCT ON (item_code) {COLS}, retired_at
              FROM knowledge_items
             WHERE category = $1 AND school_track IN ($2, 'both')
             ORDER BY item_code, version DESC) latest
          WHERE retired_at IS NULL
          ORDER BY item_code"
    ))
    .bind(CATEGORY)
    .bind(track)
    .fetch_all(db)
    .await
}

/// Orders `items` by how well they match `query` (keyword retrieval); items that do not match
/// follow in code order. Also returns how many matched.
fn ranked(items: Vec<CurriculumItem>, query: &str) -> (Vec<CurriculumItem>, usize) {
    let kb: Vec<KbItem> = items
        .iter()
        .map(|i| KbItem {
            item_code: i.item_code.clone(),
            version: i.version,
            category: i.category.clone(),
            school_track: i.school_track.clone(),
            title: i.title.clone(),
            content: i.content.clone(),
            source_doc: i.source_doc.clone(),
            source_page: i.source_page.clone(),
        })
        .collect();
    let order: Vec<String> = retrieval::rank(query, kb, items.len())
        .into_iter()
        .map(|s| s.item.item_code)
        .collect();
    let mut by_code: HashMap<String, CurriculumItem> = items
        .iter()
        .cloned()
        .map(|i| (i.item_code.clone(), i))
        .collect();
    let mut out: Vec<CurriculumItem> = order
        .iter()
        .filter_map(|code| by_code.remove(code))
        .collect();
    let matched = out.len();
    out.extend(
        items
            .into_iter()
            .filter(|i| by_code.contains_key(&i.item_code)),
    );
    (out, matched)
}

/// The items the model may link objectives to: all of the track's items when they fit in
/// `limit`, otherwise the `limit` best matches for `query`.
pub async fn items_for_track(
    db: &PgPool,
    track: &str,
    query: &str,
    limit: usize,
) -> sqlx::Result<Vec<CurriculumItem>> {
    let items = all_for_track(db, track).await?;
    if items.len() <= limit {
        return Ok(items);
    }
    let (mut items, _) = ranked(items, query);
    items.truncate(limit);
    Ok(items)
}

/// Keyword search for the teacher's own linking (no AI). A blank query lists the first items.
pub async fn search(
    db: &PgPool,
    track: &str,
    query: &str,
    limit: usize,
) -> sqlx::Result<Vec<CurriculumItem>> {
    let items = all_for_track(db, track).await?;
    let mut items = if query.trim().is_empty() {
        items
    } else {
        let (mut ordered, matched) = ranked(items, query);
        ordered.truncate(matched);
        ordered
    };
    items.truncate(limit);
    Ok(items)
}

/// The latest version of each code, whatever its track and whether or not it was retired since:
/// a link a teacher saved earlier must stay readable.
pub async fn by_codes(
    db: &PgPool,
    codes: &[String],
) -> sqlx::Result<HashMap<String, CurriculumItem>> {
    if codes.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<CurriculumItem> = sqlx::query_as(&format!(
        "SELECT DISTINCT ON (item_code) {COLS}
           FROM knowledge_items
          WHERE item_code = ANY($1)
          ORDER BY item_code, version DESC"
    ))
    .bind(codes)
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(|i| (i.item_code.clone(), i)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(code: &str, title: &str, content: &str) -> CurriculumItem {
        CurriculumItem {
            item_code: code.into(),
            version: 1,
            category: CATEGORY.into(),
            school_track: "general".into(),
            title: title.into(),
            content: content.into(),
            source_doc: String::new(),
            source_version: String::new(),
            source_page: String::new(),
        }
    }

    #[test]
    fn ranked_puts_matches_first_and_keeps_the_rest() {
        let items = vec![
            item("A", "口語", "能以簡短口語描述個人經驗"),
            item("B", "閱讀", "能閱讀議論文並找出論點與理由"),
            item("C", "寫作", "能寫出段落"),
        ];
        let (out, matched) = ranked(items.clone(), "議論文 論點");
        assert_eq!(
            out.iter().map(|i| i.item_code.as_str()).collect::<Vec<_>>(),
            ["B", "A", "C"]
        );
        assert_eq!(matched, 1);
        assert_eq!(ranked(items.clone(), "  "), (items, 0));
    }
}
