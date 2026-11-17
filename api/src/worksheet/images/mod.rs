//! Pictures on worksheets (docs/API.md「學習單的圖片」; design: docs/WORKSHEET_BLOCK_EDIT_RESEARCH.md
//! §6).
//!
//! * **Slots** ([`ImageSlot`], in the block): the text model that writes a block also writes
//!   what picture belongs there, its alt text and an optional caption (words go in the caption,
//!   never in the picture). Only asked for while pictures are on ([`Images::enabled`]): with
//!   `IMAGE_PROVIDER=none` the requests to the text model are what they were before. The server
//!   cleans them ([`clean_slots`]): lengths, at most two per block, and a slot that needs a real
//!   photo (a map, a flag, a real person or landmark, history) becomes `photo_needed`, which is
//!   never generated.
//! * **Pictures** (`worksheet_images`, outside the versioned document): after a generation (or a
//!   block regeneration) passes its checks and is saved, every new slot gets a row
//!   ([`enqueue`]): `queued` and a background job ([`job`]), or `skipped` (photo needed, pictures
//!   off, no storage, the cap per worksheet reached). The job calls the picture model with the
//!   house style ([`STYLE_KEY`]), re-encodes the result as JPEG ([`normalize`]) and stores it.
//!   A failure leaves an empty box with the slot's purpose.
//! * **Teacher actions** (owner only): 重畫 with an optional note ([`regenerate`], capped per
//!   worksheet), 換成我的圖 ([`replace_with_upload`]), 拿掉 ([`remove`]). Each makes a new
//!   current row; older rows stay.
//! * **Shown** through `GET /api/worksheets/:id/images/:image_id` (owner only, ETag, private
//!   cache), in the sheet projection ([`decorate`]) and in the DOCX / PDF exports
//!   ([`bank_for`]).
//! * **Restarts**: open rows are taken over at startup ([`recover`]), like OCR.
//! * **Logs**: counts, sizes, hashes and token counts only; never a prompt or the teacher's note.

pub mod client;
pub mod job;

use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::sync::LazyLock;

use bytes::Bytes;
use chrono::{DateTime, Utc};
use futures_util::TryStreamExt;
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;

pub use client::{ImageConfig, ImageProviderKind, Images};

use super::assemble::{Sheet, SheetImage};
use super::export::{ExportImage, ImageBank};
use super::store::Worksheet;
use super::template::{enum_of, obj, s_max};
use super::{Block, Content};
use crate::error::{AppError, AppResult};
use crate::routes::{clip_chars, log_usage};
use crate::storage::StorageError;
use crate::AppState;

// ---------------------------------------------------------------- slots

pub const STATUS_QUEUED: &str = "queued";
pub const STATUS_RUNNING: &str = "running";
pub const STATUS_DONE: &str = "done";
pub const STATUS_FAILED: &str = "failed";
pub const STATUS_SKIPPED: &str = "skipped";
pub const STATUS_REMOVED: &str = "removed";

pub const SOURCE_GENERATED: &str = "generated";
pub const SOURCE_UPLOAD: &str = "upload";
pub const SOURCE_NONE: &str = "none";

/// `ai_interactions.module` of every picture call.
pub const MODULE: &str = "worksheet_image";
pub const SCAFFOLD: &str = "SC-WS-IMAGE";
/// The rules appended to a step that asks for `image_slots`.
pub const SLOTS_RULES_KEY: &str = "worksheet_image_slots";
/// The house style of every picture (system text) and the prompt around the slot's
/// description (user template).
pub const STYLE_KEY: &str = "image_style";
/// Used when the `image_style` prompt is missing (it is seeded by the migration).
pub const FALLBACK_STYLE: &str = "Simple flat vector illustration for a Taiwanese senior-high English worksheet. Clean outlines, soft limited palette, plain white background, one clear subject, classroom-appropriate. No text, letters, numbers, labels, logos, watermarks or brand names anywhere in the picture. No real or famous people.";
const FALLBACK_USER: &str = "Picture: {{description}}{{teacher_note}}";

/// Slots per block a template block (one call per block) may have …
pub const MAX_SLOTS_PER_BLOCK: usize = 2;
/// … and per block of the tasks step (one call for all the blocks).
pub const MAX_SLOTS_PER_TASK_BLOCK: usize = 1;
pub const PURPOSE_CHARS: usize = 80;
pub const PROMPT_CHARS: usize = 300;
pub const ALT_CHARS: usize = 120;
pub const CAPTION_CHARS: usize = 80;
/// The teacher's note for a 重畫.
pub const HINT_CHARS: usize = 200;
pub const KINDS: [&str; 4] = ["scene", "object", "icon", "photo_needed"];

/// Longest side of a stored picture.
pub const MAX_SIDE: u32 = 1536;
const JPEG_QUALITY: u8 = 85;
pub const MIME: &str = "image/jpeg";

pub const MSG_DISABLED: &str = "這個環境沒有開啟 AI 產圖；請上傳自己的圖。";
pub const MSG_PHOTO_NEEDED: &str =
    "這裡需要真實、準確的照片（地圖、國旗、真實人物或地標、歷史照片），Manna 不會自己畫；請上傳一張。";
const MSG_NO_STORAGE: &str = "這個環境目前沒有連上檔案儲存空間，無法保存圖片。";
const MSG_CAP: &str = "這份學習單自動產生的圖已經到上限；這一張可以按「重畫」，或上傳自己的圖。";
const MSG_GONE: &str = "這個圖片位置已經不在學習單上了。";
const MSG_STORE_FAILED: &str = "圖片沒有存成功；可以按「重畫」再試一次。";
const MSG_INTERNAL: &str = "產圖時發生錯誤；可以按「重畫」再試一次。";
pub const MSG_STALE: &str = "產圖中斷了（可能是系統剛更新）；可以按「重畫」再試一次。";
pub const MSG_CANNOT_RESUME: &str =
    "系統更新後無法繼續產這張圖（產圖或檔案儲存目前沒有開啟）；可以稍後按「重畫」，或上傳自己的圖。";
const MSG_SLOT_NOT_FOUND: &str = "找不到這個圖片位置。";
const MSG_BUSY: &str = "這張圖正在產生，好了之後再重畫。";
const MSG_PHOTO_ONLY: &str = "這個位置需要真實照片，Manna 不會自己畫；請上傳一張圖。";
const MSG_NOT_AN_IMAGE: &str = "只能上傳圖片（.png、.jpg、.webp、.gif）。";
const MSG_UNREADABLE: &str = "這張圖讀不出來（可能已損壞），請換一張。";

/// What a slot's picture is.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SlotKind {
    /// A scene or situation.
    #[default]
    Scene,
    /// One object.
    Object,
    /// A simple icon.
    Icon,
    /// A real, accurate photo (map, flag, real person or landmark, history): never generated.
    PhotoNeeded,
}

impl SlotKind {
    fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "scene" => Some(Self::Scene),
            "object" => Some(Self::Object),
            "icon" => Some(Self::Icon),
            "photo_needed" => Some(Self::PhotoNeeded),
            _ => None,
        }
    }
}

/// One picture slot of a block. Printed after the block's instructions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct ImageSlot {
    /// Unique in the worksheet; set by the server (a regenerated block gets new ones).
    pub id: String,
    pub kind: SlotKind,
    /// For the teacher: what the picture does here (Chinese).
    pub purpose_zh: String,
    /// For the picture model: only what is in the picture.
    pub prompt_en: String,
    pub alt_en: String,
    /// Printed under the picture (empty: none).
    pub caption: String,
}

/// A slot as the model wrote it (lenient: the checks decide).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct RawImageSlot {
    pub kind: String,
    pub purpose_zh: String,
    pub prompt_en: String,
    pub alt_en: String,
    pub caption: String,
}

/// A step output with the slots the model wrote beside it (template blocks).
#[derive(Debug, Clone, Deserialize)]
pub struct WithSlots<T> {
    #[serde(flatten)]
    pub inner: T,
    #[serde(default)]
    pub image_slots: Vec<RawImageSlot>,
}

/// The `image_slots` property: at most `max` slots, every string capped, properties in the
/// order they are written (the kind first, so `photo_needed` is decided before the prompt).
pub fn slots_schema(max: usize) -> Value {
    json!({
        "type": "array",
        "maxItems": max,
        "items": obj(vec![
            ("kind", enum_of(&KINDS)),
            ("purpose_zh", s_max(PURPOSE_CHARS)),
            ("prompt_en", s_max(PROMPT_CHARS)),
            ("alt_en", s_max(ALT_CHARS)),
            ("caption", s_max(CAPTION_CHARS)),
        ]),
    })
}

/// Adds the required `image_slots` property to an object schema, right after `after` (or last).
pub fn add_slots_property(object: &mut Value, after: Option<&str>, max: usize) {
    let Some(props) = object["properties"].as_object() else {
        return;
    };
    let mut next = serde_json::Map::new();
    let mut placed = false;
    for (k, v) in props {
        next.insert(k.clone(), v.clone());
        if after == Some(k.as_str()) {
            next.insert("image_slots".into(), slots_schema(max));
            placed = true;
        }
    }
    if !placed {
        next.insert("image_slots".into(), slots_schema(max));
    }
    let required: Vec<Value> = next.keys().map(|k| json!(k)).collect();
    object["properties"] = Value::Object(next);
    object["required"] = Value::Array(required);
}

fn clip(s: &str, max: usize) -> String {
    let flat = s.split_whitespace().collect::<Vec<_>>().join(" ");
    flat.chars()
        .take(max)
        .collect::<String>()
        .trim()
        .to_string()
}

/// A web address or e-mail address.
fn names_a_resource(text: &str) -> bool {
    let t = text.to_lowercase();
    [
        "http://", "https://", "www.", ".com", ".org", ".net", ".tw/", "@",
    ]
    .iter()
    .any(|k| t.contains(k))
}

static PHOTO_WORDS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(maps?|flags?|portraits?|famous|celebrit(?:y|ies)|president|prime minister|king|queen|emperor|historical|history|war|battles?|landmarks?|monuments?|logos?|brands?|trademarks?|photographs?|real photo)\b|地圖|國旗|名人|總統|歷史|戰爭|地標",
    )
    .expect("valid regex")
});

/// Two capitalised words in a row (`Eiffel Tower`, `Taylor Swift`, `Statue of Liberty`): most
/// likely a real person or place. A title with a full stop (`Mrs. Lin`) is not one.
static NAMED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b[A-Z][a-z]+(?:\s+(?:of\s+)?[A-Z][a-z]+)+").expect("valid regex")
});

/// Whether a slot asks for something that must be a real, accurate photo: a map, a flag, a real
/// person or landmark, history (doc §6.3). Such a slot is never generated.
pub fn needs_real_photo(slot: &ImageSlot) -> bool {
    let text = format!("{} {}", slot.prompt_en, slot.purpose_zh);
    PHOTO_WORDS.is_match(&text) || NAMED.is_match(&slot.prompt_en)
}

/// The slots of one block as the server keeps them (no ids yet), and what was changed (for the
/// log). Strings are cut to their caps; a slot naming a web address or with nothing to draw is
/// dropped; one that needs a real photo becomes `photo_needed`; repeated descriptions and slots
/// beyond `max` are dropped. Never a reason for a repair call: slots are optional.
pub fn clean_slots(raw: &[RawImageSlot], max: usize) -> (Vec<ImageSlot>, Vec<String>) {
    let mut out: Vec<ImageSlot> = Vec::new();
    let mut notes = Vec::new();
    for (i, r) in raw.iter().enumerate() {
        let n = i + 1;
        let mut slot = ImageSlot {
            id: String::new(),
            kind: SlotKind::parse(&r.kind).unwrap_or_default(),
            purpose_zh: clip(&r.purpose_zh, PURPOSE_CHARS),
            prompt_en: clip(&r.prompt_en, PROMPT_CHARS),
            alt_en: clip(&r.alt_en, ALT_CHARS),
            caption: clip(&r.caption, CAPTION_CHARS),
        };
        let texts = [
            &slot.purpose_zh,
            &slot.prompt_en,
            &slot.alt_en,
            &slot.caption,
        ];
        if texts.iter().any(|t| names_a_resource(t)) {
            notes.push(format!("第 {n} 個圖片位置寫了網址，拿掉"));
            continue;
        }
        if slot.prompt_en.is_empty() && slot.kind != SlotKind::PhotoNeeded {
            notes.push(format!("第 {n} 個圖片位置沒有畫面描述，拿掉"));
            continue;
        }
        if slot.purpose_zh.is_empty() {
            slot.purpose_zh = clip(
                if slot.alt_en.is_empty() {
                    &slot.prompt_en
                } else {
                    &slot.alt_en
                },
                PURPOSE_CHARS,
            );
        }
        if slot.alt_en.is_empty() {
            slot.alt_en = clip(&slot.prompt_en, ALT_CHARS);
        }
        if slot.kind != SlotKind::PhotoNeeded && needs_real_photo(&slot) {
            notes.push(format!("第 {n} 個圖片位置需要真實照片，改成留給教師"));
            slot.kind = SlotKind::PhotoNeeded;
        }
        if out
            .iter()
            .any(|o| o.prompt_en.eq_ignore_ascii_case(&slot.prompt_en) && o.kind == slot.kind)
        {
            notes.push(format!("第 {n} 個圖片位置與前面重複，拿掉"));
            continue;
        }
        if out.len() >= max {
            notes.push(format!("圖片位置超過 {max} 個，只留前面的"));
            break;
        }
        out.push(slot);
    }
    (out, notes)
}

/// Gives cleaned slots their ids: `{prefix}-p1`, `{prefix}-p2`…
pub fn with_ids(mut slots: Vec<ImageSlot>, prefix: &str) -> Vec<ImageSlot> {
    for (k, s) in slots.iter_mut().enumerate() {
        s.id = format!("{prefix}-p{}", k + 1);
    }
    slots
}

/// Checks the slots of a saved block (what a client sent): at most two, ids unique in the
/// worksheet, every string within its cap.
pub fn check_block_slots(block: &Block, seen: &mut HashSet<String>) -> Result<(), String> {
    if block.image_slots.len() > MAX_SLOTS_PER_BLOCK {
        return Err(format!("一個區塊最多 {MAX_SLOTS_PER_BLOCK} 個圖片位置。"));
    }
    for s in &block.image_slots {
        let id = s.id.trim();
        if id.is_empty() || id.chars().count() > super::MAX_ID_CHARS {
            return Err(format!(
                "圖片位置的 id 需為 1 到 {} 個字元。",
                super::MAX_ID_CHARS
            ));
        }
        if !seen.insert(id.to_string()) {
            return Err(format!("圖片位置的 id「{}」重複。", clip_chars(id, 40)));
        }
        for (what, text, max) in [
            ("圖片用途", &s.purpose_zh, PURPOSE_CHARS),
            ("畫面描述", &s.prompt_en, PROMPT_CHARS),
            ("替代文字", &s.alt_en, ALT_CHARS),
            ("圖說", &s.caption, CAPTION_CHARS),
        ] {
            if text.chars().count() > max {
                return Err(format!("{what}最多 {max} 字。"));
            }
        }
    }
    Ok(())
}

/// How many pictures OCR found on a template's sample (`materials.ocr_pages[].pictures`); 0 for
/// a sample that was not scanned (DOCX and PDFs with a text layer are not looked at yet).
pub async fn sample_pictures(db: &PgPool, material_id: Option<i64>) -> sqlx::Result<i64> {
    let Some(id) = material_id else {
        return Ok(0);
    };
    let n: Option<i64> = sqlx::query_scalar(
        "SELECT SUM(COALESCE((p->>'pictures')::bigint, 0))::bigint
           FROM materials m, jsonb_array_elements(COALESCE(m.ocr_pages, '[]'::jsonb)) p
          WHERE m.id = $1",
    )
    .bind(id)
    .fetch_one(db)
    .await?;
    Ok(n.unwrap_or(0))
}

/// The slot rules, with a line saying the teacher's sample had pictures (when it had).
pub fn rules_with_sample_hint(rules: &str, sample_pictures: i64) -> String {
    if sample_pictures <= 0 {
        return rules.to_string();
    }
    format!(
        "{rules}\n- 這位老師的範例學習單裡有 {sample_pictures} 張圖（圖的位置與內容目前讀不到）：照版型做的學習單也可以有圖，在需要圖的區塊寫出圖片位置。"
    )
}

/// The slot with this id in `content`, with its block.
pub fn find_slot<'a>(content: &'a Content, slot_id: &str) -> Option<(&'a Block, &'a ImageSlot)> {
    content.blocks.iter().find_map(|b| {
        b.image_slots
            .iter()
            .find(|s| s.id == slot_id)
            .map(|s| (b, s))
    })
}

// ---------------------------------------------------------------- the prompt

/// The house style and the prompt template (the latest `image_style` version, or the built-in
/// one).
pub struct Style {
    pub id: Option<i64>,
    pub version_no: Option<i32>,
    pub system: String,
    pub user: String,
}

pub async fn style(db: &PgPool) -> sqlx::Result<Style> {
    Ok(match crate::prompts::latest(db, STYLE_KEY).await? {
        Some(p) => Style {
            id: Some(p.id),
            version_no: Some(p.version_no),
            system: p.system_text,
            user: if p.user_template.trim().is_empty() {
                FALLBACK_USER.to_string()
            } else {
                p.user_template
            },
        },
        None => Style {
            id: None,
            version_no: None,
            system: FALLBACK_STYLE.to_string(),
            user: FALLBACK_USER.to_string(),
        },
    })
}

/// What the picture model gets: the house style, then the slot's description and the
/// teacher's note.
pub fn picture_prompt(style: &Style, slot: &ImageSlot, hint: &str) -> String {
    let hint = hint.trim();
    let vars = HashMap::from([
        ("description", slot.prompt_en.trim().to_string()),
        (
            "teacher_note",
            if hint.is_empty() {
                String::new()
            } else {
                format!("\nTeacher's note: {hint}")
            },
        ),
    ]);
    format!(
        "{}\n\n{}",
        style.system.trim(),
        crate::prompts::render(&style.user, &vars).trim()
    )
}

/// The teacher's stable, privacy-preserving id for the provider's abuse monitoring: a hash,
/// never a name or a research id.
pub fn safety_identifier(teacher_id: i64) -> String {
    crate::auth::sha256_hex(format!("manna-teacher:{teacher_id}"))
}

// ---------------------------------------------------------------- stored pictures

/// A picture ready to store: JPEG, upright, at most [`MAX_SIDE`] on its longest side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Normalized {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Decodes any picture (PNG, JPEG, WebP, GIF's first frame), turns it upright, scales it down
/// to [`MAX_SIDE`] and writes it as JPEG (transparency on white). Drops every piece of metadata
/// (EXIF, GPS). CPU-bound: call it from `spawn_blocking`.
pub fn normalize(data: &[u8]) -> Result<Normalized, String> {
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
    Ok(Normalized {
        bytes: out.into_inner(),
        width: flat.width(),
        height: flat.height(),
    })
}

async fn normalize_blocking(data: Vec<u8>) -> AppResult<Result<Normalized, String>> {
    tokio::task::spawn_blocking(move || normalize(&data))
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("picture task failed: {e}")))
}

fn storage_key(worksheet_id: i64, image_id: i64) -> String {
    let token = crate::auth::new_token();
    format!(
        "worksheet-images/w{worksheet_id}/{image_id}-{}.jpg",
        &token[..16]
    )
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

// ---------------------------------------------------------------- state of the slots

/// The current picture of one slot (`GET /api/worksheets/:id` → `images.slots`).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SlotState {
    pub slot_id: String,
    pub block_id: String,
    /// queued | running | done | failed | skipped | removed
    pub status: String,
    /// generated | upload | none
    pub source: String,
    /// With `done`: `GET /api/worksheets/:id/images/:image_id`.
    pub image_id: Option<i64>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    /// Why there is no picture (Chinese, for the teacher).
    pub message: Option<String>,
    pub updated_at: DateTime<Utc>,
}

/// `images` of the worksheet state.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct WorksheetImages {
    /// Whether pictures can be generated here (`IMAGE_PROVIDER` not `none`).
    pub enabled: bool,
    /// 重畫 left for this worksheet.
    pub regenerations_left: i64,
    pub slots: Vec<SlotState>,
}

#[derive(sqlx::FromRow)]
struct SlotRow {
    id: i64,
    slot_id: String,
    block_id: String,
    status: String,
    source: String,
    width: Option<i32>,
    height: Option<i32>,
    message: Option<String>,
    updated_at: DateTime<Utc>,
}

impl From<SlotRow> for SlotState {
    fn from(r: SlotRow) -> Self {
        let done = r.status == STATUS_DONE;
        Self {
            slot_id: r.slot_id,
            block_id: r.block_id,
            image_id: done.then_some(r.id),
            width: r.width.filter(|_| done),
            height: r.height.filter(|_| done),
            status: r.status,
            source: r.source,
            message: r.message,
            updated_at: r.updated_at,
        }
    }
}

const SLOT_COLS: &str = "id, slot_id, block_id, status, source, width, height, message,
    COALESCE(finished_at, started_at, created_at) AS updated_at";

/// The current picture of every slot that has one, by slot id.
pub async fn current(db: &PgPool, worksheet_id: i64) -> sqlx::Result<Vec<SlotState>> {
    let rows: Vec<SlotRow> = sqlx::query_as(&format!(
        "SELECT {SLOT_COLS} FROM worksheet_images
          WHERE worksheet_id = $1 AND current ORDER BY id"
    ))
    .bind(worksheet_id)
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(SlotState::from).collect())
}

async fn slot_state(
    db: &PgPool,
    worksheet_id: i64,
    slot_id: &str,
) -> sqlx::Result<Option<SlotState>> {
    let row: Option<SlotRow> = sqlx::query_as(&format!(
        "SELECT {SLOT_COLS} FROM worksheet_images
          WHERE worksheet_id = $1 AND slot_id = $2 AND current"
    ))
    .bind(worksheet_id)
    .bind(slot_id)
    .fetch_optional(db)
    .await?;
    Ok(row.map(SlotState::from))
}

async fn regenerations_used(db: &PgPool, worksheet_id: i64) -> sqlx::Result<i64> {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM worksheet_images WHERE worksheet_id = $1 AND regeneration",
    )
    .bind(worksheet_id)
    .fetch_one(db)
    .await
}

/// The pictures of a worksheet, after open rows that died with another process are closed.
pub async fn states(state: &AppState, worksheet_id: i64) -> sqlx::Result<WorksheetImages> {
    expire_stale(state, worksheet_id).await?;
    let used = regenerations_used(&state.db, worksheet_id).await?;
    Ok(WorksheetImages {
        enabled: state.images.enabled(),
        regenerations_left: (i64::from(state.images.config().max_regenerations) - used).max(0),
        slots: current(&state.db, worksheet_id).await?,
    })
}

// ---------------------------------------------------------------- new slots → pictures

/// Gives every slot of the enabled blocks of `content` that has never had a picture its first
/// row, and starts the jobs. Called once a generation or a regenerated block has been saved.
/// Returns how many pictures were queued.
pub async fn enqueue(
    state: &AppState,
    worksheet_id: i64,
    teacher_id: i64,
    content: &Content,
) -> AppResult<usize> {
    let wanted: Vec<(&Block, &ImageSlot)> = content
        .blocks
        .iter()
        .filter(|b| b.enabled)
        .flat_map(|b| b.image_slots.iter().map(move |s| (b, s)))
        .collect();
    if wanted.is_empty() {
        return Ok(0);
    }
    let config = state.images.config();
    let mut tx = state.db.begin().await?;
    sqlx::query("SELECT id FROM worksheets WHERE id = $1 FOR UPDATE")
        .bind(worksheet_id)
        .execute(&mut *tx)
        .await?;
    let known: HashSet<String> = sqlx::query_scalar::<_, String>(
        "SELECT DISTINCT slot_id FROM worksheet_images WHERE worksheet_id = $1",
    )
    .bind(worksheet_id)
    .fetch_all(&mut *tx)
    .await?
    .into_iter()
    .collect();
    let mut generated: i64 = sqlx::query_scalar(
        "SELECT COUNT(DISTINCT slot_id) FROM worksheet_images
          WHERE worksheet_id = $1 AND source = 'generated' AND NOT regeneration",
    )
    .bind(worksheet_id)
    .fetch_one(&mut *tx)
    .await?;
    let mut queued: Vec<(i64, DateTime<Utc>)> = Vec::new();
    let (mut skipped, mut seen) = (0usize, HashSet::new());
    for (block, slot) in wanted {
        if known.contains(&slot.id) || !seen.insert(slot.id.clone()) {
            continue;
        }
        let skip = if slot.kind == SlotKind::PhotoNeeded {
            Some(("photo_needed", MSG_PHOTO_NEEDED))
        } else if !state.images.enabled() {
            Some(("disabled", MSG_DISABLED))
        } else if state.storage.is_none() {
            Some(("storage", MSG_NO_STORAGE))
        } else if generated >= i64::from(config.max_per_worksheet) {
            Some(("cap", MSG_CAP))
        } else {
            None
        };
        match skip {
            Some((kind, message)) => {
                sqlx::query(
                    "INSERT INTO worksheet_images
                       (worksheet_id, teacher_id, block_id, slot_id, source, status, message,
                        error_kind, finished_at)
                     VALUES ($1, $2, $3, $4, 'none', 'skipped', $5, $6, now())",
                )
                .bind(worksheet_id)
                .bind(teacher_id)
                .bind(&block.id)
                .bind(&slot.id)
                .bind(message)
                .bind(kind)
                .execute(&mut *tx)
                .await?;
                skipped += 1;
            }
            None => {
                let row: (i64, DateTime<Utc>) = sqlx::query_as(
                    "INSERT INTO worksheet_images
                       (worksheet_id, teacher_id, block_id, slot_id, source, status, started_at)
                     VALUES ($1, $2, $3, $4, 'generated', 'queued', clock_timestamp())
                     RETURNING id, started_at",
                )
                .bind(worksheet_id)
                .bind(teacher_id)
                .bind(&block.id)
                .bind(&slot.id)
                .fetch_one(&mut *tx)
                .await?;
                queued.push(row);
                generated += 1;
            }
        }
    }
    if !queued.is_empty() || skipped > 0 {
        log_usage(
            &mut *tx,
            teacher_id,
            "worksheet_images_queued",
            json!({ "worksheet_id": worksheet_id, "queued": queued.len(), "skipped": skipped }),
        )
        .await?;
    }
    tx.commit().await?;
    for (id, run) in &queued {
        job::spawn(state, *id, *run);
    }
    Ok(queued.len())
}

/// [`enqueue`] where a failure must not undo what was saved before it: logged instead.
pub async fn enqueue_logged(
    state: &AppState,
    worksheet_id: i64,
    teacher_id: i64,
    content: &Content,
) {
    if let Err(e) = enqueue(state, worksheet_id, teacher_id, content).await {
        tracing::error!(worksheet_id, error = %e, "could not queue the worksheet's pictures");
    }
}

// ---------------------------------------------------------------- teacher actions

/// The latest content and the slot in it, or 404.
async fn slot_of(db: &PgPool, worksheet_id: i64, slot_id: &str) -> AppResult<(String, ImageSlot)> {
    let content = super::store::latest(db, worksheet_id).await?.content;
    find_slot(&content, slot_id)
        .map(|(b, s)| (b.id.clone(), s.clone()))
        .ok_or_else(|| AppError::not_found(MSG_SLOT_NOT_FOUND))
}

/// Takes the slot's current row out (a queued or running one is closed, so its job stops at its
/// next write).
async fn retire_current(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    worksheet_id: i64,
    slot_id: &str,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE worksheet_images
            SET current = FALSE,
                status = CASE WHEN status IN ('queued', 'running') THEN 'skipped' ELSE status END,
                error_kind = CASE WHEN status IN ('queued', 'running') THEN 'replaced'
                                  ELSE error_kind END,
                finished_at = COALESCE(finished_at, now())
          WHERE worksheet_id = $1 AND slot_id = $2 AND current",
    )
    .bind(worksheet_id)
    .bind(slot_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn lock_worksheet(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    worksheet_id: i64,
) -> sqlx::Result<()> {
    sqlx::query("SELECT id FROM worksheets WHERE id = $1 FOR UPDATE")
        .bind(worksheet_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// The teacher's note for a 重畫, trimmed; too long or with a web address is a 400.
pub fn checked_hint(hint: Option<&str>) -> AppResult<String> {
    let hint = hint
        .unwrap_or_default()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if hint.chars().count() > HINT_CHARS {
        return Err(AppError::bad_request(format!(
            "給這張圖的說明最多 {HINT_CHARS} 字。"
        )));
    }
    if names_a_resource(&hint) {
        return Err(AppError::bad_request("給這張圖的說明不能有網址。"));
    }
    Ok(hint)
}

/// 「重畫」: a new picture for the slot, with the teacher's note. 503 without pictures or
/// storage, 404 for an unknown slot, 400 for a slot that needs a real photo, 409 while its
/// picture is being made or when the worksheet's 重畫 are used up.
pub async fn regenerate(
    state: &AppState,
    ws: &Worksheet,
    slot_id: &str,
    hint: Option<&str>,
) -> AppResult<SlotState> {
    let hint = checked_hint(hint)?;
    if !state.images.enabled() {
        return Err(AppError::Unavailable(MSG_DISABLED.into()));
    }
    if state.storage.is_none() {
        return Err(crate::materials::store::storage_missing());
    }
    let (block_id, slot) = slot_of(&state.db, ws.id, slot_id).await?;
    if slot.kind == SlotKind::PhotoNeeded {
        return Err(AppError::bad_request(MSG_PHOTO_ONLY));
    }
    let max = i64::from(state.images.config().max_regenerations);
    let mut tx = state.db.begin().await?;
    lock_worksheet(&mut tx, ws.id).await?;
    let busy: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM worksheet_images
                         WHERE worksheet_id = $1 AND slot_id = $2 AND current
                           AND status IN ('queued', 'running'))",
    )
    .bind(ws.id)
    .bind(slot_id)
    .fetch_one(&mut *tx)
    .await?;
    if busy {
        return Err(AppError::Conflict(MSG_BUSY.into()));
    }
    let used: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM worksheet_images WHERE worksheet_id = $1 AND regeneration",
    )
    .bind(ws.id)
    .fetch_one(&mut *tx)
    .await?;
    if used >= max {
        return Err(AppError::Conflict(format!(
            "這份學習單的圖已經重畫 {max} 次，到上限了；可以上傳自己的圖。"
        )));
    }
    retire_current(&mut tx, ws.id, slot_id).await?;
    let (id, run): (i64, DateTime<Utc>) = sqlx::query_as(
        "INSERT INTO worksheet_images
           (worksheet_id, teacher_id, block_id, slot_id, source, status, regeneration, hint,
            started_at)
         VALUES ($1, $2, $3, $4, 'generated', 'queued', TRUE, $5, clock_timestamp())
         RETURNING id, started_at",
    )
    .bind(ws.id)
    .bind(ws.teacher_id)
    .bind(&block_id)
    .bind(slot_id)
    .bind(&hint)
    .fetch_one(&mut *tx)
    .await?;
    log_usage(
        &mut *tx,
        ws.teacher_id,
        "worksheet_image_regenerated",
        json!({ "worksheet_id": ws.id, "slot_id": slot_id, "hint_chars": hint.chars().count(),
                "regenerations_used": used + 1 }),
    )
    .await?;
    tx.commit().await?;
    // Read the answer before the job starts: a fast job could otherwise finish first.
    let answer = slot_state(&state.db, ws.id, slot_id)
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("picture row vanished")))?;
    job::spawn(state, id, run);
    Ok(answer)
}

/// 「換成我的圖」: the teacher's picture (already checked as an image upload) becomes the
/// slot's. Stored as JPEG like every picture.
pub async fn replace_with_upload(
    state: &AppState,
    ws: &Worksheet,
    slot_id: &str,
    filename: &str,
    data: Bytes,
) -> AppResult<SlotState> {
    let Some(storage) = state.storage.clone() else {
        return Err(crate::materials::store::storage_missing());
    };
    let (block_id, _) = slot_of(&state.db, ws.id, slot_id).await?;
    let file_type = crate::materials::file_type_for_name(filename)
        .filter(|t| t.kind == crate::materials::Kind::Image)
        .ok_or_else(|| AppError::bad_request(MSG_NOT_AN_IMAGE))?;
    if !crate::materials::content_matches(file_type, &data) {
        return Err(AppError::bad_request(MSG_UNREADABLE));
    }
    let original_bytes = data.len();
    let picture = normalize_blocking(data.to_vec())
        .await?
        .map_err(AppError::bad_request)?;
    let mut tx = state.db.begin().await?;
    lock_worksheet(&mut tx, ws.id).await?;
    retire_current(&mut tx, ws.id, slot_id).await?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO worksheet_images
           (worksheet_id, teacher_id, block_id, slot_id, source, status, current)
         VALUES ($1, $2, $3, $4, 'upload', 'queued', FALSE) RETURNING id",
    )
    .bind(ws.id)
    .bind(ws.teacher_id)
    .bind(&block_id)
    .bind(slot_id)
    .fetch_one(&mut *tx)
    .await?;
    let key = storage_key(ws.id, id);
    storage
        .put(&key, MIME, picture.bytes.clone().into())
        .await
        .map_err(|e| {
            tracing::error!(worksheet_id = ws.id, error = %e, "storing an uploaded picture failed");
            AppError::Unavailable(MSG_NO_STORAGE.into())
        })?;
    sqlx::query(
        "UPDATE worksheet_images
            SET status = 'done', current = TRUE, storage_key = $2, content_type = $3,
                bytes = $4, width = $5, height = $6, sha256 = $7, finished_at = now()
          WHERE id = $1",
    )
    .bind(id)
    .bind(&key)
    .bind(MIME)
    .bind(picture.bytes.len() as i32)
    .bind(picture.width as i32)
    .bind(picture.height as i32)
    .bind(crate::auth::sha256_hex(&picture.bytes))
    .execute(&mut *tx)
    .await?;
    log_usage(
        &mut *tx,
        ws.teacher_id,
        "worksheet_image_uploaded",
        json!({ "worksheet_id": ws.id, "slot_id": slot_id, "original_bytes": original_bytes,
                "stored_bytes": picture.bytes.len(), "width": picture.width,
                "height": picture.height }),
    )
    .await?;
    tx.commit().await?;
    slot_state(&state.db, ws.id, slot_id)
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("picture row vanished")))
}

/// 「拿掉」: the slot keeps its place in the block but prints nothing; 重畫 or an upload brings
/// a picture back.
pub async fn remove(state: &AppState, ws: &Worksheet, slot_id: &str) -> AppResult<SlotState> {
    let (block_id, _) = slot_of(&state.db, ws.id, slot_id).await?;
    let mut tx = state.db.begin().await?;
    lock_worksheet(&mut tx, ws.id).await?;
    retire_current(&mut tx, ws.id, slot_id).await?;
    sqlx::query(
        "INSERT INTO worksheet_images
           (worksheet_id, teacher_id, block_id, slot_id, source, status, finished_at)
         VALUES ($1, $2, $3, $4, 'none', 'removed', now())",
    )
    .bind(ws.id)
    .bind(ws.teacher_id)
    .bind(&block_id)
    .bind(slot_id)
    .execute(&mut *tx)
    .await?;
    log_usage(
        &mut *tx,
        ws.teacher_id,
        "worksheet_image_removed",
        json!({ "worksheet_id": ws.id, "slot_id": slot_id }),
    )
    .await?;
    tx.commit().await?;
    slot_state(&state.db, ws.id, slot_id)
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("picture row vanished")))
}

/// A stored picture of the worksheet: its storage key and SHA-256 (the ETag).
pub async fn stored(
    db: &PgPool,
    worksheet_id: i64,
    image_id: i64,
) -> sqlx::Result<Option<(String, String)>> {
    sqlx::query_as(
        "SELECT storage_key, sha256 FROM worksheet_images
          WHERE id = $1 AND worksheet_id = $2 AND status = 'done'
            AND storage_key IS NOT NULL AND sha256 IS NOT NULL",
    )
    .bind(image_id)
    .bind(worksheet_id)
    .fetch_optional(db)
    .await
}

/// The bytes of a stored picture (404 when it is gone, 503 when storage is down).
pub async fn bytes_of(state: &AppState, key: &str) -> AppResult<Vec<u8>> {
    if state.storage.is_none() {
        return Err(crate::materials::store::storage_missing());
    }
    read_all(state, key).await.map_err(|e| match e {
        StorageError::NotFound => AppError::not_found("這張圖已經不在儲存空間裡了。"),
        StorageError::Other(e) => {
            tracing::error!(error = %e, "reading a worksheet picture failed");
            AppError::Unavailable("圖片暫時讀不到，請稍後再試。".into())
        }
    })
}

// ---------------------------------------------------------------- printed sheets

/// Puts the slots of `content` into the projected `sheet`, with their pictures: a picture that
/// is there, or an empty box (with the purpose and the reason in the teacher version). Removed
/// slots print nothing. Slots of blocks the sheet does not print are left out.
pub fn decorate(sheet: &mut Sheet, content: &Content, states: &[SlotState]) {
    let teacher = sheet.audience == super::Audience::Teacher;
    let by_slot: HashMap<&str, &SlotState> =
        states.iter().map(|s| (s.slot_id.as_str(), s)).collect();
    for b in &mut sheet.blocks {
        let Some(block) = content.block(&b.id) else {
            continue;
        };
        b.images = block
            .image_slots
            .iter()
            .filter_map(|slot| {
                let st = by_slot.get(slot.id.as_str());
                let status = st.map_or(STATUS_SKIPPED, |s| s.status.as_str());
                if status == STATUS_REMOVED {
                    return None;
                }
                let done = status == STATUS_DONE;
                Some(SheetImage {
                    slot_id: slot.id.clone(),
                    status: status.to_string(),
                    image_id: st.and_then(|s| s.image_id).filter(|_| done),
                    width: st.and_then(|s| s.width),
                    height: st.and_then(|s| s.height),
                    alt: slot.alt_en.clone(),
                    caption: slot.caption.clone(),
                    ai_generated: done && st.is_some_and(|s| s.source == SOURCE_GENERATED),
                    purpose: teacher.then(|| slot.purpose_zh.clone()),
                    message: if teacher {
                        st.and_then(|s| s.message.clone())
                    } else {
                        None
                    },
                })
            })
            .collect();
    }
}

/// The stored pictures a sheet prints, for an export. A picture that cannot be read is left
/// out (the export prints its box instead).
pub async fn bank_for(state: &AppState, worksheet_id: i64, sheet: &Sheet) -> AppResult<ImageBank> {
    let mut bank = ImageBank::new();
    for img in sheet.blocks.iter().flat_map(|b| &b.images) {
        let Some(id) = img.image_id else { continue };
        if bank.contains_key(&id) {
            continue;
        }
        let Some((key, _)) = stored(&state.db, worksheet_id, id).await? else {
            continue;
        };
        match read_all(state, &key).await {
            Ok(bytes) => {
                let (w, h) = (img.width.unwrap_or(0), img.height.unwrap_or(0));
                if w > 0 && h > 0 {
                    bank.insert(
                        id,
                        ExportImage {
                            bytes,
                            width: w as u32,
                            height: h as u32,
                        },
                    );
                }
            }
            Err(e) => {
                tracing::warn!(worksheet_id, image_id = id, error = %e, "a picture could not be read for an export");
            }
        }
    }
    Ok(bank)
}

// ---------------------------------------------------------------- restarts

/// Seconds after which an open row that no job of this process holds is reported as failed:
/// two attempts and some slack.
fn stale_after(state: &AppState) -> f64 {
    (state.images.config().timeout_secs * 2 + 120) as f64
}

/// Open rows of the worksheet started long ago and held by no job of this process are marked
/// failed (the teacher can 重畫).
pub async fn expire_stale(state: &AppState, worksheet_id: i64) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE worksheet_images
            SET status = 'failed', message = $2, error_kind = 'interrupted', finished_at = now()
          WHERE worksheet_id = $1 AND status IN ('queued', 'running')
            AND started_at < now() - make_interval(secs => $3::float8)
            AND NOT (id = ANY($4))",
    )
    .bind(worksheet_id)
    .bind(MSG_STALE)
    .bind(stale_after(state))
    .bind(state.images.live_ids())
    .execute(&state.db)
    .await?;
    Ok(())
}

/// At startup, before requests are served: every row left open by the previous process (the
/// app runs as one instance) is taken over with a new run time, so a job of an old container
/// still running stops writing, and its picture is made again. Without pictures or storage
/// now, it is marked failed. Returns how many were resumed.
pub async fn recover(state: &AppState) -> sqlx::Result<usize> {
    let live = state.images.live_ids();
    let open: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM worksheet_images WHERE status IN ('queued', 'running') ORDER BY id",
    )
    .fetch_all(&state.db)
    .await?;
    let mut resumed = 0;
    for id in open.into_iter().filter(|id| !live.contains(id)) {
        if !state.images.enabled() || state.storage.is_none() {
            sqlx::query(
                "UPDATE worksheet_images
                    SET status = 'failed', message = $2, error_kind = 'interrupted',
                        finished_at = now()
                  WHERE id = $1 AND status IN ('queued', 'running')",
            )
            .bind(id)
            .bind(MSG_CANNOT_RESUME)
            .execute(&state.db)
            .await?;
            continue;
        }
        let taken: Option<DateTime<Utc>> = sqlx::query_scalar(
            "UPDATE worksheet_images SET status = 'queued', started_at = clock_timestamp()
              WHERE id = $1 AND status IN ('queued', 'running')
              RETURNING started_at",
        )
        .bind(id)
        .fetch_optional(&state.db)
        .await?;
        if let Some(at) = taken {
            job::spawn(state, id, at);
            resumed += 1;
        }
    }
    Ok(resumed)
}

/// What the teacher-facing message and the counted kind of a failed picture are.
pub(crate) fn failure_of(e: &client::ImageError) -> (&'static str, String) {
    use client::ImageError as E;
    let kind = match e {
        E::Disabled => "disabled",
        E::Timeout => "timeout",
        E::Http { .. } => "http",
        E::Moderated { .. } => "moderated",
        E::Network(_) => "network",
        E::Invalid(_) => "invalid",
    };
    (kind, e.slot_message())
}

pub(crate) const MSG_JOB_INTERNAL: &str = MSG_INTERNAL;
pub(crate) const MSG_JOB_GONE: &str = MSG_GONE;
pub(crate) const MSG_JOB_STORE_FAILED: &str = MSG_STORE_FAILED;
pub(crate) const MSG_JOB_NO_STORAGE: &str = MSG_NO_STORAGE;

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(kind: &str, prompt: &str) -> RawImageSlot {
        RawImageSlot {
            kind: kind.into(),
            purpose_zh: "幫助學生想像場景".into(),
            prompt_en: prompt.into(),
            alt_en: String::new(),
            caption: "A rainy street".into(),
        }
    }

    #[test]
    fn the_schema_is_bounded_and_ordered() {
        let s = slots_schema(2);
        assert_eq!(s["maxItems"], 2);
        let props = s["items"]["properties"].as_object().unwrap();
        assert_eq!(
            props.keys().map(String::as_str).collect::<Vec<_>>(),
            ["kind", "purpose_zh", "prompt_en", "alt_en", "caption"]
        );
        assert_eq!(
            s["items"]["required"],
            json!(["kind", "purpose_zh", "prompt_en", "alt_en", "caption"])
        );
        assert_eq!(props["kind"]["enum"], json!(KINDS));
        assert_eq!(props["purpose_zh"]["maxLength"], PURPOSE_CHARS);
        assert_eq!(props["prompt_en"]["maxLength"], PROMPT_CHARS);
        assert_eq!(props["alt_en"]["maxLength"], ALT_CHARS);
        assert_eq!(props["caption"]["maxLength"], CAPTION_CHARS);
        assert_eq!(s["items"]["additionalProperties"], false);

        let mut o = obj(vec![
            ("title", s_max(10)),
            ("items", json!({"type": "array"})),
            ("reason", s_max(10)),
        ]);
        add_slots_property(&mut o, Some("items"), 1);
        let keys: Vec<&String> = o["properties"].as_object().unwrap().keys().collect();
        assert_eq!(keys, ["title", "items", "image_slots", "reason"]);
        assert_eq!(
            o["required"],
            json!(["title", "items", "image_slots", "reason"])
        );
        assert_eq!(o["properties"]["image_slots"]["maxItems"], 1);
        let mut last = obj(vec![("title", s_max(10))]);
        add_slots_property(&mut last, None, 2);
        assert_eq!(last["required"], json!(["title", "image_slots"]));
    }

    #[test]
    fn slots_are_cleaned() {
        let long = "word ".repeat(200);
        let (slots, notes) = clean_slots(
            &[
                raw("scene", "A girl holding an umbrella in the rain."),
                raw("weird", &long),
                raw("object", "see https://example.com"),
                raw("scene", "A girl holding an umbrella in the rain."),
            ],
            2,
        );
        assert_eq!(slots.len(), 2, "{notes:?}");
        assert_eq!(slots[0].kind, SlotKind::Scene);
        assert_eq!(
            slots[0].alt_en, "A girl holding an umbrella in the rain.",
            "alt filled in"
        );
        assert_eq!(slots[1].kind, SlotKind::Scene, "unknown kind → scene");
        assert_eq!(
            slots[1].prompt_en.chars().count(),
            PROMPT_CHARS - 1,
            "cut and trimmed"
        );
        assert!(notes.iter().any(|n| n.contains("網址")));
        // Over the cap: only the first ones.
        let (one, notes) = clean_slots(&[raw("scene", "A cat."), raw("icon", "A dog.")], 1);
        assert_eq!(one.len(), 1);
        assert!(notes.iter().any(|n| n.contains("超過")));
        // Nothing to draw: dropped, unless it is a photo for the teacher.
        let (none, _) = clean_slots(&[raw("scene", "  ")], 2);
        assert!(none.is_empty());
        let (photo, _) = clean_slots(&[raw("photo_needed", "")], 2);
        assert_eq!(photo[0].kind, SlotKind::PhotoNeeded);
    }

    #[test]
    fn real_photos_are_left_for_the_teacher() {
        for prompt in [
            "A map of Taiwan with the main cities.",
            "The flag of Japan waving.",
            "A portrait of a famous scientist.",
            "Taylor Swift singing on a stage.",
            "Tourists in front of the Eiffel Tower.",
            "Soldiers in a historical battle.",
            "A bottle with a Coca-Cola logo.",
        ] {
            let (slots, notes) = clean_slots(&[raw("scene", prompt)], 2);
            assert_eq!(slots[0].kind, SlotKind::PhotoNeeded, "{prompt}");
            assert!(!notes.is_empty());
        }
        for prompt in [
            "A girl holding an umbrella in the rain.",
            "Students sharing umbrellas outside a small shop in Taipei.",
            "Mrs. Lin smiling next to an umbrella stand.",
            "FAKE_IMG_FAIL a shop.",
        ] {
            let (slots, _) = clean_slots(&[raw("scene", prompt)], 2);
            assert_eq!(slots[0].kind, SlotKind::Scene, "{prompt}");
        }
    }

    #[test]
    fn saved_slots_are_checked() {
        let slot = |id: &str| ImageSlot {
            id: id.into(),
            prompt_en: "A cat.".into(),
            ..ImageSlot::default()
        };
        let mut block = Block {
            id: "b1".into(),
            image_slots: vec![slot("b1-p1"), slot("b1-p2")],
            ..Block::default()
        };
        let mut seen = HashSet::new();
        check_block_slots(&block, &mut seen).unwrap();
        assert!(check_block_slots(&block, &mut seen)
            .unwrap_err()
            .contains("重複"));
        block.image_slots.push(slot("b1-p3"));
        assert!(check_block_slots(&block, &mut HashSet::new()).is_err());
        block.image_slots = vec![ImageSlot {
            caption: "c".repeat(CAPTION_CHARS + 1),
            ..slot("x")
        }];
        assert!(check_block_slots(&block, &mut HashSet::new())
            .unwrap_err()
            .contains("圖說"));
        block.image_slots = vec![slot("")];
        assert!(check_block_slots(&block, &mut HashSet::new()).is_err());
    }

    #[test]
    fn the_prompt_has_the_house_style_first() {
        let style = Style {
            id: None,
            version_no: None,
            system: FALLBACK_STYLE.into(),
            user: FALLBACK_USER.into(),
        };
        let slot = ImageSlot {
            prompt_en: "A girl with an umbrella.".into(),
            ..ImageSlot::default()
        };
        let p = picture_prompt(&style, &slot, "");
        assert!(p.starts_with("Simple flat vector illustration"));
        assert!(p.ends_with("Picture: A girl with an umbrella."));
        assert!(p.contains("No text, letters"));
        let with = picture_prompt(&style, &slot, "  make it sunny ");
        assert!(with.ends_with("Picture: A girl with an umbrella.\nTeacher's note: make it sunny"));
        let id = safety_identifier(7);
        assert_eq!(id.len(), 64);
        assert_ne!(id, safety_identifier(8));
    }

    #[test]
    fn hints_are_checked() {
        assert_eq!(
            checked_hint(Some("  more  colour ")).unwrap(),
            "more colour"
        );
        assert_eq!(checked_hint(None).unwrap(), "");
        assert!(checked_hint(Some(&"x".repeat(HINT_CHARS + 1))).is_err());
        assert!(checked_hint(Some("like www.example.com")).is_err());
    }

    #[test]
    fn pictures_are_stored_as_upright_jpeg_within_the_size() {
        let png = client::fake_png("x");
        let n = normalize(&png).unwrap();
        assert!(n.bytes.starts_with(b"\xFF\xD8\xFF"));
        assert_eq!((n.width, n.height), (48, 32));
        let big = image::RgbaImage::from_pixel(3000, 1000, image::Rgba([0, 0, 0, 0]));
        let mut out = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(big)
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
        let n = normalize(&out.into_inner()).unwrap();
        assert_eq!((n.width, n.height), (MAX_SIDE, 512));
        assert!(normalize(b"not a picture").is_err());
    }
}
