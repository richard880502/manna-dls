//! 版本紀錄: putting the whole worksheet back as it was in an earlier version (docs/API.md
//! 「版本紀錄與還原整份學習單」).
//!
//! Nothing is deleted or overwritten. Restoring writes a NEW version (`cause = "restore"`,
//! `source.adopted_kind = "worksheet"`) whose content is the old version's, puts the pictures of
//! that version back as the current ones, and keeps each block it changed in the per-block 修改歷程
//! so 復原上一版 of a block still works.

use std::collections::{BTreeSet, HashMap};

use serde_json::{json, Value};
use sqlx::types::Json;

use super::images::undo as picture_undo;
use super::{store, Content};
use crate::audit::log_usage;
use crate::error::{AppError, AppResult};
use crate::AppState;

pub const MSG_IS_LATEST: &str = "這已經是現在的版本，不用還原。";
pub const MSG_SAME_AS_LATEST: &str = "這一版的內容和現在的一樣，不用還原。";
pub const MSG_GENERATING: &str = "Manna 正在產生這份學習單，好了之後再還原。";
pub const MSG_CHAT_RUNNING: &str = "備課對話正在修改這份學習單，等這一輪結束後再還原。";
pub const MSG_STALE: &str =
    "這份學習單在你開啟版本紀錄之後又有了新的版本，請重新開啟版本紀錄再還原。";

/// Whether a prep chat turn that works on the worksheet is still running.
pub async fn chat_running(state: &AppState, worksheet_id: i64) -> sqlx::Result<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM prep_turns
                         WHERE status = 'running'
                           AND (chat_worksheet_id = $1 OR worksheet_id = $1))",
    )
    .bind(worksheet_id)
    .fetch_one(&state.db)
    .await
}

/// Why the worksheet cannot be restored right now, if so (409 message).
pub async fn busy(state: &AppState, worksheet_id: i64) -> AppResult<Option<&'static str>> {
    if super::ai::jobs(state, worksheet_id).await?.1 {
        return Ok(Some(MSG_GENERATING));
    }
    if chat_running(state, worksheet_id).await? {
        return Ok(Some(MSG_CHAT_RUNNING));
    }
    Ok(None)
}

pub struct Restored {
    pub version_no: i32,
    pub from_version_no: i32,
}

/// `base_version_no`: the latest version the client has seen; anything newer is a 409.
pub async fn restore(
    state: &AppState,
    ws: &store::Worksheet,
    teacher_id: i64,
    version_no: i32,
    base_version_no: i32,
) -> AppResult<Restored> {
    let db = &state.db;
    if let Some(why) = busy(state, ws.id).await? {
        return Err(AppError::Conflict(why.to_string()));
    }
    let (target, target_pictures) = store::version_at(db, ws.id, version_no)
        .await?
        .ok_or_else(|| AppError::not_found("找不到這一版。"))?;

    let mut tx = db.begin().await?;
    sqlx::query("SELECT id FROM worksheets WHERE id = $1 FOR UPDATE")
        .bind(ws.id)
        .execute(&mut *tx)
        .await?;
    let latest = store::latest(&mut *tx, ws.id).await?;
    if latest.version_no != base_version_no {
        return Err(AppError::Conflict(MSG_STALE.to_string()));
    }
    if latest.version_no == version_no {
        return Err(AppError::Conflict(MSG_IS_LATEST.to_string()));
    }
    let mut content: Content = target.content.clone();
    content
        .normalise()
        .map_err(|e| AppError::Conflict(format!("這一版的內容現在放不回這份學習單：{e}")))?;

    // The pictures of that version become the current ones again (nothing is drawn). A version
    // from before pictures were kept leaves them as they are.
    let mut slots_touched = 0usize;
    if let Some(map) = target_pictures.as_ref().and_then(Value::as_object) {
        let map: HashMap<&str, i64> = map
            .iter()
            .filter_map(|(k, v)| v.as_i64().map(|id| (k.as_str(), id)))
            .collect();
        let before = picture_undo::snapshot(&mut *tx, ws.id).await?;
        let slots: BTreeSet<&str> = latest
            .content
            .blocks
            .iter()
            .chain(content.blocks.iter())
            .flat_map(|b| b.image_slots.iter().map(|s| s.id.as_str()))
            .collect();
        for slot in slots {
            let in_target = content
                .blocks
                .iter()
                .any(|b| b.image_slots.iter().any(|s| s.id == slot));
            let want = if in_target {
                map.get(slot).copied()
            } else {
                None
            };
            let now = before.get(slot).map(|(id, _)| *id);
            if now != want {
                picture_undo::set_current(&mut tx, ws.id, slot, want).await?;
                slots_touched += 1;
            }
        }
    }
    if content == latest.content && slots_touched == 0 {
        return Err(AppError::Conflict(MSG_SAME_AS_LATEST.to_string()));
    }

    let changed: Vec<String> = content
        .blocks
        .iter()
        .filter(|b| latest.content.block(&b.id).is_some_and(|old| old != *b))
        .map(|b| b.id.clone())
        .collect();
    let saved = store::insert_version_from(
        &mut tx,
        ws.id,
        &content,
        "restore",
        Some(json!({
            "cause": "restore", "adopted_kind": "worksheet", "adopted_version_no": version_no,
            "from_version_no": latest.version_no, "block_ids": changed,
        })),
    )
    .await?;
    // Each block that changed keeps what it was, so the block's own 修改歷程 stays complete.
    for id in &changed {
        let (Some(old), Some(new)) = (latest.content.block(id), content.block(id)) else {
            continue;
        };
        sqlx::query(
            "INSERT INTO worksheet_block_history
               (worksheet_id, block_id, block, interaction_id, version_no, after_block)
             VALUES ($1, $2, $3, NULL, $4, $5)",
        )
        .bind(ws.id)
        .bind(id)
        .bind(Json(old))
        .bind(saved.version_no)
        .bind(Json(new))
        .execute(&mut *tx)
        .await?;
    }
    log_usage(
        &mut *tx,
        teacher_id,
        "worksheet_version_restored",
        json!({
            // from = the version restored; previous = the one it replaced (the latest before).
            "worksheet_id": ws.id, "from_version_no": version_no,
            "previous_version_no": latest.version_no, "new_version_no": saved.version_no,
        }),
    )
    .await?;
    tx.commit().await?;
    Ok(Restored {
        version_no: saved.version_no,
        from_version_no: version_no,
    })
}
