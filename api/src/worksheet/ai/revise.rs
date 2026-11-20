//! 逐區修改: one model call (`worksheet_block_revise`) for one block, run in the background, whose
//! result is a proposal ([`crate::worksheet::revise`]). Nothing in the worksheet changes until
//! the teacher accepts it, so a revision is not a worksheet job: it does not lock the editor or
//! hide the preview. One runs at a time per worksheet (parts of one worksheet asked for at once
//! repeat each other); 「再改一次」 is a new proposal with the next `round`, which changes the
//! request (the gateway caches identical ones).
//!
//! The call goes through the same helper as the 備課對話 calls (`application::generation::call`): `max_tokens`
//! always set, one retry with the temperature raised and the failed checks listed, every
//! attempt its own `ai_interactions` row, the material logged only as its length and a hash.

use std::collections::{HashMap, HashSet};

use serde_json::{json, Value};

use super::RunInput;
use crate::application::generation::{call, nothing, Call, Checked};
use crate::audit::log_usage;
use crate::error::{AppError, AppResult};
use crate::text::{clip_chars, or_blank};
use crate::worksheet::ai::template::marked_passage;
use crate::worksheet::assemble::project;
use crate::worksheet::bloom::{Bloom, Output};
use crate::worksheet::checks::SectionCtx;
use crate::worksheet::comments::{self, Comment, Field, STATUS_OPEN};
use crate::worksheet::images::actions;
use crate::worksheet::revise::{
    self as rv, code, max_items, max_tokens, merge, MergeCtx, Merged, NewProposal, Proposal,
    RawBlockRevision, Scope, KEY_BLOCK_REVISE, SCAFFOLD, SCHEMA_NAME,
};
use crate::worksheet::template::MSG_EXTERNAL;
use crate::worksheet::{
    store, Audience, Block, BlockKind, Content, Level, ANSWER_FORMATS, MAX_ID_CHARS,
};
use crate::AppState;

const BASE_TEMPERATURE: f32 = 0.2;
const MSG_INTERNAL: &str = "處理這次修改時發生錯誤。請再請 Manna 改一次。";
const MSG_NOTHING_TO_DO: &str = "請寫下想怎麼改這一區，或先在這一區加一則勾了「提供給 AI」的註記。";

/// What the teacher asked for.
#[derive(Debug, Clone, Default)]
pub struct ReviseRequest {
    pub instruction: String,
    /// Notes to revise by; none (and no instruction) means every open note of the block that is
    /// shared with Manna.
    pub comment_ids: Vec<i64>,
    /// 「可以動到這一區的其他題」
    pub whole_block: bool,
    /// Pictures uploaded to the worksheet's prep session (`POST /api/prep-sessions/:id/images`),
    /// at most 2, sent with the request.
    pub image_ids: Vec<i64>,
}

/// After this many seconds a running revision died with the server: two attempts and a margin.
pub fn stale_after(state: &AppState) -> u64 {
    state.ai.longest_timeout_secs() * 2 + 60
}

/// Checks the request while it can still be answered with a 4xx, opens a `running` proposal and
/// starts the call in the background.
pub async fn start(
    state: &AppState,
    input: RunInput,
    block_id: &str,
    req: ReviseRequest,
) -> AppResult<Proposal> {
    let db = &state.db;
    let ws = &input.worksheet;
    let latest = store::latest(db, ws.id).await?;
    let block = latest
        .content
        .block(block_id)
        .cloned()
        .ok_or_else(|| AppError::not_found("找不到這個區塊。"))?;
    if super::jobs(state, ws.id).await?.1 {
        return Err(AppError::Conflict(
            "Manna 正在產生這份學習單，好了之後再請 Manna 改這一區。".to_string(),
        ));
    }
    let instruction = req.instruction.trim().to_string();
    if instruction.chars().count() > rv::MAX_INSTRUCTION_CHARS {
        return Err(AppError::bad_request(format!(
            "修改指示最多 {} 字。",
            rv::MAX_INSTRUCTION_CHARS
        )));
    }
    let in_block = |c: &Comment| c.anchor.block_id.as_deref().unwrap_or(&c.block_id) == block.id;
    let notes: Vec<Comment> = if req.comment_ids.is_empty() {
        if instruction.is_empty() {
            comments::list(db, ws.id, ws.teacher_id, &latest.content)
                .await?
                .into_iter()
                .filter(|c| in_block(c) && c.share_with_ai && c.status == STATUS_OPEN)
                .collect()
        } else {
            Vec::new()
        }
    } else {
        let mut ids = req.comment_ids.clone();
        ids.sort_unstable();
        ids.dedup();
        let found =
            comments::by_ids(db, ws.id, ws.teacher_id, &req.comment_ids, &latest.content).await?;
        if found.len() != ids.len() {
            return Err(AppError::bad_request("找不到其中一則註記。"));
        }
        if let Some(c) = found.iter().find(|c| !in_block(c)) {
            return Err(AppError::bad_request(format!("註記 {} 不在這一區。", c.id)));
        }
        if found.iter().any(|c| !c.share_with_ai) {
            return Err(AppError::bad_request(
                "有一則註記沒有勾「提供給 AI」，Manna 不會讀它。",
            ));
        }
        if found.iter().any(|c| c.status != STATUS_OPEN) {
            return Err(AppError::bad_request(
                "有一則註記已經處理過了（已依此修改或已略過）。",
            ));
        }
        found
    };
    if instruction.is_empty() && notes.is_empty() {
        return Err(AppError::bad_request(MSG_NOTHING_TO_DO));
    }
    // Attached pictures belong to the prep session the worksheet was made in.
    let image_ids = if req.image_ids.is_empty() {
        Vec::new()
    } else {
        let session: Option<i64> =
            sqlx::query_scalar("SELECT prep_session_id FROM worksheets WHERE id = $1")
                .bind(ws.id)
                .fetch_one(db)
                .await?;
        let Some(session_id) = session else {
            return Err(AppError::bad_request(
                "這份學習單不是在備課對話裡做的，沒有地方可以附上圖片。",
            ));
        };
        crate::prep::images::check_ids(db, ws.teacher_id, session_id, &req.image_ids).await?
    };
    rv::expire_stale(db, ws.id, stale_after(state)).await?;
    // Checked before the block's open proposal is set aside (the insert checks again).
    if rv::any_running(db, ws.id).await? {
        return Err(AppError::Conflict(rv::MSG_ANOTHER_RUNNING.to_string()));
    }
    let previous_reply = rv::set_aside_previous(db, ws.id, &block.id, ws.teacher_id).await?;
    let round = rv::rounds(db, ws.id, &block.id).await? + 1;
    let comment_ids: Vec<i64> = notes.iter().map(|c| c.id).collect();
    let proposal = rv::insert_running(
        db,
        NewProposal {
            worksheet_id: ws.id,
            teacher_id: ws.teacher_id,
            block: &block,
            base_version_no: latest.version_no,
            instruction: &instruction,
            comment_ids: &comment_ids,
            whole_block: req.whole_block,
            round: round as i32,
        },
    )
    .await?;
    if let Err(e) = crate::prep::images::attach_to_proposal(db, &image_ids, proposal.id).await {
        let message = match &e {
            AppError::Conflict(m) | AppError::BadRequest(m) => m.clone(),
            _ => MSG_INTERNAL.to_string(),
        };
        rv::finish(db, proposal.id, Err(message), None).await?;
        return Err(e);
    }
    log_usage(
        db,
        ws.teacher_id,
        "worksheet_block_revise_started",
        json!({
            "worksheet_id": ws.id, "block_id": block.id, "proposal_id": proposal.id,
            "round": round, "comments": comment_ids.len(), "instruction": !instruction.is_empty(),
            "whole_block": req.whole_block, "images": image_ids.len(),
        }),
    )
    .await?;
    let job = Job {
        proposal_id: proposal.id,
        round: round as i32,
        previous_reply: if round > 1 { previous_reply } else { None },
        scope: Scope::from(&instruction, &notes, req.whole_block),
        instruction,
        notes,
        content: latest.content,
        block,
        input,
    };
    spawn(state, job);
    Ok(proposal)
}

/// Everything the background call works with.
struct Job {
    proposal_id: i64,
    round: i32,
    previous_reply: Option<String>,
    scope: Scope,
    instruction: String,
    notes: Vec<Comment>,
    content: Content,
    block: Block,
    input: RunInput,
}

fn spawn(state: &AppState, job: Job) {
    let state = state.clone();
    let id = job.proposal_id;
    let pin = state.ai.active();
    tokio::spawn(async move {
        let work = {
            let state = state.clone();
            pin.scope(async move { run(&state, job).await })
        };
        let failed = crate::jobs::run("worksheet_revise", id, work)
            .await
            .err()
            .map(|_| MSG_INTERNAL);
        if let Some(message) = failed {
            if let Err(e) = rv::finish(&state.db, id, Err(message.to_string()), None).await {
                tracing::error!(proposal_id = id, error = %e, "could not close the block revise");
            }
        }
    });
}

fn or_none(s: &str) -> String {
    if s.trim().is_empty() {
        "（沒有）".to_string()
    } else {
        s.trim().to_string()
    }
}

/// Where a note is, in the words the model reads.
fn place_of(c: &Comment, codes: &HashMap<String, String>) -> String {
    match c.field {
        Field::Block => "整區".to_string(),
        Field::Title => "標題".to_string(),
        Field::Instructions => "作答指示".to_string(),
        Field::Item => c
            .anchor
            .item_id
            .as_deref()
            .and_then(|id| codes.get(id))
            .cloned()
            .unwrap_or_else(|| "整區（原本標的那一題已經不在了）".to_string()),
    }
}

fn options_line(options: &[String]) -> String {
    options
        .iter()
        .map(|o| clip_chars(o.trim(), 150))
        .collect::<Vec<_>>()
        .join(" ｜ ")
}

/// The block as the model reads it: title, instructions, its fixed Bloom level or need for the
/// teacher's material, and every question with its code and printed number.
fn block_detail(
    block: &Block,
    printed: &HashMap<String, usize>,
    bloom: Option<Bloom>,
    needs_external: bool,
) -> String {
    let mut lines = vec![
        format!("標題：{}", or_none(&block.title)),
        format!("作答指示：{}", or_none(&block.instructions)),
    ];
    if let Some(b) = bloom {
        lines.push(format!(
            "這一區的 Bloom 層次固定是 {}：每一題（包括新增的）都用這個層次。",
            b.as_str()
        ));
    }
    if needs_external {
        lines.push(
            "這一區需要教師提供素材（影片、錄音、圖片或另一篇文章）：不可以寫出或編造素材、網址或影片名稱，題目只能留空位。".to_string(),
        );
    }
    if block.items.is_empty() {
        lines.push("（這一區還沒有題目）".to_string());
    }
    for (k, item) in block.items.iter().enumerate() {
        let no = printed
            .get(&item.id)
            .map(|n| format!("（學生版第 {n} 題）"))
            .unwrap_or_default();
        lines.push(format!(
            "{}{no}｜{}｜bloom: {}",
            code(k),
            item.answer_format.as_deref().unwrap_or("未指定"),
            item.bloom.map_or("未標", |b| b.as_str())
        ));
        lines.push(format!("  題目：{}", item.prompt.trim()));
        if !item.options.is_empty() {
            lines.push(format!("  選項：{}", options_line(&item.options)));
        }
        if !item.answer.trim().is_empty() {
            lines.push(format!("  參考答案：{}", item.answer.trim()));
        }
    }
    lines.join("\n")
}

/// The other blocks, read-only: their letter, title and questions.
fn other_blocks(content: &Content, own: &str, labels: &HashMap<String, String>) -> String {
    let lines: Vec<String> = content
        .blocks
        .iter()
        .filter(|b| b.enabled && b.id != own)
        .map(|b| {
            let name = labels
                .get(&b.id)
                .filter(|l| !l.is_empty())
                .map(|l| format!("{l} 區"))
                .unwrap_or_else(|| block_kind_name(b.kind).to_string());
            let prompts: Vec<String> = b
                .items
                .iter()
                .map(|i| clip_chars(i.prompt.trim(), 120))
                .filter(|p| !p.is_empty())
                .collect();
            format!(
                "- {name}「{}」：{}",
                clip_chars(b.title.trim(), 60),
                if prompts.is_empty() {
                    "（沒有題目）".to_string()
                } else {
                    prompts.join("；")
                }
            )
        })
        .collect();
    if lines.is_empty() {
        "（沒有其他區塊）".to_string()
    } else {
        clip_chars(&lines.join("\n"), 4000)
    }
}

fn block_kind_name(kind: BlockKind) -> &'static str {
    match kind {
        BlockKind::Warmup => "暖身",
        BlockKind::Activity => "活動區塊",
        BlockKind::ExitTicket => "Exit ticket",
    }
}

fn objectives_summary(content: &Content) -> String {
    let lines: Vec<String> = content
        .objectives
        .iter()
        .filter(|o| !o.text.trim().is_empty() || !o.student_facing_en.trim().is_empty())
        .map(|o| {
            let both = [o.text.trim(), o.student_facing_en.trim()]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("｜");
            format!("- {}：{both}", o.kind.label())
        })
        .collect();
    if lines.is_empty() {
        "（沒有）".to_string()
    } else {
        lines.join("\n")
    }
}

async fn run(state: &AppState, job: Job) -> AppResult<()> {
    let db = &state.db;
    let Job {
        proposal_id,
        round,
        previous_reply,
        scope,
        instruction,
        notes,
        content,
        block,
        input,
    } = job;
    let ws = &input.worksheet;
    let material = &ws.material;

    // The worksheet's first version as the student sheet prints it: letters and numbers.
    let output = content
        .chosen
        .as_ref()
        .map_or(ws.settings.output, |c| c.settings.output);
    let tiered = output == Output::Tiered;
    let levels: Vec<Level> = if !content.levels.is_empty() {
        content.levels.clone()
    } else if tiered {
        Level::ALL.to_vec()
    } else {
        vec![Level::Standard]
    };
    let first = levels.first().copied().unwrap_or(Level::Standard);
    let sheet = project(&ws.title, &content, first, Audience::Student);
    let labels: HashMap<String, String> = sheet
        .blocks
        .iter()
        .map(|b| (b.id.clone(), b.label.clone()))
        .collect();
    let printed: HashMap<String, usize> = sheet
        .blocks
        .iter()
        .flat_map(|b| b.items.iter().map(|i| (i.id.clone(), i.no)))
        .collect();
    let codes: HashMap<String, String> = block
        .items
        .iter()
        .enumerate()
        .map(|(k, i)| (i.id.clone(), code(k)))
        .collect();

    // The template section it came from: its style, and whether it needs the teacher's material.
    let template = super::template::for_block(db, ws, &block).await?;
    let section = template
        .as_ref()
        .map(|(tpl, index)| &tpl.content.sections[*index]);
    let needs_external = section.is_some_and(|s| s.needs_external_material)
        || block.message.as_deref() == Some(MSG_EXTERNAL);
    let style_note = match &template {
        Some((tpl, index)) => {
            let s = &tpl.content.sections[*index];
            let mut parts = vec![format!("指示語言：{}", tpl.content.instruction_language)];
            if !s.instruction_style.trim().is_empty() {
                parts.push(format!("指示語寫法：{}", s.instruction_style.trim()));
            }
            if !s.item_patterns.is_empty() {
                parts.push(format!("題型樣式：{}", s.item_patterns.join("；")));
            }
            if !tpl.content.style_notes.is_empty() {
                parts.push(format!(
                    "整份的設計習慣：{}",
                    tpl.content.style_notes.join("；")
                ));
            }
            format!(
                "\n\n這一區照教師的版型做（{}）。修改後仍要維持這個風格。",
                parts.join("；")
            )
        }
        None => String::new(),
    };
    let block_bloom = if output.by_bloom() {
        super::bloom::level_of_block(&input, &block)
    } else {
        None
    };

    let material_notes = match material.source.material_id {
        Some(material_id) => {
            crate::materials::annotations::shared_with_ai(db, ws.teacher_id, material_id).await?
        }
        None => Vec::new(),
    };
    let notes_for_ai = crate::materials::annotations::ai_context_block(&material_notes);
    let block_notes = if notes.is_empty() {
        "（沒有）".to_string()
    } else {
        notes
            .iter()
            .map(|c| comments::ai_line(c, &place_of(c, &codes)))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let round_note = if round > 1 {
        format!(
            "\n\n這是教師第 {round} 次請你改這一區。上一次的改法教師沒有採用{}；請換一種改法。",
            previous_reply
                .as_deref()
                .map(|r| format!("（上一次你說：「{}」）", clip_chars(r, 200)))
                .unwrap_or_default()
        )
    } else {
        String::new()
    };
    let block_name = match labels.get(&block.id).filter(|l| !l.is_empty()) {
        Some(l) => format!("{l} 區「{}」", clip_chars(block.title.trim(), 60)),
        None => format!(
            "{}「{}」",
            block_kind_name(block.kind),
            clip_chars(block.title.trim(), 60)
        ),
    };
    let max = max_items(block.items.len());
    let pictures_on = state.images.enabled();
    let states = crate::worksheet::images::current(db, ws.id).await?;
    let slot_ids = actions::slot_ids(&content, Some(&block.id));
    let attached = crate::prep::images::load(state, None, Some(proposal_id)).await?;
    let mut vars: HashMap<&'static str, String> = HashMap::from([
        ("picture_rules", actions::rules(state).await?),
        (
            "pictures_note",
            format!(
                "這一區目前的圖片（redraw、remove 的 slot_id 只能用這裡列出的；沒有就是沒有圖）：\n{}",
                actions::describe_text(&block, &states)
            ),
        ),
        ("attachments_note", attached.note()),
        (
            "instruction",
            if instruction.is_empty() {
                "（沒有；請依下面的註記修改）".to_string()
            } else {
                instruction.clone()
            },
        ),
        ("block_notes", block_notes),
        ("scope", scope.describe(&block)),
        ("round_note", round_note),
        ("block_name", block_name),
        (
            "block_detail",
            block_detail(&block, &printed, block_bloom, needs_external),
        ),
        ("style_note", style_note),
        ("other_blocks", other_blocks(&content, &block.id, &labels)),
        ("objectives_summary", objectives_summary(&content)),
        ("class_profile", or_blank(&input.class_profile)),
        ("teacher_request", or_none(&ws.request)),
        ("teacher_notes_for_ai", notes_for_ai.clone()),
        (
            "material_title",
            if material.title.trim().is_empty() {
                "未命名教材".to_string()
            } else {
                material.title.trim().to_string()
            },
        ),
        ("paragraph_count", material.paragraphs.len().to_string()),
        ("passage_numbered", marked_passage(material)),
        ("answer_format_labels", ANSWER_FORMATS.join("、")),
    ]);
    // 內容目標 and 語言目標 (SIOP G1), and both in one line for `{{teacher_objective}}`.
    vars.extend(ws.settings.teacher_objectives().vars());
    let mut required = vec![
        ("instruction", "教師的修改指示"),
        ("block_notes", "教師在這一區的註記"),
        ("scope", "可以修改的範圍"),
        ("block_detail", "這一區目前的內容"),
        ("class_profile", "班級特性"),
    ];
    if !notes_for_ai.is_empty() {
        required.push(("teacher_notes_for_ai", "參考資料"));
    }
    let targets: Vec<String> = block
        .items
        .iter()
        .enumerate()
        .filter(|(_, i)| !scope.open && scope.items.contains(&i.id))
        .map(|(k, _)| code(k))
        .collect();
    let fake_context = json!({
        "task": KEY_BLOCK_REVISE,
        "items": block.items.iter().enumerate().map(|(k, i)| json!({
            "code": code(k), "answer_format": i.answer_format, "prompt": i.prompt,
            "options": i.options, "answer": i.answer,
            "bloom": i.bloom.map(|b| b.as_str()),
        })).collect::<Vec<Value>>(),
        "targets": targets,
        "pictures": actions::describe(&block, &states),
        "block_id": block.id,
        "open": scope.open,
        "round": round,
        "title": block.title,
        "instructions": block.instructions,
    });
    let c = Call {
        key: KEY_BLOCK_REVISE,
        scaffold: SCAFFOLD,
        schema_name: SCHEMA_NAME,
        schema: rv::schema(
            block.items.len(),
            max,
            &block.id,
            &slot_ids,
            if pictures_on { actions::MAX_ACTIONS } else { 0 },
        ),
        vars,
        required,
        redact: vec![
            ("passage_numbered", "學習單教材"),
            ("block_detail", "由教材衍生的內容"),
            ("other_blocks", "由教材衍生的內容"),
        ],
        fake_context,
        max_tokens: max_tokens(max),
        temperature: BASE_TEMPERATURE + 0.1 * (round - 1).clamp(0, 6) as f32,
        trace: json!({
            "worksheet_id": ws.id, "block_id": block.id, "proposal_id": proposal_id,
            "round": round, "comment_ids": notes.iter().map(|n| n.id).collect::<Vec<_>>(),
            "scope_open": scope.open, "material_source": material.source,
            // Ids, sizes and hashes of the attached pictures; never their bytes.
            "attached_images": attached.record,
        }),
        progress: None,
        images: attached.images.clone(),
    };

    let passage: String = material
        .paragraphs
        .iter()
        .map(|p| p.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let section_ctx = SectionCtx {
        passage: &passage,
        item_count: max,
        needs_external,
    };
    let taken: HashSet<String> = content
        .blocks
        .iter()
        .flat_map(|b| b.items.iter().map(|i| i.id.clone()))
        .collect();
    let prefix = {
        let p = format!("{}-p{proposal_id}", block.id);
        // Room for `n<k>` (and a suffix when taken) within the id limit.
        if p.chars().count() + 6 <= MAX_ID_CHARS {
            p
        } else {
            format!("p{proposal_id}")
        }
    };
    let mctx = MergeCtx {
        old: &block,
        scope: &scope,
        section: &section_ctx,
        levels: &levels,
        block_bloom,
        taken: &taken,
        new_prefix: &prefix,
        printed: &printed,
        max_items: max,
    };
    let name_of = |_: &str| None::<String>;
    let result = call(
        state,
        ws.teacher_id,
        c,
        crate::objectives::required,
        |raw: &RawBlockRevision| {
            let (merged, problems) = merge(raw, &mctx);
            // The picture actions: only this block's pictures, and nothing while pictures are
            // off. A dropped action is no reason for a second call.
            let (pictures, _notes) = actions::check(
                &raw.picture_actions,
                &actions::CheckCtx {
                    content: &content,
                    only_block: Some(&block.id),
                    name_of: &name_of,
                },
                pictures_on,
            );
            let dropped = if pictures_on {
                raw.picture_actions.len().saturating_sub(pictures.len())
            } else {
                0
            };
            let off = !pictures_on && !raw.picture_actions.is_empty();
            Checked {
                output: Some((merged, pictures, dropped, off)),
                problems,
            }
        },
        nothing,
    )
    .await?;
    let (outcome, interaction_id) = match result {
        Ok(((merged, pictures, dropped, off), id)) => {
            let Merged {
                block: revised,
                reply,
                changed,
                failed,
            } = merged;
            let mut notes = String::new();
            if dropped > 0 {
                notes.push_str(&format!(
                    "（有 {dropped} 個圖片動作沒有通過檢查，沒有列進建議。）"
                ));
            }
            if actions::off_notice(usize::from(off), pictures_on).is_some() {
                notes.push_str("（這個環境沒有開啟 AI 產圖，圖片的部分沒有處理。）");
            }
            if changed || !pictures.is_empty() {
                let reply = if reply.is_empty() {
                    if changed {
                        "已依你的註記修改這一區。".to_string()
                    } else {
                        "建議調整這一區的圖片。".to_string()
                    }
                } else {
                    reply
                };
                (Ok((revised, format!("{reply}{notes}"), pictures)), Some(id))
            } else if !failed.is_empty() {
                (
                    Err(format!(
                        "Manna 的修改（{}）沒有通過學習單的檢查，這一區維持原樣。可以再請 Manna 改一次。{notes}",
                        failed.join("、")
                    )),
                    Some(id),
                )
            } else {
                (
                    Err(if reply.trim().is_empty() {
                        format!("Manna 這次沒有改動這一區。可以把註記寫得更具體，再請 Manna 改一次。{notes}")
                    } else {
                        format!("Manna 沒有改動這一區：{}{notes}", reply.trim())
                    }),
                    Some(id),
                )
            }
        }
        Err(message) => (Err(message), None),
    };
    rv::finish(db, proposal_id, outcome, interaction_id).await?;
    Ok(())
}
