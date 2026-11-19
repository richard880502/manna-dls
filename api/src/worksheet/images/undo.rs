//! Taking a step's pictures back (docs/API.md「復原與修改歷程」).
//!
//! Nothing here draws, redraws or deletes: every picture ever made stays in `worksheet_images`
//! (`current = FALSE` once superseded). A step records, per slot it touched, which row was
//! current before and after it ([`PictureUndo`]); going back makes the earlier row current again.
//! The image model is never called.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, Transaction};

use super::{lock_worksheet, retire_current};
use crate::error::{AppError, AppResult};

/// Said when a picture changed after the step (a redraw, an upload, a removal of its own).
pub const MSG_PICTURES_CHANGED: &str = "這一區的圖片在採用之後又動過了，沒辦法直接復原到採用前。";

/// What a step did to one slot's picture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlotChange {
    pub slot_id: String,
    /// The slot was created by the step (its picture slot goes away with the block).
    #[serde(default)]
    pub added: bool,
    /// The image row current before the step (`None`: the slot had none).
    pub before: Option<i64>,
    /// The image row current right after the step's pictures were queued.
    pub after: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PictureUndo {
    pub slots: Vec<SlotChange>,
}

impl PictureUndo {
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    pub fn from_json(v: Option<&serde_json::Value>) -> Self {
        v.and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default()
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_default()
    }
}

/// The current image row of every slot: slot id → (image row id, block id).
pub type Snapshot = BTreeMap<String, (i64, String)>;

pub async fn snapshot(db: impl sqlx::PgExecutor<'_>, worksheet_id: i64) -> sqlx::Result<Snapshot> {
    let rows: Vec<(String, i64, String)> = sqlx::query_as(
        "SELECT slot_id, id, block_id FROM worksheet_images
          WHERE worksheet_id = $1 AND current ORDER BY id",
    )
    .bind(worksheet_id)
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(|(s, id, b)| (s, (id, b))).collect())
}

/// `{slot_id: image row id}` as kept in `worksheet_versions.pictures`.
pub fn snapshot_json(s: &Snapshot) -> serde_json::Value {
    serde_json::Value::Object(
        s.iter()
            .map(|(slot, (id, _))| (slot.clone(), serde_json::json!(id)))
            .collect(),
    )
}

/// What changed for the slots of `block_id` between two snapshots; `added` are the slots the
/// step created.
pub fn diff(
    block_id: &str,
    before: &Snapshot,
    after: &Snapshot,
    added: &HashSet<String>,
) -> PictureUndo {
    let slots: HashSet<&String> = before
        .iter()
        .chain(after.iter())
        .filter(|(_, (_, b))| b == block_id)
        .map(|(s, _)| s)
        .chain(added.iter())
        .collect();
    let mut changes: Vec<SlotChange> = slots
        .into_iter()
        .filter_map(|slot| {
            let b = before.get(slot).map(|(id, _)| *id);
            let a = after.get(slot).map(|(id, _)| *id);
            let is_added = added.contains(slot);
            (b != a || is_added).then(|| SlotChange {
                slot_id: slot.clone(),
                added: is_added,
                before: b,
                after: a,
            })
        })
        .collect();
    changes.sort_by(|x, y| x.slot_id.cmp(&y.slot_id));
    PictureUndo { slots: changes }
}

/// Whether every touched slot still shows what the step left (a 409 message when not).
pub async fn check(db: &PgPool, worksheet_id: i64, undo: &PictureUndo) -> AppResult<()> {
    if undo.is_empty() {
        return Ok(());
    }
    let now = snapshot(db, worksheet_id).await?;
    for c in &undo.slots {
        if now.get(&c.slot_id).map(|(id, _)| *id) != c.after {
            return Err(AppError::Conflict(MSG_PICTURES_CHANGED.to_string()));
        }
    }
    Ok(())
}

/// Makes `target` the current picture of the slot (`None`: no picture), inside the caller's
/// transaction. The row that was current is kept, no longer current; a picture still being
/// drawn is closed (its job stops at its next write).
pub async fn set_current(
    tx: &mut Transaction<'_, Postgres>,
    worksheet_id: i64,
    slot_id: &str,
    target: Option<i64>,
) -> AppResult<()> {
    retire_current(tx, worksheet_id, slot_id).await?;
    if let Some(id) = target {
        let done = sqlx::query(
            "UPDATE worksheet_images SET current = TRUE
              WHERE id = $1 AND worksheet_id = $2 AND slot_id = $3",
        )
        .bind(id)
        .bind(worksheet_id)
        .bind(slot_id)
        .execute(&mut **tx)
        .await?;
        if done.rows_affected() == 0 {
            return Err(AppError::Conflict(MSG_PICTURES_CHANGED.to_string()));
        }
    }
    Ok(())
}

/// Takes a step's pictures back: every touched slot gets the row that was current before.
pub async fn revert(
    tx: &mut Transaction<'_, Postgres>,
    worksheet_id: i64,
    undo: &PictureUndo,
) -> AppResult<()> {
    if undo.is_empty() {
        return Ok(());
    }
    lock_worksheet(tx, worksheet_id).await?;
    for c in &undo.slots {
        set_current(tx, worksheet_id, &c.slot_id, c.before).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(rows: &[(&str, i64, &str)]) -> Snapshot {
        rows.iter()
            .map(|(s, id, b)| (s.to_string(), (*id, b.to_string())))
            .collect()
    }

    #[test]
    fn diff_lists_changed_added_and_removed_slots_of_the_block_only() {
        let before = snap(&[("b1-p1", 1, "b1"), ("b1-p2", 2, "b1"), ("b2-p1", 3, "b2")]);
        let after = snap(&[
            ("b1-p1", 5, "b1"),
            ("b1-p2", 2, "b1"),
            ("b1-xp9n1", 6, "b1"),
            ("b2-p1", 9, "b2"),
        ]);
        let added: HashSet<String> = ["b1-xp9n1".to_string()].into();
        let d = diff("b1", &before, &after, &added);
        assert_eq!(d.slots.len(), 2, "{d:?}");
        assert_eq!(d.slots[0].slot_id, "b1-p1");
        assert_eq!((d.slots[0].before, d.slots[0].after), (Some(1), Some(5)));
        assert!(d.slots[1].added);
        assert_eq!(d.slots[1].before, None);
        let round: PictureUndo = PictureUndo::from_json(Some(&d.to_json()));
        assert_eq!(round, d);
        assert!(diff("b1", &before, &before, &HashSet::new()).is_empty());
    }
}
