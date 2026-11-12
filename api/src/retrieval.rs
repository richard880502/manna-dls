//! Phase 1 keyword retrieval over the (small) knowledge base.
//!
//! * Candidates are filtered by the teacher's school track (`track` or `both`) and exclude
//!   retired items; only the latest version of each `item_code` is considered.
//! * Approved teacher cases with `in_rag = true` join as items with category `teacher_case`
//!   (`in_rag` defaults to false, so they are normally excluded — see BLUEPRINT §8).
//! * Query and items are tokenized the same way: lowercased English words (≥ 3 chars,
//!   stopwords removed, naive plural stripping) plus CJK character bigrams. An item's score is
//!   the number of distinct query tokens it contains (title ∪ content) plus the number found in
//!   its title (a title match counts twice: the title names what the item is about); ties are
//!   broken by item_code. Items with no match are dropped.
//! * The `embedding` column is reserved for hybrid search once an approved embedding model exists.

use std::collections::HashSet;

use serde::Serialize;
use sqlx::PgPool;

const EN_STOPWORDS: &[&str] = &[
    "the", "and", "for", "are", "but", "not", "you", "all", "any", "can", "had", "her", "was",
    "one", "our", "out", "has", "have", "him", "his", "how", "its", "may", "new", "now", "see",
    "two", "did", "get", "let", "put", "say", "she", "too", "use", "with", "this", "that", "from",
    "they", "will", "would", "there", "their", "what", "about", "like", "into", "than", "then",
    "them", "these", "those", "some", "could", "other", "been", "were", "your", "more", "most",
    "such", "each", "very", "just", "over", "should", "does", "doing", "done", "here", "while",
    "after", "before", "because", "being", "both", "own", "same", "few", "via", "per", "etc",
];

// Words that name grammar points are deliberately not stopwords: "only"/"also"
// (Not only … but also), "who"/"which"/"where"/"when"/"why" (relative clauses), "make"
// (collocations). Domain-generic CJK bigrams that would match almost every item are dropped;
// 示範 also appears in every DEMO item's "（示範）" marker.
const CJK_STOP_BIGRAMS: &[&str] = &[
    "學生", "老師", "教師", "我們", "這個", "一個", "可以", "什麼", "如何", "進行", "本課", "課程",
    "這節", "節課", "希望", "能夠", "以及", "或是", "還是", "使用", "他們", "自己", "然後", "因為",
    "所以", "的是", "了解", "一下", "示範",
];

pub fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3400..=0x4DBF   // CJK Extension A
        | 0x4E00..=0x9FFF // CJK Unified Ideographs
        | 0xF900..=0xFAFF // CJK Compatibility Ideographs
        | 0x20000..=0x2EBEF // Extensions B–F
        | 0x30000..=0x3134F) // Extension G
}

fn stem_en(word: &str) -> String {
    if word.len() > 4 && word.ends_with("ies") {
        format!("{}y", &word[..word.len() - 3])
    } else if word.len() > 3 && word.ends_with('s') && !word.ends_with("ss") {
        word[..word.len() - 1].to_string()
    } else {
        word.to_string()
    }
}

#[derive(Default)]
struct Acc {
    seen: HashSet<String>,
    out: Vec<String>,
}

impl Acc {
    fn push(&mut self, t: String) {
        if self.seen.insert(t.clone()) {
            self.out.push(t);
        }
    }

    fn flush_word(&mut self, word: &mut String) {
        if word.len() >= 3 && !EN_STOPWORDS.contains(&word.as_str()) {
            self.push(stem_en(word));
        }
        word.clear();
    }

    fn flush_cjk(&mut self, run: &mut Vec<char>) {
        for pair in run.windows(2) {
            let bigram: String = pair.iter().collect();
            if !CJK_STOP_BIGRAMS.contains(&bigram.as_str()) {
                self.push(bigram);
            }
        }
        run.clear();
    }
}

/// Distinct tokens in first-seen order.
pub fn tokenize(text: &str) -> Vec<String> {
    let mut acc = Acc::default();
    let mut word = String::new();
    let mut run: Vec<char> = Vec::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            acc.flush_cjk(&mut run);
            word.push(c.to_ascii_lowercase());
        } else if is_cjk(c) {
            acc.flush_word(&mut word);
            run.push(c);
        } else {
            acc.flush_word(&mut word);
            acc.flush_cjk(&mut run);
        }
    }
    acc.flush_word(&mut word);
    acc.flush_cjk(&mut run);
    acc.out
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct KbItem {
    pub item_code: String,
    pub version: i32,
    pub category: String,
    pub school_track: String,
    pub title: String,
    pub content: String,
    pub source_doc: String,
    pub source_page: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Scored {
    pub item: KbItem,
    pub score: usize,
    pub title_score: usize,
}

/// Pure scorer: returns at most `top_k` items with score > 0, best first.
pub fn rank(query: &str, items: Vec<KbItem>, top_k: usize) -> Vec<Scored> {
    let q: Vec<String> = tokenize(query);
    if q.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<Scored> = items
        .into_iter()
        .filter_map(|item| {
            let title_tokens: HashSet<String> = tokenize(&item.title).into_iter().collect();
            let mut all: HashSet<String> = tokenize(&item.content).into_iter().collect();
            all.extend(title_tokens.iter().cloned());
            let matched = q.iter().filter(|t| all.contains(*t)).count();
            let title_score = q.iter().filter(|t| title_tokens.contains(*t)).count();
            let score = matched + title_score;
            (matched > 0).then_some(Scored {
                item,
                score,
                title_score,
            })
        })
        .collect();
    scored.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then(a.item.item_code.cmp(&b.item.item_code))
    });
    scored.truncate(top_k);
    scored
}

/// Loads candidates for `track` and ranks them.
pub async fn retrieve(
    db: &PgPool,
    track: &str,
    query: &str,
    top_k: usize,
) -> sqlx::Result<Vec<Scored>> {
    let mut items: Vec<KbItem> = sqlx::query_as(
        "SELECT DISTINCT ON (item_code)
                item_code, version, category, school_track, title, content, source_doc, source_page
           FROM knowledge_items
          WHERE retired_at IS NULL AND school_track IN ($1, 'both')
          ORDER BY item_code, version DESC",
    )
    .bind(track)
    .fetch_all(db)
    .await?;
    let cases: Vec<KbItem> = sqlx::query_as(
        "SELECT case_code AS item_code, 1 AS version, 'teacher_case' AS category, school_track,
                CASE WHEN topic = '' THEN summary ELSE topic END AS title,
                summary || E'\n' || details AS content,
                '教師備課經驗（非官方來源）' AS source_doc, '' AS source_page
           FROM teacher_cases
          WHERE approved AND in_rag AND school_track IN ($1, 'both')",
    )
    .bind(track)
    .fetch_all(db)
    .await?;
    items.extend(cases);
    Ok(rank(query, items, top_k))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(code: &str, title: &str, content: &str) -> KbItem {
        KbItem {
            item_code: code.into(),
            version: 1,
            category: "grammar".into(),
            school_track: "both".into(),
            title: title.into(),
            content: content.into(),
            source_doc: String::new(),
            source_page: String::new(),
        }
    }

    #[test]
    fn english_tokens_lowercased_filtered_and_stemmed() {
        let t = tokenize("The Relative Clauses and an IS of Inversion, inversion!");
        assert_eq!(t, vec!["relative", "clause", "inversion"]);
    }

    #[test]
    fn plural_stemming() {
        assert_eq!(
            tokenize("activities class buses"),
            vec!["activity", "class", "buse"]
        );
    }

    #[test]
    fn cjk_bigrams_with_stoplist() {
        let t = tokenize("倒裝句教學");
        assert_eq!(t, vec!["倒裝", "裝句", "句教", "教學"]);
        // "學生" is a stop bigram; single CJK characters yield nothing.
        let t = tokenize("學生 句");
        assert!(t.is_empty(), "{t:?}");
    }

    #[test]
    fn mixed_text_splits_runs() {
        let t = tokenize("用Not only倒裝");
        assert_eq!(t, vec!["only", "倒裝"]);
        let t = tokenize("relative子句clause");
        assert_eq!(t, vec!["relative", "子句", "clause"]);
    }

    #[test]
    fn digits_count_as_words() {
        assert_eq!(tokenize("2000 字 a1"), vec!["2000"]);
    }

    #[test]
    fn ranking_prefers_more_matches_then_title() {
        let items = vec![
            item("B", "關係子句", "relative clause 與 who/which"),
            item("A", "倒裝句", "Not only 置於句首時倒裝"),
            item("C", "被動語態", "passive voice"),
            item("D", "閱讀策略", "倒裝 句子 的閱讀"),
        ];
        let r = rank("倒裝句 inversion", items, 8);
        let codes: Vec<_> = r.iter().map(|s| s.item.item_code.as_str()).collect();
        assert_eq!(codes, vec!["A", "D"]);
        assert!(r[0].score >= r[1].score);
    }

    #[test]
    fn ranking_truncates_to_top_k_and_drops_zero() {
        let items = (0..20)
            .map(|i| item(&format!("I{i:02}"), "relative clause", "who which that"))
            .chain(std::iter::once(item("Z", "unrelated", "nothing here")))
            .collect();
        let r = rank("relative clauses", items, 5);
        assert_eq!(r.len(), 5);
        assert!(r.iter().all(|s| s.item.item_code != "Z"));
        // ties broken by item_code
        assert_eq!(r[0].item.item_code, "I00");
    }

    #[test]
    fn title_matches_count_twice() {
        let items = vec![
            item("A", "其他主題", "倒裝 關係 子句"),
            item("B", "倒裝句", "說明"),
        ];
        let r = rank("倒裝 關係", items, 8);
        // A: 2 content matches; B: 1 match in title (counted twice).
        assert_eq!(r[0].score, 2);
        assert_eq!(r[1].score, 2);
        assert_eq!(r[0].item.item_code, "A", "ties broken by item_code");
        let r = rank(
            "倒裝",
            vec![item("A", "x", "倒裝"), item("B", "倒裝", "y")],
            8,
        );
        assert_eq!(r[0].item.item_code, "B");
    }

    #[test]
    fn grammar_words_are_kept() {
        assert_eq!(
            tokenize("who which where make that"),
            vec!["who", "which", "where", "make"]
        );
        assert!(tokenize("（示範）").is_empty());
    }

    #[test]
    fn empty_query_matches_nothing() {
        assert!(rank("the and of", vec![item("A", "the", "and")], 8).is_empty());
    }
}
