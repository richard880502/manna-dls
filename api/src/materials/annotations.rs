//! Teacher annotations on a material's text (規格 §4.4, §9.7).
//!
//! An annotation never changes the material. It remembers where it was made (paragraph id and
//! UTF-16 offsets inside that paragraph, which is what a browser selection gives) and what was
//! selected (the quote plus a little text on both sides). Where it is *now* is worked out on
//! every read ([`resolve`]): still in place, found again somewhere else, or lost, in which case
//! the teacher is asked to confirm the position (「需要重新確認位置」).

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;

use crate::error::{AppError, AppResult};
use crate::sop::Paragraph;

pub const MAX_NOTE_CHARS: usize = 2_000;
pub const MAX_QUOTE_CHARS: usize = 2_000;
pub const MAX_ANNOTATIONS_PER_MATERIAL: i64 = 500;
/// Characters of context kept on each side of the quote.
const CONTEXT_CHARS: usize = 32;

// ---------------------------------------------------------------- UTF-16 positions

pub fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// Byte index of the UTF-16 offset `at`, when it falls on a character boundary.
fn byte_index(s: &str, at: usize) -> Option<usize> {
    let mut units = 0;
    for (i, c) in s.char_indices() {
        if units == at {
            return Some(i);
        }
        if units > at {
            return None;
        }
        units += c.len_utf16();
    }
    (units == at).then_some(s.len())
}

/// The text between two UTF-16 offsets, or `None` when they are out of range or split a
/// character.
pub fn utf16_slice(s: &str, start: usize, end: usize) -> Option<&str> {
    if start > end {
        return None;
    }
    Some(&s[byte_index(s, start)?..byte_index(s, end)?])
}

fn context_before(text: &str, byte_start: usize) -> String {
    let before: Vec<char> = text[..byte_start]
        .chars()
        .rev()
        .take(CONTEXT_CHARS)
        .collect();
    before.into_iter().rev().collect()
}

fn context_after(text: &str, byte_end: usize) -> String {
    text[byte_end..].chars().take(CONTEXT_CHARS).collect()
}

// ---------------------------------------------------------------- anchoring

/// What is stored with an annotation to find its place again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredAnchor {
    pub paragraph_id: String,
    pub start: usize,
    pub end: usize,
    pub quote: String,
    pub prefix: String,
    pub suffix: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AnchorStatus {
    /// Exactly where it was made.
    Ok,
    /// The text around it changed; the quote was found again.
    Moved,
    /// The quote is gone (or cannot be told apart): the teacher has to confirm the position.
    Lost,
}

/// Where an annotation is in the material as it is now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Anchor {
    pub status: AnchorStatus,
    pub paragraph_id: Option<String>,
    pub paragraph_no: Option<i32>,
    pub start: Option<usize>,
    pub end: Option<usize>,
}

impl Anchor {
    fn at(status: AnchorStatus, p: &Paragraph, start: usize, end: usize) -> Self {
        Self {
            status,
            paragraph_id: Some(p.id.clone()),
            paragraph_no: Some(p.no),
            start: Some(start),
            end: Some(end),
        }
    }

    fn lost() -> Self {
        Self {
            status: AnchorStatus::Lost,
            paragraph_id: None,
            paragraph_no: None,
            start: None,
            end: None,
        }
    }
}

/// Builds the stored anchor for a selection, taking the quote and its context from the
/// paragraph itself (never from the client). `None` when the range is not inside the paragraph.
pub fn make_anchor(p: &Paragraph, start: usize, end: usize) -> Option<StoredAnchor> {
    if start >= end {
        return None;
    }
    let byte_start = byte_index(&p.text, start)?;
    let byte_end = byte_index(&p.text, end)?;
    Some(StoredAnchor {
        paragraph_id: p.id.clone(),
        start,
        end,
        quote: p.text[byte_start..byte_end].to_string(),
        prefix: context_before(&p.text, byte_start),
        suffix: context_after(&p.text, byte_end),
    })
}

/// How well the text around an occurrence matches the remembered context.
fn context_score(text: &str, byte_start: usize, byte_end: usize, anchor: &StoredAnchor) -> usize {
    let before = context_before(text, byte_start);
    let after = context_after(text, byte_end);
    let back = before
        .chars()
        .rev()
        .zip(anchor.prefix.chars().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let forward = after
        .chars()
        .zip(anchor.suffix.chars())
        .take_while(|(a, b)| a == b)
        .count();
    back + forward
}

struct Candidate<'a> {
    paragraph: &'a Paragraph,
    start: usize,
    end: usize,
    score: usize,
}

fn candidates<'a>(p: &'a Paragraph, anchor: &StoredAnchor) -> Vec<Candidate<'a>> {
    p.text
        .match_indices(&anchor.quote)
        .map(|(byte_start, m)| {
            let start = utf16_len(&p.text[..byte_start]);
            Candidate {
                paragraph: p,
                start,
                end: start + utf16_len(m),
                score: context_score(&p.text, byte_start, byte_start + m.len(), anchor),
            }
        })
        .collect()
}

/// Finds the annotation in the current paragraphs:
/// 1. the same paragraph, same offsets, same text → `ok`;
/// 2. the quote elsewhere in the same paragraph (best context, then nearest) → `moved`;
/// 3. the quote in another paragraph, when one place is clearly the best match → `moved`;
/// 4. otherwise → `lost`.
pub fn resolve(anchor: &StoredAnchor, paragraphs: &[Paragraph]) -> Anchor {
    if anchor.quote.is_empty() {
        return Anchor::lost();
    }
    let home = paragraphs.iter().find(|p| p.id == anchor.paragraph_id);
    if let Some(p) = home {
        if utf16_slice(&p.text, anchor.start, anchor.end) == Some(anchor.quote.as_str()) {
            return Anchor::at(AnchorStatus::Ok, p, anchor.start, anchor.end);
        }
        let best = candidates(p, anchor)
            .into_iter()
            .max_by_key(|c| (c.score, std::cmp::Reverse(c.start.abs_diff(anchor.start))));
        if let Some(c) = best {
            return Anchor::at(AnchorStatus::Moved, p, c.start, c.end);
        }
    }
    let mut all: Vec<Candidate> = paragraphs
        .iter()
        .filter(|p| p.id != anchor.paragraph_id)
        .flat_map(|p| candidates(p, anchor))
        .collect();
    all.sort_by_key(|c| std::cmp::Reverse(c.score));
    match all.as_slice() {
        [only] => Anchor::at(AnchorStatus::Moved, only.paragraph, only.start, only.end),
        [best, next, ..] if best.score > next.score => {
            Anchor::at(AnchorStatus::Moved, best.paragraph, best.start, best.end)
        }
        _ => Anchor::lost(),
    }
}

// ---------------------------------------------------------------- storage

#[derive(Debug, Clone, Serialize)]
pub struct Annotation {
    pub id: i64,
    pub material_id: i64,
    pub quote: String,
    pub note: String,
    pub share_with_ai: bool,
    pub hidden: bool,
    /// Where the note is now (`status: "lost"` → 「需要重新確認位置」).
    pub anchor: Anchor,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct Row {
    id: i64,
    material_id: i64,
    paragraph_id: String,
    start_offset: i32,
    end_offset: i32,
    quote: String,
    prefix: String,
    suffix: String,
    note: String,
    share_with_ai: bool,
    hidden: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

const COLS: &str = "id, material_id, paragraph_id, start_offset, end_offset, quote, prefix, suffix,
                    note, share_with_ai, hidden, created_at, updated_at";

impl Row {
    fn into_annotation(self, paragraphs: &[Paragraph]) -> Annotation {
        let stored = StoredAnchor {
            paragraph_id: self.paragraph_id,
            start: usize::try_from(self.start_offset).unwrap_or(0),
            end: usize::try_from(self.end_offset).unwrap_or(0),
            quote: self.quote,
            prefix: self.prefix,
            suffix: self.suffix,
        };
        Annotation {
            id: self.id,
            material_id: self.material_id,
            anchor: resolve(&stored, paragraphs),
            quote: stored.quote,
            note: self.note,
            share_with_ai: self.share_with_ai,
            hidden: self.hidden,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

/// Every annotation `teacher_id` made on the material (hidden ones included; the viewer decides
/// what to show), in reading order, lost ones last. Annotations are personal: on a platform
/// material each teacher sees only her own.
pub async fn list(
    db: &PgPool,
    material_id: i64,
    teacher_id: i64,
    paragraphs: &[Paragraph],
) -> sqlx::Result<Vec<Annotation>> {
    let rows: Vec<Row> = sqlx::query_as(&format!(
        "SELECT {COLS} FROM material_annotations WHERE material_id = $1 AND teacher_id = $2
          ORDER BY id"
    ))
    .bind(material_id)
    .bind(teacher_id)
    .fetch_all(db)
    .await?;
    let mut list: Vec<Annotation> = rows
        .into_iter()
        .map(|r| r.into_annotation(paragraphs))
        .collect();
    list.sort_by_key(|a| {
        (
            a.anchor.paragraph_no.unwrap_or(i32::MAX),
            a.anchor.start.unwrap_or(usize::MAX),
            a.id,
        )
    });
    Ok(list)
}

pub struct NewAnnotation<'a> {
    pub material_id: i64,
    pub teacher_id: i64,
    pub material_revision: i32,
    pub anchor: &'a StoredAnchor,
    pub note: &'a str,
    pub share_with_ai: bool,
}

pub async fn create(
    db: &PgPool,
    new: NewAnnotation<'_>,
    paragraphs: &[Paragraph],
) -> AppResult<Annotation> {
    // Per teacher: on a platform material every teacher has her own notes.
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM material_annotations WHERE material_id = $1 AND teacher_id = $2",
    )
    .bind(new.material_id)
    .bind(new.teacher_id)
    .fetch_one(db)
    .await?;
    if count >= MAX_ANNOTATIONS_PER_MATERIAL {
        return Err(AppError::bad_request(format!(
            "一份教材最多 {MAX_ANNOTATIONS_PER_MATERIAL} 則註記。"
        )));
    }
    let row: Row = sqlx::query_as(&format!(
        "INSERT INTO material_annotations
           (material_id, teacher_id, paragraph_id, start_offset, end_offset, quote, prefix, suffix,
            note, share_with_ai, material_revision)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
         RETURNING {COLS}"
    ))
    .bind(new.material_id)
    .bind(new.teacher_id)
    .bind(&new.anchor.paragraph_id)
    .bind(new.anchor.start as i32)
    .bind(new.anchor.end as i32)
    .bind(&new.anchor.quote)
    .bind(&new.anchor.prefix)
    .bind(&new.anchor.suffix)
    .bind(new.note)
    .bind(new.share_with_ai)
    .bind(new.material_revision)
    .fetch_one(db)
    .await?;
    Ok(row.into_annotation(paragraphs))
}

/// The material of the annotation, when the annotation is the teacher's (and she can still read
/// the material); otherwise 404.
pub async fn owned(db: &PgPool, id: i64, teacher_id: i64) -> AppResult<i64> {
    sqlx::query_scalar(
        "SELECT a.material_id FROM material_annotations a JOIN materials m ON m.id = a.material_id
          WHERE a.id = $1 AND a.teacher_id = $2
            AND (m.scope = 'platform' OR m.teacher_id = $2)",
    )
    .bind(id)
    .bind(teacher_id)
    .fetch_optional(db)
    .await?
    .ok_or_else(|| AppError::not_found("找不到這則註記。"))
}

#[derive(Default)]
pub struct Patch<'a> {
    pub note: Option<&'a str>,
    pub share_with_ai: Option<bool>,
    pub hidden: Option<bool>,
    /// A new position (the teacher confirmed where a lost note belongs).
    pub anchor: Option<(&'a StoredAnchor, i32)>,
}

pub async fn update(
    db: &PgPool,
    id: i64,
    patch: Patch<'_>,
    paragraphs: &[Paragraph],
) -> AppResult<Annotation> {
    let mut tx = db.begin().await?;
    if let Some(note) = patch.note {
        sqlx::query("UPDATE material_annotations SET note = $2 WHERE id = $1")
            .bind(id)
            .bind(note)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(share) = patch.share_with_ai {
        sqlx::query("UPDATE material_annotations SET share_with_ai = $2 WHERE id = $1")
            .bind(id)
            .bind(share)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(hidden) = patch.hidden {
        sqlx::query("UPDATE material_annotations SET hidden = $2 WHERE id = $1")
            .bind(id)
            .bind(hidden)
            .execute(&mut *tx)
            .await?;
    }
    if let Some((anchor, revision)) = patch.anchor {
        sqlx::query(
            "UPDATE material_annotations
                SET paragraph_id = $2, start_offset = $3, end_offset = $4, quote = $5,
                    prefix = $6, suffix = $7, material_revision = $8
              WHERE id = $1",
        )
        .bind(id)
        .bind(&anchor.paragraph_id)
        .bind(anchor.start as i32)
        .bind(anchor.end as i32)
        .bind(&anchor.quote)
        .bind(&anchor.prefix)
        .bind(&anchor.suffix)
        .bind(revision)
        .execute(&mut *tx)
        .await?;
    }
    let row: Row = sqlx::query_as(&format!(
        "UPDATE material_annotations SET updated_at = now() WHERE id = $1 RETURNING {COLS}"
    ))
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(row.into_annotation(paragraphs))
}

/// Deletes the note only. The material is not touched.
pub async fn delete(db: &PgPool, id: i64) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM material_annotations WHERE id = $1")
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

// ---------------------------------------------------------------- what a model may see

/// One teacher note a model may read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AiNote {
    pub annotation_id: i64,
    pub paragraph_id: String,
    pub paragraph_no: i32,
    pub quote: String,
    pub note: String,
}

/// The only way annotations reach an AI pipeline: the notes `teacher_id` made on this material
/// that she switched to 「提供給 AI」, are not hidden and can still be located. Everything else
/// (the default, and every other teacher's notes on a platform material) stays out. The teacher
/// must be able to read the material (her own, or a platform one), otherwise the list is empty.
pub async fn shared_with_ai(
    db: &PgPool,
    teacher_id: i64,
    material_id: i64,
) -> sqlx::Result<Vec<AiNote>> {
    let paragraphs: Option<sqlx::types::Json<Vec<Paragraph>>> = sqlx::query_scalar(&format!(
        "SELECT paragraphs FROM materials WHERE id = $1 AND {}",
        super::store::READABLE_SQL
    ))
    .bind(material_id)
    .bind(teacher_id)
    .fetch_optional(db)
    .await?;
    let Some(paragraphs) = paragraphs else {
        return Ok(Vec::new());
    };
    let rows: Vec<Row> = sqlx::query_as(&format!(
        "SELECT {COLS} FROM material_annotations
          WHERE material_id = $1 AND teacher_id = $2 AND share_with_ai AND NOT hidden ORDER BY id"
    ))
    .bind(material_id)
    .bind(teacher_id)
    .fetch_all(db)
    .await?;
    let mut notes: Vec<(usize, AiNote)> = rows
        .into_iter()
        .map(|r| r.into_annotation(&paragraphs.0))
        .filter_map(|a| {
            Some((
                a.anchor.start?,
                AiNote {
                    annotation_id: a.id,
                    paragraph_id: a.anchor.paragraph_id?,
                    paragraph_no: a.anchor.paragraph_no?,
                    quote: a.quote,
                    note: a.note,
                },
            ))
        })
        .collect();
    notes.sort_by_key(|(start, n)| (n.paragraph_no, *start, n.annotation_id));
    Ok(notes.into_iter().map(|(_, n)| n).collect())
}

/// The heading every prompt uses for teacher notes, so a model (and a reader of the log)
/// cannot take them for the material itself or for AI output.
pub const AI_NOTES_HEADING: &str =
    "教師註記（教師自己寫的備課筆記，不是教材原文，也不是外部來源；只供理解教學重點，不得寫進學生版）";

/// The notes as a block of prompt text, or an empty string when there are none.
pub fn ai_context_block(notes: &[AiNote]) -> String {
    if notes.is_empty() {
        return String::new();
    }
    let lines: Vec<String> = notes
        .iter()
        .map(|n| {
            let note = if n.note.trim().is_empty() {
                "（教師標記了這段文字，沒有寫說明）"
            } else {
                n.note.trim()
            };
            format!("- [P{}]「{}」→ 教師註記：{}", n.paragraph_no, n.quote, note)
        })
        .collect();
    format!("{AI_NOTES_HEADING}：\n{}", lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paras(texts: &[(&str, &str)]) -> Vec<Paragraph> {
        texts
            .iter()
            .enumerate()
            .map(|(i, (id, text))| Paragraph {
                id: id.to_string(),
                no: i as i32 + 1,
                text: text.to_string(),
                page: None,
            })
            .collect()
    }

    fn anchor_for(p: &Paragraph, quote: &str) -> StoredAnchor {
        let byte = p.text.find(quote).unwrap();
        let start = utf16_len(&p.text[..byte]);
        make_anchor(p, start, start + utf16_len(quote)).unwrap()
    }

    #[test]
    fn utf16_offsets_follow_the_browser() {
        // 「你」 is one UTF-16 unit, 「😀」 two.
        let s = "a你😀b";
        assert_eq!(utf16_len(s), 5);
        assert_eq!(utf16_slice(s, 1, 2), Some("你"));
        assert_eq!(utf16_slice(s, 2, 4), Some("😀"));
        assert_eq!(utf16_slice(s, 4, 5), Some("b"));
        assert_eq!(utf16_slice(s, 3, 4), None, "inside a surrogate pair");
        assert_eq!(utf16_slice(s, 0, 9), None);
        assert_eq!(utf16_slice(s, 3, 2), None);
    }

    #[test]
    fn the_quote_and_context_come_from_the_paragraph() {
        let ps = paras(&[("p1", "Mrs. Lin put an umbrella stand outside her shop.")]);
        let a = make_anchor(&ps[0], 16, 30).unwrap();
        assert_eq!(a.quote, "umbrella stand");
        assert_eq!(a.prefix, "Mrs. Lin put an ");
        assert_eq!(a.suffix, " outside her shop.");
        assert!(make_anchor(&ps[0], 5, 5).is_none());
        assert!(make_anchor(&ps[0], 5, 500).is_none());
    }

    #[test]
    fn an_untouched_anchor_is_ok() {
        let ps = paras(&[("p1", "One two three."), ("p2", "Four five six.")]);
        let a = anchor_for(&ps[1], "five");
        let r = resolve(&a, &ps);
        assert_eq!(r.status, AnchorStatus::Ok);
        assert_eq!(
            (r.paragraph_no, r.start, r.end),
            (Some(2), Some(5), Some(9))
        );
    }

    #[test]
    fn an_edit_before_the_quote_moves_it_within_the_paragraph() {
        let ps = paras(&[("p1", "The cat sat. The cat ran.")]);
        // The second 「cat」.
        let a = make_anchor(&ps[0], 17, 20).unwrap();
        assert_eq!(a.quote, "cat");
        // Same paragraph id (text edited in place by a later revision), words added in front.
        let edited = paras(&[("p1", "Yesterday the cat sat. The cat ran.")]);
        let r = resolve(&a, &edited);
        assert_eq!(r.status, AnchorStatus::Moved);
        // Context (「. The 」 before, 「 ran.」 after) picks the second occurrence, not the first.
        assert_eq!(
            utf16_slice(&edited[0].text, r.start.unwrap(), r.end.unwrap()),
            Some("cat")
        );
        assert_eq!(r.start, Some(27));
    }

    #[test]
    fn a_rewritten_paragraph_is_found_by_its_quote_elsewhere() {
        let ps = paras(&[("p1", "Intro."), ("p2", "Twelve umbrellas did disappear.")]);
        let a = anchor_for(&ps[1], "umbrellas did disappear");
        // The paragraph was edited, so it got a new id; the quote survives in it.
        let edited = paras(&[
            ("p1", "Intro."),
            (
                "p3",
                "In the first month, twelve umbrellas did disappear, sadly.",
            ),
        ]);
        let r = resolve(&a, &edited);
        assert_eq!(r.status, AnchorStatus::Moved);
        assert_eq!(r.paragraph_id.as_deref(), Some("p3"));
        assert_eq!(
            utf16_slice(&edited[1].text, r.start.unwrap(), r.end.unwrap()),
            Some("umbrellas did disappear")
        );
    }

    #[test]
    fn a_vanished_or_ambiguous_quote_is_lost() {
        let ps = paras(&[("p1", "Keep this sentence."), ("p2", "Trust matters here.")]);
        let a = anchor_for(&ps[1], "Trust matters");
        let gone = paras(&[
            ("p1", "Keep this sentence."),
            ("p9", "Something else entirely."),
        ]);
        assert_eq!(resolve(&a, &gone).status, AnchorStatus::Lost);
        assert_eq!(resolve(&a, &[]).status, AnchorStatus::Lost);

        // The word now appears twice in other paragraphs with equally (un)helpful context.
        let b = anchor_for(&ps[0], "Keep");
        let twice = paras(&[("p5", "Keep calm."), ("p6", "Keep going.")]);
        assert_eq!(resolve(&b, &twice).status, AnchorStatus::Lost);
        // …but context can settle it.
        let twice = paras(&[
            ("p5", "Keep calm."),
            ("p6", "So: Keep this sentence, please."),
        ]);
        let r = resolve(&b, &twice);
        assert_eq!(
            (r.status, r.paragraph_id.as_deref()),
            (AnchorStatus::Moved, Some("p6"))
        );
    }

    #[test]
    fn the_ai_block_labels_notes_as_teacher_notes() {
        assert_eq!(ai_context_block(&[]), "");
        let block = ai_context_block(&[
            AiNote {
                annotation_id: 1,
                paragraph_id: "p2".into(),
                paragraph_no: 2,
                quote: "however".into(),
                note: "學生常忽略轉折".into(),
            },
            AiNote {
                annotation_id: 2,
                paragraph_id: "p3".into(),
                paragraph_no: 3,
                quote: "trust".into(),
                note: " ".into(),
            },
        ]);
        assert!(block.starts_with(AI_NOTES_HEADING));
        assert!(block.contains("- [P2]「however」→ 教師註記：學生常忽略轉折"));
        assert!(block.contains("沒有寫說明"));
    }
}
