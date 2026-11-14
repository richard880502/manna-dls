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

async fn active_items(
    db: &PgPool,
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

/// Loads candidates for `track` and ranks them (plan suggestions). Curriculum items outside the
/// default scope (other stages, non-English subjects) are not candidates.
pub async fn retrieve(
    db: &PgPool,
    track: &str,
    query: &str,
    top_k: usize,
) -> sqlx::Result<Vec<Scored>> {
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
    Ok(rank(query, items, top_k))
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

/// Active curriculum items with exactly these codes (`item_code` such as `108-G-3-V-7`, or the
/// official code such as `3-Ⅴ-7` / `3-V-7`; see [`match_codes`] for how `track` applies). Codes
/// that match nothing are simply absent from the result: the caller must treat them as
/// unsupported and never fill in a text.
pub async fn curriculum_by_codes(
    db: &PgPool,
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

    #[test]
    fn empty_query_matches_nothing() {
        assert!(rank("the and of", vec![item("A", "the", "and")], 8).is_empty());
    }
}
