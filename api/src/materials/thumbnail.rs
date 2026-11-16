//! Preview images for the 知識庫 gallery (docs/API.md「知識庫：教材牆與預覽圖」),
//! `GET /api/materials/:id/thumbnail`.
//!
//! * **PDF**: page 1, rendered by `pdftoppm` ([`render::render_pdf_page_at`]) [`WIDTH`] px wide.
//! * **Images**: turned upright by their EXIF orientation and scaled down to fit [`WIDTH`] ×
//!   [`MAX_HEIGHT`] (never up).
//!
//! Both are sent as a JPEG (transparent pixels on white). A preview is rendered once per file:
//! it is kept in the file storage under `thumbs/w480/<SHA-256 of the original>.jpg` and removed
//! with the last material that holds that file ([`forget`], called by the hard delete). Any
//! material with the same file may be shown the same preview: whoever may read one of them
//! already has those bytes.

use std::io::Cursor;

use futures_util::TryStreamExt;
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::DynamicImage;

use super::ocr::render::{self, WorkDir};
use super::store::MaterialRow;
use crate::error::{AppError, AppResult};
use crate::storage::{Storage, StorageError};
use crate::AppState;

/// Width of a preview.
pub const WIDTH: u32 = 480;
/// An image preview is at most this tall (a long scroll of a screenshot stays a card).
pub const MAX_HEIGHT: u32 = 3 * WIDTH;
pub const MIME: &str = "image/jpeg";
const JPEG_QUALITY: u8 = 80;
/// Previews rendered at once across the server: each holds its original file in memory.
pub const MAX_CONCURRENT_RENDERS: usize = 2;

pub const MSG_NONE: &str = "這份教材沒有預覽圖（只有上傳的 PDF 與圖片有）。";
const MSG_NO_STORAGE: &str = "這個環境目前沒有連上檔案儲存空間，無法產生預覽圖。";
const MSG_FILE_GONE: &str = "原檔已經不在儲存空間裡了。";
const MSG_FILE_UNAVAILABLE: &str = "原檔暫時讀不到，請稍後再試。";
const MSG_NO_PDFTOPPM: &str = "這個環境沒有安裝 PDF 轉圖工具（pdftoppm），無法產生預覽圖。";
const MSG_UNREADABLE: &str = "這份檔案轉不成預覽圖（可能已加密或損壞）。";
const MSG_FORGET_FAILED: &str =
    "預覽圖沒有刪除成功（儲存空間暫時無法使用），所以這份教材還沒有刪除。請稍後再試一次。";

fn is_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Whether the material has a preview: an uploaded PDF or image whose file is stored.
pub fn has_preview(row: &MaterialRow) -> bool {
    row.sha256.as_deref().is_some_and(is_sha256) && super::ocr::can_read(row)
}

/// Where the preview of the file with this SHA-256 is kept.
pub fn cache_key(sha256: &str) -> String {
    format!("thumbs/w{WIDTH}/{}.jpg", sha256.to_ascii_lowercase())
}

/// The preview's validator: the original's SHA-256 (a material's file never changes) and the
/// preview width.
pub fn etag(row: &MaterialRow) -> String {
    format!(
        "\"{}-w{WIDTH}\"",
        row.sha256
            .as_deref()
            .unwrap_or_default()
            .to_ascii_lowercase()
    )
}

async fn read_all(storage: &dyn Storage, key: &str) -> Result<Vec<u8>, StorageError> {
    storage
        .get(key, None)
        .await?
        .try_fold(Vec::new(), |mut acc, chunk| async move {
            acc.extend_from_slice(&chunk);
            Ok(acc)
        })
        .await
        .map_err(|e| StorageError::Other(e.into()))
}

/// The JPEG preview of `row`: the kept one, or rendered now (and kept).
pub async fn preview(state: &AppState, row: &MaterialRow) -> AppResult<Vec<u8>> {
    let (Some(sha), Some(original)) = (row.sha256.as_deref(), row.storage_key.as_deref()) else {
        return Err(AppError::not_found(MSG_NONE));
    };
    if !has_preview(row) {
        return Err(AppError::not_found(MSG_NONE));
    }
    let Some(storage) = state.storage.as_deref() else {
        return Err(AppError::Unavailable(MSG_NO_STORAGE.into()));
    };
    let key = cache_key(sha);
    match read_all(storage, &key).await {
        Ok(bytes) => return Ok(bytes),
        Err(StorageError::NotFound) => {}
        Err(e) => {
            tracing::warn!(material_id = row.id, error = %e, "reading a kept preview failed");
        }
    }

    let _slot = state
        .thumb_slots
        .acquire()
        .await
        .map_err(|e| anyhow::anyhow!("preview slots closed: {e}"))?;
    // Another request may have rendered it while this one waited for a slot.
    if let Ok(bytes) = read_all(storage, &key).await {
        return Ok(bytes);
    }
    let data = read_all(storage, original).await.map_err(|e| match e {
        StorageError::NotFound => AppError::not_found(MSG_FILE_GONE),
        StorageError::Other(e) => {
            tracing::error!(material_id = row.id, error = %e, "reading a material for its preview failed");
            AppError::Unavailable(MSG_FILE_UNAVAILABLE.into())
        }
    })?;
    let bytes = render(row, data).await?;
    if let Err(e) = storage.put(&key, MIME, bytes.clone().into()).await {
        // Sent anyway; the next request renders it again.
        tracing::warn!(material_id = row.id, error = %e, "keeping a preview failed");
    }
    Ok(bytes)
}

async fn render(row: &MaterialRow, data: Vec<u8>) -> AppResult<Vec<u8>> {
    let image = if row.ext.as_deref() == Some("pdf") {
        if !render::have_pdftoppm().await {
            return Err(AppError::Unavailable(MSG_NO_PDFTOPPM.into()));
        }
        let dir = WorkDir::new().map_err(anyhow::Error::from)?;
        let file = dir.path().join("source.pdf");
        tokio::fs::write(&file, &data)
            .await
            .map_err(anyhow::Error::from)?;
        drop(data);
        render::render_pdf_page_at(&file, dir.path(), 1, WIDTH)
            .await
            .map_err(|e| {
                tracing::warn!(material_id = row.id, error = %e, "rendering a preview failed");
                AppError::not_found(MSG_UNREADABLE)
            })?
    } else {
        data
    };
    tokio::task::spawn_blocking(move || shrink(&image))
        .await
        .map_err(|e| anyhow::anyhow!("preview task failed: {e}"))?
        .map_err(|e| {
            tracing::warn!(material_id = row.id, error = %e, "decoding a preview failed");
            AppError::not_found(MSG_UNREADABLE)
        })
}

/// An image (PNG, JPEG, WebP, GIF's first frame) as a preview: upright, within [`WIDTH`] ×
/// [`MAX_HEIGHT`], a JPEG on white. CPU-bound: call it from `spawn_blocking`.
pub fn shrink(data: &[u8]) -> image::ImageResult<Vec<u8>> {
    let (mut image, orientation) = render::decode(data)?;
    image.apply_orientation(orientation);
    if image.width() > WIDTH || image.height() > MAX_HEIGHT {
        image = image.resize(WIDTH, MAX_HEIGHT, FilterType::Triangle);
    }
    let mut out = Cursor::new(Vec::new());
    on_white(&image).write_with_encoder(JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY))?;
    Ok(out.into_inner())
}

/// JPEG has no transparency: transparent pixels become white, not black.
fn on_white(image: &DynamicImage) -> DynamicImage {
    if !image.color().has_alpha() {
        return DynamicImage::ImageRgb8(image.to_rgb8());
    }
    let rgba = image.to_rgba8();
    DynamicImage::ImageRgb8(image::RgbImage::from_fn(
        rgba.width(),
        rgba.height(),
        |x, y| {
            let [r, g, b, a] = rgba.get_pixel(x, y).0;
            let a = u16::from(a);
            // c·a + 255·(255 − a) ≤ 255·255: fits in u16.
            let blend = |c: u8| ((u16::from(c) * a + 255 * (255 - a)) / 255) as u8;
            image::Rgb([blend(r), blend(g), blend(b)])
        },
    ))
}

/// Removes the kept preview of `row`'s file, unless another material holds the same file. The
/// hard delete calls it before removing the original; when it fails nothing is deleted.
pub async fn forget(state: &AppState, row: &MaterialRow) -> AppResult<()> {
    let (Some(sha), true) = (row.sha256.as_deref(), has_preview(row)) else {
        return Ok(());
    };
    let Some(storage) = state.storage.as_deref() else {
        return Ok(());
    };
    let shared: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM materials
                         WHERE sha256 = $1 AND storage_key IS NOT NULL AND id <> $2)",
    )
    .bind(sha)
    .bind(row.id)
    .fetch_one(&state.db)
    .await?;
    if shared {
        return Ok(());
    }
    match storage.delete(&cache_key(sha)).await {
        Ok(()) | Err(StorageError::NotFound) => Ok(()),
        Err(e) => {
            tracing::error!(material_id = row.id, error = %e, "deleting a kept preview failed");
            Err(AppError::Unavailable(MSG_FORGET_FAILED.into()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::ImageFormat;

    fn encoded(image: DynamicImage, format: ImageFormat) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        image.write_to(&mut out, format).unwrap();
        out.into_inner()
    }

    fn size_of(bytes: &[u8]) -> (u32, u32) {
        let image = image::load_from_memory_with_format(bytes, ImageFormat::Jpeg).unwrap();
        (image.width(), image.height())
    }

    #[test]
    fn previews_are_jpegs_scaled_down_never_up() {
        let big = encoded(
            DynamicImage::ImageRgb8(image::RgbImage::new(1200, 900)),
            ImageFormat::Png,
        );
        assert_eq!(size_of(&shrink(&big).unwrap()), (480, 360));
        let small = encoded(
            DynamicImage::ImageRgb8(image::RgbImage::new(200, 100)),
            ImageFormat::Jpeg,
        );
        assert_eq!(size_of(&shrink(&small).unwrap()), (200, 100));
        // A long screenshot stops at three widths tall.
        let tall = encoded(
            DynamicImage::ImageRgb8(image::RgbImage::new(400, 4000)),
            ImageFormat::Png,
        );
        assert_eq!(size_of(&shrink(&tall).unwrap()), (144, 1440));
        assert!(shrink(b"\x89PNG\r\n\x1a\n not a png").is_err());
    }

    #[test]
    fn transparency_becomes_white() {
        let clear = encoded(
            DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
                16,
                16,
                image::Rgba([0, 0, 0, 0]),
            )),
            ImageFormat::Png,
        );
        let preview = image::load_from_memory(&shrink(&clear).unwrap())
            .unwrap()
            .to_rgb8();
        let [r, g, b] = preview.get_pixel(8, 8).0;
        assert!(r > 245 && g > 245 && b > 245, "{r} {g} {b}");
    }

    #[test]
    fn previews_are_kept_by_the_files_hash() {
        let sha = "AB".repeat(32);
        assert_eq!(
            cache_key(&sha),
            format!("thumbs/w480/{}.jpg", "ab".repeat(32))
        );
        assert!(crate::storage::valid_key(&cache_key(&sha)));
        assert!(is_sha256(&sha));
        assert!(!is_sha256("abc"));
        assert!(!is_sha256(&"zz".repeat(32)));
    }
}
