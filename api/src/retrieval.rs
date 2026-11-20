//! Retrieval over the (small) knowledge base: keyword matching, fused with embedding
//! similarity when an embedding model is configured (Phase 2.4, at the end of this header).
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
//!
//! Phase 2.0 (real 108 curriculum items):
//! * `comprehensive` (綜合型高中) is a track of its own with its own 領綱; a teacher of any
//!   track sees that track's items plus `both`.
//! * The knowledge base holds every extracted curriculum item, including the elementary and
//!   junior-high stages of the general 領綱 and the non-English subjects of the 外語群. By default
//!   only senior-high (stage Ⅴ) items of English subjects are retrieved ([`CurriculumFilter`]);
//!   the others are reachable by explicit filter or by exact code.
//! * [`curriculum_by_codes`] returns the official text of cited codes. Curriculum text shown to
//!   a teacher must always come from here, never from a model.
//!
//! Phase 2.4 (hybrid retrieval, docs/RETRIEVAL_EVAL.md):
//! * The `*_hybrid` functions rank the same candidates (same track / kind / stage / subject
//!   filters) by keyword score **and** by cosine similarity between the query's embedding and
//!   the items' stored embeddings (`knowledge_embeddings`, current model only), and merge the
//!   two rankings with [`fuse`]. Keyword matching alone finds nothing for an English query
//!   against the Chinese curriculum text; the embedding does.
//! * Embeddings are optional. With `EMBEDDING_PROVIDER=none`, an empty query, a slow or failing
//!   embedding call, or no stored vectors, the result is exactly the keyword ranking and
//!   [`Trace::fallback_reason`] says why. Nothing here returns an error because of embeddings.
//! * Items without a vector (not indexed yet, teacher cases) still compete through keywords.
//! * [`log`] records the method and the fused candidates in `retrieval_logs`.

use std::collections::{HashMap, HashSet};

use serde::Serialize;
use sqlx::PgPool;

use crate::embedding::{vector_literal, Embedder};

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
    pub source_version: String,
    pub source_page: String,
    /// The code as printed in the source (e.g. `3-Ⅴ-7`); empty for non-curriculum items.
    pub official_code: String,
    /// `learning_performance` | `learning_content` | empty.
    pub item_kind: String,
    pub strand_name: String,
    pub stage: String,
    pub subject: String,
    pub advanced: bool,
    pub recurring: bool,
}

/// Columns of [`KbItem`] in `knowledge_items`.
const KB_COLUMNS: &str = "item_code, version, category, school_track, title, content, source_doc,
    source_version, source_page, official_code, item_kind, strand_name, stage, subject, advanced,
    recurring";

/// 學習階段 of senior high school.
pub const SENIOR_HIGH_STAGE: &str = "V";
/// Subjects retrieved by default: the 語文領域 英語文 and the English subjects of the 外語群
/// (英語 I–VII, 職場英文實務). Japanese and the business/computing subjects are left out.
pub const ENGLISH_SUBJECTS: [&str; 3] = ["英語文", "英語", "職場"];
/// Filter value meaning "do not filter".
pub const ANY: &str = "all";

/// Which curriculum items a search considers. `None` means the default scope.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CurriculumFilter {
    /// `learning_performance` | `learning_content`; `None` = both.
    pub kind: Option<String>,
    /// 學習階段 (`II`–`V`), or [`ANY`]; `None` = senior high ([`SENIOR_HIGH_STAGE`]).
    pub stage: Option<String>,
    /// Subject as stored (e.g. `英語文`, `日語`), or [`ANY`]; `None` = [`ENGLISH_SUBJECTS`].
    pub subject: Option<String>,
}

impl CurriculumFilter {
    /// Items without a stage or subject (hand-written curriculum notes) are always in scope.
    pub fn accepts(&self, item: &KbItem) -> bool {
        let kind_ok = self.kind.as_deref().is_none_or(|k| item.item_kind == k);
        let stage_ok = item.stage.is_empty()
            || match self.stage.as_deref() {
                None => item.stage == SENIOR_HIGH_STAGE,
                Some(ANY) => true,
                Some(s) => item.stage == normalize_code(s),
            };
        let subject_ok = item.subject.is_empty()
            || match self.subject.as_deref() {
                None => ENGLISH_SUBJECTS.contains(&item.subject.as_str()),
                Some(ANY) => true,
                Some(s) => item.subject == s,
            };
        kind_ok && stage_ok && subject_ok
    }
}

/// Tracks whose items a teacher of `track` may be shown.
pub fn tracks_for(track: &str) -> [&str; 2] {
    [track, "both"]
}

/// Canonical form for comparing curriculum codes: no whitespace, no `*`/`◎` marks, Roman
/// numeral characters (Ⅴ) as ASCII letters (V), every dash variant as `-`.
pub fn normalize_code(code: &str) -> String {
    let mut out = String::with_capacity(code.len());
    for c in code.chars() {
        match c {
            'Ⅰ' => out.push('I'),
            'Ⅱ' => out.push_str("II"),
            'Ⅲ' => out.push_str("III"),
            'Ⅳ' => out.push_str("IV"),
            'Ⅴ' => out.push('V'),
            'Ⅵ' => out.push_str("VI"),
            'Ⅶ' => out.push_str("VII"),
            '*' | '＊' | '◎' => {}
            '－' | '‐' | '‑' | '–' | '—' => out.push('-'),
            c if c.is_whitespace() => {}
            c => out.push(c),
        }
    }
    out
}

/// Orders codes the way a person reads them: `1-V-2` before `1-V-10`.
fn natural_key(code: &str) -> Vec<(u8, u64, String)> {
    let mut key = Vec::new();
    let mut digits = String::new();
    let mut text = String::new();
    for c in code.chars() {
        if c.is_ascii_digit() {
            if !text.is_empty() {
                key.push((1, 0, std::mem::take(&mut text)));
            }
            digits.push(c);
        } else {
            if !digits.is_empty() {
                key.push((0, digits.parse().unwrap_or(u64::MAX), String::new()));
                digits.clear();
            }
            text.push(c);
        }
    }
    if !digits.is_empty() {
        key.push((0, digits.parse().unwrap_or(u64::MAX), String::new()));
    }
    if !text.is_empty() {
        key.push((1, 0, text));
    }
    key
}

/// An official curriculum item: category `curriculum` with the code printed in its source.
/// Hand-written notes filed under `curriculum` (the seeded `DEMO-CUR-*` items, which say they
/// are not curriculum text) have no official code and are never returned as curriculum.
pub fn is_official_curriculum(item: &KbItem) -> bool {
    item.category == "curriculum" && !item.official_code.is_empty()
}

/// Pure curriculum search over already-loaded items: applies `filter`, then ranks by `query`.
/// An empty query (no usable token) lists the filtered items in code order with score 0.
pub fn search_curriculum(
    items: Vec<KbItem>,
    filter: &CurriculumFilter,
    query: &str,
    top_k: usize,
) -> Vec<Scored> {
    let items: Vec<KbItem> = items
        .into_iter()
        .filter(|i| is_official_curriculum(i) && filter.accepts(i))
        .collect();
    if !tokenize(query).is_empty() {
        return rank(query, items, top_k);
    }
    let mut listed: Vec<Scored> = items
        .into_iter()
        .map(|item| Scored {
            item,
            score: 0,
            title_score: 0,
            similarity: None,
        })
        .collect();
    listed.sort_by_cached_key(|s| natural_key(&s.item.item_code));
    listed.truncate(top_k);
    listed
}

/// Pure exact-code match after [`normalize_code`]. A code equal to an `item_code` (unique)
/// always matches; a code equal to an `official_code` matches the items of `track` (plus
/// `both`), or of every track when `track` is `None` — the same official code exists in several
/// 領綱. Results follow the order of `codes`, without duplicates. No fuzzy matching.
pub fn match_codes(items: &[KbItem], track: Option<&str>, codes: &[String]) -> Vec<KbItem> {
    let in_track =
        |item: &KbItem| track.is_none_or(|t| tracks_for(t).contains(&item.school_track.as_str()));
    let mut out: Vec<KbItem> = Vec::new();
    for code in codes {
        let want = normalize_code(code);
        if want.is_empty() {
            continue;
        }
        for item in items.iter().filter(|i| is_official_curriculum(i)) {
            let hit = normalize_code(&item.item_code) == want
                || (in_track(item) && normalize_code(&item.official_code) == want);
            if hit && !out.iter().any(|o| o.item_code == item.item_code) {
                out.push(item.clone());
            }
        }
    }
    out
}

#[derive(Debug, Clone, Serialize)]
pub struct Scored {
    pub item: KbItem,
    /// Keyword score (0 for an item found only by its embedding).
    pub score: usize,
    pub title_score: usize,
    /// Cosine similarity to the query, when hybrid retrieval was used and the item has a vector.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub similarity: Option<f32>,
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
                similarity: None,
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

// ---------------------------------------------------------------- hybrid retrieval (Phase 2.4)

/// Reciprocal-rank-fusion constant (the usual 60).
pub const RRF_K: f64 = 60.0;
/// Weights of the two rankings in [`fuse`]. Tuned in docs/RETRIEVAL_EVAL.md: other settings
/// were within noise of these.
pub const KEYWORD_WEIGHT: f64 = 1.0;
pub const VECTOR_WEIGHT: f64 = 1.0;

/// One fused candidate, as logged in `retrieval_logs.candidates`. Ranks start at 1.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Candidate {
    pub item_code: String,
    pub keyword_rank: Option<usize>,
    pub keyword_score: usize,
    pub vector_rank: Option<usize>,
    pub similarity: Option<f32>,
    pub fused: f64,
}

/// Weighted reciprocal rank fusion of a keyword ranking and a vector ranking, both best first.
///
/// `fused = KEYWORD_WEIGHT · (score / best score) / (RRF_K + keyword rank)
///        + VECTOR_WEIGHT / (RRF_K + vector rank)`
///
/// * Keyword ranks are competition ranks: items with the same keyword score share the rank of
///   the first of them, so the item_code tie-break of [`rank`] carries no weight.
/// * The keyword term is scaled by the item's score relative to the best keyword score: an
///   item sharing one bigram with the query must not count like the item matching ten.
/// * Ties in the fused score: higher similarity first, then item_code.
pub fn fuse(keyword: &[(String, usize)], vector: &[(String, f32)]) -> Vec<Candidate> {
    let mut by_code: HashMap<&str, Candidate> = HashMap::new();
    let blank = |code: &str| Candidate {
        item_code: code.to_string(),
        keyword_rank: None,
        keyword_score: 0,
        vector_rank: None,
        similarity: None,
        fused: 0.0,
    };
    let best = keyword.first().map_or(1, |(_, s)| (*s).max(1)) as f64;
    let (mut rank, mut prev) = (0usize, None);
    for (i, (code, score)) in keyword.iter().enumerate() {
        if prev != Some(*score) {
            rank = i + 1;
            prev = Some(*score);
        }
        let c = by_code.entry(code).or_insert_with(|| blank(code));
        c.keyword_rank = Some(rank);
        c.keyword_score = *score;
        c.fused += KEYWORD_WEIGHT * (*score as f64 / best) / (RRF_K + rank as f64);
    }
    for (i, (code, similarity)) in vector.iter().enumerate() {
        let c = by_code.entry(code).or_insert_with(|| blank(code));
        c.vector_rank = Some(i + 1);
        c.similarity = Some(*similarity);
        c.fused += VECTOR_WEIGHT / (RRF_K + (i + 1) as f64);
    }
    let mut out: Vec<Candidate> = by_code.into_values().collect();
    out.sort_by(|a, b| {
        b.fused
            .total_cmp(&a.fused)
            .then_with(|| {
                b.similarity
                    .unwrap_or(f32::MIN)
                    .total_cmp(&a.similarity.unwrap_or(f32::MIN))
            })
            .then_with(|| a.item_code.cmp(&b.item_code))
    });
    out
}

/// Pure hybrid ranking of already-loaded items: keyword ranking over all of them, vector
/// ranking over those present in `similarities` (by item_code), fused, cut to `top_k`.
pub fn rank_hybrid(
    query: &str,
    items: Vec<KbItem>,
    similarities: &HashMap<String, f32>,
    top_k: usize,
) -> (Vec<Scored>, Vec<Candidate>) {
    let keyword = rank(query, items.clone(), usize::MAX);
    let keyword_list: Vec<(String, usize)> = keyword
        .iter()
        .map(|s| (s.item.item_code.clone(), s.score))
        .collect();
    let title_scores: HashMap<&str, usize> = keyword
        .iter()
        .map(|s| (s.item.item_code.as_str(), s.title_score))
        .collect();
    let mut vector: Vec<(String, f32)> = items
        .iter()
        .filter_map(|i| {
            similarities
                .get(&i.item_code)
                .map(|s| (i.item_code.clone(), *s))
        })
        .collect();
    vector.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    let mut candidates = fuse(&keyword_list, &vector);
    candidates.truncate(top_k);
    let mut by_code: HashMap<String, KbItem> = items
        .into_iter()
        .map(|i| (i.item_code.clone(), i))
        .collect();
    let scored = candidates
        .iter()
        .filter_map(|c| {
            by_code.remove(&c.item_code).map(|item| Scored {
                score: c.keyword_score,
                title_score: title_scores.get(c.item_code.as_str()).copied().unwrap_or(0),
                similarity: c.similarity,
                item,
            })
        })
        .collect();
    (scored, candidates)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    /// Keyword ranking only.
    Keyword,
    /// Keyword and embedding rankings fused.
    Hybrid,
    /// No ranking: every item in scope was returned (SOP objectives of a small track).
    All,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Keyword => "keyword",
            Self::Hybrid => "hybrid",
            Self::All => "all",
        }
    }
}

/// How a result was produced; stored with it in `retrieval_logs`.
#[derive(Debug, Clone, Serialize)]
pub struct Trace {
    pub method: Method,
    /// The embedding model, when one was used.
    pub embedding_model: Option<String>,
    /// Why an enabled embedding model was not used: `empty_query`, `embedding_timeout`,
    /// `embedding_failed`, `no_vectors`, `vector_lookup_failed`. `None` when hybrid was used
    /// or embeddings are simply off.
    pub fallback_reason: Option<&'static str>,
    /// The returned items in order, with both ranks.
    pub candidates: Vec<Candidate>,
}

impl Trace {
    pub fn keyword(items: &[Scored], fallback_reason: Option<&'static str>) -> Self {
        Self {
            method: Method::Keyword,
            embedding_model: None,
            fallback_reason,
            candidates: items
                .iter()
                .enumerate()
                .map(|(i, s)| Candidate {
                    item_code: s.item.item_code.clone(),
                    // Position in the returned list (score 0 = listed, not ranked).
                    keyword_rank: (s.score > 0).then_some(i + 1),
                    keyword_score: s.score,
                    vector_rank: None,
                    similarity: None,
                    fused: 0.0,
                })
                .collect(),
        }
    }

    /// Every item in scope was returned without ranking.
    pub fn all(codes: &[String]) -> Self {
        Self {
            method: Method::All,
            embedding_model: None,
            fallback_reason: None,
            candidates: codes
                .iter()
                .map(|c| Candidate {
                    item_code: c.clone(),
                    keyword_rank: None,
                    keyword_score: 0,
                    vector_rank: None,
                    similarity: None,
                    fused: 0.0,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Retrieval {
    pub items: Vec<Scored>,
    pub trace: Trace,
}

/// Cosine similarity between `query` and the stored vectors of `codes` (active items embedded
/// with `embedder`'s current model and dimensions; others are simply absent).
///
/// An exact scan over the filtered candidates: with a knowledge base of a few thousand rows it
/// takes milliseconds and, unlike an approximate index scan, cannot lose candidates to the
/// track / stage / subject filters. The HNSW index of migration 0011 is there for an
/// `ORDER BY … LIMIT` prefilter once the knowledge base is large enough to need one.
async fn similarities(
    db: &PgPool,
    embedder: &Embedder,
    query: &[f32],
    codes: &[String],
) -> sqlx::Result<HashMap<String, f32>> {
    let rows: Vec<(String, f32)> = sqlx::query_as(
        "SELECT DISTINCT ON (k.item_code)
                k.item_code, (1 - (e.embedding <=> $1::vector))::float4
           FROM knowledge_embeddings e
           JOIN knowledge_items k ON k.id = e.knowledge_item_id
          WHERE k.retired_at IS NULL AND e.model = $2 AND e.dims = $3
            AND k.item_code = ANY($4)
          ORDER BY k.item_code, k.version DESC",
    )
    .bind(vector_literal(query))
    .bind(embedder.model())
    .bind(embedder.dims() as i32)
    .bind(codes)
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().collect())
}

/// Ranks `items` for `query`: hybrid when an embedding model is configured and answers in
/// time, otherwise keyword-only. Never fails.
pub async fn rank_with_embeddings(
    db: &PgPool,
    embedder: &Embedder,
    query: &str,
    items: Vec<KbItem>,
    top_k: usize,
) -> Retrieval {
    let keyword_only = |items: Vec<KbItem>, reason: Option<&'static str>| {
        let items = rank(query, items, top_k);
        let trace = Trace::keyword(&items, reason);
        Retrieval { items, trace }
    };
    if !embedder.enabled() {
        return keyword_only(items, None);
    }
    if query.trim().is_empty() {
        return keyword_only(items, Some("empty_query"));
    }
    let vector = match embedder.embed_query(query).await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, "query embedding failed; keyword retrieval only");
            return keyword_only(items, Some(e.code()));
        }
    };
    let codes: Vec<String> = items.iter().map(|i| i.item_code.clone()).collect();
    let sims = match similarities(db, embedder, &vector, &codes).await {
        Ok(s) if s.is_empty() => return keyword_only(items, Some("no_vectors")),
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(error = %e, "vector lookup failed; keyword retrieval only");
            return keyword_only(items, Some("vector_lookup_failed"));
        }
    };
    let (items, candidates) = rank_hybrid(query, items, &sims, top_k);
    Retrieval {
        items,
        trace: Trace {
            method: Method::Hybrid,
            embedding_model: Some(embedder.model().to_string()),
            fallback_reason: None,
            candidates,
        },
    }
}

/// Writes one `retrieval_logs` row: the query, the item codes handed on, and how they were
/// found ([`Trace`]).
pub async fn log<'e, E>(
    executor: E,
    ai_interaction_id: Option<i64>,
    query_text: &str,
    track: &str,
    codes: &[String],
    trace: &Trace,
) -> sqlx::Result<()>
where
    E: sqlx::PgExecutor<'e>,
{
    sqlx::query(
        "INSERT INTO retrieval_logs
           (ai_interaction_id, query_text, school_track, item_codes, method, embedding_model,
            fallback_reason, candidates)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(ai_interaction_id)
    .bind(query_text)
    .bind(track)
    .bind(codes)
    .bind(trace.method.as_str())
    .bind(&trace.embedding_model)
    .bind(trace.fallback_reason)
    .bind(serde_json::to_value(&trace.candidates).unwrap_or_default())
    .execute(executor)
    .await?;
    Ok(())
}

async fn active_items(
    db: impl sqlx::PgExecutor<'_>,
    track: Option<&str>,
    curriculum_only: bool,
) -> sqlx::Result<Vec<KbItem>> {
    let tracks: Option<Vec<&str>> = track.map(|t| tracks_for(t).to_vec());
    sqlx::query_as(&format!(
        "SELECT DISTINCT ON (item_code) {KB_COLUMNS}
           FROM knowledge_items
          WHERE retired_at IS NULL
            AND ($1::text[] IS NULL OR school_track = ANY($1))
            AND (NOT $2 OR category = 'curriculum')
          ORDER BY item_code, version DESC"
    ))
    .bind(tracks)
    .bind(curriculum_only)
    .fetch_all(db)
    .await
}

/// Candidates for plan suggestions: the track's active items (curriculum items only within
/// the default scope) plus approved teacher cases marked `in_rag`.
async fn suggestion_candidates(db: &PgPool, track: &str) -> sqlx::Result<Vec<KbItem>> {
    let scope = CurriculumFilter::default();
    let mut items: Vec<KbItem> = active_items(db, Some(track), false)
        .await?
        .into_iter()
        .filter(|i| i.category != "curriculum" || scope.accepts(i))
        .collect();
    let cases: Vec<KbItem> = sqlx::query_as(
        "SELECT case_code AS item_code, 1 AS version, 'teacher_case' AS category, school_track,
                CASE WHEN topic = '' THEN summary ELSE topic END AS title,
                summary || E'\n' || details AS content,
                '教師備課經驗（非官方來源）' AS source_doc, '' AS source_version,
                '' AS source_page, '' AS official_code, '' AS item_kind, '' AS strand_name,
                '' AS stage, '' AS subject, false AS advanced, false AS recurring
           FROM teacher_cases
          WHERE approved AND in_rag AND school_track = ANY($1)",
    )
    .bind(tracks_for(track).to_vec())
    .fetch_all(db)
    .await?;
    items.extend(cases);
    Ok(items)
}

/// Loads candidates for `track` and ranks them by keyword. Curriculum items outside the
/// default scope (other stages, non-English subjects) are not candidates.
pub async fn retrieve(
    db: &PgPool,
    track: &str,
    query: &str,
    top_k: usize,
) -> sqlx::Result<Vec<Scored>> {
    Ok(rank(query, suggestion_candidates(db, track).await?, top_k))
}

/// [`retrieve`] with hybrid ranking when embeddings are available (plan suggestions).
pub async fn retrieve_hybrid(
    db: &PgPool,
    embedder: &Embedder,
    track: &str,
    query: &str,
    top_k: usize,
) -> sqlx::Result<Retrieval> {
    let items = suggestion_candidates(db, track).await?;
    Ok(rank_with_embeddings(db, embedder, query, items, top_k).await)
}

/// Curriculum items of `track` (plus `both`) matching `filter`, ranked by `query`; with an empty
/// query, listed in code order.
pub async fn retrieve_curriculum(
    db: &PgPool,
    track: &str,
    filter: &CurriculumFilter,
    query: &str,
    top_k: usize,
) -> sqlx::Result<Vec<Scored>> {
    let items = active_items(db, Some(track), true).await?;
    Ok(search_curriculum(items, filter, query, top_k))
}

/// [`retrieve_curriculum`] with hybrid ranking when embeddings are available. Without a query
/// the items are listed in code order, as before.
pub async fn retrieve_curriculum_hybrid(
    db: &PgPool,
    embedder: &Embedder,
    track: &str,
    filter: &CurriculumFilter,
    query: &str,
    top_k: usize,
) -> sqlx::Result<Retrieval> {
    let items: Vec<KbItem> = active_items(db, Some(track), true)
        .await?
        .into_iter()
        .filter(|i| is_official_curriculum(i) && filter.accepts(i))
        .collect();
    if embedder.enabled() && !query.trim().is_empty() {
        let found = rank_with_embeddings(db, embedder, query, items.clone(), top_k).await;
        if found.trace.method == Method::Hybrid {
            return Ok(found);
        }
        // Keyword fallback: keep the listing behaviour for a query without usable tokens.
        let listed = search_curriculum(items, filter, query, top_k);
        let trace = Trace::keyword(&listed, found.trace.fallback_reason);
        return Ok(Retrieval {
            items: listed,
            trace,
        });
    }
    let listed = search_curriculum(items, filter, query, top_k);
    let trace = Trace::keyword(&listed, None);
    Ok(Retrieval {
        items: listed,
        trace,
    })
}

/// Active curriculum items with exactly these codes (`item_code` such as `108-G-3-V-7`, or the
/// official code such as `3-Ⅴ-7` / `3-V-7`; see [`match_codes`] for how `track` applies). Codes
/// that match nothing are simply absent from the result: the caller must treat them as
/// unsupported and never fill in a text.
pub async fn curriculum_by_codes(
    db: impl sqlx::PgExecutor<'_>,
    track: Option<&str>,
    codes: &[String],
) -> sqlx::Result<Vec<KbItem>> {
    if codes.is_empty() {
        return Ok(Vec::new());
    }
    let items = active_items(db, None, true).await?;
    Ok(match_codes(&items, track, codes))
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
            source_version: String::new(),
            source_page: String::new(),
            official_code: String::new(),
            item_kind: String::new(),
            strand_name: String::new(),
            stage: String::new(),
            subject: String::new(),
            advanced: false,
            recurring: false,
        }
    }

    /// Synthetic curriculum row (not real curriculum text).
    fn cur(
        code: &str,
        official: &str,
        kind: &str,
        stage: &str,
        subject: &str,
        text: &str,
    ) -> KbItem {
        KbItem {
            category: "curriculum".into(),
            school_track: "general".into(),
            official_code: official.into(),
            item_kind: kind.into(),
            stage: stage.into(),
            subject: subject.into(),
            ..item(code, &format!("{official} 測試類別"), text)
        }
    }

    fn fixtures() -> Vec<KbItem> {
        vec![
            cur(
                "T-G-3-V-10",
                "3-Ⅴ-10",
                "learning_performance",
                "V",
                "英語文",
                "能讀懂測試用的短文並找出主旨。",
            ),
            cur(
                "T-G-3-V-2",
                "◎3-Ⅴ-2",
                "learning_performance",
                "V",
                "英語文",
                "能讀懂測試用的句子。",
            ),
            cur(
                "T-G-Ae-V-1",
                "Ae-Ⅴ-1",
                "learning_content",
                "V",
                "英語文",
                "測試用的短文與故事。",
            ),
            cur(
                "T-G-3-IV-1",
                "3-Ⅳ-1",
                "learning_performance",
                "IV",
                "英語文",
                "能讀懂測試用的短文。",
            ),
            cur(
                "T-JP-1",
                "外語-技-日語I-A-a",
                "learning_content",
                "V",
                "日語",
                "測試用的日語短文閱讀。",
            ),
            KbItem {
                school_track: "comprehensive".into(),
                ..cur(
                    "T-C-3-V-2",
                    "3-Ⅴ-2",
                    "learning_performance",
                    "V",
                    "英語文",
                    "能讀懂測試用的句子（綜高）。",
                )
            },
            item("T-GRM-1", "短文閱讀策略", "不是課綱條目"),
            KbItem {
                category: "curriculum".into(),
                ..item(
                    "T-NOTE-1",
                    "示範學習表現：短文",
                    "示範用的短文描述，不是課綱原文",
                )
            },
        ]
    }

    fn codes(scored: &[Scored]) -> Vec<&str> {
        scored.iter().map(|s| s.item.item_code.as_str()).collect()
    }

    #[test]
    fn curriculum_search_defaults_to_senior_high_english() {
        let r = search_curriculum(fixtures(), &CurriculumFilter::default(), "短文 閱讀", 10);
        let got = codes(&r);
        assert!(
            got.contains(&"T-G-3-V-10") && got.contains(&"T-G-Ae-V-1"),
            "{got:?}"
        );
        // junior-high stage, Japanese subject and non-curriculum items are out of scope
        for out in ["T-G-3-IV-1", "T-JP-1", "T-GRM-1", "T-NOTE-1"] {
            assert!(!got.contains(&out), "{out} in {got:?}");
        }
    }

    #[test]
    fn curriculum_search_filters_by_kind_stage_and_subject() {
        let kind = CurriculumFilter {
            kind: Some("learning_content".into()),
            ..Default::default()
        };
        assert_eq!(
            codes(&search_curriculum(fixtures(), &kind, "短文", 10)),
            vec!["T-G-Ae-V-1"]
        );
        let stage = CurriculumFilter {
            stage: Some("Ⅳ".into()),
            ..Default::default()
        };
        assert_eq!(
            codes(&search_curriculum(fixtures(), &stage, "短文", 10)),
            vec!["T-G-3-IV-1"]
        );
        let subject = CurriculumFilter {
            subject: Some("日語".into()),
            ..Default::default()
        };
        assert_eq!(
            codes(&search_curriculum(fixtures(), &subject, "短文", 10)),
            vec!["T-JP-1"]
        );
        let all = CurriculumFilter {
            stage: Some(ANY.into()),
            subject: Some(ANY.into()),
            ..Default::default()
        };
        assert_eq!(search_curriculum(fixtures(), &all, "短文", 10).len(), 4);
    }

    #[test]
    fn curriculum_search_without_query_lists_in_natural_code_order() {
        let r = search_curriculum(fixtures(), &CurriculumFilter::default(), "", 3);
        assert_eq!(codes(&r), vec!["T-C-3-V-2", "T-G-3-V-2", "T-G-3-V-10"]);
        assert!(r.iter().all(|s| s.score == 0));
    }

    #[test]
    fn codes_are_normalized() {
        assert_eq!(normalize_code(" ◎3-Ⅴ-7 "), "3-V-7");
        assert_eq!(normalize_code("*Ae－Ⅳ-12"), "Ae-IV-12");
        assert_eq!(normalize_code("外語-技-英語 Ⅱ-A-a"), "外語-技-英語II-A-a");
    }

    #[test]
    fn exact_code_lookup_matches_item_code_or_official_code() {
        let items = fixtures();
        let by = |track: Option<&str>, codes: &[&str]| -> Vec<String> {
            let codes: Vec<String> = codes.iter().map(|c| c.to_string()).collect();
            match_codes(&items, track, &codes)
                .into_iter()
                .map(|i| i.item_code)
                .collect()
        };
        assert_eq!(by(None, &["T-G-3-V-10"]), vec!["T-G-3-V-10"]);
        // official code in either spelling; marks ignored; out-of-default-scope items included
        assert_eq!(
            by(None, &["3-V-10", "3-Ⅳ-1"]),
            vec!["T-G-3-V-10", "T-G-3-IV-1"]
        );
        // an official code shared by two tracks: both without a track, one with it
        assert_eq!(by(None, &["3-Ⅴ-2"]), vec!["T-G-3-V-2", "T-C-3-V-2"]);
        assert_eq!(by(Some("comprehensive"), &["3-Ⅴ-2"]), vec!["T-C-3-V-2"]);
        assert_eq!(by(Some("general"), &["◎3-V-2"]), vec!["T-G-3-V-2"]);
        // a unique item_code is found whatever the track; another track's official code is not
        assert_eq!(by(Some("vocational"), &["T-C-3-V-2"]), vec!["T-C-3-V-2"]);
        assert!(by(Some("vocational"), &["3-V-2"]).is_empty());
        // unknown codes and near misses return nothing: no fuzzy matching
        assert!(by(None, &["3-V-1", "9-V-99", ""]).is_empty());
        // items without an official code are not curriculum text
        assert!(by(None, &["T-GRM-1", "T-NOTE-1"]).is_empty());
        assert_eq!(
            by(None, &["3-V-10", "T-G-3-V-10"]).len(),
            1,
            "no duplicates"
        );
    }

    #[test]
    fn tracks_include_shared_items() {
        assert_eq!(tracks_for("comprehensive"), ["comprehensive", "both"]);
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

    fn kw(pairs: &[(&str, usize)]) -> Vec<(String, usize)> {
        pairs.iter().map(|(c, s)| (c.to_string(), *s)).collect()
    }

    fn vecs(pairs: &[(&str, f32)]) -> Vec<(String, f32)> {
        pairs.iter().map(|(c, s)| (c.to_string(), *s)).collect()
    }

    fn fused_codes(c: &[Candidate]) -> Vec<&str> {
        c.iter().map(|c| c.item_code.as_str()).collect()
    }

    #[test]
    fn fusion_rewards_agreement_between_the_two_rankings() {
        // B is second in both lists; A and C each lead one list and are absent from the other.
        let f = fuse(&kw(&[("A", 4), ("B", 4)]), &vecs(&[("C", 0.9), ("B", 0.8)]));
        assert_eq!(fused_codes(&f), vec!["B", "C", "A"]);
        let b = &f[0];
        // A and B tie on keyword score and share rank 1.
        assert_eq!((b.keyword_rank, b.vector_rank), (Some(1), Some(2)));
        let expected = 1.0 / (RRF_K + 1.0) + 1.0 / (RRF_K + 2.0);
        assert!((b.fused - expected).abs() < 1e-12);
        // C (vector rank 1) and A (keyword rank 1) have the same fused score: the item with a
        // similarity goes first.
        assert_eq!(f[1].fused, f[2].fused);
    }

    #[test]
    fn fusion_scales_weak_keyword_matches_down() {
        // K1 matches 10 tokens, K2 only 1; V1 is the nearest vector.
        let f = fuse(
            &kw(&[("K1", 10), ("K2", 1)]),
            &vecs(&[("V1", 0.9), ("V2", 0.8), ("V3", 0.7)]),
        );
        let pos = |c: &str| f.iter().position(|x| x.item_code == c).unwrap();
        assert!(
            pos("K1") < pos("V2"),
            "a strong keyword match keeps its place"
        );
        assert!(
            pos("K2") > pos("V3"),
            "one shared bigram does not outrank near vectors"
        );
        let k2 = &f[pos("K2")];
        assert_eq!(k2.keyword_rank, Some(2));
        assert!((k2.fused - 0.1 / (RRF_K + 2.0)).abs() < 1e-12);
    }

    #[test]
    fn fusion_of_one_list_keeps_its_order() {
        let only_kw = fuse(&kw(&[("B", 3), ("A", 2), ("C", 2)]), &[]);
        assert_eq!(fused_codes(&only_kw), vec!["B", "A", "C"]);
        let only_vec = fuse(&[], &vecs(&[("Z", 0.9), ("Y", 0.5)]));
        assert_eq!(fused_codes(&only_vec), vec!["Z", "Y"]);
        assert!(only_vec[0].keyword_rank.is_none() && only_vec[0].keyword_score == 0);
        assert!(fuse(&[], &[]).is_empty());
    }

    #[test]
    fn hybrid_ranking_finds_items_keywords_miss_and_truncates() {
        let items = vec![
            item("A", "倒裝句", "Not only 置於句首時倒裝"),
            item("B", "被動語態", "passive voice"),
            item("C", "閱讀策略", "skimming"),
        ];
        // The query shares no token with B, but B's vector is the nearest.
        let sims = HashMap::from([("A".to_string(), 0.2), ("B".to_string(), 0.9)]);
        let (scored, candidates) = rank_hybrid("倒裝句", items.clone(), &sims, 8);
        assert_eq!(
            codes(&scored),
            vec!["A", "B"],
            "C has neither match nor vector"
        );
        assert_eq!(scored[1].score, 0);
        assert_eq!(scored[1].similarity, Some(0.9));
        assert!(scored[0].score > 0 && scored[0].title_score > 0);
        assert_eq!(candidates.len(), 2);
        let (top, _) = rank_hybrid("倒裝句", items.clone(), &sims, 1);
        assert_eq!(codes(&top), vec!["A"]);
        // No vectors at all: exactly the keyword ranking.
        let (kw_only, _) = rank_hybrid("倒裝句", items.clone(), &HashMap::new(), 8);
        assert_eq!(codes(&kw_only), codes(&rank("倒裝句", items, 8)));
    }

    #[tokio::test]
    async fn disabled_embedder_falls_back_to_keywords_without_touching_the_database() {
        use crate::embedding::{Embedder, EmbeddingConfig};
        // A pool that cannot connect: the keyword path must not need it.
        let db = sqlx::postgres::PgPoolOptions::new()
            .acquire_timeout(std::time::Duration::from_millis(200))
            .connect_lazy("postgres://nobody@127.0.0.1:1/none")
            .unwrap();
        let items = vec![
            item("A", "倒裝句", "Not only 置於句首時倒裝"),
            item("B", "被動語態", "passive voice"),
        ];
        let off = Embedder::new(EmbeddingConfig::disabled());
        let r = rank_with_embeddings(&db, &off, "倒裝句", items.clone(), 8).await;
        assert_eq!(r.trace.method, Method::Keyword);
        assert_eq!(r.trace.fallback_reason, None);
        assert_eq!(codes(&r.items), vec!["A"]);
        assert_eq!(r.trace.candidates[0].keyword_rank, Some(1));

        // Enabled, but the vector lookup fails (no database): still the keyword ranking.
        let fake = Embedder::new(EmbeddingConfig::fake());
        let r = rank_with_embeddings(&db, &fake, "倒裝句", items.clone(), 8).await;
        assert_eq!(r.trace.method, Method::Keyword);
        assert_eq!(r.trace.fallback_reason, Some("vector_lookup_failed"));
        assert_eq!(codes(&r.items), vec!["A"]);
        let r = rank_with_embeddings(&db, &fake, "   ", items, 8).await;
        assert_eq!(r.trace.fallback_reason, Some("empty_query"));
        assert!(r.items.is_empty());
    }

    #[test]
    fn empty_query_matches_nothing() {
        assert!(rank("the and of", vec![item("A", "the", "and")], 8).is_empty());
    }
}
