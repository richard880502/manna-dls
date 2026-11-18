//! The first page of a worksheet's student sheet as a small picture, for the cards of 我的作品
//! (`GET /api/worksheets/:id/thumbnail`, docs/API.md「我的作品：學習單預覽圖」).
//!
//! The page is the one the PDF export prints (default level, student version, pictures
//! included), rendered by the same code ([`super::export::pdf_with_images`]), turned into a
//! picture by `pdftoppm` and shrunk like a material's preview
//! ([`crate::materials::thumbnail`]). It is a preview, not an export: nothing is logged.
//!
//! A rendered preview is kept in the file storage under
//! `thumbs/worksheets/<id>/v<version_no>-<fingerprint>-w480.jpg`. The key changes whenever the
//! printed page could: a new version, a picture that finished, was replaced or removed
//! (pictures arrive after a generation without a new version), or a changed title / material
//! text. Keys of older states are not removed when a newer one is rendered; only the current
//! key goes with the worksheet ([`forget`]).

use std::future::Future;

use sha2::{Digest, Sha256};
use sqlx::PgPool;

use crate::error::{AppError, AppResult};
use crate::materials::ocr::render::{self, WorkDir};
use crate::materials::thumbnail::{
    read_all, shrink, MIME, MSG_NO_PDFTOPPM, MSG_NO_STORAGE, MSG_UNREADABLE, WIDTH,
};
use crate::storage::StorageError;
use crate::worksheet::store::{self, Worksheet};
use crate::AppState;

pub const CONTENT_TYPE: &str = MIME;
pub const MSG_NOT_GENERATED: &str = "這份學習單還沒產生，沒有預覽圖。";
pub const MSG_RUNNING: &str = "這份學習單還在產生中，預覽圖等一下才有。";

/// What the printed first page depends on besides the version: the pictures (how many are
/// done and the newest one's id: a replacement is a new row) and a short hash of the title and
/// the material text, which are not part of a version.
pub async fn fingerprint(db: &PgPool, ws: &Worksheet) -> sqlx::Result<String> {
    let (done, newest): (i64, Option<i64>) = sqlx::query_as(
        "SELECT COUNT(*), MAX(id) FROM worksheet_images
          WHERE worksheet_id = $1 AND current AND status = 'done'",
    )
    .bind(ws.id)
    .fetch_one(db)
    .await?;
    let mut hash = Sha256::new();
    hash.update(ws.title.as_bytes());
    hash.update([0]);
    hash.update(serde_json::to_vec(&ws.material).unwrap_or_default());
    let digest = hash.finalize();
    let short: String = digest.iter().take(4).map(|b| format!("{b:02x}")).collect();
    Ok(format!("p{done}-{}-{short}", newest.unwrap_or(0)))
}

/// Where the preview of this state of the worksheet is kept.
pub fn cache_key(worksheet_id: i64, version_no: i32, fingerprint: &str) -> String {
    format!("thumbs/worksheets/{worksheet_id}/v{version_no}-{fingerprint}-w{WIDTH}.jpg")
}

/// The preview's validator: the same facts as the key.
pub fn etag(worksheet_id: i64, version_no: i32, fingerprint: &str) -> String {
    format!("\"w{worksheet_id}-v{version_no}-{fingerprint}-w{WIDTH}\"")
}

/// The kept preview under `key`, or the one `make_pdf` builds now (and keeps). `make_pdf` runs
/// inside one of the server's preview slots.
pub async fn get_or_render<F, Fut>(state: &AppState, key: &str, make_pdf: F) -> AppResult<Vec<u8>>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = AppResult<Vec<u8>>>,
{
    let Some(storage) = state.storage.as_deref() else {
        return Err(AppError::Unavailable(MSG_NO_STORAGE.into()));
    };
    match read_all(storage, key).await {
        Ok(bytes) => return Ok(bytes),
        Err(StorageError::NotFound) => {}
        Err(e) => tracing::warn!(key, error = %e, "reading a kept worksheet preview failed"),
    }
    if !render::have_pdftoppm().await {
        return Err(AppError::Unavailable(MSG_NO_PDFTOPPM.into()));
    }
    let _slot = state
        .thumb_slots
        .acquire()
        .await
        .map_err(|e| anyhow::anyhow!("preview slots closed: {e}"))?;
    // Another request may have rendered it while this one waited for a slot.
    if let Ok(bytes) = read_all(storage, key).await {
        return Ok(bytes);
    }
    let pdf = make_pdf().await?;
    let dir = WorkDir::new().map_err(anyhow::Error::from)?;
    let file = dir.path().join("sheet.pdf");
    tokio::fs::write(&file, &pdf)
        .await
        .map_err(anyhow::Error::from)?;
    drop(pdf);
    let page = render::render_pdf_page_at(&file, dir.path(), 1, WIDTH)
        .await
        .map_err(|e| {
            tracing::warn!(key, error = %e, "rendering a worksheet preview failed");
            AppError::not_found(MSG_UNREADABLE)
        })?;
    let bytes = tokio::task::spawn_blocking(move || shrink(&page))
        .await
        .map_err(|e| anyhow::anyhow!("preview task failed: {e}"))?
        .map_err(|e| {
            tracing::warn!(key, error = %e, "decoding a worksheet preview failed");
            AppError::not_found(MSG_UNREADABLE)
        })?;
    if let Err(e) = storage.put(key, MIME, bytes.clone().into()).await {
        // Sent anyway; the next request renders it again.
        tracing::warn!(key, error = %e, "keeping a worksheet preview failed");
    }
    Ok(bytes)
}

/// Removes the preview kept for the worksheet's current state. Best effort: a failure is
/// logged and never stops the delete. Previews of older states stay in the storage.
pub async fn forget(state: &AppState, ws: &Worksheet) {
    let Some(storage) = state.storage.as_deref() else {
        return;
    };
    let result = async {
        let latest = store::latest(&state.db, ws.id).await?;
        let fp = fingerprint(&state.db, ws).await?;
        let key = cache_key(ws.id, latest.version_no, &fp);
        match storage.delete(&key).await {
            Ok(()) | Err(StorageError::NotFound) => Ok(()),
            Err(e) => Err(AppError::Internal(e.into())),
        }
    }
    .await;
    if let Err(e) = result {
        tracing::warn!(worksheet_id = ws.id, error = %e, "removing a worksheet preview failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_valid_and_change_with_the_state() {
        let a = cache_key(7, 3, "p2-41-ab12cd34");
        assert_eq!(a, "thumbs/worksheets/7/v3-p2-41-ab12cd34-w480.jpg");
        assert!(crate::storage::valid_key(&a));
        assert_ne!(a, cache_key(7, 4, "p2-41-ab12cd34"));
        assert_ne!(a, cache_key(7, 3, "p3-42-ab12cd34"));
        assert_eq!(etag(7, 3, "x"), "\"w7-v3-x-w480\"");
    }
}
