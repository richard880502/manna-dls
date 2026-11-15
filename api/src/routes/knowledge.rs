//! Knowledge base and vocabulary: what teachers can look up, and the read-only browse for
//! knowledge-base managers (docs/API.md「Phase 2.0：知識庫與詞彙」).
//!
//! * Teachers (any signed-in account): curriculum search, official text by exact code,
//!   vocabulary-level analysis. Curriculum text always comes from the database.
//! * Managers ([`Capability::ManageKnowledge`]): browse every knowledge item and vocabulary row,
//!   retired ones included. Imports live in `routes::admin`.

use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::auth::{valid_track, AuthTeacher, Capability};
use crate::error::{ApiJson, ApiQuery, AppError, AppResult};
use crate::retrieval::{self, CurriculumFilter, ANY};
use crate::routes::feedback::escape_like;
use crate::routes::log_usage;
use crate::vocabulary;
use crate::AppState;

const DEFAULT_LIMIT: i64 = 20;
const MAX_LIMIT: i64 = 100;
const MAX_CODES: usize = 50;

fn some_text(s: Option<String>) -> Option<String> {
    s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn limit_of(limit: Option<i64>) -> i64 {
    limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
}

/// The track to search: the caller's own unless a valid one is asked for (a teacher may teach
/// a class of another school type).
fn track_of(teacher: &AuthTeacher, track: Option<String>) -> AppResult<String> {
    match some_text(track) {
        None => Ok(teacher.track().to_string()),
        Some(t) if valid_track(&t) => Ok(t),
        Some(_) => Err(AppError::bad_request(
            "track 需為 general、vocational 或 comprehensive。",
        )),
    }
}

// ---------------------------------------------------------------- curriculum (teachers)

#[derive(Debug, Deserialize)]
pub struct CurriculumQuery {
    pub q: Option<String>,
    pub track: Option<String>,
    pub kind: Option<String>,
    pub stage: Option<String>,
    pub subject: Option<String>,
    pub limit: Option<i64>,
}

/// Curriculum items for a track, ranked by `q` — keywords, fused with embedding similarity
/// when an embedding model is configured (`method`) — or listed in code order without `q`. Defaults to senior-high items of English subjects; `stage=all` / `subject=all` widen.
pub async fn search_curriculum(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiQuery(q): ApiQuery<CurriculumQuery>,
) -> AppResult<Json<Value>> {
    let track = track_of(&teacher, q.track)?;
    let kind = some_text(q.kind);
    if let Some(k) = &kind {
        if !matches!(k.as_str(), "learning_performance" | "learning_content") {
            return Err(AppError::bad_request(
                "kind 需為 learning_performance 或 learning_content。",
            ));
        }
    }
    let filter = CurriculumFilter {
        kind,
        stage: some_text(q.stage),
        subject: some_text(q.subject),
    };
    let query = q.q.unwrap_or_default();
    let limit = limit_of(q.limit) as usize;
    let found = retrieval::retrieve_curriculum_hybrid(
        &state.db,
        &state.embedder,
        &track,
        &filter,
        &query,
        limit,
    )
    .await?;
    let method = found.trace.method;
    let items = found.items;
    Ok(Json(json!({
        "track": track,
        "filter": {
            "kind": filter.kind,
            "stage": filter.stage.as_deref().unwrap_or(retrieval::SENIOR_HIGH_STAGE),
            "subject": match filter.subject.as_deref() {
                None => json!(retrieval::ENGLISH_SUBJECTS),
                Some(s) => json!(s),
            },
        },
        "ranked": method == retrieval::Method::Hybrid || !retrieval::tokenize(&query).is_empty(),
        "method": method,
        "items": items,
    })))
}

#[derive(Debug, Deserialize)]
pub struct ByCodeQuery {
    /// Comma-separated codes.
    pub codes: String,
    /// A track, or `all`. Default: the caller's track.
    pub track: Option<String>,
}

/// The official text and source of cited curriculum codes. A code may be an `item_code`
/// (unique) or an official code as printed (`3-Ⅴ-7` or `3-V-7`); official codes are resolved
/// within `track`. Codes with no active item are returned in `not_found` and must be shown as
/// unsupported, never filled in.
pub async fn curriculum_by_code(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiQuery(q): ApiQuery<ByCodeQuery>,
) -> AppResult<Json<Value>> {
    let codes: Vec<String> = q
        .codes
        .split([',', '，', '、'])
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
        .collect();
    if codes.is_empty() {
        return Err(AppError::bad_request("請提供至少一個課綱代碼。"));
    }
    if codes.len() > MAX_CODES {
        return Err(AppError::bad_request(format!(
            "一次最多查詢 {MAX_CODES} 個代碼。"
        )));
    }
    let track = match q.track.as_deref().map(str::trim) {
        Some(ANY) => None,
        _ => Some(track_of(&teacher, q.track)?),
    };
    let items = retrieval::curriculum_by_codes(&state.db, track.as_deref(), &codes).await?;
    let not_found: Vec<&String> = codes
        .iter()
        .filter(|c| {
            let want = retrieval::normalize_code(c);
            !items.iter().any(|i| {
                retrieval::normalize_code(&i.item_code) == want
                    || retrieval::normalize_code(&i.official_code) == want
            })
        })
        .collect();
    Ok(Json(json!({
        "track": track.as_deref().unwrap_or(ANY),
        "items": items,
        "not_found": not_found,
    })))
}

// ---------------------------------------------------------------- vocabulary analysis (teachers)

#[derive(Debug, Deserialize)]
pub struct AnalyzeRequest {
    pub text: String,
    /// 1–6; default 4.
    pub target_level: Option<i64>,
    /// Optional multiword expressions to set aside (the list has no phrases).
    #[serde(default)]
    pub phrases: Vec<String>,
    /// Default: the most recently imported list.
    pub list_version: Option<String>,
}

/// Deterministic vocabulary-level analysis of a text (no model call; nothing of the text is
/// stored, only the counts are logged).
pub async fn analyze_vocabulary(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiJson(req): ApiJson<AnalyzeRequest>,
) -> AppResult<Json<vocabulary::Analysis>> {
    if req.text.trim().is_empty() {
        return Err(AppError::bad_request("請提供要分析的文字。"));
    }
    if req.text.chars().count() > vocabulary::MAX_TEXT_CHARS {
        return Err(AppError::bad_request(format!(
            "文字太長（上限 {} 字元），請分段分析。",
            vocabulary::MAX_TEXT_CHARS
        )));
    }
    let target = req
        .target_level
        .unwrap_or(vocabulary::DEFAULT_TARGET_LEVEL as i64);
    if !(1..=6).contains(&target) {
        return Err(AppError::bad_request("target_level 需為 1 到 6。"));
    }
    if req.phrases.len() > vocabulary::MAX_PHRASES {
        return Err(AppError::bad_request(format!(
            "phrases 最多 {} 個。",
            vocabulary::MAX_PHRASES
        )));
    }
    let list_version = some_text(req.list_version);
    let lexicon = vocabulary::load_lexicon(&state.db, list_version.as_deref())
        .await?
        .filter(|l| !l.is_empty())
        .ok_or_else(|| {
            AppError::Conflict(match &list_version {
                Some(v) => format!("找不到詞彙表版本「{v}」，無法分析。"),
                None => "系統尚未匯入大考中心參考詞彙表，無法分析詞彙級別。".to_string(),
            })
        })?;
    let analysis = vocabulary::analyze(&req.text, &lexicon, target as u8, &req.phrases);
    log_usage(
        &state.db,
        teacher.id(),
        "vocabulary_analyzed",
        json!({
            "wordlist_version": analysis.wordlist_version,
            "target_level": analysis.target_level,
            "counted_tokens": analysis.basis.counted_tokens,
            "above_target": analysis.above_target.len(),
            "unknown": analysis.basis.excluded_tokens.unknown,
        }),
    )
    .await?;
    Ok(Json(analysis))
}

// ---------------------------------------------------------------- browse (managers)

#[derive(Debug, Deserialize)]
pub struct BrowseQuery {
    pub q: Option<String>,
    pub category: Option<String>,
    pub track: Option<String>,
    pub kind: Option<String>,
    /// `active` (default) | `retired` | `all`
    pub status: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct BrowseItem {
    item_code: String,
    version: i32,
    category: String,
    school_track: String,
    title: String,
    content: String,
    official_code: String,
    item_kind: String,
    strand_name: String,
    stage: String,
    subject: String,
    advanced: bool,
    recurring: bool,
    source_doc: String,
    source_version: String,
    source_page: String,
    source_sha256: String,
    import_batch_id: Option<i64>,
    created_at: chrono::DateTime<chrono::Utc>,
    retired_at: Option<chrono::DateTime<chrono::Utc>>,
}

fn status_of(status: Option<String>) -> AppResult<Option<bool>> {
    match some_text(status).as_deref() {
        None | Some("active") => Ok(Some(false)),
        Some("retired") => Ok(Some(true)),
        Some("all") => Ok(None),
        Some(_) => Err(AppError::bad_request(
            "status 需為 active、retired 或 all。",
        )),
    }
}

/// Read-only list of knowledge items (every category), filtered and paged.
pub async fn browse_items(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiQuery(q): ApiQuery<BrowseQuery>,
) -> AppResult<Json<Value>> {
    teacher.require(Capability::ManageKnowledge)?;
    let retired = status_of(q.status)?;
    let pattern = some_text(q.q).map(|s| format!("%{}%", escape_like(&s)));
    let (category, track, kind) = (some_text(q.category), some_text(q.track), some_text(q.kind));
    const WHERE: &str = "($1::bool IS NULL OR (retired_at IS NOT NULL) = $1)
        AND ($2::text IS NULL OR item_code ILIKE $2 OR official_code ILIKE $2
             OR title ILIKE $2 OR content ILIKE $2)
        AND ($3::text IS NULL OR category = $3)
        AND ($4::text IS NULL OR school_track = $4)
        AND ($5::text IS NULL OR item_kind = $5)";
    let total: i64 = sqlx::query_scalar(&format!(
        "SELECT COUNT(*) FROM knowledge_items WHERE {WHERE}"
    ))
    .bind(retired)
    .bind(&pattern)
    .bind(&category)
    .bind(&track)
    .bind(&kind)
    .fetch_one(&state.db)
    .await?;
    let items: Vec<BrowseItem> = sqlx::query_as(&format!(
        "SELECT item_code, version, category, school_track, title, content, official_code,
                item_kind, strand_name, stage, subject, advanced, recurring, source_doc,
                source_version, source_page, source_sha256, import_batch_id, created_at,
                retired_at
           FROM knowledge_items WHERE {WHERE}
          ORDER BY id LIMIT $6 OFFSET $7"
    ))
    .bind(retired)
    .bind(&pattern)
    .bind(&category)
    .bind(&track)
    .bind(&kind)
    .bind(limit_of(q.limit))
    .bind(q.offset.unwrap_or(0).max(0))
    .fetch_all(&state.db)
    .await?;
    Ok(Json(json!({ "total": total, "items": items })))
}

#[derive(Debug, Deserialize)]
pub struct VocabularyBrowseQuery {
    /// Matches the start of a word form or of the printed entry.
    pub q: Option<String>,
    /// 1–6, or `appendix`.
    pub level: Option<String>,
    pub status: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct BrowseWord {
    word: String,
    headword: String,
    level: Option<i16>,
    pos: String,
    kind: String,
    appendix_category: String,
    list_version: String,
    source_doc: String,
    source_page: String,
    source_sha256: String,
    version: i32,
    import_batch_id: Option<i64>,
    retired_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Read-only list of vocabulary rows, filtered and paged.
pub async fn browse_vocabulary(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiQuery(q): ApiQuery<VocabularyBrowseQuery>,
) -> AppResult<Json<Value>> {
    teacher.require(Capability::ManageKnowledge)?;
    let retired = status_of(q.status)?;
    let pattern = some_text(q.q).map(|s| format!("{}%", escape_like(&s)));
    let (level, appendix): (Option<i16>, bool) = match some_text(q.level).as_deref() {
        None => (None, false),
        Some("appendix") => (None, true),
        Some(l) => match l.parse::<i16>() {
            Ok(n) if (1..=6).contains(&n) => (Some(n), false),
            _ => return Err(AppError::bad_request("level 需為 1 到 6 或 appendix。")),
        },
    };
    const WHERE: &str = "($1::bool IS NULL OR (retired_at IS NOT NULL) = $1)
        AND ($2::text IS NULL OR word ILIKE $2 OR headword ILIKE $2)
        AND ($3::smallint IS NULL OR level = $3)
        AND (NOT $4 OR kind = 'appendix')";
    let total: i64 = sqlx::query_scalar(&format!(
        "SELECT COUNT(*) FROM vocabulary_items WHERE {WHERE}"
    ))
    .bind(retired)
    .bind(&pattern)
    .bind(level)
    .bind(appendix)
    .fetch_one(&state.db)
    .await?;
    let items: Vec<BrowseWord> = sqlx::query_as(&format!(
        "SELECT word, headword, level, pos, kind, appendix_category, list_version, source_doc,
                source_page, source_sha256, version, import_batch_id, retired_at
           FROM vocabulary_items WHERE {WHERE}
          ORDER BY lower(word), id LIMIT $5 OFFSET $6"
    ))
    .bind(retired)
    .bind(&pattern)
    .bind(level)
    .bind(appendix)
    .bind(limit_of(q.limit))
    .bind(q.offset.unwrap_or(0).max(0))
    .fetch_all(&state.db)
    .await?;
    Ok(Json(json!({ "total": total, "items": items })))
}
