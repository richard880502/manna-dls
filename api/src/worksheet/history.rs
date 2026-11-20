//! 修改歷程: the earlier states of one block, and taking one of them back as the block's content
//! (docs/API.md「修改歷程與採用過往的版本」).
//!
//! Nothing here writes history of its own, except 採用 (`adopt`): the steps are derived from what
//! is already kept append-only.
//!
//! * `worksheet_versions` — every version of the worksheet. A step is a version in which the block
//!   (or the pictures of its slots) differs from the version before. Its kind comes from the
//!   version's `source` (what produced it), its pictures from the version's `pictures`.
//! * `worksheet_block_proposals` — what Manna proposed for the block. An adopted proposal is a
//!   version step already; a proposal that was not adopted (放棄, set aside by 再改一次, gone
//!   stale) is a step of its own, with the block it proposed and the picture actions that never
//!   ran.
//!
//! Adopting writes a NEW version (`cause = "restore"`) that puts the block back, with its
//! pictures: the picture rows that were current at that step become current again. The picture
//! model is only called for the picture actions of a proposal that never ran.

use std::collections::{BTreeSet, HashMap, HashSet};

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::types::Json;
use sqlx::PgPool;

use super::images::actions::{self, PictureAction, PictureOp, PictureOutcome};
use super::images::{self, undo as picture_undo};
use super::revise::{self as rv, Proposal};
use super::{store, Block, Content};
use crate::audit::log_usage;
use crate::error::{AppError, AppResult};
use crate::AppState;

pub const KIND_ORIGINAL: &str = "original";
pub const KIND_PROPOSAL_ADOPTED: &str = "proposal_adopted";
pub const KIND_PROPOSAL_REJECTED: &str = "proposal_rejected";
pub const KIND_REGENERATE: &str = "regenerate";
pub const KIND_PREP_REVISE: &str = "prep_revise";
pub const KIND_VOCABULARY: &str = "vocabulary";
pub const KIND_MANUAL: &str = "manual";
pub const KIND_PICTURES: &str = "pictures";
pub const KIND_UNDO: &str = "undo";
pub const KIND_RESTORE: &str = "restore";

pub const MSG_RUNNING: &str = "Manna 正在產生這份學習單，好了之後再採用。";
pub const MSG_BLOCK_RUNNING: &str = "Manna 還在改這一區，好了之後再採用。";
pub const MSG_IS_CURRENT: &str = "這就是現在的版本。";

/// Manual edits closer together than this are shown as one step (the editor saves as the
/// teacher types); each of their versions is still kept and can be adopted by its number.
const MANUAL_RUN: Duration = Duration::minutes(5);

fn label_of(kind: &str) -> &'static str {
    match kind {
        KIND_ORIGINAL => "原本",
        KIND_PROPOSAL_ADOPTED => "Manna 的提案（你採用）",
        KIND_PROPOSAL_REJECTED => "Manna 的提案（沒採用）",
        KIND_REGENERATE => "Manna 重新產生",
        KIND_PREP_REVISE => "備課對話修改",
        KIND_VOCABULARY => "換字",
        KIND_MANUAL => "你手動修改",
        KIND_PICTURES => "改圖片",
        KIND_UNDO => "復原",
        _ => "回到這一步",
    }
}

/// A picture of a step: the row that was current in the slot at that step.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PicturePreview {
    pub slot_id: String,
    /// With `done`: `GET /api/worksheets/:id/images/:image_id`.
    pub image_id: Option<i64>,
    /// `queued` | `running` | `done` | `failed` | `skipped` | `removed` | `none` (no row yet)
    pub status: String,
    /// `generated` | `upload` | `none`
    pub source: String,
    pub purpose_zh: String,
}

/// What a proposal that never ran would do to the pictures when adopted.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ActionPreview {
    pub action: PictureOp,
    pub slot_id: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AdoptInfo {
    /// Whether 採用這個版本 is offered (not for the version the block is in now).
    pub possible: bool,
    /// Nothing to warn about or to run first: a version of the block with its pictures as they
    /// were, or a proposal made from the block as it is.
    pub clean: bool,
    /// Made from an older state of the block: adopting replaces what came after.
    pub warn_older: bool,
    /// The proposal's picture actions never ran: adopting runs them (the picture model is called).
    pub draws_pictures: bool,
    /// The step kept no record of its pictures: they stay as they are now.
    pub pictures_unknown: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Step {
    pub kind: &'static str,
    pub label: &'static str,
    pub at: DateTime<Utc>,
    /// The version of the worksheet (a version step).
    pub version_no: Option<i32>,
    /// The proposal (a proposal that was not adopted, or the one an adopted step came from).
    pub proposal_id: Option<i64>,
    /// The block is in this state now.
    pub current: bool,
    /// One plain line: what was asked or said, or what the teacher did.
    pub detail: String,
    /// Earlier versions folded into this step (a run of manual edits), newest first.
    pub merged_version_nos: Vec<i32>,
    /// An adopted proposal the teacher took back with 復原.
    pub undone: bool,
    /// A proposal not adopted at the time, adopted later from this history (the version).
    pub adopted_later_version_no: Option<i32>,
    /// The block at this step (what a proposal would write, for a proposal).
    pub block: Block,
    /// The pictures of the block's slots at this step; `None` when the step kept no record.
    pub pictures: Option<Vec<PicturePreview>>,
    /// A proposal's picture actions (never run).
    pub picture_actions: Vec<ActionPreview>,
    pub adopt: AdoptInfo,
}

#[derive(Debug, Serialize)]
pub struct History {
    pub block_id: String,
    pub version_no: i32,
    /// Manna is generating or revising: 採用 is refused for now.
    pub busy: bool,
    /// Newest first.
    pub steps: Vec<Step>,
}

#[derive(sqlx::FromRow)]
struct VersionRow {
    version_no: i32,
    cause: String,
    source: Option<Value>,
    pictures: Option<Value>,
    created_at: DateTime<Utc>,
    block: Option<Value>,
}

const BLOCK_OF_VERSION: &str = "SELECT version_no, cause, source, pictures, created_at,
        (SELECT b FROM jsonb_array_elements(content -> 'blocks') b WHERE b ->> 'id' = $2 LIMIT 1)
            AS block
   FROM worksheet_versions";

async fn version_rows(
    db: &PgPool,
    worksheet_id: i64,
    block_id: &str,
) -> sqlx::Result<Vec<VersionRow>> {
    sqlx::query_as(&format!(
        "{BLOCK_OF_VERSION} WHERE worksheet_id = $1 ORDER BY version_no"
    ))
    .bind(worksheet_id)
    .bind(block_id)
    .fetch_all(db)
    .await
}

fn pictures_map(v: &Option<Value>) -> Option<HashMap<String, i64>> {
    let obj = v.as_ref()?.as_object()?;
    Some(
        obj.iter()
            .filter_map(|(k, v)| v.as_i64().map(|id| (k.clone(), id)))
            .collect(),
    )
}

/// The rows of the slots of `blocks` in a version's pictures: slot id → row (None: no picture).
fn slot_rows(
    map: &Option<HashMap<String, i64>>,
    blocks: &[&Block],
) -> Option<Vec<(String, Option<i64>)>> {
    let map = map.as_ref()?;
    let slots: BTreeSet<&str> = blocks
        .iter()
        .flat_map(|b| b.image_slots.iter().map(|s| s.id.as_str()))
        .collect();
    Some(
        slots
            .into_iter()
            .map(|s| (s.to_string(), map.get(s).copied()))
            .collect(),
    )
}

fn source_str<'a>(source: &'a Option<Value>, key: &str) -> Option<&'a str> {
    source.as_ref()?.get(key)?.as_str()
}

fn source_i64(source: &Option<Value>, key: &str) -> Option<i64> {
    source.as_ref()?.get(key)?.as_i64()
}

/// What produced a version, for the block: (kind, proposal id, one line).
fn classify(
    row: &VersionRow,
    first: bool,
    accepted_by_version: &HashMap<i32, &Proposal>,
) -> (&'static str, Option<i64>, String) {
    let cause = source_str(&row.source, "cause").unwrap_or(&row.cause);
    match cause {
        "block_proposal" | "block_proposal_pictures" => {
            let id = source_i64(&row.source, "proposal_id");
            (KIND_PROPOSAL_ADOPTED, id, String::new())
        }
        "regenerate" => (KIND_REGENERATE, None, String::new()),
        "generate" if !first => (KIND_REGENERATE, None, "整份學習單重新產生".to_string()),
        "generate" | "created" => (KIND_ORIGINAL, None, String::new()),
        "prep_turn" => (KIND_PREP_REVISE, None, String::new()),
        "vocabulary" => (KIND_VOCABULARY, None, String::new()),
        "undo" => (KIND_UNDO, None, String::new()),
        "restore" => (KIND_RESTORE, None, String::new()),
        "pictures" => {
            let what = match source_str(&row.source, "action") {
                Some("regenerate") => "重畫了一張圖",
                Some("remove") => "拿掉了一張圖",
                Some("upload") => "換成自己的圖",
                _ => "改了圖片",
            };
            (KIND_PICTURES, None, what.to_string())
        }
        // `revise` written before the source was kept: a block proposal when one says so.
        "revise" => match accepted_by_version.get(&row.version_no) {
            Some(p) => (KIND_PROPOSAL_ADOPTED, Some(p.id), String::new()),
            None => (KIND_PREP_REVISE, None, String::new()),
        },
        _ if first => (KIND_ORIGINAL, None, String::new()),
        _ => (KIND_MANUAL, None, String::new()),
    }
}

fn proposal_line(p: &Proposal) -> String {
    let reply = p.reply.trim();
    let line = if !reply.is_empty() {
        reply
    } else {
        p.instruction.trim()
    };
    line.chars().take(120).collect()
}

async fn picture_previews(
    db: &PgPool,
    worksheet_id: i64,
    wanted: &[(&Block, Option<HashMap<String, i64>>)],
) -> sqlx::Result<Vec<Option<Vec<PicturePreview>>>> {
    let ids: Vec<i64> = wanted
        .iter()
        .filter_map(|(_, m)| m.as_ref())
        .flat_map(|m| m.values().copied())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let rows: Vec<(i64, String, String)> = sqlx::query_as(
        "SELECT id, status, source FROM worksheet_images WHERE worksheet_id = $1 AND id = ANY($2)",
    )
    .bind(worksheet_id)
    .bind(&ids)
    .fetch_all(db)
    .await?;
    let by_id: HashMap<i64, (String, String)> =
        rows.into_iter().map(|(i, st, so)| (i, (st, so))).collect();
    Ok(wanted
        .iter()
        .map(|(block, map)| {
            let map = map.as_ref()?;
            Some(
                block
                    .image_slots
                    .iter()
                    .map(|slot| {
                        match map
                            .get(&slot.id)
                            .and_then(|id| by_id.get(id).map(|r| (id, r)))
                        {
                            Some((id, (status, source))) => PicturePreview {
                                slot_id: slot.id.clone(),
                                image_id: (status == images::STATUS_DONE).then_some(*id),
                                status: status.clone(),
                                source: source.clone(),
                                purpose_zh: slot.purpose_zh.clone(),
                            },
                            None => PicturePreview {
                                slot_id: slot.id.clone(),
                                image_id: None,
                                status: "none".to_string(),
                                source: "none".to_string(),
                                purpose_zh: slot.purpose_zh.clone(),
                            },
                        }
                    })
                    .collect(),
            )
        })
        .collect())
}

fn picture_actions_of(actions: &[PictureAction]) -> Vec<ActionPreview> {
    actions
        .iter()
        .map(|a| ActionPreview {
            action: a.action,
            slot_id: a.slot_id.clone(),
            text: a.text.clone(),
        })
        .collect()
}

/// Whether Manna is generating the worksheet or revising this block.
pub async fn busy(
    state: &AppState,
    worksheet_id: i64,
    block_id: &str,
) -> AppResult<Option<&'static str>> {
    if crate::worksheet::ai::jobs(state, worksheet_id).await?.1 {
        return Ok(Some(MSG_RUNNING));
    }
    let running: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM worksheet_block_proposals
                         WHERE worksheet_id = $1 AND block_id = $2 AND status = 'running')",
    )
    .bind(worksheet_id)
    .bind(block_id)
    .fetch_one(&state.db)
    .await?;
    Ok(running.then_some(MSG_BLOCK_RUNNING))
}

/// The steps of a block, newest first. `current` is the block in the latest version.
pub async fn list(
    state: &AppState,
    worksheet_id: i64,
    block_id: &str,
    current: &Block,
    latest_version_no: i32,
) -> AppResult<History> {
    let db = &state.db;
    let rows = version_rows(db, worksheet_id, block_id).await?;
    let proposals = rv::for_block(db, worksheet_id, block_id).await?;
    let accepted_by_version: HashMap<i32, &Proposal> = proposals
        .iter()
        .filter_map(|p| p.accepted_version_no.map(|v| (v, p)))
        .collect();
    let by_id: HashMap<i64, &Proposal> = proposals.iter().map(|p| (p.id, p)).collect();

    // ---- the steps of the versions
    struct Draft {
        step: Step,
        pictures: Option<HashMap<String, i64>>,
    }
    let mut drafts: Vec<Draft> = Vec::new();
    let mut prev: Option<(Block, Option<HashMap<String, i64>>)> = None;
    for row in &rows {
        let Some(block) = row
            .block
            .clone()
            .and_then(|b| serde_json::from_value::<Block>(b).ok())
        else {
            prev = None;
            continue;
        };
        let map = pictures_map(&row.pictures);
        let first = prev.is_none();
        let picture_step = row.cause == "pictures"
            && source_str(&row.source, "block_id").is_some_and(|b| b == block_id);
        if let Some((pb, pmap)) = &prev {
            let slots_then = slot_rows(pmap, &[pb, &block]);
            let slots_now = slot_rows(&map, &[pb, &block]);
            let moved = matches!((&slots_then, &slots_now), (Some(a), Some(b)) if a != b);
            if *pb == block && !moved && !picture_step {
                prev = Some((block, map));
                continue;
            }
        }
        let (kind, proposal_id, mut detail) = classify(row, first, &accepted_by_version);
        let proposal = proposal_id.and_then(|id| by_id.get(&id).copied());
        if let Some(p) = proposal {
            detail = proposal_line(p);
        } else if kind == KIND_RESTORE {
            detail = match (
                source_str(&row.source, "adopted_kind"),
                source_i64(&row.source, "adopted_version_no"),
            ) {
                (Some("version"), Some(n)) => format!("回到第 {n} 版這一區的樣子"),
                (Some("proposal"), _) => "採用了先前沒採用的 Manna 提案".to_string(),
                _ => String::new(),
            };
        } else if kind == KIND_UNDO {
            detail = match source_i64(&row.source, "undone_version_no") {
                Some(n) => format!("取消第 {n} 版的修改"),
                None => String::new(),
            };
        }
        drafts.push(Draft {
            step: Step {
                kind,
                label: label_of(kind),
                at: row.created_at,
                version_no: Some(row.version_no),
                proposal_id,
                current: false,
                detail,
                merged_version_nos: Vec::new(),
                undone: proposal.is_some_and(|p| p.undone_at.is_some()),
                adopted_later_version_no: None,
                block: block.clone(),
                pictures: None,
                picture_actions: Vec::new(),
                adopt: AdoptInfo {
                    possible: true,
                    clean: true,
                    warn_older: false,
                    draws_pictures: false,
                    pictures_unknown: map.is_none(),
                    reason: None,
                },
            },
            pictures: map.clone(),
        });
        prev = Some((block, map));
    }

    // Runs of manual edits count as one step (the newest of the run shows).
    let mut folded: Vec<Draft> = Vec::new();
    for d in drafts {
        if let Some(last) = folded.last_mut() {
            if last.step.kind == KIND_MANUAL
                && d.step.kind == KIND_MANUAL
                && d.step.at - last.step.at <= MANUAL_RUN
            {
                let mut merged = std::mem::take(&mut last.step.merged_version_nos);
                merged.insert(0, last.step.version_no.unwrap_or_default());
                *last = d;
                last.step.merged_version_nos = merged;
                continue;
            }
        }
        folded.push(d);
    }

    // Pictures of the version steps.
    let wanted: Vec<(&Block, Option<HashMap<String, i64>>)> = folded
        .iter()
        .map(|d| (&d.step.block, d.pictures.clone()))
        .collect();
    let previews = picture_previews(db, worksheet_id, &wanted).await?;
    let mut steps: Vec<Step> = folded
        .into_iter()
        .zip(previews)
        .map(|(d, p)| Step {
            pictures: p,
            ..d.step
        })
        .collect();
    // The newest step of the versions is the block as it is now.
    if let Some(last) = steps.last_mut() {
        if last.version_no.is_some_and(|n| n <= latest_version_no) && last.block == *current {
            last.current = true;
            last.adopt.possible = false;
            last.adopt.reason = Some(MSG_IS_CURRENT.to_string());
        }
    }
    // ---- proposals that were not adopted
    for p in &proposals {
        let Some(proposed) = &p.block else { continue };
        if p.status != rv::STATUS_REJECTED && p.status != rv::STATUS_STALE {
            continue;
        }
        let draws = p
            .picture_actions
            .iter()
            .any(|a| matches!(a.action, PictureOp::Add | PictureOp::Redraw));
        let warn_older = p.before != *current;
        steps.push(Step {
            kind: KIND_PROPOSAL_REJECTED,
            label: label_of(KIND_PROPOSAL_REJECTED),
            at: p.decided_at.or(p.finished_at).unwrap_or(p.created_at),
            version_no: None,
            proposal_id: Some(p.id),
            current: false,
            detail: proposal_line(p),
            merged_version_nos: Vec::new(),
            undone: false,
            adopted_later_version_no: p.adopted_later_version_no,
            block: proposed.clone(),
            pictures: None,
            picture_actions: picture_actions_of(&p.picture_actions),
            adopt: AdoptInfo {
                possible: true,
                clean: !warn_older && !draws,
                warn_older,
                draws_pictures: draws,
                pictures_unknown: false,
                reason: None,
            },
        });
    }
    steps.sort_by(|a, b| {
        b.at.cmp(&a.at)
            .then(b.version_no.cmp(&a.version_no))
            .then(b.proposal_id.cmp(&a.proposal_id))
    });
    let busy = busy(state, worksheet_id, block_id).await?.is_some();
    Ok(History {
        block_id: block_id.to_string(),
        version_no: latest_version_no,
        busy,
        steps,
    })
}

// ---------------------------------------------------------------- adopting

#[derive(Debug, Clone, Copy)]
pub enum Target {
    Version(i32),
    Proposal(i64),
}

pub struct Adopted {
    /// The version 採用 wrote.
    pub version_no: i32,
    pub from_version_no: i32,
    pub kind: &'static str,
    pub warned_older: bool,
    /// What happened to the picture actions of a proposal (`[]` otherwise).
    pub pictures: Vec<PictureOutcome>,
}

const MSG_PROPOSAL_ADOPTED: &str = "這個提案已經採用過了，請在歷程裡找它採用後的那一步。";
const MSG_PROPOSAL_OPEN: &str = "這個提案還在等你決定，請在這一區下方採用或放棄。";
const MSG_NO_SUCH_STEP: &str = "找不到這一步。";

pub async fn adopt(
    state: &AppState,
    ws: &super::store::Worksheet,
    teacher_id: i64,
    block_id: &str,
    target: Target,
) -> AppResult<Adopted> {
    let db = &state.db;
    if let Some(why) = busy(state, ws.id, block_id).await? {
        return Err(AppError::Conflict(why.to_string()));
    }
    // What is adopted.
    enum What {
        Version {
            no: i32,
            block: Box<Block>,
            pictures: Option<HashMap<String, i64>>,
        },
        Proposal(Box<Proposal>),
    }
    let what = match target {
        Target::Version(no) => {
            let row: Option<VersionRow> = sqlx::query_as(&format!(
                "{BLOCK_OF_VERSION} WHERE worksheet_id = $1 AND version_no = $3"
            ))
            .bind(ws.id)
            .bind(block_id)
            .bind(no)
            .fetch_optional(db)
            .await?;
            let row = row.ok_or_else(|| AppError::not_found(MSG_NO_SUCH_STEP))?;
            let block = row
                .block
                .clone()
                .and_then(|b| serde_json::from_value::<Block>(b).ok())
                .ok_or_else(|| AppError::not_found("這一步沒有這一區。"))?;
            What::Version {
                no,
                block: Box::new(block),
                pictures: pictures_map(&row.pictures),
            }
        }
        Target::Proposal(id) => {
            let p = rv::get(db, ws.id, id).await?;
            if p.block_id != block_id {
                return Err(AppError::not_found(MSG_NO_SUCH_STEP));
            }
            match p.status.as_str() {
                rv::STATUS_REJECTED | rv::STATUS_STALE if p.block.is_some() => {}
                rv::STATUS_ACCEPTED => {
                    return Err(AppError::Conflict(MSG_PROPOSAL_ADOPTED.to_string()))
                }
                rv::STATUS_PROPOSED | rv::STATUS_RUNNING => {
                    return Err(AppError::Conflict(MSG_PROPOSAL_OPEN.to_string()))
                }
                _ => {
                    return Err(AppError::Conflict(
                        "這個提案沒有可以採用的內容。".to_string(),
                    ))
                }
            }
            What::Proposal(Box::new(p))
        }
    };

    let before = picture_undo::snapshot(db, ws.id).await?;
    let mut tx = db.begin().await?;
    sqlx::query("SELECT id FROM worksheets WHERE id = $1 FOR UPDATE")
        .bind(ws.id)
        .execute(&mut *tx)
        .await?;
    let latest: (Value, i32) = sqlx::query_as(
        "SELECT content, version_no FROM worksheet_versions WHERE worksheet_id = $1
          ORDER BY version_no DESC LIMIT 1",
    )
    .bind(ws.id)
    .fetch_one(&mut *tx)
    .await?;
    let from_version_no = latest.1;
    let mut content: Content = serde_json::from_value(latest.0).unwrap_or_default();
    let pos = content
        .blocks
        .iter()
        .position(|b| b.id == block_id)
        .ok_or_else(|| AppError::not_found("找不到這個區塊。"))?;
    let previous = content.blocks[pos].clone();

    let (kind, restored, wanted_pictures, proposal, warned_older) = match &what {
        What::Version {
            block, pictures, ..
        } => (
            "version",
            (**block).clone(),
            pictures.clone(),
            None::<&Proposal>,
            false,
        ),
        What::Proposal(p) => (
            "proposal",
            p.block.clone().unwrap_or_default(),
            None,
            Some(p.as_ref()),
            p.before != previous,
        ),
    };
    content.blocks[pos] = restored.clone();
    // A proposal's new picture slots become slots of the block, part of the same version.
    let new_no = from_version_no + 1;
    let added = match proposal {
        Some(p) => actions::add_slots(&mut content, &p.picture_actions, &format!("h{new_no}")),
        None => Vec::new(),
    };
    content
        .normalise()
        .map_err(|e| AppError::Conflict(format!("這一步的內容現在放不回這份學習單：{e}")))?;
    let written = content.blocks[pos].clone();

    // The pictures of a version go back with it: the rows that were current then become
    // current again (nothing is drawn).
    let mut slots_touched = 0usize;
    if let Some(map) = &wanted_pictures {
        let slots_now: BTreeSet<&str> = previous
            .image_slots
            .iter()
            .chain(restored.image_slots.iter())
            .map(|s| s.id.as_str())
            .collect();
        for slot in slots_now {
            let in_restored = restored.image_slots.iter().any(|s| s.id == slot);
            let target = if in_restored {
                map.get(slot).copied()
            } else {
                None
            };
            let now = before.get(slot).map(|(id, _)| *id);
            if now != target {
                picture_undo::set_current(&mut tx, ws.id, slot, target).await?;
                slots_touched += 1;
            }
        }
    }
    if proposal.is_none() && previous == written && slots_touched == 0 {
        return Err(AppError::Conflict(MSG_IS_CURRENT.to_string()));
    }

    let source = match &what {
        What::Version { no, .. } => json!({
            "cause": "restore", "adopted_kind": "version", "adopted_version_no": no,
            "from_version_no": from_version_no, "block_ids": [block_id],
        }),
        What::Proposal(p) => json!({
            "cause": "restore", "adopted_kind": "proposal", "proposal_id": p.id,
            "interaction_id": p.interaction_id, "round": p.round,
            "from_version_no": from_version_no, "warned_older": warned_older,
            "block_ids": [block_id],
        }),
    };
    let saved =
        store::insert_version_from(&mut tx, ws.id, &content, "restore", Some(source)).await?;
    let interaction_id = proposal.and_then(|p| p.interaction_id);
    let history_id: i64 = sqlx::query_scalar(
        "INSERT INTO worksheet_block_history
           (worksheet_id, block_id, block, interaction_id, version_no, after_block)
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
    )
    .bind(ws.id)
    .bind(block_id)
    .bind(Json(&previous))
    .bind(interaction_id)
    .bind(saved.version_no)
    .bind(Json(&written))
    .fetch_one(&mut *tx)
    .await?;
    if let Some(p) = proposal {
        sqlx::query(
            "UPDATE worksheet_block_proposals
                SET adopted_later_at = now(), adopted_later_version_no = $2
              WHERE id = $1",
        )
        .bind(p.id)
        .bind(saved.version_no)
        .execute(&mut *tx)
        .await?;
        // The earlier reject row stays; this is a new decision on the same interaction.
        rv::decision(
            &mut *tx,
            p.interaction_id,
            teacher_id,
            "adopt",
            "學習單：從修改歷程採用先前沒採用的提案",
        )
        .await?;
    }
    let adopted_json = match &what {
        What::Version { no, .. } => json!({ "adopted_version_no": no, "proposal_id": null }),
        What::Proposal(p) => json!({ "adopted_version_no": null, "proposal_id": p.id }),
    };
    log_usage(
        &mut *tx,
        teacher_id,
        "worksheet_block_version_adopted",
        json!({
            "worksheet_id": ws.id, "block_id": block_id, "from_version_no": from_version_no,
            "version_no": saved.version_no, "kind": kind,
            "adopted_version_no": adopted_json["adopted_version_no"],
            "proposal_id": adopted_json["proposal_id"], "warned_older": warned_older,
            "pictures_restored": slots_touched,
        }),
    )
    .await?;
    tx.commit().await?;

    // A proposal never ran its picture actions: 採用 runs them now, as 採用 does.
    let outcomes = match proposal {
        Some(p) => actions::run(state, ws, &p.picture_actions, &added, &content).await,
        None => Vec::new(),
    };
    if let Some(p) = proposal {
        if !outcomes.is_empty() {
            actions::log_applied(
                state,
                teacher_id,
                ws.id,
                "block_history_adopt",
                json!({ "proposal_id": p.id, "block_id": block_id }),
                &outcomes,
            )
            .await;
        }
    }
    // What happened to the pictures goes with the step, so 復原上一版 can give them back.
    let after = picture_undo::snapshot(db, ws.id).await?;
    let added_slots: HashSet<String> = added.iter().filter_map(|(_, r)| r.clone().ok()).collect();
    let undo = picture_undo::diff(block_id, &before, &after, &added_slots);
    if !undo.is_empty() {
        sqlx::query("UPDATE worksheet_block_history SET picture_undo = $2 WHERE id = $1")
            .bind(history_id)
            .bind(undo.to_json())
            .execute(db)
            .await?;
    }
    store::set_version_pictures(
        db,
        ws.id,
        saved.version_no,
        &picture_undo::snapshot_json(&after),
    )
    .await?;
    Ok(Adopted {
        version_no: saved.version_no,
        from_version_no,
        kind,
        warned_older,
        pictures: outcomes,
    })
}

// ---------------------------------------------------------------- the picture buttons

/// What the teacher did with a picture button.
#[derive(Debug, Clone, Copy)]
pub enum PictureButton {
    Regenerate,
    Upload,
    Remove,
}

impl PictureButton {
    fn name(self) -> &'static str {
        match self {
            Self::Regenerate => "regenerate",
            Self::Upload => "upload",
            Self::Remove => "remove",
        }
    }
}

/// Keeps the teacher's own 重畫 / 拿掉 / 換成我的圖 as a step of the block: a new version with the
/// content the worksheet had, `cause = "pictures"`, and the pictures as they are now (the
/// version's `pictures` holds the new row). Called after the button's own work is saved; a
/// failure here is logged and never undoes the picture.
pub async fn record_picture_button(
    db: &PgPool,
    worksheet_id: i64,
    slot_id: &str,
    button: PictureButton,
    hint: &str,
) {
    let result: AppResult<()> = async {
        let mut tx = db.begin().await?;
        sqlx::query("SELECT id FROM worksheets WHERE id = $1 FOR UPDATE")
            .bind(worksheet_id)
            .execute(&mut *tx)
            .await?;
        let row: Option<(i64, String)> = sqlx::query_as(
            "SELECT id, block_id FROM worksheet_images
              WHERE worksheet_id = $1 AND slot_id = $2 AND current",
        )
        .bind(worksheet_id)
        .bind(slot_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some((image_id, block_id)) = row else {
            return Ok(());
        };
        let latest: (Value,) = sqlx::query_as(
            "SELECT content FROM worksheet_versions WHERE worksheet_id = $1
              ORDER BY version_no DESC LIMIT 1",
        )
        .bind(worksheet_id)
        .fetch_one(&mut *tx)
        .await?;
        let content: Content = serde_json::from_value(latest.0).unwrap_or_default();
        store::insert_version_from(
            &mut tx,
            worksheet_id,
            &content,
            "pictures",
            Some(json!({
                "cause": "pictures", "action": button.name(), "block_id": block_id,
                "block_ids": [block_id], "slot_id": slot_id, "image_id": image_id,
                "hint": hint,
            })),
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }
    .await;
    if let Err(e) = result {
        tracing::error!(worksheet_id, slot_id, error = %e, "could not keep the picture step");
    }
}
