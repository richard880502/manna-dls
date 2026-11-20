//! The OCR job of one material: render the pages, read them (at most `OCR_CONCURRENCY` pages
//! at a time across the server), record each page as it comes, then replace the material's
//! text.
//!
//! A run is identified by `ocr_started_at`: every write is guarded by it (and by the run still
//! being open), so a run that was taken over (restart) or whose material was deleted stops
//! quietly at its next write. Pages already in `ocr_pages` are not read again, which is how a
//! resumed run continues.
//!
//! Every model call is an `ai_interactions` row (module `material_ocr`) with the page number,
//! the image's size and SHA-256 and the output length: never the image or the text.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Instant;

use chrono::{DateTime, Utc};
use futures_util::{StreamExt, TryStreamExt};
use serde_json::json;
use sqlx::types::Json;

use super::client::{FakeHints, OcrError, PageImage, MAX_TOKENS, PROMPT};
use super::html::{self, OcrPage};
use super::render::{self, WorkDir};
use super::{
    pages_to_read, MODULE, MSG_DISABLED, MSG_NOT_OCRABLE, SOURCE_OCR, STATUS_DONE, STATUS_FAILED,
    STATUS_RUNNING,
};
use crate::ai::pipeline::{finish_interaction, insert_interaction, Finish, InteractionMeta};
use crate::audit::log_usage;
use crate::auth::sha256_hex;
use crate::error::AppResult;
use crate::materials::detect;
use crate::materials::extract::{Block, Extracted, TextStatus};
use crate::sop::{assign_paragraph_ids, Paragraph};
use crate::AppState;

const MSG_INTERNAL: &str = "文字辨識時發生錯誤。請按「用 OCR 重新讀取」再試一次。";
const MSG_NO_STORAGE: &str = "這個環境目前沒有連上檔案儲存空間，無法讀取原檔來辨識文字。";
const MSG_FILE_GONE: &str = "讀不到這份教材的原檔，無法辨識文字。";
const MSG_NO_PDFTOPPM: &str =
    "這個環境沒有安裝 PDF 轉圖工具（pdftoppm），無法辨識 PDF。請聯絡計畫團隊。";
const MSG_PDF_UNREADABLE: &str = "這份 PDF 打不開（可能已加密或損壞），無法辨識文字。";
const MSG_RENDER_FAILED: &str = "這一頁轉成圖片失敗";
const MSG_NO_TEXT_FOUND: &str = "辨識完成，但沒有找到文字。";

/// Starts the job of the run opened at `run` (see [`super::open_run`]). The caller has read its
/// response already: a fast job could otherwise finish first.
pub fn spawn(state: &AppState, material_id: i64, run: DateTime<Utc>) {
    let state = state.clone();
    // Registered now, so `expire_stale` never takes a queued job for a dead one.
    let live = state.ocr.track(material_id);
    tokio::spawn(async move {
        let _live = live;
        let work = {
            let state = state.clone();
            async move { run_job(&state, material_id, run).await }
        };
        let failed = crate::jobs::run("ocr", material_id, work).await.is_err();
        if failed {
            if let Err(e) = close_failed(&state, material_id, run, MSG_INTERNAL).await {
                tracing::error!(material_id, error = %e, "could not close the OCR run");
            }
        }
    });
}

#[derive(sqlx::FromRow)]
struct JobRow {
    teacher_id: i64,
    kind: String,
    ext: Option<String>,
    filename: Option<String>,
    storage_key: Option<String>,
    page_count: Option<i32>,
    ocr_pages: Option<Json<Vec<OcrPage>>>,
}

enum Source {
    Pdf { dir: WorkDir, file: PathBuf },
    Image(PageImage),
}

/// The run's guard: the material, this run, still open (`queued` or `running`).
const RUN_GUARD: &str = "id = $1 AND ocr_started_at = $2 AND ocr_status IN ('queued', 'running')";

async fn close_failed(
    state: &AppState,
    id: i64,
    run: DateTime<Utc>,
    message: &str,
) -> sqlx::Result<()> {
    sqlx::query(&format!(
        "UPDATE materials SET ocr_status = '{STATUS_FAILED}', ocr_error = $3 WHERE {RUN_GUARD}"
    ))
    .bind(id)
    .bind(run)
    .bind(message)
    .execute(&state.db)
    .await?;
    Ok(())
}

async fn run_job(state: &AppState, id: i64, run: DateTime<Utc>) -> AppResult<()> {
    let db = &state.db;
    // At most `OCR_CONCURRENCY` files at once: the file (or its rendered pages) is held from
    // here on. A run waiting here is `queued`.
    let _file_slot = state
        .ocr
        .files
        .clone()
        .acquire_owned()
        .await
        .map_err(|e| anyhow::anyhow!("OCR slots closed: {e}"))?;
    let started: Option<JobRow> = sqlx::query_as(&format!(
        "UPDATE materials SET ocr_status = '{STATUS_RUNNING}' WHERE {RUN_GUARD}
         RETURNING teacher_id, kind, ext, filename, storage_key, page_count, ocr_pages"
    ))
    .bind(id)
    .bind(run)
    .fetch_optional(db)
    .await?;
    let Some(row) = started else {
        return Ok(());
    };
    let fail = |message: &'static str| close_failed(state, id, run, message);
    if !state.ocr.enabled() {
        return Ok(fail(MSG_DISABLED).await?);
    }
    let Some(storage) = state.storage.clone() else {
        return Ok(fail(MSG_NO_STORAGE).await?);
    };
    let Some(key) = row.storage_key.clone() else {
        return Ok(fail(MSG_NOT_OCRABLE).await?);
    };
    let data: Vec<u8> = match storage.get(&key, None).await {
        Ok(stream) => match stream
            .try_fold(Vec::new(), |mut acc, chunk| async move {
                acc.extend_from_slice(&chunk);
                Ok(acc)
            })
            .await
        {
            Ok(data) => data,
            Err(e) => {
                tracing::warn!(material_id = id, error = %e, "reading the file for OCR failed");
                return Ok(fail(MSG_FILE_GONE).await?);
            }
        },
        Err(e) => {
            tracing::warn!(material_id = id, error = %e, "reading the file for OCR failed");
            return Ok(fail(MSG_FILE_GONE).await?);
        }
    };

    // ---- the pages
    let ext = row.ext.clone().unwrap_or_default();
    let (source, pages) = if ext == "pdf" {
        if !render::have_pdftoppm().await {
            return Ok(fail(MSG_NO_PDFTOPPM).await?);
        }
        let dir = WorkDir::new().map_err(anyhow::Error::from)?;
        let file = dir.path().join("source.pdf");
        tokio::fs::write(&file, &data)
            .await
            .map_err(anyhow::Error::from)?;
        drop(data);
        let pages = match row.page_count.filter(|n| *n > 0) {
            Some(n) => n,
            None => match render::pdf_page_count(&file).await {
                Ok(n) if n > 0 => n,
                Ok(_) | Err(_) => return Ok(fail(MSG_PDF_UNREADABLE).await?),
            },
        };
        (Source::Pdf { dir, file }, pages)
    } else if row.kind == "image" {
        let prepared = tokio::task::spawn_blocking(move || render::prepare_image(&data, &ext))
            .await
            .map_err(|e| anyhow::anyhow!("image task failed: {e}"))?;
        match prepared {
            Ok(image) => (Source::Image(image), 1),
            Err(message) => {
                close_failed(state, id, run, &message).await?;
                return Ok(());
            }
        }
    } else {
        return Ok(fail(MSG_NOT_OCRABLE).await?);
    };
    let (to_read, skipped) = pages_to_read(pages as usize, state.ocr.config().max_pages);
    let counted = sqlx::query(&format!(
        "UPDATE materials SET ocr_total_pages = $3,
                page_count = CASE WHEN ext = 'pdf' THEN COALESCE(page_count, $4) ELSE page_count END
          WHERE {RUN_GUARD}"
    ))
    .bind(id)
    .bind(run)
    .bind(to_read as i32)
    .bind(pages)
    .execute(db)
    .await?;
    if counted.rows_affected() == 0 {
        return Ok(());
    }

    // ---- read them; a resumed run skips the pages it already has
    let done: HashSet<i32> = row
        .ocr_pages
        .as_ref()
        .map(|p| p.0.iter().map(|p| p.page).collect())
        .unwrap_or_default();
    let todo: Vec<i32> = (1..=to_read as i32).filter(|n| !done.contains(n)).collect();
    let ctx = PageCtx {
        state,
        material_id: id,
        teacher_id: row.teacher_id,
        filename: row.filename.clone().unwrap_or_default(),
        source: &source,
    };
    let mut results = futures_util::stream::iter(todo)
        .map(|page| ctx.read(page))
        .buffer_unordered(state.ocr.config().concurrency.max(1));
    while let Some(page) = results.next().await {
        let page = page?;
        let stored = sqlx::query(&format!(
            "UPDATE materials
                SET ocr_pages = COALESCE(ocr_pages, '[]'::jsonb) || jsonb_build_array($3::jsonb),
                    ocr_done_pages = jsonb_array_length(COALESCE(ocr_pages, '[]'::jsonb)) + 1
              WHERE {RUN_GUARD}"
        ))
        .bind(id)
        .bind(run)
        .bind(Json(&page))
        .execute(db)
        .await?;
        if stored.rows_affected() == 0 {
            // Taken over or deleted: stop here.
            return Ok(());
        }
    }
    drop(results);
    drop(source);
    finish(state, id, run, pages, skipped).await
}

struct PageCtx<'a> {
    state: &'a AppState,
    material_id: i64,
    teacher_id: i64,
    filename: String,
    source: &'a Source,
}

impl PageCtx<'_> {
    /// One page: rendered, read (retried once), turned into blocks. A failure is a page with
    /// an error, not an error of the job.
    async fn read(&self, page: i32) -> AppResult<OcrPage> {
        let _slot = self
            .state
            .ocr
            .pages
            .clone()
            .acquire_owned()
            .await
            .map_err(|e| anyhow::anyhow!("OCR slots closed: {e}"))?;
        let rendered;
        let image = match self.source {
            Source::Image(image) => image,
            Source::Pdf { dir, file } => {
                match render::render_pdf_page(file, dir.path(), page).await {
                    Ok(bytes) => {
                        rendered = PageImage {
                            bytes,
                            mime: "image/png",
                        };
                        &rendered
                    }
                    Err(e) => {
                        tracing::warn!(material_id = self.material_id, page, error = %e,
                        "rendering a PDF page failed");
                        return Ok(OcrPage::failed(page, MSG_RENDER_FAILED));
                    }
                }
            }
        };
        Ok(match self.call(page, image).await? {
            Ok(html) => OcrPage::read(page, &html),
            Err(e) => OcrPage::failed(page, e.page_message()),
        })
    }

    /// The model call, logged; once more after a timeout or a 5xx.
    async fn call(&self, page: i32, image: &PageImage) -> AppResult<Result<String, OcrError>> {
        let state = self.state;
        let db = &state.db;
        let meta = InteractionMeta {
            teacher_id: self.teacher_id,
            module: MODULE,
            unit_id: None,
            lesson_id: None,
            scaffold_id: None,
            prompt_version_id: None,
        };
        let snapshot = json!({
            "material_id": self.material_id,
            "page": page,
            "image": { "bytes": image.bytes.len(), "sha256": sha256_hex(&image.bytes),
                       "content_type": image.mime },
            "request": { "prompt": PROMPT, "max_tokens": MAX_TOKENS },
        });
        let hints = FakeHints {
            filename: self.filename.clone(),
            page,
        };
        let mut retry_of = None;
        let mut attempt = 1;
        loop {
            let interaction = insert_interaction(
                db,
                &meta,
                state.ocr.provider_name(),
                state.ocr.model(),
                &snapshot,
                attempt,
                retry_of,
            )
            .await?;
            let started = Instant::now();
            let reply = state.ocr.read_page(image, &hints).await;
            let latency_ms = started.elapsed().as_millis().min(i32::MAX as u128) as i32;
            let finish = match &reply {
                Ok(r) => Finish {
                    status: "shown",
                    output: Some(json!({ "chars": r.html.chars().count() })),
                    citations: Vec::new(),
                    check_results: None,
                    model_version: r.model_version.clone(),
                    error_detail: None,
                    latency_ms,
                },
                Err(e) => Finish {
                    status: e.interaction_status(),
                    output: None,
                    citations: Vec::new(),
                    check_results: None,
                    model_version: None,
                    error_detail: Some(e.to_string()),
                    latency_ms,
                },
            };
            finish_interaction(db, interaction, finish).await?;
            match reply {
                Ok(r) => return Ok(Ok(r.html)),
                Err(e) if attempt == 1 && e.retryable() => {
                    retry_of = Some(interaction);
                    attempt = 2;
                }
                Err(e) => return Ok(Err(e)),
            }
        }
    }
}

/// What the pages give: the blocks of the material's text (with their page), the pages that
/// failed, and whether any page was read.
#[derive(Debug, Default, PartialEq)]
pub struct Assembled {
    pub blocks: Vec<Block>,
    pub failed_pages: Vec<i32>,
    pub read_pages: usize,
}

/// The pages in page order (one entry per page; the first wins).
pub fn assemble(pages: &[OcrPage]) -> Assembled {
    let mut sorted: Vec<&OcrPage> = pages.iter().collect();
    sorted.sort_by_key(|p| p.page);
    sorted.dedup_by_key(|p| p.page);
    let mut out = Assembled::default();
    for p in sorted {
        if p.error.is_some() {
            out.failed_pages.push(p.page);
            continue;
        }
        out.read_pages += 1;
        out.blocks
            .extend(html::paragraphs(&p.blocks).into_iter().map(|text| Block {
                text,
                page: Some(p.page),
            }));
    }
    out
}

/// The note kept with a finished run: pages that failed, pages skipped.
pub fn note(failed_pages: &[i32], pages: i32, skipped: usize) -> Option<String> {
    let mut parts = Vec::new();
    if !failed_pages.is_empty() {
        let list: Vec<String> = failed_pages.iter().map(i32::to_string).collect();
        parts.push(format!("第 {} 頁沒有辨識成功。", list.join("、")));
    }
    if skipped > 0 {
        parts.push(format!(
            "這份檔案有 {pages} 頁，只辨識了前 {} 頁。",
            pages as usize - skipped
        ));
    }
    (!parts.is_empty()).then(|| parts.join(""))
}

#[derive(sqlx::FromRow)]
struct FinishRow {
    teacher_id: i64,
    paragraphs: Json<Vec<Paragraph>>,
    paragraph_seq: i32,
    ocr_pages: Option<Json<Vec<OcrPage>>>,
}

/// Replaces the material's text with what was read (or records why nothing was).
async fn finish(
    state: &AppState,
    id: i64,
    run: DateTime<Utc>,
    pages: i32,
    skipped: usize,
) -> AppResult<()> {
    let mut tx = state.db.begin().await?;
    let row: Option<FinishRow> = sqlx::query_as(&format!(
        "SELECT teacher_id, paragraphs, paragraph_seq, ocr_pages FROM materials
          WHERE {RUN_GUARD} FOR UPDATE"
    ))
    .bind(id)
    .bind(run)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(row) = row else {
        return Ok(());
    };
    let mut stored = row.ocr_pages.map(|p| p.0).unwrap_or_default();
    stored.sort_by_key(|p| p.page);
    let assembled = assemble(&stored);
    let mut note = note(&assembled.failed_pages, pages, skipped);
    let extracted = Extracted::from_blocks(assembled.blocks, None);
    let (status, chars) = if assembled.read_pages == 0 {
        let why = stored
            .iter()
            .find_map(|p| p.error.clone())
            .unwrap_or_else(|| MSG_INTERNAL.to_string());
        note = Some(format!("沒有一頁辨識成功（{why}）。"));
        sqlx::query(&format!(
            "UPDATE materials SET ocr_status = '{STATUS_FAILED}', ocr_error = $3, ocr_pages = $4
              WHERE {RUN_GUARD}"
        ))
        .bind(id)
        .bind(run)
        .bind(&note)
        .bind(Json(&stored))
        .execute(&mut *tx)
        .await?;
        (STATUS_FAILED, 0)
    } else if extracted.status != TextStatus::Ready {
        // Nothing legible: the material keeps its text.
        let note = Some(match note {
            Some(n) => format!("{MSG_NO_TEXT_FOUND}{n}"),
            None => MSG_NO_TEXT_FOUND.to_string(),
        });
        sqlx::query(&format!(
            "UPDATE materials SET ocr_status = '{STATUS_DONE}', ocr_error = $3, ocr_pages = $4
              WHERE {RUN_GUARD}"
        ))
        .bind(id)
        .bind(run)
        .bind(&note)
        .bind(Json(&stored))
        .execute(&mut *tx)
        .await?;
        (STATUS_DONE, 0)
    } else {
        let text = extracted.text();
        let (mut paragraphs, seq) = assign_paragraph_ids(
            &row.paragraphs.0,
            row.paragraph_seq,
            extracted.blocks.iter().map(|b| b.text.clone()).collect(),
        );
        for (p, block) in paragraphs.iter_mut().zip(&extracted.blocks) {
            p.page = block.page;
        }
        sqlx::query(&format!(
            "UPDATE materials
                SET text = $3, paragraphs = $4, paragraph_seq = $5,
                    text_status = '{ready}', text_truncated = $6, text_source = '{SOURCE_OCR}',
                    looks_like_worksheet = $7,
                    revision = revision + CASE WHEN text = $3 THEN 0 ELSE 1 END,
                    updated_at = now(),
                    ocr_status = '{STATUS_DONE}', ocr_error = $8, ocr_pages = $9
              WHERE {RUN_GUARD}",
            ready = TextStatus::Ready.as_str(),
        ))
        .bind(id)
        .bind(run)
        .bind(&text)
        .bind(Json(&paragraphs))
        .bind(seq)
        .bind(extracted.truncated || skipped > 0)
        .bind(detect::looks_like_worksheet(&text))
        .bind(&note)
        .bind(Json(&stored))
        .execute(&mut *tx)
        .await?;
        (STATUS_DONE, text.chars().count())
    };
    log_usage(
        &mut *tx,
        row.teacher_id,
        "material_ocr_finished",
        json!({
            "material_id": id, "status": status, "pages": pages, "read_pages": assembled.read_pages,
            "failed_pages": assembled.failed_pages, "skipped_pages": skipped, "chars": chars,
            "replaced_text": chars > 0,
        }),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::materials::ocr::html::OcrBlock;

    fn page(n: i32, texts: &[(&str, &str)]) -> OcrPage {
        OcrPage {
            page: n,
            blocks: texts
                .iter()
                .map(|(label, text)| OcrBlock {
                    label: label.to_string(),
                    text: text.to_string(),
                    bbox: None,
                })
                .collect(),
            pictures: 0,
            error: None,
        }
    }

    #[test]
    fn pages_are_put_in_order_and_failed_ones_noted() {
        let pages = vec![
            page(3, &[("Text", "third")]),
            page(1, &[("Section-Header", "Page 1"), ("Page-Footer", "1")]),
            OcrPage::failed(2, "辨識逾時"),
            page(1, &[("Text", "duplicate of page 1")]),
        ];
        let a = assemble(&pages);
        assert_eq!(
            a.blocks,
            [
                Block {
                    text: "Page 1".into(),
                    page: Some(1)
                },
                Block {
                    text: "third".into(),
                    page: Some(3)
                }
            ]
        );
        assert_eq!(a.failed_pages, [2]);
        assert_eq!(a.read_pages, 2);
        assert_eq!(
            note(&a.failed_pages, 52, 12).unwrap(),
            "第 2 頁沒有辨識成功。這份檔案有 52 頁，只辨識了前 40 頁。"
        );
        assert_eq!(note(&[2, 5], 5, 0).unwrap(), "第 2、5 頁沒有辨識成功。");
        assert_eq!(note(&[], 3, 0), None);
    }
}
