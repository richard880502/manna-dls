//! 社群 (phase 1, docs/COMMUNITY.md): a teacher opts in to share a worksheet; the other teachers
//! can browse it, save it and remix it into a private copy.
//!
//! What this module never hands out, whatever the caller asks: the owner's `teacher_id`, the
//! research id of an owner who shares anonymously, the owner's other worksheets, lessons, classes,
//! material library, requests, 備課對話, memory or AI logs. A shared worksheet is only ever read
//! as its student sheet (what is printed), and a remix copies the document without any of those
//! links ([`remix`]).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{PgPool, Row};

use crate::audit::log_usage;
use crate::error::{AppError, AppResult};
use crate::worksheet::assemble::{project, Sheet};
use crate::worksheet::images::{self, SlotState};
use crate::worksheet::material::{MaterialSource, SOURCE_PASTED};
use crate::worksheet::store;
use crate::worksheet::{auto_filename, Audience, Content, Level};
use crate::AppState;

pub const MSG_DISABLED: &str = "社群功能目前沒有開放。";
pub const MSG_NOT_FOUND: &str = "找不到這份分享的學習單，它可能已經取消分享。";
/// What a worksheet's owner is called when they chose to be anonymous.
pub const ANONYMOUS_NAME: &str = "一位老師";

pub const DEFAULT_LIMIT: i64 = 24;
pub const MAX_LIMIT: i64 = 60;
const MAX_QUERY_CHARS: usize = 100;
/// A cover shows the top of the worksheet: this many blocks, this many questions in each.
const COVER_BLOCKS: usize = 2;
const COVER_ITEMS: usize = 3;
const COVER_WORDS: usize = 6;

/// Every community endpoint calls this first: with the switch off nothing is read or written.
pub async fn require_enabled(db: &PgPool) -> AppResult<()> {
    if crate::settings::community_enabled(db).await? {
        Ok(())
    } else {
        Err(AppError::Unavailable(MSG_DISABLED.into()))
    }
}

// ---------------------------------------------------------------- sharing

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShareName {
    /// The owner's research id is shown.
    Code,
    /// 「一位老師」 is shown.
    Anonymous,
}

impl ShareName {
    fn as_str(self) -> &'static str {
        match self {
            Self::Code => "code",
            Self::Anonymous => "anonymous",
        }
    }
    fn parse(s: &str) -> Option<Self> {
        match s {
            "code" => Some(Self::Code),
            "anonymous" => Some(Self::Anonymous),
            _ => None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShareRequest {
    pub shared: bool,
    /// Required when sharing for the first time: the choice is the teacher's, there is no default.
    #[serde(default)]
    pub name: Option<ShareName>,
    /// Default: allowed for a new share, unchanged for a share that is updated.
    #[serde(default)]
    pub allow_remix: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct ShareState {
    pub id: i64,
    pub shared: bool,
    pub shared_at: Option<DateTime<Utc>>,
    pub name: Option<ShareName>,
    pub allow_remix: bool,
}

#[derive(sqlx::FromRow)]
struct ShareRow {
    generated: bool,
    shared_at: Option<DateTime<Utc>>,
    share_name: Option<String>,
    allow_remix: bool,
}

/// Shares, updates or withdraws one of the caller's own worksheets. Unsharing is immediate: every
/// read of the community checks `shared_at`.
pub async fn set_share(
    state: &AppState,
    teacher_id: i64,
    id: i64,
    req: ShareRequest,
) -> AppResult<ShareState> {
    require_enabled(&state.db).await?;
    let mut tx = state.db.begin().await?;
    // Only the owner finds it (a 404 for anybody else, like every other worksheet endpoint).
    let current: Option<ShareRow> = sqlx::query_as(
        "SELECT generated, shared_at, share_name, share_allow_remix AS allow_remix FROM worksheets
          WHERE id = $1 AND teacher_id = $2 AND deleted_at IS NULL FOR UPDATE",
    )
    .bind(id)
    .bind(teacher_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(ShareRow {
        generated,
        shared_at,
        share_name: name,
        allow_remix,
    }) = current
    else {
        return Err(AppError::not_found("找不到這份學習單。"));
    };
    let name = name.as_deref().and_then(ShareName::parse);
    if !req.shared {
        if shared_at.is_some() {
            sqlx::query("UPDATE worksheets SET shared_at = NULL, share_name = NULL WHERE id = $1")
                .bind(id)
                .execute(&mut *tx)
                .await?;
            log_usage(
                &mut *tx,
                teacher_id,
                "community_unshare",
                json!({"worksheet_id": id}),
            )
            .await?;
        }
        tx.commit().await?;
        return Ok(ShareState {
            id,
            shared: false,
            shared_at: None,
            name: None,
            allow_remix,
        });
    }
    if !generated {
        return Err(AppError::Conflict(
            "這份學習單還沒有內容，產生之後才能分享。".into(),
        ));
    }
    let update = shared_at.is_some();
    let Some(name) = req.name.or(name) else {
        return Err(AppError::bad_request(
            "分享時請選擇顯示的名字：name 需為 code 或 anonymous。",
        ));
    };
    let allow_remix = req
        .allow_remix
        .unwrap_or(if update { allow_remix } else { true });
    let shared_at: DateTime<Utc> = sqlx::query_scalar(
        "UPDATE worksheets SET shared_at = COALESCE(shared_at, now()), share_name = $2,
                share_allow_remix = $3
          WHERE id = $1 RETURNING shared_at",
    )
    .bind(id)
    .bind(name.as_str())
    .bind(allow_remix)
    .fetch_one(&mut *tx)
    .await?;
    log_usage(
        &mut *tx,
        teacher_id,
        "community_share",
        json!({"worksheet_id": id, "name": name, "allow_remix": allow_remix, "update": update}),
    )
    .await?;
    tx.commit().await?;
    Ok(ShareState {
        id,
        shared: true,
        shared_at: Some(shared_at),
        name: Some(name),
        allow_remix,
    })
}

// ---------------------------------------------------------------- cards

/// What a card and the preview header say about a shared worksheet. No id of anybody.
#[derive(Debug, Serialize)]
pub struct Meta {
    pub id: i64,
    pub title: String,
    /// The research id (the owner chose 代號) or 「一位老師」.
    pub owner_name: String,
    pub shared_at: DateTime<Utc>,
    pub saves: i64,
    pub remixes: i64,
    pub is_mine: bool,
    pub saved: bool,
    pub allow_remix: bool,
    /// `worksheet`, `comprehension` or `tiered`: the kind of worksheet.
    pub output: Option<String>,
    pub levels: Vec<Level>,
}

#[derive(Debug, Serialize)]
pub struct Card {
    #[serde(flatten)]
    pub meta: Meta,
    /// The top of the student sheet (a few questions, no answers, no pictures), to be drawn small.
    pub cover: Option<Sheet>,
    /// 我的收藏 only: when the teacher saved it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
pub struct Page {
    pub items: Vec<Card>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
    /// 我的收藏 only: saved worksheets that are not available now (withdrawn or deleted).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable: Option<i64>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Sort {
    #[default]
    Newest,
    Remixed,
    Saved,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    /// A part of the title.
    pub q: Option<String>,
    pub sort: Option<Sort>,
    /// `worksheet`, `comprehension` or `tiered`.
    pub output: Option<String>,
    /// `basic`, `standard` or `advanced`: worksheets that have this version.
    pub level: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

fn like_pattern(q: &str) -> String {
    let mut out = String::from("%");
    for c in q.chars() {
        if matches!(c, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('%');
    out
}

/// The student sheet of the first version, cut to the top of the page.
fn cover(title: &str, content: &Content) -> Option<Sheet> {
    let level = *content.levels.first()?;
    let mut sheet = project(title, content, level, Audience::Student);
    if sheet.blocks.is_empty() {
        return None;
    }
    sheet.blocks.truncate(COVER_BLOCKS);
    for b in &mut sheet.blocks {
        b.items.truncate(COVER_ITEMS);
        b.vocabulary.truncate(COVER_WORDS);
        b.images.clear();
        b.glosses.clear();
    }
    sheet.end_words.clear();
    Some(sheet)
}

fn content_of(value: serde_json::Value) -> Content {
    serde_json::from_value(value).unwrap_or_default()
}

fn owner_name(code: Option<String>) -> String {
    code.unwrap_or_else(|| ANONYMOUS_NAME.to_string())
}

const CARD_COLUMNS: &str = "w.id, w.title, w.shared_at, w.share_allow_remix AS allow_remix,
        CASE WHEN w.share_name = 'code' THEN t.research_id END AS owner_code,
        (w.teacher_id = $1) AS is_mine, w.settings ->> 'output' AS output,
        (SELECT COUNT(*) FROM worksheet_saves s WHERE s.worksheet_id = w.id) AS saves,
        (SELECT COUNT(*) FROM worksheets r WHERE r.remixed_from = w.id) AS remixes,
        EXISTS (SELECT 1 FROM worksheet_saves s
                 WHERE s.worksheet_id = w.id AND s.teacher_id = $1) AS saved,
        v.content";

const CARD_FROM: &str = "FROM worksheets w
        JOIN teachers t ON t.id = w.teacher_id
        JOIN LATERAL (SELECT content FROM worksheet_versions
                       WHERE worksheet_id = w.id ORDER BY version_no DESC LIMIT 1) v ON TRUE";

/// Shared, not deleted: the only worksheets the community ever shows.
const VISIBLE: &str = "w.shared_at IS NOT NULL AND w.deleted_at IS NULL";

struct CardRow {
    meta: Meta,
    content: Content,
    saved_at: Option<DateTime<Utc>>,
    total: i64,
}

fn card_row(r: &sqlx::postgres::PgRow) -> sqlx::Result<CardRow> {
    let content = content_of(r.try_get("content")?);
    Ok(CardRow {
        meta: Meta {
            id: r.try_get("id")?,
            title: r.try_get("title")?,
            owner_name: owner_name(r.try_get("owner_code")?),
            shared_at: r.try_get("shared_at")?,
            saves: r.try_get("saves")?,
            remixes: r.try_get("remixes")?,
            is_mine: r.try_get("is_mine")?,
            saved: r.try_get("saved")?,
            allow_remix: r.try_get("allow_remix")?,
            output: r.try_get("output")?,
            levels: content.levels.clone(),
        },
        saved_at: r.try_get("saved_at")?,
        total: r.try_get("total")?,
        content,
    })
}

fn clamp(q: &Query) -> (i64, i64) {
    (
        q.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT),
        q.offset.unwrap_or(0).max(0),
    )
}

fn check_query(q: &Query) -> AppResult<(Option<String>, Option<&str>, Option<&str>)> {
    let text = q.q.as_deref().map(str::trim).filter(|t| !t.is_empty());
    if text.is_some_and(|t| t.chars().count() > MAX_QUERY_CHARS) {
        return Err(AppError::bad_request(format!(
            "搜尋字最多 {MAX_QUERY_CHARS} 字。"
        )));
    }
    let output = q.output.as_deref().filter(|o| !o.is_empty());
    if output.is_some_and(|o| crate::worksheet::Output::parse(o).is_none()) {
        return Err(AppError::bad_request(
            "output 需為 worksheet、comprehension 或 tiered。",
        ));
    }
    let level = q.level.as_deref().filter(|l| !l.is_empty());
    if level.is_some_and(|l| Level::parse(l).is_none()) {
        return Err(AppError::bad_request(
            "level 需為 basic、standard 或 advanced。",
        ));
    }
    Ok((text.map(like_pattern), output, level))
}

async fn page(db: &PgPool, viewer: i64, q: &Query, saved_only: bool) -> AppResult<Page> {
    let (pattern, output, level) = check_query(q)?;
    let (limit, offset) = clamp(q);
    let order = if saved_only {
        "ss.created_at DESC, w.id DESC"
    } else {
        match q.sort.unwrap_or_default() {
            Sort::Newest => "w.shared_at DESC, w.id DESC",
            Sort::Remixed => "remixes DESC, w.shared_at DESC, w.id DESC",
            Sort::Saved => "saves DESC, w.shared_at DESC, w.id DESC",
        }
    };
    let (saved_col, saved_join) = if saved_only {
        (
            "ss.created_at",
            "JOIN worksheet_saves ss ON ss.worksheet_id = w.id AND ss.teacher_id = $1",
        )
    } else {
        ("NULL::timestamptz", "")
    };
    let sql = format!(
        "SELECT {CARD_COLUMNS}, {saved_col} AS saved_at, COUNT(*) OVER () AS total
           {CARD_FROM} {saved_join}
          WHERE {VISIBLE}
            AND ($2::TEXT IS NULL OR w.title ILIKE $2 ESCAPE '\\')
            AND ($3::TEXT IS NULL OR w.settings ->> 'output' = $3)
            AND ($4::TEXT IS NULL OR jsonb_exists(v.content -> 'levels', $4))
          ORDER BY {order} LIMIT $5 OFFSET $6"
    );
    let rows = sqlx::query(&sql)
        .bind(viewer)
        .bind(pattern)
        .bind(output)
        .bind(level)
        .bind(limit)
        .bind(offset)
        .fetch_all(db)
        .await?;
    let mut total = 0;
    let mut items = Vec::with_capacity(rows.len());
    for r in &rows {
        let row = card_row(r)?;
        total = row.total;
        let cover = cover(&row.meta.title, &row.content);
        items.push(Card {
            meta: row.meta,
            cover,
            saved_at: row.saved_at,
        });
    }
    let unavailable = if saved_only {
        Some(
            sqlx::query_scalar(
                "SELECT COUNT(*) FROM worksheet_saves ss
                   JOIN worksheets w ON w.id = ss.worksheet_id
                  WHERE ss.teacher_id = $1 AND NOT (w.shared_at IS NOT NULL AND w.deleted_at IS NULL)",
            )
            .bind(viewer)
            .fetch_one(db)
            .await?,
        )
    } else {
        None
    };
    Ok(Page {
        items,
        total,
        limit,
        offset,
        unavailable,
    })
}

/// 探索: the shared worksheets of every teacher, the caller's own included (marked `is_mine`).
pub async fn explore(state: &AppState, viewer: i64, q: &Query) -> AppResult<Page> {
    require_enabled(&state.db).await?;
    page(&state.db, viewer, q, false).await
}

/// 我的收藏: what the caller saved and is still shared, newest save first.
pub async fn saved(state: &AppState, viewer: i64, q: &Query) -> AppResult<Page> {
    require_enabled(&state.db).await?;
    page(&state.db, viewer, q, true).await
}

// ---------------------------------------------------------------- my shares

/// One of the caller's own worksheets in 我的分享.
#[derive(Debug, Serialize)]
pub struct Mine {
    pub id: i64,
    pub title: String,
    pub updated_at: DateTime<Utc>,
    pub shared: bool,
    pub shared_at: Option<DateTime<Utc>>,
    pub name: Option<ShareName>,
    pub allow_remix: bool,
    pub saves: i64,
    pub remixes: i64,
    pub output: Option<String>,
    pub levels: Vec<Level>,
    pub cover: Option<Sheet>,
}

#[derive(Debug, Serialize)]
pub struct MinePage {
    pub items: Vec<Mine>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

/// The caller's worksheets that have content (the ones that can be shared), newest edit first.
pub async fn mine(state: &AppState, teacher_id: i64, q: &Query) -> AppResult<MinePage> {
    require_enabled(&state.db).await?;
    let (pattern, _, _) = check_query(q)?;
    let (limit, offset) = clamp(q);
    let rows = sqlx::query(
        "SELECT w.id, w.title, w.updated_at, w.shared_at, w.share_name, w.share_allow_remix,
                w.settings ->> 'output' AS output,
                (SELECT COUNT(*) FROM worksheet_saves s WHERE s.worksheet_id = w.id) AS saves,
                (SELECT COUNT(*) FROM worksheets r WHERE r.remixed_from = w.id) AS remixes,
                v.content, COUNT(*) OVER () AS total
           FROM worksheets w
           JOIN LATERAL (SELECT content FROM worksheet_versions
                          WHERE worksheet_id = w.id ORDER BY version_no DESC LIMIT 1) v ON TRUE
          WHERE w.teacher_id = $1 AND w.deleted_at IS NULL AND w.generated
            AND ($2::TEXT IS NULL OR w.title ILIKE $2 ESCAPE '\\')
          ORDER BY w.updated_at DESC, w.id DESC LIMIT $3 OFFSET $4",
    )
    .bind(teacher_id)
    .bind(pattern)
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.db)
    .await?;
    let mut total = 0;
    let mut items = Vec::with_capacity(rows.len());
    for r in &rows {
        total = r.try_get("total")?;
        let content = content_of(r.try_get("content")?);
        let title: String = r.try_get("title")?;
        let shared_at: Option<DateTime<Utc>> = r.try_get("shared_at")?;
        let name: Option<String> = r.try_get("share_name")?;
        items.push(Mine {
            id: r.try_get("id")?,
            cover: cover(&title, &content),
            title,
            updated_at: r.try_get("updated_at")?,
            shared: shared_at.is_some(),
            shared_at,
            name: name.as_deref().and_then(ShareName::parse),
            allow_remix: r.try_get("share_allow_remix")?,
            saves: r.try_get("saves")?,
            remixes: r.try_get("remixes")?,
            output: r.try_get("output")?,
            levels: content.levels,
        });
    }
    Ok(MinePage {
        items,
        total,
        limit,
        offset,
    })
}

// ---------------------------------------------------------------- preview

#[derive(Debug, Serialize)]
pub struct Preview {
    #[serde(flatten)]
    pub meta: Meta,
    pub level: Level,
    /// The student version: what is printed on the worksheet, nothing else.
    pub sheet: Sheet,
}

async fn visible_meta(db: &PgPool, viewer: i64, id: i64) -> AppResult<(Meta, Content)> {
    let sql = format!(
        "SELECT {CARD_COLUMNS}, NULL::timestamptz AS saved_at, 1::BIGINT AS total
           {CARD_FROM} WHERE w.id = $2 AND {VISIBLE}"
    );
    let row = sqlx::query(&sql)
        .bind(viewer)
        .bind(id)
        .fetch_optional(db)
        .await?
        .ok_or_else(|| AppError::not_found(MSG_NOT_FOUND))?;
    let row = card_row(&row)?;
    Ok((row.meta, row.content))
}

/// Read-only content of one shared worksheet: its student sheet at `level` (the first version
/// when none is asked for), with the pictures it prints. Logged as `community_view`.
pub async fn preview(
    state: &AppState,
    viewer: i64,
    id: i64,
    level: Option<&str>,
) -> AppResult<Preview> {
    require_enabled(&state.db).await?;
    let (meta, content) = visible_meta(&state.db, viewer, id).await?;
    let level = match level.filter(|l| !l.is_empty()) {
        Some(l) => Level::parse(l)
            .ok_or_else(|| AppError::bad_request("level 需為 basic、standard 或 advanced。"))?,
        None => content.levels.first().copied().unwrap_or(Level::Standard),
    };
    if !content.levels.contains(&level) {
        return Err(AppError::bad_request(format!(
            "這份學習單沒有{}。",
            level.label()
        )));
    }
    let mut sheet = project(&meta.title, &content, level, Audience::Student);
    // The passage is printed only where the version prints its own simplified text.
    if let Some(ws) = store::by_id(&state.db, id).await? {
        crate::worksheet::assemble::keep_original_text(&mut sheet, &content, &ws.material);
    }
    let slots: Vec<SlotState> = images::current(&state.db, id).await?;
    images::decorate(&mut sheet, &content, &slots);
    log_usage(
        &state.db,
        viewer,
        "community_view",
        json!({"worksheet_id": id, "own": meta.is_mine, "level": level}),
    )
    .await?;
    Ok(Preview { meta, level, sheet })
}

/// A picture of a shared worksheet that its sheet prints: the storage key and SHA-256.
pub async fn picture(state: &AppState, id: i64, image_id: i64) -> AppResult<(String, String)> {
    require_enabled(&state.db).await?;
    let shared: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM worksheets w WHERE w.id = $1 AND w.shared_at IS NOT NULL
            AND w.deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?;
    if shared.is_none() {
        return Err(AppError::not_found(MSG_NOT_FOUND));
    }
    // Only the current picture of a slot (what the sheet prints), never an older one.
    let current: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM worksheet_images WHERE id = $1 AND worksheet_id = $2 AND current",
    )
    .bind(image_id)
    .bind(id)
    .fetch_optional(&state.db)
    .await?;
    if current.is_none() {
        return Err(AppError::not_found("找不到這張圖。"));
    }
    images::stored(&state.db, id, image_id)
        .await?
        .ok_or_else(|| AppError::not_found("找不到這張圖。"))
}

// ---------------------------------------------------------------- save, unsave

/// The owner of a shared worksheet (to refuse saving and remixing one's own) and whether it may
/// be remixed. 404 when it is not shared or deleted.
async fn shared_owner(db: &PgPool, id: i64) -> AppResult<(i64, bool)> {
    sqlx::query_as(
        "SELECT teacher_id, share_allow_remix FROM worksheets
          WHERE id = $1 AND shared_at IS NOT NULL AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(db)
    .await?
    .ok_or_else(|| AppError::not_found(MSG_NOT_FOUND))
}

pub async fn save(state: &AppState, teacher_id: i64, id: i64) -> AppResult<()> {
    require_enabled(&state.db).await?;
    let (owner, _) = shared_owner(&state.db, id).await?;
    if owner == teacher_id {
        return Err(AppError::bad_request("這是你自己的學習單，不必收藏。"));
    }
    let mut tx = state.db.begin().await?;
    let inserted = sqlx::query(
        "INSERT INTO worksheet_saves (teacher_id, worksheet_id) VALUES ($1, $2)
         ON CONFLICT DO NOTHING",
    )
    .bind(teacher_id)
    .bind(id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if inserted > 0 {
        log_usage(
            &mut *tx,
            teacher_id,
            "community_save",
            json!({"worksheet_id": id}),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Removing a save needs no visible worksheet: a teacher can always clean up their own list.
pub async fn unsave(state: &AppState, teacher_id: i64, id: i64) -> AppResult<()> {
    require_enabled(&state.db).await?;
    let mut tx = state.db.begin().await?;
    let removed =
        sqlx::query("DELETE FROM worksheet_saves WHERE teacher_id = $1 AND worksheet_id = $2")
            .bind(teacher_id)
            .bind(id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
    if removed > 0 {
        log_usage(
            &mut *tx,
            teacher_id,
            "community_unsave",
            json!({"worksheet_id": id}),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

// ---------------------------------------------------------------- remix

/// 拿來改: a private copy of a shared worksheet for the caller. Returns the copy's id.
///
/// The copy is the printed document (material text, settings and the latest version, with its
/// pictures) and nothing that belongs to the owner: no lesson, class, 備課對話, template, SOP,
/// library material, 教師的要求 or brief, no comments, proposals or history, and the ids that
/// point into the owner's SOP and templates are cleared. It starts private. `remixed_from` is
/// the only link back.
pub async fn remix(state: &AppState, teacher_id: i64, id: i64) -> AppResult<i64> {
    let db = &state.db;
    require_enabled(db).await?;
    let (owner, allow_remix) = shared_owner(db, id).await?;
    if owner == teacher_id {
        return Err(AppError::bad_request(
            "這是你自己的學習單，直接在我的作品裡編修就好。",
        ));
    }
    if !allow_remix {
        return Err(AppError::Forbidden("這份學習單的老師沒有開放改編。".into()));
    }
    let source = store::by_id(db, id)
        .await?
        .ok_or_else(|| AppError::not_found(MSG_NOT_FOUND))?;
    let mut content = store::latest(db, id).await?.content;
    for objective in &mut content.objectives {
        objective.sop_id = None;
    }
    for block in &mut content.blocks {
        block.sop_block_id = None;
        block.template_section_id = None;
    }
    let mut material = source.material.clone();
    material.source = MaterialSource {
        kind: SOURCE_PASTED.to_string(),
        ..MaterialSource::default()
    };
    let filename = auto_filename(&material, None);
    let brief = crate::worksheet::Brief::default();

    let mut tx = db.begin().await?;
    // Re-check inside the transaction: a withdrawal that came in meanwhile wins.
    let still: Option<bool> = sqlx::query_scalar(
        "SELECT share_allow_remix FROM worksheets
          WHERE id = $1 AND shared_at IS NOT NULL AND deleted_at IS NULL FOR SHARE",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?;
    if still != Some(true) {
        return Err(if still.is_some() {
            AppError::Forbidden("這份學習單的老師沒有開放改編。".into())
        } else {
            AppError::not_found(MSG_NOT_FOUND)
        });
    }
    let copy: i64 = sqlx::query_scalar(
        "INSERT INTO worksheets (teacher_id, title, filename, material, paragraph_seq, settings,
                                 suggested, brief, generated, remixed_from)
         VALUES ($1, $2, $3, $4, $5, $6, $6, $7, TRUE, $8) RETURNING id",
    )
    .bind(teacher_id)
    .bind(&source.title)
    .bind(&filename)
    .bind(sqlx::types::Json(&material))
    .bind(source.paragraph_seq)
    .bind(sqlx::types::Json(&source.settings))
    .bind(sqlx::types::Json(&brief))
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    // The pictures of the slots: the same stored files (a picture never changes and is never
    // deleted), their own rows without the owner's model call.
    sqlx::query(
        "INSERT INTO worksheet_images (worksheet_id, teacher_id, block_id, slot_id, source, status,
                                       current, storage_key, content_type, bytes, width, height,
                                       sha256, finished_at)
         SELECT $1, $2, block_id, slot_id, source, status, TRUE, storage_key, content_type, bytes,
                width, height, sha256, now()
           FROM worksheet_images
          WHERE worksheet_id = $3 AND current AND status IN ('done', 'removed')",
    )
    .bind(copy)
    .bind(teacher_id)
    .bind(id)
    .execute(&mut *tx)
    .await?;
    store::insert_version(&mut tx, copy, &content, "created").await?;
    log_usage(
        &mut *tx,
        teacher_id,
        "community_remix",
        json!({"worksheet_id": id, "copy_id": copy}),
    )
    .await?;
    tx.commit().await?;
    Ok(copy)
}

/// Where a copy came from, for 「改編自 …」: only while the original is still shared (and the
/// switch is on). Afterwards the copy says nothing about it.
#[derive(Debug, Serialize)]
pub struct RemixSource {
    pub id: i64,
    pub title: String,
    pub owner_name: String,
}

pub async fn remix_source(db: &PgPool, worksheet_id: i64) -> AppResult<Option<RemixSource>> {
    if !crate::settings::community_enabled(db).await? {
        return Ok(None);
    }
    let row: Option<(i64, String, Option<String>)> = sqlx::query_as(
        "SELECT o.id, o.title, CASE WHEN o.share_name = 'code' THEN t.research_id END
           FROM worksheets c
           JOIN worksheets o ON o.id = c.remixed_from
           JOIN teachers t ON t.id = o.teacher_id
          WHERE c.id = $1 AND o.shared_at IS NOT NULL AND o.deleted_at IS NULL",
    )
    .bind(worksheet_id)
    .fetch_optional(db)
    .await?;
    Ok(row.map(|(id, title, code)| RemixSource {
        id,
        title,
        owner_name: owner_name(code),
    }))
}
