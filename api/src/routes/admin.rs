//! PI-only endpoints: invites, prompt versions, knowledge-base and teacher-case CSV imports.

use std::collections::HashSet;

use axum::body::Bytes;
use axum::extract::State;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::{self, AuthTeacher};
use crate::error::{ApiJson, AppError, AppResult};
use crate::prompts::PromptVersion;
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
            "學校類型需為 general 或 vocational。",
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

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
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
        if !matches!(r.school_track.as_str(), "general" | "vocational" | "both") {
            return Err(format!(
                "第 {line} 列的 school_track 需為 general、vocational 或 both。"
            ));
        }
        if !codes.insert(r.item_code.clone()) {
            return Err(format!("第 {line} 列的 item_code {} 重複。", r.item_code));
        }
    }
    Ok(rows)
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
            if !matches!(r.school_track.as_str(), "general" | "vocational" | "both") {
                return Err(format!(
                    "第 {line} 列的 school_track 需為 general、vocational 或 both。"
                ));
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
    version: i32,
    category: String,
    school_track: String,
    title: String,
    content: String,
    source_doc: String,
    source_version: String,
    source_page: String,
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
    }
}

/// Imports knowledge items. A changed row becomes a new version of its item_code (older
/// versions are retired); unchanged rows are left as they are. All-or-nothing.
pub async fn import_knowledge(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    body: Bytes,
) -> AppResult<Json<Value>> {
    teacher.require_pi()?;
    let sha = auth::sha256_hex(&body);
    let rows = parse_knowledge_csv(&body).map_err(AppError::BadRequest)?;
    let mut tx = state.db.begin().await?;
    for r in &rows {
        let existing: Option<ExistingItem> = sqlx::query_as(
            "SELECT version, category, school_track, title, content, source_doc, source_version,
                    source_page, retired_at
               FROM knowledge_items WHERE item_code = $1 ORDER BY version DESC LIMIT 1
                FOR UPDATE",
        )
        .bind(&r.item_code)
        .fetch_optional(&mut *tx)
        .await?;
        if existing.as_ref().is_some_and(|e| e.same_as(r)) {
            continue;
        }
        let version = existing.map(|e| e.version + 1).unwrap_or(1);
        sqlx::query("UPDATE knowledge_items SET retired_at = now() WHERE item_code = $1 AND retired_at IS NULL")
            .bind(&r.item_code)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO knowledge_items
               (item_code, version, category, school_track, title, content, source_doc,
                source_version, source_page, source_sha256, approved_by, approved_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, now())",
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
        .bind(&sha)
        .bind(&teacher.0.research_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(Json(json!({ "imported": rows.len(), "sha256": sha })))
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
