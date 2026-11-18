//! 詞彙級別檢查 (the deterministic part of spec §9.6) and key-word levels from data.
//!
//! The analysed text is the student version of one level, one printed unit per line, so the
//! positions the vocabulary analysis reports (line, column) can be named in the teacher's words
//! (「C 活動 · 第 4 題」). No model is involved: levels, denominators, limits and the disclaimer
//! all come from `crate::vocabulary` (Phase 2.0). Every result is stored with the worksheet
//! version it was made from; once the worksheet has a newer version the result is stale.
//!
//! Each word above the target level also gets what the teacher needs to choose what to do with
//! it ([`OverLevelWord`]: 提供字義, 換字 or 不處理, see [`super::vocab_actions`]): where it is
//! printed and whether 換字 may change it there, whether the reading text has it too, whether
//! the lesson teaches it (字詞表), whether this version already glosses it, and whether the
//! teacher chose to leave it (kept from one check of the version to the next).

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::types::Json;
use sqlx::PgPool;

use super::assemble::{project, text_units, TextUnit};
use super::store::{self, Worksheet};
use super::{Audience, Content, Level};
use crate::error::{AppError, AppResult};
use crate::vocabulary::{self, Analysis, Lexicon};

/// Most words a check remembers as 不處理.
pub const MAX_KEPT: usize = 200;
/// Contexts kept per word (sent to the model with a suggestion request).
pub const MAX_CONTEXTS: usize = 3;

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

/// What the check suggests doing with a word above the target level.
pub const ACTION_GLOSS: &str = "gloss";
pub const ACTION_REPLACE: &str = "replace";
pub const ACTION_LEAVE: &str = "leave";

/// A word above the target level, with what the teacher needs to choose 提供字義, 換字 or
/// 不處理. Positions are in `analysis.above_target` (same `base`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OverLevelWord {
    pub base: String,
    pub level: u8,
    /// Parts of speech the word list prints for it (`v.`, `n./v.`); "" when none.
    pub pos: String,
    pub count: usize,
    /// The forms in the text, lower-cased, in order of first appearance.
    pub forms: Vec<String>,
    /// Occurrences 換字 would change (questions, options, instructions, supports …).
    pub replaceable: usize,
    /// Where the other occurrences are; 換字 leaves them as they are (「A 暖身 · 字詞表」「引號內」).
    pub fixed: Vec<String>,
    /// The reading text has it too (the text itself is never changed).
    pub in_material: bool,
    /// A key word the lesson teaches (字詞表).
    pub key_word: bool,
    /// This version's Glossary already explains it (its Chinese meaning).
    pub glossed: Option<String>,
    /// The teacher chose 不處理 for it.
    pub kept: bool,
    /// `gloss` | `replace` | `leave`
    pub suggested: String,
    /// Why, in the teacher's words.
    pub reason: String,
    /// Up to [`MAX_CONTEXTS`] of its contexts.
    pub contexts: Vec<String>,
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
    /// The words above the target level and what can be done with them; empty for a result
    /// stored before this existed (analyse again).
    pub words: Vec<OverLevelWord>,
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
        words: serde_json::from_value(result["words"].clone()).unwrap_or_default(),
    }
}

const CHECK_SELECT: &str = "SELECT c.id, c.level, c.target_level, c.result, c.created_at,
        v.version_no, v.id AS version_id
   FROM worksheet_checks c JOIN worksheet_versions v ON v.id = c.worksheet_version_id";

// ---------------------------------------------------------------- quotes

/// Whether the word at `start..end` (character offsets) stands inside quotation marks: a word
/// quoted in a question names the word itself or quotes the reading text (「What does
/// "abandon" mean?」), so 換字 leaves it.
pub fn quoted(chars: &[char], start: usize, end: usize) -> bool {
    let before = &chars[..start.min(chars.len())];
    let after = &chars[end.min(chars.len())..];
    let opens = before.iter().filter(|c| **c == '“').count();
    let closes = before.iter().filter(|c| **c == '”').count();
    if opens > closes && after.contains(&'”') {
        return true;
    }
    let straight = before.iter().filter(|c| **c == '"').count();
    if straight % 2 == 1 && after.contains(&'"') {
        return true;
    }
    let single_open = before.iter().filter(|c| **c == '‘').count();
    let single_close = before.iter().filter(|c| **c == '’').count();
    // ’ is also an apostrophe: only a ‘ … ’ pair right around the word counts.
    if single_open > single_close
        && before.last() == Some(&'‘')
        && matches!(after.first(), Some('’'))
    {
        return true;
    }
    // 'abandon': a straight single quote right before and right after the word.
    start > 0
        && before.last() == Some(&'\'')
        && after.first() == Some(&'\'')
        && (start < 2 || !chars[start - 2].is_alphanumeric())
}

// ---------------------------------------------------------------- the words

/// The reasons an occurrence is left as it is by 換字, in the teacher's words.
fn fixed_reason(unit: &TextUnit) -> String {
    use super::assemble::UnitKind::*;
    let what = match unit.kind {
        Title => "標題",
        Objective => "學習目標",
        KeyWord => "字詞表（這節課要教的字）",
        SimplifiedText => "調整文本（改寫的課文）",
        WordBank => "字庫（作答用的字）",
        Caption => "圖說",
        OrganizerContent => "圖像組織的中心詞或預填答案",
        _ => return format!("{}（引號內）", unit.place),
    };
    format!("{}：{what}", unit.place)
}

/// The base words of `text` above `target` (lower-cased).
fn bases_above(text: &str, lex: &Lexicon, target: u8) -> BTreeSet<String> {
    if text.trim().is_empty() {
        return BTreeSet::new();
    }
    vocabulary::analyze(text, lex, target, &[])
        .above_target
        .into_iter()
        .map(|a| a.base)
        .collect()
}

/// Whether `word` (a key word, a glossed word) is `base`, by the same lookup the analysis uses.
fn same_word(lex: &Lexicon, word: &str, base: &str) -> bool {
    let w = word.trim().to_lowercase();
    w == base || vocabulary::word_level(lex, &w).base.as_deref() == Some(base)
}

/// What the check says about each word above the target level.
pub fn over_level_words(
    analysis: &Analysis,
    units: &[TextUnit],
    content: &Content,
    level: Level,
    material_text: &str,
    lex: &Lexicon,
    kept: &BTreeSet<String>,
) -> Vec<OverLevelWord> {
    let in_material = bases_above(material_text, lex, analysis.target_level);
    let key_words: Vec<&str> = content
        .blocks
        .iter()
        .filter(|b| b.enabled)
        .flat_map(|b| b.vocabulary.iter().map(|v| v.word.as_str()))
        .collect();
    let glosses: Vec<&super::Gloss> = content
        .blocks
        .iter()
        .filter(|b| b.enabled)
        .flat_map(|b| b.glosses.iter())
        .filter(|g| g.levels.contains(&level))
        .collect();
    analysis
        .above_target
        .iter()
        .map(|a| {
            let mut forms: Vec<String> = Vec::new();
            let mut replaceable = 0;
            let mut fixed: Vec<String> = Vec::new();
            let mut contexts: Vec<String> = Vec::new();
            for p in &a.positions {
                let form = p.form.to_lowercase();
                if !forms.contains(&form) {
                    forms.push(form);
                }
                if contexts.len() < MAX_CONTEXTS && !contexts.contains(&p.context) {
                    contexts.push(p.context.clone());
                }
                let Some(unit) = units.get(p.line.saturating_sub(1)) else {
                    continue;
                };
                let chars: Vec<char> = unit.text.chars().collect();
                let start = p.col.saturating_sub(1);
                let end = start + p.form.chars().count();
                if unit.kind.replaceable() && !quoted(&chars, start, end) {
                    replaceable += 1;
                } else {
                    let reason = fixed_reason(unit);
                    if !fixed.contains(&reason) {
                        fixed.push(reason);
                    }
                }
            }
            let key_word = key_words.iter().any(|w| same_word(lex, w, &a.base));
            let glossed = glosses
                .iter()
                .find(|g| same_word(lex, &g.word, &a.base))
                .map(|g| g.meaning_zh.clone());
            let in_material = in_material.contains(&a.base);
            let is_kept = kept.contains(&a.base);
            let (suggested, reason) = if is_kept {
                (ACTION_LEAVE, "你之前選了不處理。")
            } else if key_word {
                (ACTION_LEAVE, "字詞表裡的字：這節課要教，通常保留。")
            } else if glossed.is_some() {
                (ACTION_GLOSS, "這個版本已經附上字義。")
            } else if in_material {
                (
                    ACTION_GLOSS,
                    "課文裡也有這個字；課文不會改，附上字義學生才對得起來。",
                )
            } else if replaceable == 0 {
                (ACTION_GLOSS, "出現的地方都不適合換字。")
            } else {
                (ACTION_REPLACE, "只出現在題目或指示裡，可以換成較簡單的字。")
            };
            OverLevelWord {
                base: a.base.clone(),
                level: a.level,
                pos: lex.listed_pos(&a.base),
                count: a.count,
                forms,
                replaceable,
                fixed,
                in_material,
                key_word,
                glossed,
                kept: is_kept,
                suggested: suggested.to_string(),
                reason: reason.to_string(),
                contexts,
            }
        })
        .collect()
}

/// The words the teacher chose to leave in the version's latest check.
async fn kept_words(db: &PgPool, worksheet_id: i64, level: Level) -> sqlx::Result<Vec<String>> {
    let result: Option<Json<Value>> = sqlx::query_scalar(
        "SELECT result FROM worksheet_checks
          WHERE worksheet_id = $1 AND kind = 'vocabulary' AND level = $2
          ORDER BY id DESC LIMIT 1",
    )
    .bind(worksheet_id)
    .bind(level.as_str())
    .fetch_optional(db)
    .await?;
    Ok(result
        .and_then(|r| serde_json::from_value(r.0["kept"].clone()).ok())
        .unwrap_or_default())
}

/// Analyses the student version of `level` as it is now and stores the result.
pub async fn run(
    db: &PgPool,
    worksheet: &Worksheet,
    level: Level,
    target_level: u8,
) -> AppResult<VocabCheck> {
    run_with(db, worksheet, level, target_level, &[], &[]).await
}

/// [`run`], with words the teacher has just chosen to leave (`keep`) or to handle (`unkeep`).
pub async fn run_with(
    db: &PgPool,
    worksheet: &Worksheet,
    level: Level,
    target_level: u8,
    keep: &[String],
    unkeep: &[String],
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
    let units = text_units(&sheet);
    let text = units
        .iter()
        .map(|u| u.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if text.trim().is_empty() {
        return Err(AppError::bad_request("學習單還沒有內容可以分析。"));
    }
    let analysis = vocabulary::analyze(&text, &lexicon, target_level, &[]);
    let mut kept: BTreeSet<String> = kept_words(db, worksheet.id, level)
        .await?
        .into_iter()
        .chain(keep.iter().cloned())
        .collect();
    for w in unkeep {
        kept.remove(w);
    }
    // Only words still above the target level are remembered.
    kept.retain(|w| analysis.above_target.iter().any(|a| &a.base == w));
    let kept: BTreeSet<String> = kept.into_iter().take(MAX_KEPT).collect();
    let words = over_level_words(
        &analysis,
        &units,
        &latest.content,
        level,
        &worksheet.material.text(),
        &lexicon,
        &kept,
    );
    let places: Vec<String> = units.into_iter().map(|u| u.place).collect();
    let result = serde_json::json!({
        "analysis": analysis, "lines": places, "words": words, "kept": kept,
    });
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

/// One stored check of this worksheet, with whether it is about the latest version.
pub async fn get(db: &PgPool, worksheet_id: i64, check_id: i64) -> AppResult<VocabCheck> {
    let latest = store::latest(db, worksheet_id).await?;
    let row: Option<CheckRow> = sqlx::query_as(&format!(
        "{CHECK_SELECT} WHERE c.id = $1 AND c.worksheet_id = $2 AND c.kind = 'vocabulary'"
    ))
    .bind(check_id)
    .bind(worksheet_id)
    .fetch_optional(db)
    .await?;
    row.map(|r| into_check(r, latest.id))
        .ok_or_else(|| AppError::not_found("找不到這次的詞彙級別檢查。"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(text: &str, word: &str) -> bool {
        let chars: Vec<char> = text.chars().collect();
        let start = text.find(word).map(|b| text[..b].chars().count()).unwrap();
        quoted(&chars, start, start + word.chars().count())
    }

    #[test]
    fn a_quoted_word_is_left_alone() {
        assert!(q("What does \"abandon\" mean here?", "abandon"));
        assert!(q("What does “abandon” mean here?", "abandon"));
        assert!(q("What does ‘abandon’ mean here?", "abandon"));
        assert!(q("What does 'abandon' mean here?", "abandon"));
        assert!(q("The text says “they abandon the plan”.", "abandon"));
        assert!(!q("Why did they abandon the plan?", "abandon"));
        assert!(!q("“Hi,” she said. Why did they abandon it?", "abandon"));
        assert!(!q("It's the family's choice to abandon it.", "abandon"));
    }
}
