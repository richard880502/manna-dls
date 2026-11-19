//! Pictures the teacher attaches to a revise request (a screenshot, a photo of a circled problem).
//!
//! Upload first, send later: `POST /api/prep-sessions/:id/images` takes one file, re-encodes it
//! (upright, ≤ [`MAX_SIDE`] px on the long side, JPEG, metadata dropped) and stores it under
//! `prep-images/s<session>/…`; the teacher's next revise request names the ids (`image_ids`),
//! which attaches them to that turn (or to the block proposal of 改這一區). An image no request
//! picked up is removed after [`ORPHAN_AFTER_HOURS`]. Every image belongs to one prep session and
//! one teacher (anyone else's is a 404) and goes with the session when it is deleted. The model
//! gets the bytes as image parts of the user message; the research record keeps ids, sizes and
//! hashes only.

use std::collections::HashMap;
use std::io::Cursor;

use bytes::Bytes;
use futures_util::TryStreamExt;
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::PgPool;

use crate::ai::LlmImage;
use crate::error::{AppError, AppResult};
use crate::storage::StorageError;
use crate::AppState;

/// Images one message may carry.
pub const MAX_PER_MESSAGE: usize = 2;
/// Largest upload (before re-encoding).
pub const MAX_BYTES: u64 = 5 * 1024 * 1024;
/// Uploaded and not yet sent, per session.
pub const MAX_PENDING: i64 = 4;
/// Longest side of the stored (and sent) image.
pub const MAX_SIDE: u32 = 1568;
const JPEG_QUALITY: u8 = 85;
pub const MIME: &str = "image/jpeg";
pub const ORPHAN_AFTER_HOURS: i32 = 24;

pub const MSG_TYPE: &str = "只能附 PNG、JPEG 或 WebP 圖片。";
pub const MSG_TOO_BIG: &str = "圖片最大 5 MB，這張超過了。";
pub const MSG_UNREADABLE: &str = "這張圖讀不出來（可能已損壞），請換一張。";
pub const MSG_TOO_MANY: &str = "一則訊息最多附 2 張圖片。";
const MSG_PENDING: &str = "還沒送出的圖片太多了，先送出或移除幾張。";
const MSG_NOT_FOUND: &str = "找不到這張圖片。";
const MSG_NOT_PENDING: &str = "這張圖片已經送出了，不能移除。";
const MSG_IMAGE_USED: &str = "有一張圖片已經附在別的訊息上了，請重新附上。";

/// An attached image as the API shows it.
#[derive(Debug, Clone, Serialize, PartialEq, sqlx::FromRow)]
pub struct PrepImage {
    pub id: i64,
    pub session_id: i64,
    pub width: i32,
    pub height: i32,
    pub bytes: i32,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Stored {
    pub id: i64,
    pub session_id: i64,
    pub turn_id: Option<i64>,
    pub proposal_id: Option<i64>,
    pub storage_key: String,
    pub bytes: i32,
    pub original_bytes: i32,
    pub width: i32,
    pub height: i32,
    pub sha256: String,
}

const COLS: &str = "id, session_id, turn_id, proposal_id, storage_key, bytes, original_bytes,
    width, height, sha256";

/// The extension of an accepted image upload: PNG, JPEG or WebP (no GIF).
fn accepted(filename: &str) -> Option<crate::materials::FileType> {
    crate::materials::file_type_for_name(filename)
        .filter(|t| matches!(t.ext, "png" | "jpg" | "webp"))
}

/// Decodes, turns upright, scales down and writes JPEG (transparency on white). CPU-bound.
pub fn normalize(data: &[u8]) -> Result<(Vec<u8>, u32, u32), String> {
    let (mut image, orientation) =
        crate::materials::ocr::render::decode(data).map_err(|_| MSG_UNREADABLE.to_string())?;
    image.apply_orientation(orientation);
    if image.width() == 0 || image.height() == 0 {
        return Err(MSG_UNREADABLE.to_string());
    }
    if image.width() > MAX_SIDE || image.height() > MAX_SIDE {
        image = image.resize(MAX_SIDE, MAX_SIDE, FilterType::Triangle);
    }
    let flat = crate::materials::thumbnail::on_white(&image);
    let mut out = Cursor::new(Vec::new());
    flat.write_with_encoder(JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY))
        .map_err(|_| MSG_UNREADABLE.to_string())?;
    Ok((out.into_inner(), flat.width(), flat.height()))
}

fn storage_key(session_id: i64) -> String {
    let token = crate::auth::new_token();
    format!("prep-images/s{session_id}/{}.jpg", &token[..24])
}

async fn read_all(state: &AppState, key: &str) -> Result<Vec<u8>, StorageError> {
    let Some(storage) = state.storage.as_deref() else {
        return Err(StorageError::NotFound);
    };
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

async fn forget_objects(state: &AppState, keys: &[String]) {
    let Some(storage) = state.storage.as_deref() else {
        return;
    };
    for key in keys {
        match storage.delete(key).await {
            Ok(()) | Err(StorageError::NotFound) => {}
            Err(e) => tracing::warn!(key, error = %e, "removing an attached image failed"),
        }
    }
}

/// Removes the images nobody sent (older than [`ORPHAN_AFTER_HOURS`]). Best effort.
async fn purge_orphans(state: &AppState, teacher_id: i64) {
    let rows: sqlx::Result<Vec<(i64, String)>> = sqlx::query_as(
        "DELETE FROM prep_images
          WHERE teacher_id = $1 AND turn_id IS NULL AND proposal_id IS NULL
            AND created_at < now() - make_interval(hours => $2)
        RETURNING id, storage_key",
    )
    .bind(teacher_id)
    .bind(ORPHAN_AFTER_HOURS)
    .fetch_all(&state.db)
    .await;
    match rows {
        Ok(rows) => {
            let keys: Vec<String> = rows.into_iter().map(|(_, k)| k).collect();
            forget_objects(state, &keys).await;
        }
        Err(e) => tracing::warn!(error = %e, "could not purge unsent images"),
    }
}

/// One upload (`data` is within [`MAX_BYTES`] already: the route stops reading at the limit).
pub async fn upload(
    state: &AppState,
    teacher_id: i64,
    session_id: i64,
    filename: &str,
    data: Bytes,
) -> AppResult<PrepImage> {
    let Some(storage) = state.storage.clone() else {
        return Err(crate::materials::store::storage_missing());
    };
    let file_type = accepted(filename).ok_or_else(|| AppError::bad_request(MSG_TYPE))?;
    if data.len() as u64 > MAX_BYTES {
        return Err(AppError::bad_request(MSG_TOO_BIG));
    }
    if !crate::materials::content_matches(file_type, &data) {
        return Err(AppError::bad_request(MSG_UNREADABLE));
    }
    purge_orphans(state, teacher_id).await;
    let pending: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM prep_images
          WHERE session_id = $1 AND turn_id IS NULL AND proposal_id IS NULL",
    )
    .bind(session_id)
    .fetch_one(&state.db)
    .await?;
    if pending >= MAX_PENDING {
        return Err(AppError::Conflict(MSG_PENDING.to_string()));
    }
    let original = data.len();
    let (bytes, width, height) = tokio::task::spawn_blocking(move || normalize(&data))
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("image task failed: {e}")))?
        .map_err(AppError::bad_request)?;
    let key = storage_key(session_id);
    storage
        .put(&key, MIME, Bytes::from(bytes.clone()))
        .await
        .map_err(|e| {
            tracing::error!(session_id, error = %e, "storing an attached image failed");
            crate::materials::store::storage_missing()
        })?;
    let row: Result<PrepImage, sqlx::Error> = sqlx::query_as(
        "INSERT INTO prep_images
           (teacher_id, session_id, storage_key, content_type, bytes, original_bytes, width,
            height, sha256)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
         RETURNING id, session_id, width, height, bytes",
    )
    .bind(teacher_id)
    .bind(session_id)
    .bind(&key)
    .bind(MIME)
    .bind(bytes.len() as i32)
    .bind(original as i32)
    .bind(width as i32)
    .bind(height as i32)
    .bind(crate::auth::sha256_hex(&bytes))
    .fetch_one(&state.db)
    .await;
    match row {
        Ok(r) => Ok(r),
        Err(e) => {
            forget_objects(state, &[key]).await;
            Err(e.into())
        }
    }
}

/// One image of the session (owner checked by the caller through the session).
pub async fn find(db: &PgPool, session_id: i64, image_id: i64) -> AppResult<Stored> {
    sqlx::query_as(&format!(
        "SELECT {COLS} FROM prep_images WHERE id = $1 AND session_id = $2"
    ))
    .bind(image_id)
    .bind(session_id)
    .fetch_optional(db)
    .await?
    .ok_or_else(|| AppError::not_found(MSG_NOT_FOUND))
}

/// The bytes of a stored image.
pub async fn bytes_of(state: &AppState, key: &str) -> AppResult<Vec<u8>> {
    if state.storage.is_none() {
        return Err(crate::materials::store::storage_missing());
    }
    read_all(state, key).await.map_err(|e| match e {
        StorageError::NotFound => AppError::not_found(MSG_NOT_FOUND),
        StorageError::Other(e) => {
            tracing::error!(error = %e, "reading an attached image failed");
            AppError::Unavailable("圖片暫時讀不到，請稍後再試。".into())
        }
    })
}

/// 移除 an image that was not sent yet.
pub async fn delete_pending(state: &AppState, session_id: i64, image_id: i64) -> AppResult<()> {
    let row = find(&state.db, session_id, image_id).await?;
    if row.turn_id.is_some() || row.proposal_id.is_some() {
        return Err(AppError::Conflict(MSG_NOT_PENDING.to_string()));
    }
    let gone = sqlx::query(
        "DELETE FROM prep_images WHERE id = $1 AND turn_id IS NULL AND proposal_id IS NULL",
    )
    .bind(image_id)
    .execute(&state.db)
    .await?;
    if gone.rows_affected() > 0 {
        forget_objects(state, &[row.storage_key]).await;
    }
    Ok(())
}

/// The ids a request names, checked before anything is made: at most [`MAX_PER_MESSAGE`], all
/// the teacher's own, in this session, and not sent yet. Duplicates count once.
pub async fn check_ids(
    db: &PgPool,
    teacher_id: i64,
    session_id: i64,
    ids: &[i64],
) -> AppResult<Vec<i64>> {
    let mut ids = ids.to_vec();
    ids.sort_unstable();
    ids.dedup();
    if ids.is_empty() {
        return Ok(ids);
    }
    if ids.len() > MAX_PER_MESSAGE {
        return Err(AppError::bad_request(MSG_TOO_MANY));
    }
    let found: Vec<(i64, Option<i64>, Option<i64>)> = sqlx::query_as(
        "SELECT id, turn_id, proposal_id FROM prep_images
          WHERE id = ANY($1) AND session_id = $2 AND teacher_id = $3",
    )
    .bind(&ids)
    .bind(session_id)
    .bind(teacher_id)
    .fetch_all(db)
    .await?;
    if found.len() != ids.len() {
        return Err(AppError::bad_request(MSG_NOT_FOUND));
    }
    if found.iter().any(|(_, t, p)| t.is_some() || p.is_some()) {
        return Err(AppError::bad_request(MSG_IMAGE_USED));
    }
    Ok(ids)
}

/// Attaches checked images to a turn.
pub async fn attach_to_turn(db: &PgPool, ids: &[i64], turn_id: i64) -> AppResult<()> {
    attach(db, ids, "turn_id", turn_id).await
}

/// Attaches checked images to a block proposal.
pub async fn attach_to_proposal(db: &PgPool, ids: &[i64], proposal_id: i64) -> AppResult<()> {
    attach(db, ids, "proposal_id", proposal_id).await
}

async fn attach(db: &PgPool, ids: &[i64], column: &str, target: i64) -> AppResult<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let done = sqlx::query(&format!(
        "UPDATE prep_images SET {column} = $2
          WHERE id = ANY($1) AND turn_id IS NULL AND proposal_id IS NULL"
    ))
    .bind(ids)
    .bind(target)
    .execute(db)
    .await?;
    if done.rows_affected() as usize != ids.len() {
        return Err(AppError::Conflict(MSG_IMAGE_USED.to_string()));
    }
    Ok(())
}

/// The images of turns, by turn id.
pub async fn for_turns(
    db: &PgPool,
    turn_ids: &[i64],
) -> sqlx::Result<HashMap<i64, Vec<PrepImage>>> {
    let rows: Vec<(i64, i64, i64, i32, i32, i32)> = sqlx::query_as(
        "SELECT turn_id, id, session_id, width, height, bytes FROM prep_images
          WHERE turn_id = ANY($1) ORDER BY id",
    )
    .bind(turn_ids)
    .fetch_all(db)
    .await?;
    let mut out: HashMap<i64, Vec<PrepImage>> = HashMap::new();
    for (turn, id, session_id, width, height, bytes) in rows {
        out.entry(turn).or_default().push(PrepImage {
            id,
            session_id,
            width,
            height,
            bytes,
        });
    }
    Ok(out)
}

/// The images of block proposals, by proposal id.
pub async fn for_proposals(
    db: &PgPool,
    proposal_ids: &[i64],
) -> sqlx::Result<HashMap<i64, Vec<PrepImage>>> {
    let rows: Vec<(i64, i64, i64, i32, i32, i32)> = sqlx::query_as(
        "SELECT proposal_id, id, session_id, width, height, bytes FROM prep_images
          WHERE proposal_id = ANY($1) ORDER BY id",
    )
    .bind(proposal_ids)
    .fetch_all(db)
    .await?;
    let mut out: HashMap<i64, Vec<PrepImage>> = HashMap::new();
    for (proposal, id, session_id, width, height, bytes) in rows {
        out.entry(proposal).or_default().push(PrepImage {
            id,
            session_id,
            width,
            height,
            bytes,
        });
    }
    Ok(out)
}

/// What a request sends to the model, and what its record keeps.
pub struct Attached {
    pub images: Vec<LlmImage>,
    /// Ids, sizes and hashes (never the bytes), for `ai_interactions.input_snapshot`.
    pub record: Vec<Value>,
}

impl Attached {
    pub fn none() -> Self {
        Self {
            images: Vec::new(),
            record: Vec::new(),
        }
    }

    /// One line for the user message, or "".
    pub fn note(&self) -> String {
        if self.images.is_empty() {
            return String::new();
        }
        format!(
            "\n\n教師附上了 {} 張圖片（在這則訊息的最後）：用它們了解教師想改哪裡。",
            self.images.len()
        )
    }
}

/// Loads the images of a turn or a proposal for the model. An image the storage cannot give
/// back is left out (the request still runs on the teacher's words).
pub async fn load(
    state: &AppState,
    turn_id: Option<i64>,
    proposal_id: Option<i64>,
) -> AppResult<Attached> {
    let rows: Vec<Stored> = sqlx::query_as(&format!(
        "SELECT {COLS} FROM prep_images
          WHERE ($1::bigint IS NOT NULL AND turn_id = $1)
             OR ($2::bigint IS NOT NULL AND proposal_id = $2)
          ORDER BY id"
    ))
    .bind(turn_id)
    .bind(proposal_id)
    .fetch_all(&state.db)
    .await?;
    let mut out = Attached::none();
    for r in rows {
        match read_all(state, &r.storage_key).await {
            Ok(data) => {
                out.images.push(LlmImage {
                    media_type: MIME,
                    data: Bytes::from(data),
                });
                out.record.push(json!({
                    "id": r.id, "bytes": r.bytes, "original_bytes": r.original_bytes,
                    "width": r.width, "height": r.height, "sha256": r.sha256,
                }));
            }
            Err(e) => {
                tracing::warn!(image_id = r.id, error = %e, "an attached image could not be read");
            }
        }
    }
    Ok(out)
}

/// Removes every stored object of the session's images (before the session is deleted; the rows
/// go with it). Fails when the storage cannot delete, so nothing is left behind unreachable.
pub async fn purge_session(state: &AppState, session_id: i64) -> AppResult<()> {
    let keys: Vec<String> =
        sqlx::query_scalar("SELECT storage_key FROM prep_images WHERE session_id = $1")
            .bind(session_id)
            .fetch_all(&state.db)
            .await?;
    if keys.is_empty() {
        return Ok(());
    }
    let Some(storage) = state.storage.as_deref() else {
        return Err(AppError::Unavailable(
            "這個環境目前沒有連上檔案儲存空間，無法刪除附上的圖片，所以這個備課對話還沒有刪除。請聯絡計畫團隊。"
                .into(),
        ));
    };
    for key in &keys {
        match storage.delete(key).await {
            Ok(()) | Err(StorageError::NotFound) => {}
            Err(e) => {
                tracing::error!(session_id, error = %e, "deleting an attached image failed");
                return Err(AppError::Unavailable(
                    "附上的圖片沒有刪除成功（儲存空間暫時無法使用），所以這個備課對話還沒有刪除。請稍後再試一次。"
                        .into(),
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([200, 30, 30, 128]));
        let mut out = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
        out.into_inner()
    }

    #[test]
    fn images_are_scaled_down_and_become_jpeg() {
        let (bytes, w, h) = normalize(&png(3136, 784)).unwrap();
        assert_eq!((w, h), (1568, 392));
        assert!(bytes.starts_with(b"\xFF\xD8\xFF"));
        let (_, w, h) = normalize(&png(40, 30)).unwrap();
        assert_eq!((w, h), (40, 30));
        assert!(normalize(b"not an image").is_err());
    }

    #[test]
    fn only_png_jpeg_and_webp_are_accepted() {
        assert!(accepted("a.PNG").is_some());
        assert!(accepted("a.jpeg").is_some());
        assert!(accepted("a.webp").is_some());
        assert!(accepted("a.gif").is_none());
        assert!(accepted("a.pdf").is_none());
        assert!(accepted("noext").is_none());
    }
}
