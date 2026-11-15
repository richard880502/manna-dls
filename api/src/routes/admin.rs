//! Management endpoints. Invites, platform settings, prompt versions and the teacher-case import
//! are PI-only ([`Capability::Administer`]); the knowledge-base and vocabulary imports, their
//! status and the demo retirement need [`Capability::ManageKnowledge`] (PI or 知識庫管理員).
//! Contract: docs/API.md「Phase 2.0：知識庫與詞彙」.

use std::collections::{BTreeMap, HashMap, HashSet};

use axum::body::Bytes;
use axum::extract::State;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::{self, AuthTeacher, Capability};
use crate::error::{ApiJson, ApiQuery, AppError, AppResult};
use crate::prompts::PromptVersion;
use crate::routes::log_usage;
use crate::settings;
use crate::AppState;

// ---------------------------------------------------------------- invites

#[derive(Debug, Deserialize)]
pub struct InviteRequest {
    pub research_id: String,
    pub school_track: String,
    pub role: Option<String>,
}

pub async fn create_invite(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiJson(req): ApiJson<InviteRequest>,
) -> AppResult<Json<Value>> {
    teacher.require_pi()?;
    let research_id = req.research_id.trim();
    if research_id.is_empty() {
        return Err(AppError::bad_request("請輸入研究編號。"));
    }
    if !auth::valid_track(&req.school_track) {
        return Err(AppError::bad_request(
            "學校類型需為 general、vocational 或 comprehensive。",
        ));
    }
    let role = req.role.unwrap_or_else(|| "teacher".into());
    if !auth::valid_role(&role) {
        return Err(AppError::bad_request("角色不正確。"));
    }
    let code = auth::new_invite_code();
    sqlx::query(
        "INSERT INTO invite_codes (code_hash, research_id, school_track, role, created_by)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(auth::sha256_hex(&code))
    .bind(research_id)
    .bind(&req.school_track)
    .bind(&role)
    .bind(teacher.id())
    .execute(&state.db)
    .await?;
    // The clear-text code is returned only once; the database keeps its hash.
    Ok(Json(json!({ "code": code })))
}

// ---------------------------------------------------------------- settings

pub async fn get_settings(
    State(state): State<AppState>,
    teacher: AuthTeacher,
) -> AppResult<Json<Value>> {
    teacher.require_pi()?;
    Ok(Json(json!({
        "route_choice_enabled": settings::route_choice_enabled(&state.db).await?,
    })))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsRequest {
    pub route_choice_enabled: bool,
}

/// Updates the settings; each change is logged to `usage_logs` (`setting_changed`).
pub async fn put_settings(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiJson(req): ApiJson<SettingsRequest>,
) -> AppResult<Json<Value>> {
    teacher.require_pi()?;
    let value = json!(req.route_choice_enabled);
    let mut tx = state.db.begin().await?;
    let previous = settings::set(
        &mut tx,
        settings::ROUTE_CHOICE_ENABLED,
        &value,
        teacher.id(),
    )
    .await?;
    if previous.as_ref() != Some(&value) {
        log_usage(
            &mut *tx,
            teacher.id(),
            "setting_changed",
            json!({ "key": settings::ROUTE_CHOICE_ENABLED, "from": previous, "to": value }),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(Json(
        json!({ "route_choice_enabled": req.route_choice_enabled }),
    ))
}

// ---------------------------------------------------------------- prompts

pub async fn list_prompts(
    State(state): State<AppState>,
    teacher: AuthTeacher,
) -> AppResult<Json<Vec<PromptVersion>>> {
    teacher.require_pi()?;
    let rows = sqlx::query_as(
        "SELECT DISTINCT ON (prompt_key)
                id, prompt_key, version_no, system_text, user_template, created_at
           FROM prompt_versions ORDER BY prompt_key, version_no DESC",
    )
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

#[derive(Debug, Deserialize)]
pub struct NewPrompt {
    pub prompt_key: String,
    pub system_text: String,
    pub user_template: String,
}

fn valid_prompt_key(k: &str) -> bool {
    !k.is_empty()
        && k.len() <= 64
        && k.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

pub async fn create_prompt(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiJson(req): ApiJson<NewPrompt>,
) -> AppResult<Json<PromptVersion>> {
    teacher.require_pi()?;
    let key = req.prompt_key.trim();
    if !valid_prompt_key(key) {
        return Err(AppError::bad_request(
            "prompt_key 只能使用小寫英文、數字與底線。",
        ));
    }
    if req.system_text.trim().is_empty() || req.user_template.trim().is_empty() {
        return Err(AppError::bad_request("系統提示詞與使用者範本都不可空白。"));
    }
    let mut tx = state.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
        .bind(key)
        .execute(&mut *tx)
        .await?;
    let row: PromptVersion = sqlx::query_as(
        "INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template, created_by)
         VALUES ($1, (SELECT COALESCE(MAX(version_no), 0) + 1 FROM prompt_versions WHERE prompt_key = $1),
                 $2, $3, $4)
         RETURNING id, prompt_key, version_no, system_text, user_template, created_at",
    )
    .bind(key)
    .bind(&req.system_text)
    .bind(&req.user_template)
    .bind(teacher.id())
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(row))
}

// ---------------------------------------------------------------- CSV imports

pub const KNOWLEDGE_COLUMNS: [&str; 8] = [
    "item_code",
    "category",
    "school_track",
    "title",
    "content",
    "source_doc",
    "source_version",
    "source_page",
];
pub const CASE_COLUMNS: [&str; 7] = [
    "case_code",
    "school_track",
    "grade",
    "topic",
    "summary",
    "details",
    "approved",
];

/// Optional columns of the knowledge CSV (curriculum metadata). A CSV without them imports as
/// before; `source_sha256` (SHA-256 of the source document) falls back to the CSV's own hash.
pub const KNOWLEDGE_OPTIONAL_COLUMNS: [&str; 9] = [
    "official_code",
    "item_kind",
    "strand",
    "strand_name",
    "stage",
    "subject",
    "advanced",
    "recurring",
    "source_sha256",
];
pub const VOCABULARY_COLUMNS: [&str; 10] = [
    "word",
    "headword",
    "level",
    "pos",
    "kind",
    "appendix_category",
    "list_version",
    "source_doc",
    "source_page",
    "source_sha256",
];

fn bool_cell<'de, D: serde::Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    let s = String::deserialize(d)?;
    parse_bool_cell(&s)
        .ok_or_else(|| serde::de::Error::custom(format!("「{s}」不是 true 或 false")))
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct KnowledgeRow {
    pub item_code: String,
    pub category: String,
    pub school_track: String,
    pub title: String,
    pub content: String,
    #[serde(default)]
    pub source_doc: String,
    #[serde(default)]
    pub source_version: String,
    #[serde(default)]
    pub source_page: String,
    /// The code as printed in the source (e.g. `3-Ⅴ-7`); `item_code` is the unique import code.
    #[serde(default)]
    pub official_code: String,
    /// `learning_performance` (學習表現) | `learning_content` (學習內容) | empty.
    #[serde(default)]
    pub item_kind: String,
    #[serde(default)]
    pub strand: String,
    #[serde(default)]
    pub strand_name: String,
    #[serde(default)]
    pub stage: String,
    #[serde(default)]
    pub subject: String,
    #[serde(default, deserialize_with = "bool_cell")]
    pub advanced: bool,
    #[serde(default, deserialize_with = "bool_cell")]
    pub recurring: bool,
    /// SHA-256 of the source document (not of the CSV); empty when the CSV has no such column.
    #[serde(default)]
    pub source_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct VocabularyRow {
    pub word: String,
    pub headword: String,
    /// 1–6; empty for appendix words.
    #[serde(default)]
    pub level: Option<i16>,
    #[serde(default)]
    pub pos: String,
    pub kind: String,
    #[serde(default)]
    pub appendix_category: String,
    pub list_version: String,
    #[serde(default)]
    pub source_doc: String,
    #[serde(default)]
    pub source_page: String,
    #[serde(default)]
    pub source_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct RawCaseRow {
    case_code: String,
    school_track: String,
    #[serde(default)]
    grade: String,
    #[serde(default)]
    topic: String,
    summary: String,
    #[serde(default)]
    details: String,
    #[serde(default)]
    approved: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseRow {
    pub case_code: String,
    pub school_track: String,
    pub grade: String,
    pub topic: String,
    pub summary: String,
    pub details: String,
    pub approved: bool,
}

fn csv_reader(bytes: &[u8]) -> Result<csv::Reader<&[u8]>, String> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes); // Excel UTF-8 BOM
    std::str::from_utf8(bytes).map_err(|_| "檔案需為 UTF-8 編碼。".to_string())?;
    Ok(csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .flexible(false)
        .from_reader(bytes))
}

fn check_headers(rdr: &mut csv::Reader<&[u8]>, expected: &[&str]) -> Result<(), String> {
    let headers = rdr.headers().map_err(|e| format!("無法讀取標題列：{e}"))?;
    let have: HashSet<&str> = headers.iter().collect();
    let missing: Vec<&str> = expected
        .iter()
        .copied()
        .filter(|c| !have.contains(c))
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!("CSV 缺少欄位：{}", missing.join(", ")))
    }
}

fn parse_rows<T: serde::de::DeserializeOwned>(
    bytes: &[u8],
    expected: &[&str],
) -> Result<Vec<T>, String> {
    let mut rdr = csv_reader(bytes)?;
    check_headers(&mut rdr, expected)?;
    rdr.deserialize()
        .enumerate()
        .map(|(i, r)| r.map_err(|e| format!("第 {} 列格式錯誤：{e}", i + 2)))
        .collect()
}

pub fn parse_knowledge_csv(bytes: &[u8]) -> Result<Vec<KnowledgeRow>, String> {
    let rows: Vec<KnowledgeRow> = parse_rows(bytes, &KNOWLEDGE_COLUMNS)?;
    let mut codes = HashSet::new();
    for (i, r) in rows.iter().enumerate() {
        let line = i + 2;
        for (name, v) in [
            ("item_code", &r.item_code),
            ("category", &r.category),
            ("title", &r.title),
            ("content", &r.content),
        ] {
            if v.is_empty() {
                return Err(format!("第 {line} 列的 {name} 不可空白。"));
            }
        }
        if !valid_item_track(&r.school_track) {
            return Err(format!("第 {line} 列的 school_track 需為 {TRACKS_HINT}。"));
        }
        if !matches!(
            r.item_kind.as_str(),
            "" | "learning_performance" | "learning_content"
        ) {
            return Err(format!(
                "第 {line} 列的 item_kind 需為 learning_performance、learning_content 或空白。"
            ));
        }
        if !valid_sha256_cell(&r.source_sha256) {
            return Err(format!(
                "第 {line} 列的 source_sha256 需為 64 個十六進位字元或空白。"
            ));
        }
        if !codes.insert(r.item_code.clone()) {
            return Err(format!("第 {line} 列的 item_code {} 重複。", r.item_code));
        }
    }
    Ok(rows)
}

const TRACKS_HINT: &str = "general、vocational、comprehensive 或 both";

/// Tracks a knowledge item or teacher case may carry (`both` = shared by every school type).
fn valid_item_track(s: &str) -> bool {
    auth::valid_track(s) || s == "both"
}

fn valid_sha256_cell(s: &str) -> bool {
    s.is_empty() || (s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
}

pub fn parse_vocabulary_csv(bytes: &[u8]) -> Result<Vec<VocabularyRow>, String> {
    let rows: Vec<VocabularyRow> = parse_rows(bytes, &VOCABULARY_COLUMNS)?;
    let mut keys = HashSet::new();
    for (i, r) in rows.iter().enumerate() {
        let line = i + 2;
        for (name, v) in [
            ("word", &r.word),
            ("headword", &r.headword),
            ("list_version", &r.list_version),
        ] {
            if v.is_empty() {
                return Err(format!("第 {line} 列的 {name} 不可空白。"));
            }
        }
        match (r.kind.as_str(), r.level) {
            ("base" | "variant" | "derived", Some(1..=6)) | ("appendix", None) => {}
            ("base" | "variant" | "derived" | "appendix", _) => {
                return Err(format!(
                    "第 {line} 列的 level 需為 1–6（kind 為 appendix 時留空）。"
                ));
            }
            _ => {
                return Err(format!(
                    "第 {line} 列的 kind 需為 base、variant、derived 或 appendix。"
                ));
            }
        }
        if !valid_sha256_cell(&r.source_sha256) {
            return Err(format!(
                "第 {line} 列的 source_sha256 需為 64 個十六進位字元或空白。"
            ));
        }
        if !keys.insert(r.key()) {
            return Err(format!(
                "第 {line} 列的詞形 {}（詞條 {}）重複。",
                r.word, r.headword
            ));
        }
    }
    Ok(rows)
}

type VocabularyKey = (String, String, String, String, String, String);

impl VocabularyRow {
    /// Identity of a word form across imports: a row with the same key and different data is a
    /// new version of it. The appendix category is part of it because a word can be in two
    /// appendix lists (a country that is also a continent).
    fn key(&self) -> VocabularyKey {
        (
            self.list_version.clone(),
            self.word.clone(),
            self.headword.clone(),
            self.pos.clone(),
            self.kind.clone(),
            self.appendix_category.clone(),
        )
    }
}

pub fn parse_bool_cell(s: &str) -> Option<bool> {
    match s.trim().to_lowercase().as_str() {
        "true" | "1" | "yes" | "y" | "是" | "已核准" | "核准" => Some(true),
        "" | "false" | "0" | "no" | "n" | "否" | "未核准" => Some(false),
        _ => None,
    }
}

pub fn parse_cases_csv(bytes: &[u8]) -> Result<Vec<CaseRow>, String> {
    let raw: Vec<RawCaseRow> = parse_rows(bytes, &CASE_COLUMNS)?;
    let mut codes = HashSet::new();
    raw.into_iter()
        .enumerate()
        .map(|(i, r)| {
            let line = i + 2;
            if r.case_code.is_empty() || r.summary.is_empty() {
                return Err(format!("第 {line} 列的 case_code 與 summary 不可空白。"));
            }
            if !valid_item_track(&r.school_track) {
                return Err(format!("第 {line} 列的 school_track 需為 {TRACKS_HINT}。"));
            }
            if !codes.insert(r.case_code.clone()) {
                return Err(format!("第 {line} 列的 case_code {} 重複。", r.case_code));
            }
            let approved = parse_bool_cell(&r.approved)
                .ok_or_else(|| format!("第 {line} 列的 approved 需為 true 或 false。"))?;
            Ok(CaseRow {
                case_code: r.case_code,
                school_track: r.school_track,
                grade: r.grade,
                topic: r.topic,
                summary: r.summary,
                details: r.details,
                approved,
            })
        })
        .collect()
}

#[derive(sqlx::FromRow)]
struct ExistingItem {
    item_code: String,
    version: i32,
    category: String,
    school_track: String,
    title: String,
    content: String,
    source_doc: String,
    source_version: String,
    source_page: String,
    source_sha256: String,
    official_code: String,
    item_kind: String,
    strand: String,
    strand_name: String,
    stage: String,
    subject: String,
    advanced: bool,
    recurring: bool,
    retired_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl ExistingItem {
    fn same_as(&self, r: &KnowledgeRow) -> bool {
        self.retired_at.is_none()
            && self.category == r.category
            && self.school_track == r.school_track
            && self.title == r.title
            && self.content == r.content
            && self.source_doc == r.source_doc
            && self.source_version == r.source_version
            && self.source_page == r.source_page
            && self.official_code == r.official_code
            && self.item_kind == r.item_kind
            && self.strand == r.strand
            && self.strand_name == r.strand_name
            && self.stage == r.stage
            && self.subject == r.subject
            && self.advanced == r.advanced
            && self.recurring == r.recurring
            // Without a source_sha256 column the stored hash is the CSV's own, which changes
            // with any edit to the file and so says nothing about this row.
            && (r.source_sha256.is_empty()
                || self.source_sha256.eq_ignore_ascii_case(&r.source_sha256))
    }
}

#[derive(Debug, Default, Deserialize)]
pub struct ImportQuery {
    /// `true`: validate and count only; nothing is written.
    #[serde(default)]
    pub dry_run: bool,
}

/// What an import did (or, for a dry run, would do).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ImportCounts {
    pub inserted: usize,
    pub updated: usize,
    pub unchanged: usize,
}

impl ImportCounts {
    fn changes(&self) -> usize {
        self.inserted + self.updated
    }
}

fn import_response(
    rows: usize,
    counts: ImportCounts,
    by: (&str, BTreeMap<String, usize>),
    sha: &str,
    dry_run: bool,
    batch_id: Option<i64>,
) -> Json<Value> {
    Json(json!({
        "dry_run": dry_run,
        // Kept from Phase 1: rows accepted from the file (0 for a dry run).
        "imported": if dry_run { 0 } else { rows },
        "rows": rows,
        "inserted": counts.inserted,
        "updated": counts.updated,
        "unchanged": counts.unchanged,
        by.0: by.1,
        "sha256": sha,
        "batch_id": batch_id,
    }))
}

/// Serializes imports of one kind and records the batch; `None` when nothing changes.
async fn open_batch(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    kind: &str,
    sha: &str,
    rows: usize,
    counts: ImportCounts,
    teacher_id: i64,
) -> AppResult<Option<i64>> {
    if counts.changes() == 0 {
        return Ok(None);
    }
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO import_batches
           (kind, file_sha256, row_count, inserted, updated, unchanged, imported_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
    )
    .bind(kind)
    .bind(sha)
    .bind(rows as i32)
    .bind(counts.inserted as i32)
    .bind(counts.updated as i32)
    .bind(counts.unchanged as i32)
    .bind(teacher_id)
    .fetch_one(&mut **tx)
    .await?;
    Ok(Some(id))
}

async fn lock_import(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, kind: &str) -> AppResult<()> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
        .bind(format!("import:{kind}"))
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Imports knowledge items (curriculum items included). Idempotent: a row identical to the
/// latest active version of its item_code is left alone, a changed row becomes a new version
/// (older versions are retired), a new code is inserted. Rows absent from the file are not
/// touched. All-or-nothing; `?dry_run=true` only reports the counts.
pub async fn import_knowledge(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiQuery(q): ApiQuery<ImportQuery>,
    body: Bytes,
) -> AppResult<Json<Value>> {
    teacher.require(Capability::ManageKnowledge)?;
    let sha = auth::sha256_hex(&body);
    let rows = parse_knowledge_csv(&body).map_err(AppError::BadRequest)?;
    let mut by_track: BTreeMap<String, usize> = BTreeMap::new();
    for r in &rows {
        *by_track.entry(r.school_track.clone()).or_default() += 1;
    }

    let mut tx = state.db.begin().await?;
    lock_import(&mut tx, "knowledge").await?;
    let codes: Vec<&str> = rows.iter().map(|r| r.item_code.as_str()).collect();
    let existing: Vec<ExistingItem> = sqlx::query_as(
        "SELECT DISTINCT ON (item_code)
                item_code, version, category, school_track, title, content, source_doc,
                source_version, source_page, source_sha256, official_code, item_kind, strand,
                strand_name, stage, subject, advanced, recurring, retired_at
           FROM knowledge_items WHERE item_code = ANY($1)
          ORDER BY item_code, version DESC",
    )
    .bind(&codes)
    .fetch_all(&mut *tx)
    .await?;
    let existing: HashMap<&str, &ExistingItem> =
        existing.iter().map(|e| (e.item_code.as_str(), e)).collect();

    let mut counts = ImportCounts::default();
    let mut todo: Vec<(&KnowledgeRow, i32)> = Vec::new();
    for r in &rows {
        match existing.get(r.item_code.as_str()) {
            Some(e) if e.same_as(r) => counts.unchanged += 1,
            Some(e) => {
                counts.updated += 1;
                todo.push((r, e.version + 1));
            }
            None => {
                counts.inserted += 1;
                todo.push((r, 1));
            }
        }
    }
    if q.dry_run {
        return Ok(import_response(
            rows.len(),
            counts,
            ("by_track", by_track),
            &sha,
            true,
            None,
        ));
    }

    let batch_id = open_batch(&mut tx, "knowledge", &sha, rows.len(), counts, teacher.id()).await?;
    let changed: Vec<&str> = todo.iter().map(|(r, _)| r.item_code.as_str()).collect();
    sqlx::query(
        "UPDATE knowledge_items SET retired_at = now()
          WHERE item_code = ANY($1) AND retired_at IS NULL",
    )
    .bind(&changed)
    .execute(&mut *tx)
    .await?;
    for (r, version) in todo {
        sqlx::query(
            "INSERT INTO knowledge_items
               (item_code, version, category, school_track, title, content, source_doc,
                source_version, source_page, source_sha256, approved_by, approved_at,
                official_code, item_kind, strand, strand_name, stage, subject, advanced,
                recurring, import_batch_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, now(),
                     $12, $13, $14, $15, $16, $17, $18, $19, $20)",
        )
        .bind(&r.item_code)
        .bind(version)
        .bind(&r.category)
        .bind(&r.school_track)
        .bind(&r.title)
        .bind(&r.content)
        .bind(&r.source_doc)
        .bind(&r.source_version)
        .bind(&r.source_page)
        .bind(if r.source_sha256.is_empty() {
            sha.clone()
        } else {
            r.source_sha256.to_ascii_lowercase()
        })
        .bind(&teacher.0.research_id)
        .bind(&r.official_code)
        .bind(&r.item_kind)
        .bind(&r.strand)
        .bind(&r.strand_name)
        .bind(&r.stage)
        .bind(&r.subject)
        .bind(r.advanced)
        .bind(r.recurring)
        .bind(batch_id)
        .execute(&mut *tx)
        .await?;
    }
    log_usage(
        &mut *tx,
        teacher.id(),
        "knowledge_imported",
        json!({ "batch_id": batch_id, "sha256": sha, "rows": rows.len(),
                "inserted": counts.inserted, "updated": counts.updated,
                "unchanged": counts.unchanged }),
    )
    .await?;
    tx.commit().await?;
    // Embed the new rows in the background, after the commit: the import never waits for (or
    // fails because of) the embedding provider.
    if counts.changes() > 0 {
        crate::kb_embeddings::spawn_sync(&state, "import", Some(teacher.id()), false);
    }
    Ok(import_response(
        rows.len(),
        counts,
        ("by_track", by_track),
        &sha,
        false,
        batch_id,
    ))
}

#[derive(sqlx::FromRow)]
struct ExistingWord {
    id: i64,
    list_version: String,
    word: String,
    headword: String,
    pos: String,
    kind: String,
    level: Option<i16>,
    appendix_category: String,
    source_doc: String,
    source_page: String,
    source_sha256: String,
    version: i32,
    retired_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl ExistingWord {
    fn same_as(&self, r: &VocabularyRow) -> bool {
        self.retired_at.is_none()
            && self.level == r.level
            && self.source_doc == r.source_doc
            && self.source_page == r.source_page
            && self.source_sha256.eq_ignore_ascii_case(&r.source_sha256)
    }
}

/// Imports the vocabulary list (one row per matchable word form). Same idempotency rules as the
/// knowledge import, keyed by (list_version, word, headword, pos, kind, appendix_category).
pub async fn import_vocabulary(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiQuery(q): ApiQuery<ImportQuery>,
    body: Bytes,
) -> AppResult<Json<Value>> {
    teacher.require(Capability::ManageKnowledge)?;
    let sha = auth::sha256_hex(&body);
    let rows = parse_vocabulary_csv(&body).map_err(AppError::BadRequest)?;
    let mut by_level: BTreeMap<String, usize> = BTreeMap::new();
    for r in &rows {
        let key = r
            .level
            .map_or_else(|| "appendix".to_string(), |l| l.to_string());
        *by_level.entry(key).or_default() += 1;
    }

    let mut tx = state.db.begin().await?;
    lock_import(&mut tx, "vocabulary").await?;
    let versions: Vec<&str> = rows
        .iter()
        .map(|r| r.list_version.as_str())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let existing: Vec<ExistingWord> = sqlx::query_as(
        "SELECT DISTINCT ON (list_version, word, headword, pos, kind, appendix_category)
                id, list_version, word, headword, pos, kind, level, appendix_category,
                source_doc, source_page, source_sha256, version, retired_at
           FROM vocabulary_items WHERE list_version = ANY($1)
          ORDER BY list_version, word, headword, pos, kind, appendix_category, version DESC",
    )
    .bind(&versions)
    .fetch_all(&mut *tx)
    .await?;
    let existing: HashMap<VocabularyKey, &ExistingWord> = existing
        .iter()
        .map(|e| {
            (
                (
                    e.list_version.clone(),
                    e.word.clone(),
                    e.headword.clone(),
                    e.pos.clone(),
                    e.kind.clone(),
                    e.appendix_category.clone(),
                ),
                e,
            )
        })
        .collect();

    let mut counts = ImportCounts::default();
    let mut retire: Vec<i64> = Vec::new();
    let mut todo: Vec<(&VocabularyRow, i32)> = Vec::new();
    for r in &rows {
        match existing.get(&r.key()) {
            Some(e) if e.same_as(r) => counts.unchanged += 1,
            Some(e) => {
                counts.updated += 1;
                retire.push(e.id);
                todo.push((r, e.version + 1));
            }
            None => {
                counts.inserted += 1;
                todo.push((r, 1));
            }
        }
    }
    if q.dry_run {
        return Ok(import_response(
            rows.len(),
            counts,
            ("by_level", by_level),
            &sha,
            true,
            None,
        ));
    }

    let batch_id = open_batch(
        &mut tx,
        "vocabulary",
        &sha,
        rows.len(),
        counts,
        teacher.id(),
    )
    .await?;
    sqlx::query(
        "UPDATE vocabulary_items SET retired_at = now() WHERE id = ANY($1) AND retired_at IS NULL",
    )
    .bind(&retire)
    .execute(&mut *tx)
    .await?;
    // One statement per chunk: the list has several thousand rows.
    for chunk in todo.chunks(1000) {
        let col = |f: fn(&VocabularyRow) -> &str| -> Vec<&str> {
            chunk.iter().map(|(r, _)| f(r)).collect()
        };
        sqlx::query(
            "INSERT INTO vocabulary_items
               (word, headword, level, pos, kind, appendix_category, list_version, source_doc,
                source_page, source_sha256, version, import_batch_id)
             SELECT u.word, u.headword, u.level, u.pos, u.kind, u.appendix_category,
                    u.list_version, u.source_doc, u.source_page, lower(u.source_sha256),
                    u.version, $12
               FROM UNNEST($1::text[], $2::text[], $3::smallint[], $4::text[], $5::text[],
                           $6::text[], $7::text[], $8::text[], $9::text[], $10::text[],
                           $11::int[])
                    AS u(word, headword, level, pos, kind, appendix_category, list_version,
                         source_doc, source_page, source_sha256, version)",
        )
        .bind(col(|r| &r.word))
        .bind(col(|r| &r.headword))
        .bind(chunk.iter().map(|(r, _)| r.level).collect::<Vec<_>>())
        .bind(col(|r| &r.pos))
        .bind(col(|r| &r.kind))
        .bind(col(|r| &r.appendix_category))
        .bind(col(|r| &r.list_version))
        .bind(col(|r| &r.source_doc))
        .bind(col(|r| &r.source_page))
        .bind(col(|r| &r.source_sha256))
        .bind(chunk.iter().map(|(_, v)| *v).collect::<Vec<_>>())
        .bind(batch_id)
        .execute(&mut *tx)
        .await?;
    }
    log_usage(
        &mut *tx,
        teacher.id(),
        "vocabulary_imported",
        json!({ "batch_id": batch_id, "sha256": sha, "rows": rows.len(),
                "inserted": counts.inserted, "updated": counts.updated,
                "unchanged": counts.unchanged }),
    )
    .await?;
    tx.commit().await?;
    Ok(import_response(
        rows.len(),
        counts,
        ("by_level", by_level),
        &sha,
        false,
        batch_id,
    ))
}

// ---------------------------------------------------------------- knowledge-base status

#[derive(serde::Serialize, sqlx::FromRow)]
struct KnowledgeGroup {
    school_track: String,
    category: String,
    item_kind: String,
    demo: bool,
    count: i64,
}

#[derive(serde::Serialize, sqlx::FromRow)]
struct VocabularyGroup {
    list_version: String,
    level: Option<i16>,
    forms: i64,
    entries: i64,
}

#[derive(serde::Serialize, sqlx::FromRow)]
struct BatchRow {
    id: i64,
    kind: String,
    file_sha256: String,
    row_count: i32,
    inserted: i32,
    updated: i32,
    unchanged: i32,
    imported_by: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
}

/// What is currently active in the knowledge base and the vocabulary list.
pub async fn knowledge_summary(
    State(state): State<AppState>,
    teacher: AuthTeacher,
) -> AppResult<Json<Value>> {
    teacher.require(Capability::ManageKnowledge)?;
    let groups: Vec<KnowledgeGroup> = sqlx::query_as(
        "SELECT school_track, category, item_kind, item_code LIKE 'DEMO-%' AS demo,
                COUNT(*) AS count
           FROM knowledge_items WHERE retired_at IS NULL
          GROUP BY 1, 2, 3, 4 ORDER BY 4, 1, 2, 3",
    )
    .fetch_all(&state.db)
    .await?;
    let vocabulary: Vec<VocabularyGroup> = sqlx::query_as(
        "SELECT list_version, level, COUNT(*) AS forms,
                COUNT(DISTINCT headword) AS entries
           FROM vocabulary_items WHERE retired_at IS NULL
          GROUP BY 1, 2 ORDER BY 1, 2 NULLS LAST",
    )
    .fetch_all(&state.db)
    .await?;
    let batches: Vec<BatchRow> = sqlx::query_as(
        "SELECT b.id, b.kind, b.file_sha256, b.row_count, b.inserted, b.updated, b.unchanged,
                t.research_id AS imported_by, b.created_at
           FROM import_batches b LEFT JOIN teachers t ON t.id = b.imported_by
          ORDER BY b.id DESC LIMIT 10",
    )
    .fetch_all(&state.db)
    .await?;
    let total = |demo: bool| -> i64 {
        groups
            .iter()
            .filter(|g| g.demo == demo)
            .map(|g| g.count)
            .sum()
    };
    Ok(Json(json!({
        "knowledge": {
            "active": total(false) + total(true),
            "real_active": total(false),
            "demo_active": total(true),
            "groups": groups,
        },
        "vocabulary": {
            "active_forms": vocabulary.iter().map(|v| v.forms).sum::<i64>(),
            "groups": vocabulary,
        },
        "recent_batches": batches,
    })))
}

/// Retires the seeded `DEMO-*` knowledge items (they are never deleted). Explicit and PI-only
/// rather than automatic: the demo set also holds grammar / vocabulary-teaching / SIOP notes
/// that the curriculum import does not replace, so importing curriculum items must not silently
/// change what plan suggestions can cite. Refused while no real item is active, which would
/// leave the knowledge base empty. `?dry_run=true` only reports the counts.
pub async fn retire_demo_knowledge(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiQuery(q): ApiQuery<ImportQuery>,
) -> AppResult<Json<Value>> {
    teacher.require(Capability::ManageKnowledge)?;
    let mut tx = state.db.begin().await?;
    lock_import(&mut tx, "knowledge").await?;
    let demo: Vec<(String, i64)> = sqlx::query_as(
        "SELECT category, COUNT(*) FROM knowledge_items
          WHERE retired_at IS NULL AND item_code LIKE 'DEMO-%' GROUP BY 1 ORDER BY 1",
    )
    .fetch_all(&mut *tx)
    .await?;
    let real: Vec<(String, i64)> = sqlx::query_as(
        "SELECT category, COUNT(*) FROM knowledge_items
          WHERE retired_at IS NULL AND item_code NOT LIKE 'DEMO-%' GROUP BY 1 ORDER BY 1",
    )
    .fetch_all(&mut *tx)
    .await?;
    let demo_total: i64 = demo.iter().map(|(_, n)| n).sum();
    let real_total: i64 = real.iter().map(|(_, n)| n).sum();
    // Categories that will have no active item left once the demo items are retired.
    let emptied: Vec<&str> = demo
        .iter()
        .map(|(c, _)| c.as_str())
        .filter(|c| !real.iter().any(|(rc, _)| rc == c))
        .collect();
    let body = |retired: i64| {
        json!({
            "dry_run": q.dry_run,
            "demo_active": demo_total,
            "demo_by_category": demo.iter().cloned().collect::<BTreeMap<_, _>>(),
            "real_active": real_total,
            "categories_left_empty": emptied,
            "retired": retired,
        })
    };
    if q.dry_run {
        return Ok(Json(body(0)));
    }
    if real_total == 0 {
        return Err(AppError::Conflict(
            "知識庫還沒有正式條目；請先匯入正式條目，再停用示範條目。".into(),
        ));
    }
    let retired = sqlx::query(
        "UPDATE knowledge_items SET retired_at = now()
          WHERE retired_at IS NULL AND item_code LIKE 'DEMO-%'",
    )
    .execute(&mut *tx)
    .await?
    .rows_affected() as i64;
    log_usage(
        &mut *tx,
        teacher.id(),
        "demo_knowledge_retired",
        json!({ "retired": retired,
                "by_category": demo.iter().cloned().collect::<BTreeMap<_, _>>() }),
    )
    .await?;
    let out = body(retired);
    tx.commit().await?;
    Ok(Json(out))
}

/// Imports teacher cases (upsert by case_code). `in_rag` is never set here: whether cases may
/// enter RAG awaits the PI's decision (BLUEPRINT §8).
pub async fn import_teacher_cases(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    body: Bytes,
) -> AppResult<Json<Value>> {
    teacher.require_pi()?;
    let sha = auth::sha256_hex(&body);
    let rows = parse_cases_csv(&body).map_err(AppError::BadRequest)?;
    let mut tx = state.db.begin().await?;
    for r in &rows {
        sqlx::query(
            "INSERT INTO teacher_cases
               (case_code, school_track, grade, topic, summary, details, source_sha256, approved)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (case_code) DO UPDATE
               SET school_track = EXCLUDED.school_track, grade = EXCLUDED.grade,
                   topic = EXCLUDED.topic, summary = EXCLUDED.summary, details = EXCLUDED.details,
                   source_sha256 = EXCLUDED.source_sha256, approved = EXCLUDED.approved",
        )
        .bind(&r.case_code)
        .bind(&r.school_track)
        .bind(&r.grade)
        .bind(&r.topic)
        .bind(&r.summary)
        .bind(&r.details)
        .bind(&sha)
        .bind(r.approved)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(Json(json!({ "imported": rows.len(), "sha256": sha })))
}

#[cfg(test)]
mod tests {
    use super::*;

    const KB: &str =
        "item_code,category,school_track,title,content,source_doc,source_version,source_page\n\
        T-1,grammar,both,倒裝句,\"Not only, but also\",doc,v1,p.3\n\
        T-2,vocab,general,環境詞彙,pollution,,,\n";

    #[test]
    fn parses_knowledge_csv_with_bom_and_quotes() {
        let mut bytes = b"\xEF\xBB\xBF".to_vec();
        bytes.extend_from_slice(KB.as_bytes());
        let rows = parse_knowledge_csv(&bytes).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].content, "Not only, but also");
        assert_eq!(rows[1].source_doc, "");
    }

    #[test]
    fn knowledge_csv_errors() {
        assert!(parse_knowledge_csv(b"item_code,title\nA,B\n")
            .unwrap_err()
            .contains("缺少欄位"));
        let bad_track = KB.replace("T-2,vocab,general", "T-2,vocab,college");
        assert!(parse_knowledge_csv(bad_track.as_bytes())
            .unwrap_err()
            .contains("第 3 列"));
        let dup = KB.replace("T-2,", "T-1,");
        assert!(parse_knowledge_csv(dup.as_bytes())
            .unwrap_err()
            .contains("重複"));
        let empty = KB.replace("倒裝句", "");
        assert!(parse_knowledge_csv(empty.as_bytes())
            .unwrap_err()
            .contains("title"));
        assert!(parse_knowledge_csv(&[0xff, 0xfe]).is_err());
    }

    #[test]
    fn parses_cases_csv() {
        let csv = "case_code,school_track,grade,topic,summary,details,approved\n\
                   C-1,both,高二,閱讀,摘要,細節,TRUE\n\
                   C-2,vocational,,口說,摘要二,,否\n";
        let rows = parse_cases_csv(csv.as_bytes()).unwrap();
        assert!(rows[0].approved);
        assert!(!rows[1].approved);
        let bad = csv.replace("TRUE", "maybe");
        assert!(parse_cases_csv(bad.as_bytes()).is_err());
    }

    #[test]
    fn prompt_keys() {
        assert!(valid_prompt_key("core_assist_l1"));
        assert!(!valid_prompt_key("Core Assist"));
        assert!(!valid_prompt_key(""));
    }
}
