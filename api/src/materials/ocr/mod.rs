//! OCR of scanned files with `chandra-ocr-2` (docs/API.md「Phase 2.10：掃描檔與 OCR」).
//!
//! * **When**: in the background after the upload response, for every image, for a PDF
//!   without a text layer (`no_text`) or whose text layer the parser cannot read (`failed`),
//!   and for a PDF whose text layer looks broken ([`poor_text`]); and on request
//!   (`POST /api/materials/:id/ocr`). DOCX and TXT never. Knowledge-base files uploaded before
//!   OCR get their run at startup ([`backfill_platform`]).
//! * **How**: one page image per request ([`render`], [`client`]), at most `OCR_CONCURRENCY`
//!   pages at a time across the server, each page retried once on a timeout or a 5xx; a page
//!   that still fails is recorded and the others go on ([`job`]).
//! * **Result**: the layout-labelled HTML becomes blocks and paragraphs ([`html`]); they
//!   replace the material's text (`text_source = 'ocr'`); the raw blocks stay in
//!   `materials.ocr_pages`. While it runs, `text_status` stays what it was, and worksheets,
//!   prep sessions and templates refuse the material with a 409 ([`check_idle`]), except the
//!   prep session that waits for its worksheet sample (`template_material_id`).
//! * **Restarts**: open jobs are taken over at startup ([`recover`]) and continue with the pages
//!   not done yet; a job that cannot continue is marked failed. Then knowledge-base files
//!   never read are started ([`backfill_platform`]).

pub mod client;
pub mod html;
pub mod job;
pub mod render;

use serde::Serialize;
use serde_json::json;
use sqlx::PgPool;

pub use client::{Ocr, OcrConfig, OcrProviderKind};

use super::catalog::SCOPE_PLATFORM;
use super::extract::{Extracted, TextStatus};
use super::store::{self, MaterialRow, COLS};
use super::{FileType, Kind};
use crate::error::{AppError, AppResult};
use crate::routes::log_usage;
use crate::AppState;

pub const STATUS_NONE: &str = "none";
pub const STATUS_QUEUED: &str = "queued";
pub const STATUS_RUNNING: &str = "running";
pub const STATUS_DONE: &str = "done";
pub const STATUS_FAILED: &str = "failed";

pub const SOURCE_FILE: &str = "file";
pub const SOURCE_OCR: &str = "ocr";

/// `ai_interactions.module` of every page call.
pub const MODULE: &str = "material_ocr";

pub const MSG_DISABLED: &str = "這個環境沒有開啟文字辨識（OCR）。";
pub const MSG_NOT_OCRABLE: &str = "只有上傳的 PDF 與圖片可以用 OCR 重新讀取。";
pub const MSG_STALE: &str = "文字辨識中斷了（可能是系統剛更新）。請按「用 OCR 重新讀取」再試一次。";
pub const MSG_CANNOT_RESUME: &str =
    "系統更新後無法繼續這次文字辨識（OCR 或檔案儲存目前沒有開啟）。請稍後按「用 OCR 重新讀取」。";

/// `ocr` in the material JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MaterialOcr {
    /// `none` | `queued` | `running` | `done` | `failed`
    pub status: String,
    pub done_pages: i32,
    pub total_pages: i32,
    /// Why it failed; with `done`, a note (pages that failed or were skipped).
    pub error: Option<String>,
}

impl MaterialOcr {
    pub fn of(row: &MaterialRow) -> Self {
        Self {
            status: row.ocr_status.clone(),
            done_pages: row.ocr_done_pages,
            total_pages: row.ocr_total_pages,
            error: row.ocr_error.clone(),
        }
    }
}

/// Queued or running.
pub fn is_open(status: &str) -> bool {
    status == STATUS_QUEUED || status == STATUS_RUNNING
}

/// 「這份檔案正在辨識文字（第 n／m 頁），完成後再試一次。」: `n` is the page being read now.
pub fn busy_message(done_pages: i32, total_pages: i32) -> String {
    if total_pages > 0 {
        let n = (done_pages + 1).min(total_pages);
        format!("這份檔案正在辨識文字（第 {n}／{total_pages} 頁），完成後再試一次。")
    } else {
        "這份檔案正在辨識文字，完成後再試一次。".to_string()
    }
}

/// 409 while the material's OCR is queued or running (worksheets, prep sessions, templates).
pub fn check_idle(row: &MaterialRow) -> AppResult<()> {
    if is_open(&row.ocr_status) {
        return Err(AppError::Conflict(busy_message(
            row.ocr_done_pages,
            row.ocr_total_pages,
        )));
    }
    Ok(())
}

/// An uploaded PDF or image with its file.
pub fn can_read(row: &MaterialRow) -> bool {
    row.storage_key.is_some() && (row.ext.as_deref() == Some("pdf") || row.kind == "image")
}

// ---------------------------------------------------------------- when OCR runs by itself

/// Zero-width and format characters above this share of all characters…
pub const FORMAT_CHAR_SHARE: f64 = 0.02;
/// …or fewer visible characters than this per page on average…
pub const MIN_VISIBLE_CHARS_PER_PAGE: f64 = 80.0;
/// …or more than this share of the non-empty lines [`SHORT_LINE_CHARS`] characters long or
/// shorter…
pub const SHORT_LINE_SHARE: f64 = 0.40;
pub const SHORT_LINE_CHARS: usize = 2;
/// …or more replacement characters (U+FFFD) than this: the text layer is not worth reading.
pub const MAX_REPLACEMENT_CHARS: usize = 5;

/// Whether a PDF's text layer looks broken (deterministic; thresholds above). `text` is the
/// extractor's raw page text (lines intact), `pages` the page count.
pub fn poor_text(text: &str, pages: usize) -> bool {
    let total = text.chars().count();
    if total == 0 {
        return true;
    }
    let format = text.chars().filter(|c| html::is_format_char(*c)).count();
    if format as f64 > total as f64 * FORMAT_CHAR_SHARE {
        return true;
    }
    let visible = text
        .chars()
        .filter(|c| !c.is_whitespace() && !c.is_control() && !html::is_format_char(*c))
        .count();
    if (visible as f64) < MIN_VISIBLE_CHARS_PER_PAGE * pages.max(1) as f64 {
        return true;
    }
    let lengths: Vec<usize> = text
        .lines()
        .map(|l| {
            l.trim()
                .chars()
                .filter(|c| !html::is_format_char(*c))
                .count()
        })
        .filter(|n| *n > 0)
        .collect();
    let short = lengths.iter().filter(|n| **n <= SHORT_LINE_CHARS).count();
    if !lengths.is_empty() && short as f64 > lengths.len() as f64 * SHORT_LINE_SHARE {
        return true;
    }
    text.chars().filter(|c| *c == '\u{FFFD}').count() > MAX_REPLACEMENT_CHARS
}

/// Whether an upload is read by OCR right away: images always, PDFs without a text layer or
/// with a broken one. Never DOCX or TXT.
pub fn wants_auto(file_type: FileType, extracted: &Extracted) -> bool {
    match (file_type.kind, file_type.ext) {
        (Kind::Image, _) => true,
        (Kind::Document, "pdf") => match extracted.status {
            // No text layer, or one the PDF parser cannot read (Word exports with CID fonts
            // that `pdf_extract` gives up on, while the pages still render).
            TextStatus::NoText | TextStatus::Failed => true,
            TextStatus::Ready => extracted.poor_text,
            TextStatus::NotApplicable => false,
        },
        _ => false,
    }
}

/// Of `pages` pages, how many are read (`max` at most) and how many skipped.
pub fn pages_to_read(pages: usize, max: usize) -> (usize, usize) {
    let read = pages.min(max);
    (read, pages - read)
}

// ---------------------------------------------------------------- starting a job

/// Opens a new run on a material (`queued`, nothing done yet) and returns the row. `None` when
/// a run is already open. The job itself is started with [`job::spawn`] once the response has
/// been read.
pub async fn open_run<'e, E: sqlx::PgExecutor<'e>>(
    executor: E,
    id: i64,
    total_pages: i32,
) -> sqlx::Result<Option<MaterialRow>> {
    sqlx::query_as(&format!(
        "UPDATE materials
            SET ocr_status = '{STATUS_QUEUED}', ocr_done_pages = 0, ocr_total_pages = $2,
                ocr_error = NULL, ocr_pages = NULL, ocr_started_at = now()
          WHERE id = $1 AND ocr_status NOT IN ('{STATUS_QUEUED}', '{STATUS_RUNNING}')
          RETURNING {COLS}"
    ))
    .bind(id)
    .bind(total_pages)
    .fetch_optional(executor)
    .await
}

/// The page count known before the job runs: 1 for an image, the PDF's (up to the limit) when
/// the text extraction counted them, otherwise 0 until the job counts them.
pub fn known_pages(kind: &str, page_count: Option<i32>, max_pages: usize) -> i32 {
    if kind == "image" {
        return 1;
    }
    page_count.map_or(0, |n| pages_to_read(n.max(0) as usize, max_pages).0 as i32)
}

/// `POST /api/materials/:id/ocr`: checks, then a new run. 503 when OCR is off, 400 for anything
/// but an uploaded PDF or image, 409 while a run is open.
pub async fn start_manual(state: &AppState, row: &MaterialRow) -> AppResult<MaterialRow> {
    if !state.ocr.enabled() {
        return Err(AppError::Unavailable(MSG_DISABLED.into()));
    }
    if !can_read(row) {
        return Err(AppError::bad_request(MSG_NOT_OCRABLE));
    }
    if state.storage.is_none() {
        return Err(store::storage_missing());
    }
    check_idle(row)?;
    let total = known_pages(&row.kind, row.page_count, state.ocr.config().max_pages);
    let mut tx = state.db.begin().await?;
    let Some(opened) = open_run(&mut *tx, row.id, total).await? else {
        return Err(AppError::Conflict(busy_message(0, total)));
    };
    log_usage(
        &mut *tx,
        row.teacher_id,
        "material_ocr_requested",
        json!({ "material_id": row.id, "kind": row.kind, "previous_status": row.ocr_status }),
    )
    .await?;
    tx.commit().await?;
    Ok(opened)
}

/// Starts the job of a run opened by [`open_run`] (no-op for a row without an open run).
pub fn spawn_for(state: &AppState, row: &MaterialRow) {
    if let (true, Some(run)) = (is_open(&row.ocr_status), row.ocr_started_at) {
        job::spawn(state, row.id, run);
    }
}

// ---------------------------------------------------------------- restarts

/// Seconds after which an open run that no job of this process holds is reported as failed:
/// `OCR_TIMEOUT_SECS × pages + 120`.
pub fn stale_after(state: &AppState, total_pages: i32) -> f64 {
    (state.ocr.config().timeout_secs * total_pages.max(1) as u64 + 120) as f64
}

/// Runs of the teacher's materials still open long after they must have ended, and held by no
/// job of this process, are marked failed (the teacher can start another).
pub async fn expire_stale(state: &AppState, teacher_id: i64) -> sqlx::Result<()> {
    sqlx::query(&format!(
        "UPDATE materials SET ocr_status = '{STATUS_FAILED}', ocr_error = $2
          WHERE teacher_id = $1 AND ocr_status IN ('{STATUS_QUEUED}', '{STATUS_RUNNING}')
            AND ocr_started_at < now() - make_interval(
                    secs => $3::float8 * GREATEST(ocr_total_pages, 1) + 120)
            AND NOT (id = ANY($4))"
    ))
    .bind(teacher_id)
    .bind(MSG_STALE)
    .bind(state.ocr.config().timeout_secs as f64)
    .bind(state.ocr.live_ids())
    .execute(&state.db)
    .await?;
    Ok(())
}

#[derive(sqlx::FromRow)]
struct OpenRun {
    id: i64,
    kind: String,
    ext: Option<String>,
    storage_key: Option<String>,
}

/// At startup, before requests are served: every run left open by the previous process (the app runs as one instance) is
/// taken over with a new run time, so a job of an old container still finishing stops
/// writing, and continues with the pages not done yet. A run that cannot continue (OCR or
/// storage now off, no file) is marked failed. Returns how many were resumed.
pub async fn recover(state: &AppState) -> sqlx::Result<usize> {
    let db = &state.db;
    let live = state.ocr.live_ids();
    let open: Vec<OpenRun> = sqlx::query_as(&format!(
        "SELECT id, kind, ext, storage_key FROM materials
          WHERE ocr_status IN ('{STATUS_QUEUED}', '{STATUS_RUNNING}') ORDER BY id"
    ))
    .fetch_all(db)
    .await?;
    let mut resumed = 0;
    for run in open.into_iter().filter(|r| !live.contains(&r.id)) {
        let readable =
            run.storage_key.is_some() && (run.ext.as_deref() == Some("pdf") || run.kind == "image");
        if !state.ocr.enabled() || state.storage.is_none() || !readable {
            sqlx::query(&format!(
                "UPDATE materials SET ocr_status = '{STATUS_FAILED}', ocr_error = $2
                  WHERE id = $1 AND ocr_status IN ('{STATUS_QUEUED}', '{STATUS_RUNNING}')"
            ))
            .bind(run.id)
            .bind(MSG_CANNOT_RESUME)
            .execute(db)
            .await?;
            continue;
        }
        let taken: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(&format!(
            "UPDATE materials SET ocr_status = '{STATUS_QUEUED}', ocr_started_at = now()
              WHERE id = $1 AND ocr_status IN ('{STATUS_QUEUED}', '{STATUS_RUNNING}')
              RETURNING ocr_started_at"
        ))
        .bind(run.id)
        .fetch_optional(db)
        .await?;
        if let Some(at) = taken {
            job::spawn(state, run.id, at);
            resumed += 1;
        }
    }
    Ok(resumed)
}

/// At startup, after [`recover`]: knowledge-base files (`scope = 'platform'`) that were never
/// read by OCR although an upload today would be — PDFs without a readable text layer
/// (`no_text`, `failed`) and images, typically uploaded before OCR existed or while it was off —
/// get a run. Teachers' own files are left to「用 OCR 重新讀取」. Nothing happens without OCR or
/// storage. Returns how many runs were started.
pub async fn backfill_platform(state: &AppState) -> sqlx::Result<usize> {
    if !state.ocr.enabled() || state.storage.is_none() {
        return Ok(0);
    }
    let rows: Vec<(i64, String, Option<i32>)> = sqlx::query_as(&format!(
        "SELECT id, kind, page_count FROM materials
          WHERE scope = '{SCOPE_PLATFORM}' AND ocr_status = '{STATUS_NONE}'
            AND storage_key IS NOT NULL
            AND ((ext = 'pdf' AND text_status IN ('{no_text}', '{failed}')) OR kind = 'image')
          ORDER BY id",
        no_text = TextStatus::NoText.as_str(),
        failed = TextStatus::Failed.as_str(),
    ))
    .fetch_all(&state.db)
    .await?;
    let max_pages = state.ocr.config().max_pages;
    let mut started = 0;
    for (id, kind, page_count) in rows {
        let total = known_pages(&kind, page_count, max_pages);
        if let Some(row) = open_run(&state.db, id, total).await? {
            spawn_for(state, &row);
            started += 1;
        }
    }
    Ok(started)
}

/// The OCR state of one material, for waiting on it (`None` when it was deleted).
pub async fn status_of(db: &PgPool, id: i64) -> sqlx::Result<Option<(String, Option<String>)>> {
    sqlx::query_as("SELECT ocr_status, ocr_error FROM materials WHERE id = $1")
        .bind(id)
        .fetch_optional(db)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::materials::file_type_for_name;

    fn text_pages(lines: &[&str], pages: usize) -> (String, usize) {
        (lines.join("\n"), pages)
    }

    #[test]
    fn a_clean_text_layer_is_not_poor() {
        let line = "Mrs. Lin put an umbrella stand outside her shop on a rainy Monday morning.";
        let (text, pages) = text_pages(&[line; 6], 2);
        assert!(!poor_text(&text, pages));
        let chinese =
            "林太太在下雨的星期一早上，把一個傘架放在店門外，旁邊的牌子寫著需要就拿一把。"
                .repeat(3);
        assert!(!poor_text(&chinese, 1));
    }

    #[test]
    fn zero_width_characters_above_two_percent() {
        let base = "x".repeat(980);
        // 20 of 1000 characters is 2 %: not more than 2 %.
        let at_limit = format!("{base}{}", "\u{200B}".repeat(20));
        assert!(!poor_text(&at_limit, 1));
        let over = format!("{base}{}", "\u{FEFF}\u{2060}".repeat(11));
        assert!(poor_text(&over, 1));
    }

    #[test]
    fn too_little_text_per_page() {
        let page = "y".repeat(80);
        assert!(!poor_text(&page, 1), "80 per page is enough");
        assert!(poor_text(&"y".repeat(79), 1));
        assert!(poor_text(&page, 2), "40 per page on average");
        // Whitespace and format characters are not visible.
        assert!(poor_text(
            &format!("{}{}", "z ".repeat(79), "\n".repeat(50)),
            1
        ));
    }

    #[test]
    fn mostly_one_or_two_character_lines() {
        let long = "A complete line of text that is clearly long enough to count.";
        let mut lines = vec![long; 6];
        lines.extend(["表", "格", "12", "ab"]);
        // 4 of 10 lines short: 40 % is not more than 40 %.
        assert!(!poor_text(&lines.join("\n"), 1));
        lines.push("x");
        assert!(poor_text(&lines.join("\n"), 1));
        // Blank lines do not count.
        let spaced = format!("{}\n\n\n\n\n\n", [long; 3].join("\n"));
        assert!(!poor_text(&spaced, 1));
    }

    #[test]
    fn replacement_characters_above_five() {
        let base = "w".repeat(400);
        assert!(!poor_text(&format!("{base}{}", "\u{FFFD}".repeat(5)), 1));
        assert!(poor_text(&format!("{base}{}", "\u{FFFD}".repeat(6)), 1));
        assert!(poor_text("", 1));
    }

    #[test]
    fn which_uploads_are_read_by_ocr() {
        let ty = |n: &str| file_type_for_name(n).unwrap();
        let extracted = |status, poor_text| Extracted {
            status,
            blocks: Vec::new(),
            page_count: Some(1),
            truncated: false,
            poor_text,
        };
        assert!(wants_auto(
            ty("a.png"),
            &extracted(TextStatus::NotApplicable, false)
        ));
        assert!(wants_auto(
            ty("a.gif"),
            &extracted(TextStatus::NotApplicable, false)
        ));
        assert!(wants_auto(
            ty("a.pdf"),
            &extracted(TextStatus::NoText, false)
        ));
        assert!(wants_auto(ty("a.pdf"), &extracted(TextStatus::Ready, true)));
        assert!(!wants_auto(
            ty("a.pdf"),
            &extracted(TextStatus::Ready, false)
        ));
        assert!(wants_auto(
            ty("a.pdf"),
            &extracted(TextStatus::Failed, false)
        ));
        assert!(!wants_auto(
            ty("a.docx"),
            &extracted(TextStatus::NoText, true)
        ));
        assert!(!wants_auto(
            ty("a.txt"),
            &extracted(TextStatus::Ready, true)
        ));
        assert!(!wants_auto(
            ty("a.mp4"),
            &extracted(TextStatus::NotApplicable, false)
        ));
    }

    #[test]
    fn the_page_limit() {
        assert_eq!(pages_to_read(3, 40), (3, 0));
        assert_eq!(pages_to_read(40, 40), (40, 0));
        assert_eq!(pages_to_read(52, 40), (40, 12));
        assert_eq!(pages_to_read(0, 40), (0, 0));
        assert_eq!(known_pages("image", None, 40), 1);
        assert_eq!(known_pages("document", Some(52), 40), 40);
        assert_eq!(known_pages("document", Some(2), 40), 2);
        assert_eq!(known_pages("document", None, 40), 0);
    }

    #[test]
    fn the_busy_message_names_the_page_being_read() {
        assert_eq!(
            busy_message(2, 8),
            "這份檔案正在辨識文字（第 3／8 頁），完成後再試一次。"
        );
        assert_eq!(
            busy_message(8, 8),
            "這份檔案正在辨識文字（第 8／8 頁），完成後再試一次。"
        );
        assert_eq!(busy_message(0, 0), "這份檔案正在辨識文字，完成後再試一次。");
    }
}
