//! 學習單註記: the teacher's notes on a generated worksheet (docs/WORKSHEET_BLOCK_EDIT_RESEARCH.md
//! §5.1; contract: docs/API.md「學習單：逐區修改與註記」).
//!
//! A note never changes the worksheet and is never part of a projection, so it is never printed
//! or exported. Like a note on a material it remembers where it was made (block, field, item and
//! UTF-16 offsets inside the field's text as the sheet shows it, trimmed) and what was selected
//! (the quote plus a little text on both sides). Where it is now is worked out on every read
//! ([`resolve`]): item ids change when a block is regenerated, so a note on an item that is gone
//! is looked for again by its quote in the other items of the block.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use super::{Block, Content, Item};
use crate::error::{AppError, AppResult};
use crate::materials::annotations::{
    byte_index, context_after, context_before, utf16_len, utf16_slice, AnchorStatus,
};
use crate::routes::clip_chars;

pub const MAX_NOTE_CHARS: usize = 2_000;
pub const MAX_QUOTE_CHARS: usize = 500;
pub const MAX_COMMENTS_PER_WORKSHEET: i64 = 500;

pub const STATUS_OPEN: &str = "open";
pub const STATUS_APPLIED: &str = "applied";
pub const STATUS_DISMISSED: &str = "dismissed";

/// What a note is attached to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Field {
    /// The whole block (「這一區」).
    Block,
    Title,
    Instructions,
    /// One question (`item_id`).
    Item,
}

impl Field {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Block => "block",
            Self::Title => "title",
            Self::Instructions => "instructions",
            Self::Item => "item",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "title" => Self::Title,
            "instructions" => Self::Instructions,
            "item" => Self::Item,
            _ => Self::Block,
        }
    }
}

// ---------------------------------------------------------------- anchoring

/// What is stored with a note to find its place again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredAnchor {
    pub block_id: String,
    pub field: Field,
    pub item_id: Option<String>,
    /// UTF-16 offsets inside the field's text; `None` for the whole field.
    pub range: Option<(usize, usize)>,
    pub quote: String,
    pub prefix: String,
    pub suffix: String,
}

/// Where a note is in the worksheet as it is now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommentAnchor {
    /// `ok`: where it was made; `moved`: its block or question is there but the text changed or
    /// the quote is somewhere else now; `lost`: its block or question is gone.
    pub status: AnchorStatus,
    /// The block it is in now (`null` only when the block is gone).
    pub block_id: Option<String>,
    pub item_id: Option<String>,
    pub start: Option<usize>,
    pub end: Option<usize>,
}

/// The text of a field as the sheet shows it (trimmed). `None` for a whole-block note.
pub fn field_text<'a>(block: &'a Block, field: Field, item: Option<&'a Item>) -> Option<&'a str> {
    match field {
        Field::Block => None,
        Field::Title => Some(block.title.trim()),
        Field::Instructions => Some(block.instructions.trim()),
        Field::Item => item.map(|i| i.prompt.trim()),
    }
}

/// The occurrences of `quote` in `text` as UTF-16 ranges, with how well their surroundings
/// match the remembered context.
fn occurrences(text: &str, a: &StoredAnchor) -> Vec<(usize, usize, usize)> {
    if a.quote.is_empty() {
        return Vec::new();
    }
    text.match_indices(&a.quote)
        .map(|(byte, m)| {
            let start = utf16_len(&text[..byte]);
            let before = context_before(text, byte);
            let after = context_after(text, byte + m.len());
            let score = before
                .chars()
                .rev()
                .zip(a.prefix.chars().rev())
                .take_while(|(x, y)| x == y)
                .count()
                + after
                    .chars()
                    .zip(a.suffix.chars())
                    .take_while(|(x, y)| x == y)
                    .count();
            (start, start + utf16_len(m), score)
        })
        .collect()
}

/// The best place of the quote in `text`: same offsets first, then the best context, then the
/// nearest.
fn locate(text: &str, a: &StoredAnchor) -> Option<(usize, usize, bool)> {
    if let Some((s, e)) = a.range {
        if utf16_slice(text, s, e) == Some(a.quote.as_str()) {
            return Some((s, e, true));
        }
    }
    let near = a.range.map_or(0, |(s, _)| s);
    occurrences(text, a)
        .into_iter()
        .max_by_key(|(s, _, score)| (*score, std::cmp::Reverse(s.abs_diff(near))))
        .map(|(s, e, _)| (s, e, false))
}

/// Finds a note in the worksheet as it is now.
pub fn resolve(a: &StoredAnchor, content: &Content) -> CommentAnchor {
    let lost = |block_id: Option<String>| CommentAnchor {
        status: AnchorStatus::Lost,
        block_id,
        item_id: None,
        start: None,
        end: None,
    };
    let Some(block) = content.block(&a.block_id) else {
        return lost(None);
    };
    let at = |status, item_id: Option<String>, range: Option<(usize, usize)>| CommentAnchor {
        status,
        block_id: Some(block.id.clone()),
        item_id,
        start: range.map(|(s, _)| s),
        end: range.map(|(_, e)| e),
    };
    // A whole field (no quote) is wherever the field is.
    let place = |text: &str| -> (AnchorStatus, Option<(usize, usize)>) {
        if a.quote.is_empty() {
            return (AnchorStatus::Ok, None);
        }
        match locate(text, a) {
            Some((s, e, true)) => (AnchorStatus::Ok, Some((s, e))),
            Some((s, e, false)) => (AnchorStatus::Moved, Some((s, e))),
            // The field is there, the words are not: the note still belongs to it.
            None => (AnchorStatus::Moved, None),
        }
    };
    match a.field {
        Field::Block => at(AnchorStatus::Ok, None, None),
        Field::Title | Field::Instructions => {
            let (status, range) = place(field_text(block, a.field, None).unwrap_or_default());
            at(status, None, range)
        }
        Field::Item => {
            let own = a
                .item_id
                .as_deref()
                .and_then(|id| block.items.iter().find(|i| i.id == id));
            if let Some(item) = own {
                let (status, range) = place(item.prompt.trim());
                return at(status, Some(item.id.clone()), range);
            }
            // The question was replaced (a regeneration gives new ids): the one that still has
            // the quote, when one place is clearly the best.
            let mut found: Vec<(&Item, usize, usize, usize)> = block
                .items
                .iter()
                .flat_map(|i| {
                    occurrences(i.prompt.trim(), a)
                        .into_iter()
                        .map(move |(s, e, score)| (i, s, e, score))
                })
                .collect();
            found.sort_by_key(|(_, _, _, score)| std::cmp::Reverse(*score));
            match found.as_slice() {
                [(i, s, e, _)] => at(AnchorStatus::Moved, Some(i.id.clone()), Some((*s, *e))),
                [(i, s, e, best), (_, _, _, next), ..] if best > next => {
                    at(AnchorStatus::Moved, Some(i.id.clone()), Some((*s, *e)))
                }
                _ => lost(Some(block.id.clone())),
            }
        }
    }
}

/// What the teacher selected, as sent by the browser.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AnchorInput {
    pub block_id: String,
    pub field: Option<Field>,
    #[serde(default)]
    pub item_id: Option<String>,
    /// UTF-16 offsets inside the field's text as the sheet shows it.
    #[serde(default)]
    pub start: Option<usize>,
    #[serde(default)]
    pub end: Option<usize>,
    /// The selected text. Kept when it cannot be placed exactly (a printed title such as
    /// 「Part B · …」 differs from the stored one); the note then belongs to the whole field.
    #[serde(default)]
    pub quote: Option<String>,
}

/// Builds the stored anchor of a new note against the latest version: the block (and the
/// question) must be there; the quote and its context come from the worksheet itself whenever
/// the selection can be found in it.
pub fn make_anchor(input: &AnchorInput, content: &Content) -> AppResult<StoredAnchor> {
    let block = content.block(input.block_id.trim()).ok_or_else(|| {
        AppError::bad_request("找不到這個區塊；學習單可能已經改過，請重新整理後再選一次。")
    })?;
    let field = input.field.unwrap_or(if input.item_id.is_some() {
        Field::Item
    } else {
        Field::Block
    });
    let item = match field {
        Field::Item => {
            let id = input
                .item_id
                .as_deref()
                .ok_or_else(|| AppError::bad_request("註記一題時要給 item_id。"))?;
            Some(block.items.iter().find(|i| i.id == id).ok_or_else(|| {
                AppError::bad_request("找不到這一題；學習單可能已經改過，請重新整理後再選一次。")
            })?)
        }
        _ => None,
    };
    let mut anchor = StoredAnchor {
        block_id: block.id.clone(),
        field,
        item_id: item.map(|i| i.id.clone()),
        range: None,
        quote: String::new(),
        prefix: String::new(),
        suffix: String::new(),
    };
    let Some(text) = field_text(block, field, item) else {
        return Ok(anchor);
    };
    let wanted = input
        .quote
        .as_deref()
        .map(str::trim)
        .filter(|q| !q.is_empty());
    let exact = match (input.start, input.end) {
        (Some(s), Some(e)) => {
            let slice = (s < e)
                .then(|| utf16_slice(text, s, e))
                .flatten()
                .filter(|q| !q.trim().is_empty());
            match (slice, wanted) {
                (Some(q), Some(w)) if q.trim() == w => Some((s, e)),
                (Some(_), None) => Some((s, e)),
                // Placed by the quote below.
                (_, Some(_)) => None,
                (None, None) => return Err(AppError::bad_request("選取的範圍不在這段文字裡。")),
            }
        }
        _ => None,
    };
    let range = exact.or_else(|| {
        let w = wanted?;
        let probe = StoredAnchor {
            quote: w.to_string(),
            range: input.start.zip(input.end),
            ..anchor.clone()
        };
        locate(text, &probe).map(|(s, e, _)| (s, e))
    });
    match range {
        Some((s, e)) => {
            let quote = utf16_slice(text, s, e).unwrap_or_default();
            let (bs, be) = (
                byte_index(text, s).unwrap_or(0),
                byte_index(text, e).unwrap_or(text.len()),
            );
            anchor.range = Some((s, e));
            anchor.quote = quote.to_string();
            anchor.prefix = context_before(text, bs);
            anchor.suffix = context_after(text, be);
        }
        None => {
            if let Some(w) = wanted {
                anchor.quote = w.to_string();
            }
        }
    }
    if anchor.quote.chars().count() > MAX_QUOTE_CHARS {
        return Err(AppError::bad_request(format!(
            "一則註記最多標記 {MAX_QUOTE_CHARS} 字。"
        )));
    }
    Ok(anchor)
}

// ---------------------------------------------------------------- storage

#[derive(Debug, Clone, Serialize)]
pub struct Comment {
    pub id: i64,
    pub worksheet_id: i64,
    /// Where the note was made (see `anchor` for where it is now).
    pub block_id: String,
    pub field: Field,
    pub item_id: Option<String>,
    pub quote: String,
    pub note: String,
    /// 「提供給 AI」: Manna may read it (the block revise and a new generation of the worksheet).
    pub share_with_ai: bool,
    /// `open`（待處理）| `applied`（已依此修改）| `dismissed`（已略過）
    pub status: String,
    /// The proposal that applied it.
    pub proposal_id: Option<i64>,
    pub anchor: CommentAnchor,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct Row {
    id: i64,
    worksheet_id: i64,
    block_id: String,
    field: String,
    item_id: Option<String>,
    start_offset: Option<i32>,
    end_offset: Option<i32>,
    quote: String,
    prefix: String,
    suffix: String,
    note: String,
    share_with_ai: bool,
    status: String,
    proposal_id: Option<i64>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

const COLS: &str = "id, worksheet_id, block_id, field, item_id, start_offset, end_offset, quote,
                    prefix, suffix, note, share_with_ai, status, proposal_id, created_at, updated_at";

impl Row {
    fn stored(&self) -> StoredAnchor {
        let offset = |v: Option<i32>| v.and_then(|v| usize::try_from(v).ok());
        StoredAnchor {
            block_id: self.block_id.clone(),
            field: Field::parse(&self.field),
            item_id: self.item_id.clone(),
            range: offset(self.start_offset).zip(offset(self.end_offset)),
            quote: self.quote.clone(),
            prefix: self.prefix.clone(),
            suffix: self.suffix.clone(),
        }
    }

    fn into_comment(self, content: &Content) -> Comment {
        let anchor = resolve(&self.stored(), content);
        Comment {
            id: self.id,
            worksheet_id: self.worksheet_id,
            field: Field::parse(&self.field),
            block_id: self.block_id,
            item_id: self.item_id,
            quote: self.quote,
            note: self.note,
            share_with_ai: self.share_with_ai,
            status: self.status,
            proposal_id: self.proposal_id,
            anchor,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

/// Reading order: the block's place in the worksheet, then the question's, then the offset; notes
/// whose block is gone last.
fn sort(list: &mut [Comment], content: &Content) {
    let block_pos: HashMap<&str, usize> = content
        .blocks
        .iter()
        .enumerate()
        .map(|(i, b)| (b.id.as_str(), i))
        .collect();
    let item_pos = |c: &Comment| -> usize {
        let (Some(b), Some(i)) = (c.anchor.block_id.as_deref(), c.anchor.item_id.as_deref()) else {
            return 0;
        };
        content
            .block(b)
            .and_then(|b| b.items.iter().position(|x| x.id == i))
            .map_or(usize::MAX, |p| p + 1)
    };
    list.sort_by_key(|c| {
        (
            c.anchor
                .block_id
                .as_deref()
                .and_then(|b| block_pos.get(b).copied())
                .unwrap_or(usize::MAX),
            item_pos(c),
            c.anchor.start.unwrap_or(0),
            c.id,
        )
    });
}

/// Every note `teacher_id` made on the worksheet, placed against `content` (the latest version).
pub async fn list(
    db: &PgPool,
    worksheet_id: i64,
    teacher_id: i64,
    content: &Content,
) -> sqlx::Result<Vec<Comment>> {
    let rows: Vec<Row> = sqlx::query_as(&format!(
        "SELECT {COLS} FROM worksheet_comments WHERE worksheet_id = $1 AND teacher_id = $2
          ORDER BY id"
    ))
    .bind(worksheet_id)
    .bind(teacher_id)
    .fetch_all(db)
    .await?;
    let mut list: Vec<Comment> = rows.into_iter().map(|r| r.into_comment(content)).collect();
    sort(&mut list, content);
    Ok(list)
}

/// The notes with these ids (the teacher's, on this worksheet), in the order of `ids`.
pub async fn by_ids(
    db: &PgPool,
    worksheet_id: i64,
    teacher_id: i64,
    ids: &[i64],
    content: &Content,
) -> sqlx::Result<Vec<Comment>> {
    let rows: Vec<Row> = sqlx::query_as(&format!(
        "SELECT {COLS} FROM worksheet_comments
          WHERE worksheet_id = $1 AND teacher_id = $2 AND id = ANY($3) ORDER BY id"
    ))
    .bind(worksheet_id)
    .bind(teacher_id)
    .bind(ids)
    .fetch_all(db)
    .await?;
    let mut list: Vec<Comment> = rows.into_iter().map(|r| r.into_comment(content)).collect();
    list.sort_by_key(|c| ids.iter().position(|i| *i == c.id));
    Ok(list)
}

pub async fn create(
    db: &PgPool,
    worksheet_id: i64,
    teacher_id: i64,
    anchor: &StoredAnchor,
    note: &str,
    share_with_ai: bool,
    content: &Content,
) -> AppResult<Comment> {
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM worksheet_comments WHERE worksheet_id = $1")
            .bind(worksheet_id)
            .fetch_one(db)
            .await?;
    if count >= MAX_COMMENTS_PER_WORKSHEET {
        return Err(AppError::bad_request(format!(
            "一份學習單最多 {MAX_COMMENTS_PER_WORKSHEET} 則註記。"
        )));
    }
    let offset = |v: Option<usize>| v.map(|v| i32::try_from(v).unwrap_or(i32::MAX));
    let row: Row = sqlx::query_as(&format!(
        "INSERT INTO worksheet_comments
           (worksheet_id, teacher_id, block_id, field, item_id, start_offset, end_offset, quote,
            prefix, suffix, note, share_with_ai)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
         RETURNING {COLS}"
    ))
    .bind(worksheet_id)
    .bind(teacher_id)
    .bind(&anchor.block_id)
    .bind(anchor.field.as_str())
    .bind(&anchor.item_id)
    .bind(offset(anchor.range.map(|(s, _)| s)))
    .bind(offset(anchor.range.map(|(_, e)| e)))
    .bind(&anchor.quote)
    .bind(&anchor.prefix)
    .bind(&anchor.suffix)
    .bind(note)
    .bind(share_with_ai)
    .fetch_one(db)
    .await?;
    Ok(row.into_comment(content))
}

/// 404 unless the note is the teacher's and on this worksheet.
pub async fn owned(db: &PgPool, worksheet_id: i64, id: i64, teacher_id: i64) -> AppResult<()> {
    let found: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM worksheet_comments WHERE id = $1 AND worksheet_id = $2 AND teacher_id = $3",
    )
    .bind(id)
    .bind(worksheet_id)
    .bind(teacher_id)
    .fetch_optional(db)
    .await?;
    found
        .map(|_| ())
        .ok_or_else(|| AppError::not_found("找不到這則註記。"))
}

#[derive(Debug, Default)]
pub struct Patch<'a> {
    pub note: Option<&'a str>,
    pub share_with_ai: Option<bool>,
    /// `open` or `dismissed` (a teacher never sets `applied` herself).
    pub status: Option<&'a str>,
}

pub async fn update(
    db: &PgPool,
    id: i64,
    patch: Patch<'_>,
    content: &Content,
) -> AppResult<Comment> {
    let row: Row = sqlx::query_as(&format!(
        "UPDATE worksheet_comments
            SET note = COALESCE($2, note), share_with_ai = COALESCE($3, share_with_ai),
                status = COALESCE($4, status),
                proposal_id = CASE WHEN $4 IS NULL OR $4 = 'applied' THEN proposal_id END,
                updated_at = now()
          WHERE id = $1
        RETURNING {COLS}"
    ))
    .bind(id)
    .bind(patch.note)
    .bind(patch.share_with_ai)
    .bind(patch.status)
    .fetch_one(db)
    .await?;
    Ok(row.into_comment(content))
}

/// Deletes the note only. The worksheet is not touched.
pub async fn delete(db: &PgPool, id: i64) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM worksheet_comments WHERE id = $1")
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

// ---------------------------------------------------------------- what a model may see

/// The open notes on the worksheet the teacher switched to 「提供給 AI」 (a new generation of the
/// whole worksheet reads them).
pub async fn open_for_ai(
    db: &PgPool,
    worksheet_id: i64,
    teacher_id: i64,
    content: &Content,
) -> sqlx::Result<Vec<Comment>> {
    Ok(list(db, worksheet_id, teacher_id, content)
        .await?
        .into_iter()
        .filter(|c| c.share_with_ai && c.status == STATUS_OPEN)
        .collect())
}

/// One note as a line of prompt text: `[where]「quote」→ note`. `place` names its position.
pub fn ai_line(c: &Comment, place: &str) -> String {
    let note = if c.note.trim().is_empty() {
        "（教師標記了這段文字，沒有寫說明）"
    } else {
        c.note.trim()
    };
    let quote = if c.quote.trim().is_empty() {
        String::new()
    } else {
        format!("「{}」", clip_chars(c.quote.trim(), 200))
    };
    format!("- [{place}]{quote} → {}", clip_chars(note, 600))
}

/// The heading of the notes on a worksheet in a generation prompt.
pub const WORKSHEET_NOTES_HEADING: &str =
    "教師在上一版學習單上的註記（教師寫的修改意見，不是教材原文，也不是外部來源；做這份學習單時請參考，不得寫進學生版）";

/// The open notes for AI as a block of prompt text for a whole generation ("" when none). Each
/// note names its block by title (a new generation makes new blocks) and its question by number.
pub fn generation_block(notes: &[Comment], content: &Content) -> String {
    let lines: Vec<String> = notes
        .iter()
        .map(|c| {
            let block = c.anchor.block_id.as_deref().and_then(|b| content.block(b));
            let title = block.map_or(String::new(), |b| {
                format!("區塊「{}」", clip_chars(b.title.trim(), 40))
            });
            let part = match c.field {
                Field::Block => String::new(),
                Field::Title => "標題".to_string(),
                Field::Instructions => "作答指示".to_string(),
                Field::Item => match (block, c.anchor.item_id.as_deref()) {
                    (Some(b), Some(id)) => b
                        .items
                        .iter()
                        .position(|i| i.id == id)
                        .map_or("一題".to_string(), |p| format!("第 {} 題", p + 1)),
                    _ => "一題".to_string(),
                },
            };
            let place = [title, part]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            ai_line(
                c,
                if place.is_empty() {
                    "學習單"
                } else {
                    &place
                },
            )
        })
        .collect();
    if lines.is_empty() {
        String::new()
    } else {
        format!("{WORKSHEET_NOTES_HEADING}：\n{}", lines.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worksheet::{Block, Item};

    fn content() -> Content {
        Content {
            blocks: vec![Block {
                id: "b1".into(),
                title: "Part A · Find the facts".into(),
                instructions: "Read paragraph 1 and answer.".into(),
                items: vec![
                    Item {
                        id: "b1-i1".into(),
                        prompt: "Why did Mrs. Lin put out the stand?".into(),
                        ..Item::default()
                    },
                    Item {
                        id: "b1-i2".into(),
                        prompt: "What did the neighbors think?".into(),
                        ..Item::default()
                    },
                ],
                ..Block::default()
            }],
            ..Content::default()
        }
    }

    fn input(
        field: Field,
        item: Option<&str>,
        range: Option<(usize, usize)>,
        quote: Option<&str>,
    ) -> AnchorInput {
        AnchorInput {
            block_id: "b1".into(),
            field: Some(field),
            item_id: item.map(str::to_string),
            start: range.map(|(s, _)| s),
            end: range.map(|(_, e)| e),
            quote: quote.map(str::to_string),
        }
    }

    #[test]
    fn the_quote_and_context_come_from_the_worksheet() {
        let c = content();
        let a = make_anchor(&input(Field::Item, Some("b1-i2"), Some((13, 22)), None), &c).unwrap();
        assert_eq!(a.quote, "neighbors");
        assert_eq!(a.prefix, "What did the ");
        assert_eq!(resolve(&a, &c).status, AnchorStatus::Ok);
        // A whole block or question needs no selection.
        let a = make_anchor(&input(Field::Block, None, None, None), &c).unwrap();
        assert_eq!(resolve(&a, &c).block_id.as_deref(), Some("b1"));
        assert!(make_anchor(&input(Field::Item, Some("nope"), None, None), &c).is_err());
        assert!(make_anchor(
            &AnchorInput {
                block_id: "zz".into(),
                ..AnchorInput::default()
            },
            &c
        )
        .is_err());
        assert!(make_anchor(&input(Field::Title, None, Some((3, 300)), None), &c).is_err());
    }

    #[test]
    fn a_printed_title_that_differs_keeps_the_quote_on_the_field() {
        let c = content();
        // The sheet prints 「Part B · …」 for this block; the stored title says Part A.
        let a = make_anchor(&input(Field::Title, None, Some((0, 6)), Some("Part B")), &c).unwrap();
        assert_eq!((a.range, a.quote.as_str()), (None, "Part B"));
        assert_eq!(resolve(&a, &c).status, AnchorStatus::Moved);
        // Words that are there are found even when the offsets are off.
        let a = make_anchor(&input(Field::Title, None, Some((1, 5)), Some("facts")), &c).unwrap();
        assert_eq!(a.quote, "facts");
        assert_eq!(resolve(&a, &c).status, AnchorStatus::Ok);
    }

    #[test]
    fn a_replaced_question_is_found_by_its_quote() {
        let c = content();
        let a = make_anchor(&input(Field::Item, Some("b1-i2"), Some((13, 22)), None), &c).unwrap();
        let mut next = c.clone();
        next.blocks[0].items[1].id = "b1-r2i2".into();
        let r = resolve(&a, &next);
        assert_eq!(
            (r.status, r.item_id.as_deref()),
            (AnchorStatus::Moved, Some("b1-r2i2"))
        );
        next.blocks[0].items[1].prompt = "Something else.".into();
        let r = resolve(&a, &next);
        assert_eq!(
            (r.status, r.block_id.as_deref()),
            (AnchorStatus::Lost, Some("b1"))
        );
        next.blocks.clear();
        assert_eq!(resolve(&a, &next).block_id, None);
    }
}
