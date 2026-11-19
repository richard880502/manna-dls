//! Picture actions of a revision: what the model may propose besides the text changes
//! (docs/API.md「改圖片」). The model only proposes; the server validates every action against the
//! worksheet, and nothing is drawn, redrawn or removed until the teacher accepts:
//!
//! * a block revision (`worksheet_block_revise`) keeps its actions in the proposal and runs them
//!   when the proposal is accepted;
//! * a turn-level revision (`prep_revise`) applies directly (as its text changes do), so its
//!   actions run right after the new version is saved.
//!
//! Running an action goes through the machinery of the teacher's own buttons: `add` appends a
//! slot to the block (part of the saved version) and queues its picture ([`super::enqueue`]),
//! `redraw` is [`super::regenerate`] (capped by `max_regenerations`), `remove` is
//! [`super::remove`]. With pictures off nothing is proposed and nothing is run.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{
    clip, current, enum_of, find_slot, names_a_resource, obj, s_max, slot_description, ImageSlot,
    RawImageSlot, SlotKind, SlotState, CAPTION_CHARS, HINT_CHARS, KINDS, MAX_SLOTS_PER_BLOCK,
    MSG_DISABLED, PROMPT_CHARS, PURPOSE_CHARS, STATUS_DONE, STATUS_FAILED, STATUS_QUEUED,
    STATUS_REMOVED, STATUS_RUNNING, STATUS_SKIPPED,
};
use crate::error::AppError;
use crate::routes::{clip_chars, log_usage};
use crate::worksheet::store::Worksheet;
use crate::worksheet::{Block, Content, MAX_ID_CHARS};
use crate::AppState;

/// Most actions one revision may carry; the rest are dropped.
pub const MAX_ACTIONS: usize = 3;

pub const STATUS_OUTCOME_QUEUED: &str = "queued";
pub const STATUS_OUTCOME_REMOVED: &str = "removed";
pub const STATUS_OUTCOME_SKIPPED: &str = "skipped";

/// What the model is told when pictures are off.
pub const RULES_OFF: &str = "這個環境沒有開啟 AI 產圖：picture_actions 一律給空陣列，不要幫教師加圖、重畫或移除圖片；教師要求圖片時，在 reply 說明目前沒有開啟產圖。";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PictureOp {
    #[default]
    Add,
    Redraw,
    Remove,
}

impl PictureOp {
    pub const ALL: [&'static str; 3] = ["add", "redraw", "remove"];

    fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "add" => Some(Self::Add),
            "redraw" => Some(Self::Redraw),
            "remove" => Some(Self::Remove),
            _ => None,
        }
    }
}

/// An action as the model writes it (lenient: [`check`] decides). One flat object so a strict
/// JSON schema can describe it: `block_id` is used by `add`, `slot_id` by `redraw` and `remove`
/// (the unused one is ""). For `add`: `prompt_en` is the scene for the picture model, `purpose_zh`
/// and `caption` are in the teacher's language. For `redraw`: `prompt_en` is what to change (a
/// short English note for the picture model) and `purpose_zh` says it in the teacher's language.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct RawPictureAction {
    pub action: String,
    pub block_id: String,
    pub slot_id: String,
    pub kind: String,
    pub purpose_zh: String,
    pub prompt_en: String,
    pub caption: String,
}

/// A checked action, as kept in the proposal and shown to the teacher (`text`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PictureAction {
    pub action: PictureOp,
    /// The block (for `add`: where the picture goes; for the others: the slot's block).
    pub block_id: String,
    /// `redraw` and `remove`: the slot.
    pub slot_id: String,
    /// `add`: the new slot, without its id.
    pub kind: SlotKind,
    pub purpose_zh: String,
    pub prompt_en: String,
    pub alt_en: String,
    pub caption: String,
    /// `redraw`: the note for the picture model (≤ 200).
    pub hint: String,
    /// What the teacher reads: 「加一張圖：…」「重畫第 2 張圖：…」「移除圖：…」.
    pub text: String,
}

/// What happened to one action (`pictures` of an accepted proposal or of a revise turn).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PictureOutcome {
    pub action: PictureOp,
    pub text: String,
    /// `queued` (a picture is being drawn), `removed`, or `skipped` (with `message`).
    pub status: String,
    pub message: Option<String>,
}

// ---------------------------------------------------------------- the schema

/// `picture_actions`: at most `max` actions (0 when pictures are off), `block_id` and `slot_id`
/// only from this worksheet (or ""), properties in the order they are written.
pub fn schema(block_ids: &[String], slot_ids: &[String], max: usize) -> Value {
    let with_blank = |ids: &[String]| -> Value {
        let mut all: Vec<&str> = vec![""];
        all.extend(ids.iter().map(String::as_str));
        enum_of(&all)
    };
    json!({
        "type": "array",
        "maxItems": max,
        "items": obj(vec![
            ("action", enum_of(&PictureOp::ALL)),
            ("block_id", with_blank(block_ids)),
            ("slot_id", with_blank(slot_ids)),
            ("kind", enum_of(&KINDS)),
            ("purpose_zh", s_max(PURPOSE_CHARS)),
            ("prompt_en", s_max(PROMPT_CHARS)),
            ("caption", s_max(CAPTION_CHARS)),
        ]),
    })
}

/// Every slot id of the enabled blocks of `content` (optionally one block).
pub fn slot_ids(content: &Content, only_block: Option<&str>) -> Vec<String> {
    content
        .blocks
        .iter()
        .filter(|b| b.enabled && only_block.is_none_or(|id| id == b.id))
        .flat_map(|b| b.image_slots.iter().map(|s| s.id.clone()))
        .collect()
}

// ---------------------------------------------------------------- what the model reads

fn state_word(status: Option<&str>) -> &'static str {
    match status {
        Some(STATUS_DONE) => "已畫好",
        Some(STATUS_QUEUED) | Some(STATUS_RUNNING) => "繪製中",
        Some(STATUS_FAILED) => "沒畫成功",
        Some(STATUS_REMOVED) => "已移除（不印）",
        _ => "沒有圖",
    }
}

/// The pictures of a block as the model reads them (slot id, number in the block, what it is for,
/// caption, the description it was drawn from, and where it stands).
pub fn describe(block: &Block, states: &[SlotState]) -> Vec<Value> {
    block
        .image_slots
        .iter()
        .enumerate()
        .map(|(k, s)| {
            let st = states.iter().find(|x| x.slot_id == s.id);
            json!({
                "slot_id": s.id,
                "number": k + 1,
                "purpose": s.purpose_zh,
                "caption": s.caption,
                "prompt": slot_description(s),
                "state": state_word(st.map(|x| x.status.as_str())),
            })
        })
        .collect()
}

/// The same as lines of text (the block revision's message).
pub fn describe_text(block: &Block, states: &[SlotState]) -> String {
    let lines: Vec<String> = describe(block, states)
        .iter()
        .map(|d| {
            format!(
                "- 第 {} 張圖（slot_id：{}）｜{}｜用途：{}｜圖說：{}｜畫面描述：{}",
                d["number"],
                d["slot_id"].as_str().unwrap_or_default(),
                d["state"].as_str().unwrap_or_default(),
                clip_chars(d["purpose"].as_str().unwrap_or_default(), 80),
                clip_chars(d["caption"].as_str().unwrap_or_default(), 80),
                clip_chars(d["prompt"].as_str().unwrap_or_default(), 200),
            )
        })
        .collect();
    if lines.is_empty() {
        "（這一區目前沒有圖片）".to_string()
    } else {
        lines.join("\n")
    }
}

/// The rules the model reads: the picture slot rules (no text in pictures, no real people or
/// brands) while pictures are on, or [`RULES_OFF`].
pub async fn rules(state: &AppState) -> sqlx::Result<String> {
    if !state.images.enabled() {
        return Ok(RULES_OFF.to_string());
    }
    let text = crate::prompts::latest(&state.db, super::SLOTS_RULES_KEY)
        .await?
        .map(|p| p.system_text.trim().to_string())
        .unwrap_or_default();
    Ok(format!(
        "圖片的畫面描述（prompt_en）遵守下面的規則（原本是寫給 image_slots 的）：\n{text}"
    ))
}

// ---------------------------------------------------------------- checking

/// What [`check`] works against.
pub struct CheckCtx<'a> {
    pub content: &'a Content,
    /// A block revision may only touch this block.
    pub only_block: Option<&'a str>,
    /// How a block is called in `text` (「B 區」); `None` for a block revision.
    pub name_of: &'a dyn Fn(&str) -> Option<String>,
}

fn place(ctx: &CheckCtx<'_>, block_id: &str) -> String {
    (ctx.name_of)(block_id)
        .map(|n| format!("（{n}）"))
        .unwrap_or_default()
}

/// The actions that may run, and what was dropped (for the log and the retry). At most
/// [`MAX_ACTIONS`]; one per slot; only slots and blocks of this worksheet (of the block for a
/// block revision); an `add` goes through the same cleaning as `image_slots` ([`super::clean_slots`]:
/// lengths, no web addresses, something to draw) and never makes a block hold more than
/// [`MAX_SLOTS_PER_BLOCK`] slots. Pictures off: nothing is kept.
pub fn check(
    raw: &[RawPictureAction],
    ctx: &CheckCtx<'_>,
    pictures_on: bool,
) -> (Vec<PictureAction>, Vec<String>) {
    let mut out: Vec<PictureAction> = Vec::new();
    let mut notes = Vec::new();
    if raw.is_empty() {
        return (out, notes);
    }
    if !pictures_on {
        notes.push("這個環境沒有開啟 AI 產圖，圖片的部分沒有處理".to_string());
        return (out, notes);
    }
    let mut touched: HashSet<String> = HashSet::new();
    let mut added_in: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for (i, r) in raw.iter().enumerate() {
        let n = i + 1;
        if out.len() >= MAX_ACTIONS {
            notes.push(format!("圖片動作超過 {MAX_ACTIONS} 個，只留前面的"));
            break;
        }
        let Some(op) = PictureOp::parse(&r.action) else {
            notes.push(format!("第 {n} 個圖片動作不是 add、redraw 或 remove，拿掉"));
            continue;
        };
        match op {
            PictureOp::Add => {
                let block_id = r.block_id.trim();
                let Some(block) = ctx
                    .content
                    .block(block_id)
                    .filter(|b| b.enabled)
                    .filter(|b| ctx.only_block.is_none_or(|id| id == b.id))
                else {
                    notes.push(format!(
                        "第 {n} 個圖片動作的 block_id「{}」不在這份學習單（或這次可以改的範圍）裡，拿掉",
                        clip_chars(block_id, 40)
                    ));
                    continue;
                };
                let have = block.image_slots.len() + added_in.get(&block.id).copied().unwrap_or(0);
                if have >= MAX_SLOTS_PER_BLOCK {
                    notes.push(format!(
                        "第 {n} 個圖片動作：一個區塊最多 {MAX_SLOTS_PER_BLOCK} 個圖片位置，拿掉"
                    ));
                    continue;
                }
                let (slots, slot_notes) = super::clean_slots(
                    &[RawImageSlot {
                        kind: r.kind.clone(),
                        purpose_zh: r.purpose_zh.clone(),
                        prompt_en: r.prompt_en.clone(),
                        alt_en: String::new(),
                        caption: r.caption.clone(),
                    }],
                    1,
                );
                let Some(slot) = slots.into_iter().next() else {
                    notes.extend(
                        slot_notes
                            .into_iter()
                            .map(|m| format!("第 {n} 個圖片動作：{m}")),
                    );
                    continue;
                };
                // The same picture twice in one block is one picture.
                let repeated = out.iter().any(|a| {
                    a.action == PictureOp::Add
                        && a.block_id == block.id
                        && a.prompt_en.eq_ignore_ascii_case(&slot.prompt_en)
                }) || block
                    .image_slots
                    .iter()
                    .any(|s| s.prompt_en.eq_ignore_ascii_case(&slot.prompt_en));
                if repeated {
                    notes.push(format!("第 {n} 個圖片動作和已有的圖重複，拿掉"));
                    continue;
                }
                *added_in.entry(block.id.clone()).or_default() += 1;
                let text = format!(
                    "加一張圖：{}{}",
                    clip_chars(&slot.purpose_zh, PURPOSE_CHARS),
                    place(ctx, &block.id)
                );
                out.push(PictureAction {
                    action: PictureOp::Add,
                    block_id: block.id.clone(),
                    slot_id: String::new(),
                    kind: slot.kind,
                    purpose_zh: slot.purpose_zh,
                    prompt_en: slot.prompt_en,
                    alt_en: slot.alt_en,
                    caption: slot.caption,
                    hint: String::new(),
                    text,
                });
            }
            PictureOp::Redraw | PictureOp::Remove => {
                let slot_id = r.slot_id.trim();
                let found = find_slot(ctx.content, slot_id)
                    .filter(|(b, _)| b.enabled)
                    .filter(|(b, _)| ctx.only_block.is_none_or(|id| id == b.id));
                let Some((block, slot)) = found else {
                    notes.push(format!(
                        "第 {n} 個圖片動作的 slot_id「{}」不在這份學習單（或這次可以改的範圍）裡，拿掉",
                        clip_chars(slot_id, 40)
                    ));
                    continue;
                };
                if !touched.insert(slot.id.clone()) {
                    notes.push(format!("第 {n} 個圖片動作和前面動到同一張圖，拿掉"));
                    continue;
                }
                let number = block
                    .image_slots
                    .iter()
                    .position(|s| s.id == slot.id)
                    .map_or(1, |k| k + 1);
                let place = place(ctx, &block.id);
                if op == PictureOp::Remove {
                    out.push(PictureAction {
                        action: op,
                        block_id: block.id.clone(),
                        slot_id: slot.id.clone(),
                        text: format!(
                            "移除圖：第 {number} 張圖「{}」{place}",
                            clip_chars(&slot.purpose_zh, 40)
                        ),
                        ..PictureAction::default()
                    });
                    continue;
                }
                let hint = clip(&r.prompt_en, HINT_CHARS);
                if names_a_resource(&hint) || names_a_resource(&r.purpose_zh) {
                    notes.push(format!("第 {n} 個圖片動作寫了網址，拿掉"));
                    touched.remove(&slot.id);
                    continue;
                }
                let says = clip(&r.purpose_zh, PURPOSE_CHARS);
                let says = if says.is_empty() { hint.clone() } else { says };
                out.push(PictureAction {
                    action: op,
                    block_id: block.id.clone(),
                    slot_id: slot.id.clone(),
                    purpose_zh: says.clone(),
                    hint,
                    text: format!(
                        "重畫第 {number} 張圖：{}{place}",
                        if says.is_empty() {
                            "照原本的用途再畫一次".to_string()
                        } else {
                            says
                        }
                    ),
                    ..PictureAction::default()
                });
            }
        }
    }
    (out, notes)
}

/// The count line for a reply when the model's actions were dropped because pictures are off.
pub fn off_notice(raw_count: usize, pictures_on: bool) -> Option<&'static str> {
    (raw_count > 0 && !pictures_on).then_some("（這個環境沒有開啟 AI 產圖，圖片的部分沒有處理。）")
}

// ---------------------------------------------------------------- running

/// The slot an `add` becomes, or why it could not be added (the block is gone, or full).
pub type Added = Vec<(usize, Result<String, String>)>;

fn unique_slot_id(content: &Content, block: &str, tag: &str, k: usize) -> String {
    let taken: HashSet<&str> = content
        .blocks
        .iter()
        .flat_map(|b| b.image_slots.iter().map(|s| s.id.as_str()))
        .collect();
    let mut n = k;
    loop {
        let id = format!("{block}-x{tag}n{n}");
        let id = if id.chars().count() <= MAX_ID_CHARS {
            id
        } else {
            format!("x{tag}n{n}-{}", n + k)
        };
        if !taken.contains(id.as_str()) {
            return id;
        }
        n += 1;
    }
}

/// Appends the new slots of the `add` actions to their blocks (so they are part of the version
/// that is saved). `tag` makes the ids of this revision's slots (`p12`, `t7`). Returns, per
/// action index, the new slot id or why there is none.
pub fn add_slots(content: &mut Content, actions: &[PictureAction], tag: &str) -> Added {
    let mut out = Vec::new();
    for (i, a) in actions.iter().enumerate() {
        if a.action != PictureOp::Add {
            continue;
        }
        let id = unique_slot_id(content, &a.block_id, tag, i + 1);
        let Some(block) = content.blocks.iter_mut().find(|b| b.id == a.block_id) else {
            out.push((i, Err("這一區已經不在學習單上了。".to_string())));
            continue;
        };
        if block.image_slots.len() >= MAX_SLOTS_PER_BLOCK {
            out.push((
                i,
                Err(format!("這一區已經有 {MAX_SLOTS_PER_BLOCK} 個圖片位置。")),
            ));
            continue;
        }
        block.image_slots.push(ImageSlot {
            id: id.clone(),
            kind: a.kind,
            purpose_zh: a.purpose_zh.clone(),
            prompt_en: a.prompt_en.clone(),
            alt_en: a.alt_en.clone(),
            caption: a.caption.clone(),
        });
        out.push((i, Ok(id)));
    }
    out
}

fn reason(e: &AppError) -> String {
    match e {
        AppError::BadRequest(m)
        | AppError::NotFound(m)
        | AppError::Conflict(m)
        | AppError::Forbidden(m)
        | AppError::Unauthorized(m)
        | AppError::Unavailable(m) => m.clone(),
        AppError::Internal(_) => "圖片這一步發生錯誤，沒有處理。".to_string(),
    }
}

fn skipped(a: &PictureAction, message: impl Into<String>) -> PictureOutcome {
    PictureOutcome {
        action: a.action,
        text: a.text.clone(),
        status: STATUS_OUTCOME_SKIPPED.to_string(),
        message: Some(message.into()),
    }
}

/// Runs the actions once the version with the new slots is saved: queues the pictures of the
/// new slots, redraws and removes the others. Never fails as a whole: what could not be done is
/// `skipped` with the reason (a cap, a picture still being drawn, no storage).
pub async fn run(
    state: &AppState,
    ws: &Worksheet,
    actions: &[PictureAction],
    added: &Added,
    saved: &Content,
) -> Vec<PictureOutcome> {
    if actions.is_empty() {
        return Vec::new();
    }
    if !state.images.enabled() {
        return actions.iter().map(|a| skipped(a, MSG_DISABLED)).collect();
    }
    let queued_new = added.iter().any(|(_, r)| r.is_ok());
    if queued_new {
        super::enqueue_logged(state, ws.id, ws.teacher_id, saved).await;
    }
    let states = current(&state.db, ws.id).await.unwrap_or_default();
    let mut out = Vec::new();
    for (i, a) in actions.iter().enumerate() {
        out.push(match a.action {
            PictureOp::Add => match added.iter().find(|(k, _)| *k == i) {
                Some((_, Ok(slot_id))) => match states.iter().find(|s| &s.slot_id == slot_id) {
                    Some(s) if s.status == STATUS_SKIPPED => skipped(
                        a,
                        s.message
                            .clone()
                            .unwrap_or_else(|| "這張圖沒有畫。".to_string()),
                    ),
                    Some(_) => PictureOutcome {
                        action: a.action,
                        text: a.text.clone(),
                        status: STATUS_OUTCOME_QUEUED.to_string(),
                        message: None,
                    },
                    None => skipped(a, "這張圖沒有排入繪圖。"),
                },
                Some((_, Err(why))) => skipped(a, why.clone()),
                None => skipped(a, "這張圖沒有加上去。"),
            },
            PictureOp::Redraw => {
                match super::regenerate(state, ws, &a.slot_id, Some(&a.hint)).await {
                    Ok(_) => PictureOutcome {
                        action: a.action,
                        text: a.text.clone(),
                        status: STATUS_OUTCOME_QUEUED.to_string(),
                        message: None,
                    },
                    Err(e) => skipped(a, reason(&e)),
                }
            }
            PictureOp::Remove => match super::remove(state, ws, &a.slot_id).await {
                Ok(_) => PictureOutcome {
                    action: a.action,
                    text: a.text.clone(),
                    status: STATUS_OUTCOME_REMOVED.to_string(),
                    message: None,
                },
                Err(e) => skipped(a, reason(&e)),
            },
        });
    }
    out
}

/// One line for a reply: what was done with the pictures.
pub fn summary(outcomes: &[PictureOutcome]) -> String {
    if outcomes.is_empty() {
        return String::new();
    }
    let count = |op: PictureOp, status: &str| {
        outcomes
            .iter()
            .filter(|o| o.action == op && o.status == status)
            .count()
    };
    let mut done = Vec::new();
    for (op, word) in [(PictureOp::Add, "加了"), (PictureOp::Redraw, "重畫了")] {
        let n = count(op, STATUS_OUTCOME_QUEUED);
        if n > 0 {
            done.push(format!("{word} {n} 張圖"));
        }
    }
    let removed = count(PictureOp::Remove, STATUS_OUTCOME_REMOVED);
    if removed > 0 {
        done.push(format!("移除了 {removed} 張圖"));
    }
    let skipped_n = outcomes
        .iter()
        .filter(|o| o.status == STATUS_OUTCOME_SKIPPED)
        .count();
    let mut parts = Vec::new();
    if !done.is_empty() {
        parts.push(format!("圖片：{}（畫好會自動出現）", done.join("、")));
    }
    if skipped_n > 0 {
        let first = outcomes
            .iter()
            .find(|o| o.status == STATUS_OUTCOME_SKIPPED)
            .and_then(|o| o.message.clone())
            .unwrap_or_default();
        parts.push(format!("有 {skipped_n} 個圖片動作沒有做：{first}"));
    }
    parts.join("。")
}

/// The usage-log event of picture actions that were applied.
pub async fn log_applied(
    state: &AppState,
    teacher_id: i64,
    worksheet_id: i64,
    source: &str,
    reference: Value,
    outcomes: &[PictureOutcome],
) {
    let n = |op: PictureOp| outcomes.iter().filter(|o| o.action == op).count();
    let status = |s: &str| outcomes.iter().filter(|o| o.status == s).count();
    let result = log_usage(
        &state.db,
        teacher_id,
        "worksheet_picture_actions_applied",
        json!({
            "worksheet_id": worksheet_id, "source": source, "ref": reference,
            "add": n(PictureOp::Add), "redraw": n(PictureOp::Redraw), "remove": n(PictureOp::Remove),
            "queued": status(STATUS_OUTCOME_QUEUED), "removed": status(STATUS_OUTCOME_REMOVED),
            "skipped": status(STATUS_OUTCOME_SKIPPED),
        }),
    )
    .await;
    if let Err(e) = result {
        tracing::warn!(worksheet_id, error = %e, "could not log the picture actions");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worksheet::Block;

    fn slot(id: &str, prompt: &str) -> ImageSlot {
        ImageSlot {
            id: id.into(),
            kind: SlotKind::Scene,
            purpose_zh: "蜜蜂".into(),
            prompt_en: prompt.into(),
            alt_en: "a bee".into(),
            caption: String::new(),
        }
    }

    fn content() -> Content {
        Content {
            blocks: vec![
                Block {
                    id: "b1".into(),
                    enabled: true,
                    image_slots: vec![slot("b1-p1", "a bee on a flower")],
                    ..Block::default()
                },
                Block {
                    id: "b2".into(),
                    enabled: true,
                    image_slots: vec![slot("b2-p1", "a cat"), slot("b2-p2", "a dog")],
                    ..Block::default()
                },
            ],
            ..Content::default()
        }
    }

    fn raw(action: &str, block: &str, slot: &str, prompt: &str) -> RawPictureAction {
        RawPictureAction {
            action: action.into(),
            block_id: block.into(),
            slot_id: slot.into(),
            kind: "scene".into(),
            purpose_zh: "說明".into(),
            prompt_en: prompt.into(),
            caption: String::new(),
        }
    }

    fn names(id: &str) -> Option<String> {
        Some(format!("{} 區", id.to_uppercase()))
    }

    #[test]
    fn valid_actions_pass_with_a_teacher_text() {
        let c = content();
        let ctx = CheckCtx {
            content: &c,
            only_block: None,
            name_of: &names,
        };
        let (a, notes) = check(
            &[
                raw("add", "b1", "", "A bee sitting on a yellow flower"),
                raw("redraw", "", "b1-p1", "no text in the picture"),
                raw("remove", "", "b2-p2", ""),
            ],
            &ctx,
            true,
        );
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(a.len(), 3);
        assert!(a[0].text.starts_with("加一張圖："), "{}", a[0].text);
        assert!(a[1].text.starts_with("重畫第 1 張圖："), "{}", a[1].text);
        assert!(a[2].text.starts_with("移除圖："), "{}", a[2].text);
        assert_eq!(a[1].hint, "no text in the picture");
    }

    #[test]
    fn unknown_blocks_slots_full_blocks_urls_and_pictures_off_are_dropped() {
        let c = content();
        let ctx = CheckCtx {
            content: &c,
            only_block: None,
            name_of: &names,
        };
        let (a, notes) = check(
            &[
                raw("add", "nope", "", "A bee"),
                raw("add", "b2", "", "A bird"),
                raw("redraw", "", "ghost", "x"),
                raw("remove", "", "ghost", ""),
                raw("add", "b1", "", "see https://example.com/bee.png"),
                raw("explode", "b1", "", "x"),
                raw("redraw", "", "b1-p1", "x"),
                raw("remove", "", "b1-p1", ""),
            ],
            &ctx,
            true,
        );
        assert_eq!(a.len(), 1, "{a:?}");
        assert_eq!(a[0].slot_id, "b1-p1");
        assert_eq!(notes.len(), 7, "{notes:?}");
        let (none, notes) = check(&[raw("add", "b1", "", "A bee")], &ctx, false);
        assert!(none.is_empty());
        assert_eq!(notes.len(), 1);
    }

    #[test]
    fn a_block_revision_may_only_touch_its_block_and_the_list_is_trimmed() {
        let c = content();
        let ctx = CheckCtx {
            content: &c,
            only_block: Some("b1"),
            name_of: &|_| None,
        };
        let (a, _) = check(
            &[
                raw("remove", "", "b2-p1", ""),
                raw("remove", "", "b1-p1", ""),
            ],
            &ctx,
            true,
        );
        assert_eq!(a.len(), 1);
        assert!(!a[0].text.contains('（'), "{}", a[0].text);
        let many: Vec<RawPictureAction> = (0..5)
            .map(|k| raw("add", "b1", "", &format!("scene number {k}")))
            .collect();
        // b1 holds one slot: room for one more only.
        let (a, notes) = check(&many, &ctx, true);
        assert_eq!(a.len(), 1);
        assert!(!notes.is_empty());
        let c2 = Content {
            blocks: vec![Block {
                id: "b1".into(),
                enabled: true,
                ..Block::default()
            }],
            ..Content::default()
        };
        let ctx = CheckCtx {
            content: &c2,
            only_block: None,
            name_of: &|_| None,
        };
        let (a, _) = check(&many, &ctx, true);
        assert_eq!(a.len(), MAX_SLOTS_PER_BLOCK);
    }

    #[test]
    fn add_slots_gives_unique_ids_and_respects_the_block_limit() {
        let mut c = content();
        let (actions, _) = check(
            &[raw("add", "b1", "", "A bee on a flower, sunny day")],
            &CheckCtx {
                content: &c,
                only_block: None,
                name_of: &|_| None,
            },
            true,
        );
        let added = add_slots(&mut c, &actions, "p9");
        assert_eq!(added.len(), 1);
        let id = added[0].1.clone().unwrap();
        assert!(id.chars().count() <= MAX_ID_CHARS);
        assert_eq!(c.block("b1").unwrap().image_slots.len(), 2);
        assert!(c.normalise().is_ok());
        // A second add to the full block is refused.
        let more = add_slots(&mut c, &actions, "p9");
        assert!(more[0].1.is_err());
    }

    #[test]
    fn schema_lists_only_this_worksheets_ids_in_written_order() {
        let s = schema(&["b1".into()], &["b1-p1".into()], 3);
        assert_eq!(s["maxItems"], 3);
        let item = &s["items"];
        assert_eq!(item["properties"]["block_id"]["enum"], json!(["", "b1"]));
        assert_eq!(item["properties"]["slot_id"]["enum"], json!(["", "b1-p1"]));
        let keys: Vec<&str> = item["properties"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        let required: Vec<&str> = item["required"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert_eq!(keys, required);
    }

    #[test]
    fn summaries_say_what_was_done_and_what_was_not() {
        let o = |op, status: &str, message: Option<&str>| PictureOutcome {
            action: op,
            text: String::new(),
            status: status.into(),
            message: message.map(str::to_string),
        };
        assert_eq!(summary(&[]), "");
        let s = summary(&[
            o(PictureOp::Add, "queued", None),
            o(PictureOp::Redraw, "skipped", Some("到上限了")),
            o(PictureOp::Remove, "removed", None),
        ]);
        assert!(s.contains("加了 1 張圖"), "{s}");
        assert!(s.contains("移除了 1 張圖"), "{s}");
        assert!(s.contains("到上限了"), "{s}");
        assert!(off_notice(1, false).is_some());
        assert!(off_notice(0, false).is_none());
        assert!(off_notice(1, true).is_none());
    }
}
