//! 詞彙級別檢查 (the deterministic part of spec §9.6) and key-word levels from data.
//!
//! The analysed text is the student version of one level, one printed unit per line, so the
//! positions the vocabulary analysis reports (line, column) can be named in the teacher's words
//! (「C 活動 · 第 4 題」). No model is involved: levels, denominators, limits and the disclaimer
//! all come from `crate::vocabulary` (Phase 2.0). Every result is stored with the worksheet
//! version it was made from; once the worksheet has a newer version the result is stale.

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::types::Json;
use sqlx::PgPool;

use super::assemble::{project, text_lines};
use super::store::{self, Worksheet};
use super::{Audience, Content, Level};
use crate::error::{AppError, AppResult};
use crate::vocabulary::{self, Lexicon};

/// Puts the word-list level on every key word of the warm-up. A level never comes from the
/// model or from the client; without a list (or for an unlisted word) it is `None`.
pub fn relookup_levels(content: &mut Content, lexicon: Option<&Lexicon>) {
    for block in &mut content.blocks {
        for v in &mut block.vocabulary {
            v.level = lexicon.and_then(|lex| vocabulary::word_level(lex, v.word.trim()).level);
        }
    }
}

pub async fn lexicon(db: &PgPool) -> sqlx::Result<Option<Lexicon>> {
    Ok(vocabulary::load_lexicon(db, None)
        .await?
        .filter(|l| !l.is_empty()))
}

#[derive(Debug, Serialize)]
pub struct VocabCheck {
    pub id: i64,
    pub level: Level,
    pub audience: Audience,
    pub target_level: i32,
    pub version_no: i32,
    pub checked_at: DateTime<Utc>,
    pub stale: bool,
    pub analysis: Value,
    pub lines: Vec<String>,
}

#[derive(sqlx::FromRow)]
struct CheckRow {
    id: i64,
    level: String,
    target_level: i32,
    result: Json<Value>,
    created_at: DateTime<Utc>,
    version_no: i32,
    version_id: i64,
}

fn into_check(row: CheckRow, latest_version_id: i64) -> VocabCheck {
    let result = row.result.0;
    VocabCheck {
        id: row.id,
        level: Level::parse(&row.level).unwrap_or(Level::Standard),
        audience: Audience::Student,
        target_level: row.target_level,
        version_no: row.version_no,
        checked_at: row.created_at,
        stale: row.version_id != latest_version_id,
        analysis: result["analysis"].clone(),
        lines: serde_json::from_value(result["lines"].clone()).unwrap_or_default(),
    }
}

const CHECK_SELECT: &str = "SELECT c.id, c.level, c.target_level, c.result, c.created_at,
        v.version_no, v.id AS version_id
   FROM worksheet_checks c JOIN worksheet_versions v ON v.id = c.worksheet_version_id";

/// Analyses the student version of `level` as it is now and stores the result.
pub async fn run(
    db: &PgPool,
    worksheet: &Worksheet,
    level: Level,
    target_level: u8,
) -> AppResult<VocabCheck> {
    let latest = store::latest(db, worksheet.id).await?;
    if !latest.content.levels.contains(&level) {
        return Err(AppError::bad_request(format!(
            "這份學習單沒有{}。",
            level.label()
        )));
    }
    let lexicon = lexicon(db).await?.ok_or_else(|| {
        AppError::Conflict("系統尚未匯入大考中心參考詞彙表，無法分析詞彙級別。".to_string())
    })?;
    let sheet = project(&worksheet.title, &latest.content, level, Audience::Student);
    let lines = text_lines(&sheet);
    let text = lines
        .iter()
        .map(|(_, t)| t.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if text.trim().is_empty() {
        return Err(AppError::bad_request("學習單還沒有內容可以分析。"));
    }
    let analysis = vocabulary::analyze(&text, &lexicon, target_level, &[]);
    let places: Vec<String> = lines.into_iter().map(|(place, _)| place).collect();
    let result = serde_json::json!({ "analysis": analysis, "lines": places });
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO worksheet_checks (worksheet_id, worksheet_version_id, kind, level, target_level, result)
         VALUES ($1, $2, 'vocabulary', $3, $4, $5) RETURNING id",
    )
    .bind(worksheet.id)
    .bind(latest.id)
    .bind(level.as_str())
    .bind(i32::from(target_level))
    .bind(Json(&result))
    .fetch_one(db)
    .await?;
    let row: CheckRow = sqlx::query_as(&format!("{CHECK_SELECT} WHERE c.id = $1"))
        .bind(id)
        .fetch_one(db)
        .await?;
    Ok(into_check(row, latest.id))
}

/// The most recent result of each level.
pub async fn latest(db: &PgPool, worksheet_id: i64) -> AppResult<Vec<VocabCheck>> {
    let latest = store::latest(db, worksheet_id).await?;
    let rows: Vec<CheckRow> = sqlx::query_as(&format!(
        "SELECT * FROM ({CHECK_SELECT} WHERE c.worksheet_id = $1 AND c.kind = 'vocabulary') x
          WHERE x.id IN (SELECT MAX(id) FROM worksheet_checks WHERE worksheet_id = $1 GROUP BY level)
          ORDER BY x.id DESC"
    ))
    .bind(worksheet_id)
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(|r| into_check(r, latest.id)).collect())
}
