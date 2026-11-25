//! The prep chat (docs/PREP_CHAT.md): a turn of kind `chat` answers questions and, when the
//! teacher asks for a change, edits the session's worksheet with tools.
//!
//! One turn: the messages are built in a fixed order (system prompt, the fixed prefix with the
//! material, the compact worksheet, the class and the teacher's habits, the recent turns, then
//! this turn's request) so OpenAI's automatic prompt cache covers everything but the end; the
//! model is called with `tools`; every tool call is run on a working copy of the worksheet
//! ([`tools::Work`]) and answered with `role: "tool"`; later calls only append. When the model
//! stops calling tools its last message is the answer. The whole turn then writes ONE new
//! worksheet version (or none), every changed block first going to the block history. The
//! changes shown to the teacher come from the difference between the worksheet as it was and
//! as it was left, not from what the model said.
//!
//! The answer is the model's last message under a strict JSON schema (`paragraphs`,
//! `follow_ups`; the same `response_format` on every call of the turn, so the request prefix
//! stays the same). It is read like an `ask` answer; a message that is not that JSON is used as
//! plain text, so a turn never fails for the shape of its last message.

pub mod tools;
pub mod view;

use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use self::tools::Work;
use self::view::{Change, NotApplied};
use super::run::{self, Outcome, TurnInput};
use super::store;
use super::KEY_CHAT;
use super::{ask_schema, clean_text, RawAnswer, ANSWER_CHARS, FOLLOW_UPS_MAX, FOLLOW_UP_CHARS};
use crate::ai::chat::{ChatMessage, ChatRequest, ToolCall, ToolChoice};
use crate::ai::pipeline::{self, Finish, InteractionMeta};
use crate::ai::ProviderError;
use crate::application::generation::{MSG_DISABLED, MSG_ERROR, MSG_TIMEOUT, MSG_UNREADABLE};
use crate::audit::{log_usage, scaffold_id};
use crate::config::AiProviderKind;
use crate::error::{AppError, AppResult};
use crate::progress::{self, Step, Tracker};
use crate::prompts;
use crate::text::clip_chars;
use crate::worksheet::ai as ws_ai;
use crate::worksheet::images;
use crate::worksheet::store::{self as ws_store, Worksheet};
use crate::worksheet::{Block, Content, ANSWER_FORMATS};
use crate::AppState;

pub const SCAFFOLD: &str = "SC-PREP-CHAT";
pub const SCHEMA_NAME: &str = "prep_chat";
/// Most tool calls and model calls of one turn.
pub const MAX_TOOL_CALLS: u32 = 8;
pub const MAX_MODEL_CALLS: u32 = 10;
/// Output budget of one call (the reasoning gets room on top of it).
const MAX_TOKENS: u32 = 6000;
/// Turns of the conversation the model reads.
const RECENT_TURNS: usize = 6;

pub const MSG_NEEDS_OPENAI: &str = "這個功能需要 OpenAI 模型。請 PI 在後台把文字模型換成 OpenAI。";
const MSG_CHANGED_MEANWHILE: &str =
    "這份學習單在 Manna 處理的時候被改過了，所以這一輪的修改沒有套用。請再說一次。";
const MSG_LIMIT: &str =
    "（這一輪的工具次數已經用完，所以先用目前完成的部分收尾；還沒做完的請再說一次。）";
const MSG_NO_UNDO: &str = "這一輪沒有可以復原的修改。";

/// `result` of a `chat` turn (docs/PREP_CHAT.md section 4).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatResult {
    pub answer: String,
    pub follow_ups: Vec<String>,
    pub changes: Vec<Change>,
    pub not_applied: Vec<NotApplied>,
    pub version_no: Option<i32>,
    pub undoable: bool,
    pub tool_calls: u32,
}

/// What it takes to put a turn's changes back (`prep_turns.chat_undo`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct UndoRecord {
    worksheet_id: i64,
    version_no: i32,
    order_before: Vec<String>,
    order_after: Vec<String>,
    moved: bool,
    blocks: Vec<UndoBlock>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct UndoBlock {
    id: String,
    /// `None`: the turn added the block.
    before: Option<Block>,
    after: Block,
    history_id: Option<i64>,
}

// ---------------------------------------------------------------- one turn

/// Everything the calls of a turn log the same way.
struct Calls<'a> {
    state: &'a AppState,
    meta: InteractionMeta,
    ai: crate::ai::ActiveAi,
    base: Value,
    tool_names: Vec<&'static str>,
    paced: bool,
}

struct Answered {
    /// `None` when the model asked for tools.
    response: crate::ai::chat::ChatResponse,
    interaction_id: i64,
}

fn describe(m: &ChatMessage) -> Value {
    let (chars, extra) = match m {
        ChatMessage::Assistant { tool_calls, .. } if !tool_calls.is_empty() => (
            m.chars(),
            Some(json!(tool_calls
                .iter()
                .map(|c| json!({ "id": c.id, "name": c.name,
                    "arguments_chars": c.arguments.chars().count() }))
                .collect::<Vec<_>>())),
        ),
        _ => (m.chars(), None),
    };
    let text = match m {
        ChatMessage::System(t) => t.clone(),
        ChatMessage::User { text, .. } => text.clone(),
        ChatMessage::Assistant {
            content,
            tool_calls,
        } => format!(
            "{}{}",
            content.clone().unwrap_or_default(),
            tool_calls
                .iter()
                .map(|c| c.arguments.clone())
                .collect::<String>()
        ),
        ChatMessage::Tool { content, .. } => content.clone(),
    };
    let mut v = json!({
        "role": m.role(), "chars": chars,
        "sha256": &crate::auth::sha256_hex(&text)[..16],
    });
    if let Some(calls) = extra {
        v["tool_calls"] = calls;
    }
    v
}

impl Calls<'_> {
    /// One model call: its `ai_interactions` row (the messages as lengths and hashes only: the
    /// material and everything made from it stay deletable), the call, the row finished with the
    /// tokens the provider reported. `Err` is what the teacher is told.
    async fn call(
        &self,
        no: u32,
        req: &ChatRequest,
        logged: &[Value],
    ) -> AppResult<Result<Answered, String>> {
        let db = &self.state.db;
        let mut snapshot = self.base.clone();
        snapshot["request"] = json!({
            "max_tokens": req.max_tokens, "model_call": no,
            "tool_choice": match req.tool_choice { ToolChoice::Auto => "auto", ToolChoice::None => "none" },
            "parallel_tool_calls": false,
        });
        snapshot["tools"] = json!(self.tool_names);
        snapshot["messages"] = json!(logged);
        let id = pipeline::insert_interaction(
            db,
            &self.meta,
            self.ai.provider_name(),
            &self.ai.model,
            &snapshot,
            i32::try_from(no).unwrap_or(i32::MAX),
            None,
        )
        .await?;
        if self.paced {
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        }
        let start = Instant::now();
        let response = self.ai.complete_chat(req).await;
        let latency_ms = i32::try_from(start.elapsed().as_millis()).unwrap_or(i32::MAX);
        match response {
            Ok(r) => {
                let output = if r.tool_calls.is_empty() {
                    r.content
                        .as_deref()
                        .and_then(|c| serde_json::from_str::<Value>(c).ok())
                        .unwrap_or_else(|| json!({ "text_chars": r.content.as_deref().map_or(0, |c| c.chars().count()) }))
                } else {
                    json!({
                        "tool_calls": r.tool_calls.iter().map(|c| json!({
                            "id": c.id, "name": c.name,
                            "arguments_sha256": &crate::auth::sha256_hex(&c.arguments)[..16],
                        })).collect::<Vec<_>>()
                    })
                };
                pipeline::finish_interaction(
                    db,
                    id,
                    Finish {
                        status: "shown",
                        output: Some(output),
                        citations: Vec::new(),
                        check_results: None,
                        model_version: Some(r.model_version.clone()),
                        error_detail: None,
                        latency_ms,
                    },
                )
                .await?;
                if let Some(u) = r.usage {
                    sqlx::query("UPDATE ai_interactions SET usage = $2 WHERE id = $1")
                        .bind(id)
                        .bind(u.to_json())
                        .execute(db)
                        .await?;
                }
                Ok(Ok(Answered {
                    response: r,
                    interaction_id: id,
                }))
            }
            Err(e) => {
                let attempt = crate::ai::execution::Attempt {
                    id,
                    latency_ms,
                    response: Err(e.clone()),
                };
                let kind = crate::ai::execution::finish_failure(db, &attempt, &e).await?;
                use crate::ai::execution::Failure;
                Ok(Err(match (kind, &e) {
                    (Failure::Timeout, _) => MSG_TIMEOUT.to_string(),
                    (_, ProviderError::Disabled) => MSG_DISABLED.to_string(),
                    (Failure::Retryable, _) => MSG_UNREADABLE.to_string(),
                    _ => MSG_ERROR.to_string(),
                }))
            }
        }
    }
}

fn fail<T>(message: &str) -> AppResult<Result<T, String>> {
    Ok(Err(message.to_string()))
}

/// `Ok(Err(message))`: the turn failed and nothing was applied.
pub async fn run(
    state: &AppState,
    input: &TurnInput,
    progress: &Arc<Tracker>,
) -> AppResult<Outcome> {
    let db = &state.db;
    let session = &input.session;
    let turn = &input.turn;
    let ai = state.ai.active();
    if !ai.enabled() {
        return Ok(Err(MSG_DISABLED.to_string()));
    }
    if !matches!(ai.kind, AiProviderKind::OpenAi | AiProviderKind::Fake) {
        return Ok(Err(MSG_NEEDS_OPENAI.to_string()));
    }
    let request = turn.request.trim().to_string();

    // The worksheet the turn works on (none: the chat only answers).
    let ws: Option<Worksheet> = match input.worksheet_id {
        Some(id) => match ws_store::owned(db, id, session.teacher_id).await {
            Ok(ws) => Some(ws),
            Err(AppError::NotFound(_)) => return Ok(Err(run::MSG_WORKSHEET_GONE.to_string())),
            Err(e) => return Err(e),
        },
        None => None,
    };
    sqlx::query("UPDATE prep_turns SET chat_worksheet_id = $2 WHERE id = $1")
        .bind(turn.id)
        .bind(ws.as_ref().map(|w| w.id))
        .execute(db)
        .await?;
    let generating = match &ws {
        Some(w) => ws_ai::jobs(state, w.id).await?.1,
        None => false,
    };
    let can_edit = ws.is_some() && !generating;
    let (latest, ws_images, lexicon) = match &ws {
        Some(w) => {
            let v = ws_store::latest(db, w.id).await?;
            let imgs = images::states(state, w.id).await?;
            let lex = crate::vocabulary::load_lexicon(db, None)
                .await?
                .filter(|l| !l.is_empty());
            (Some(v), Some(imgs), lex)
        }
        None => (None, None, None),
    };
    progress.done(progress::READ).await;

    // ---- the fixed prefix
    let material = match &ws {
        Some(w) => Some(w.material.clone()),
        None => run::material(db, session).await?,
    };
    let (profile, class_snapshot) = run::class_profile(state, session).await?;
    let passage_text = material.as_ref().map(run::passage);
    let mut work = match (&ws, &latest, &ws_images) {
        (Some(w), Some(v), Some(imgs)) => Some(Work::new(
            w,
            turn.id,
            v.content.clone(),
            material.as_ref().map(|m| m.text()).unwrap_or_default(),
            lexicon,
            imgs.slots.clone(),
            imgs.enabled,
            state.storage.is_some(),
            imgs.regenerations_left,
        )),
        _ => None,
    };
    let compact = match (&work, &ws_images) {
        (Some(w), Some(imgs)) => view::compact(&w.content, &w.labels, w.printed(), &imgs.slots),
        _ => "（這個備課對話還沒有學習單，所以這一輪只能回答問題，不能修改。）".to_string(),
    };
    let compact = if generating {
        format!("{compact}\n\n（Manna 正在產生這份學習單，這一輪不能修改，只能回答問題。）")
    } else {
        compact
    };
    let picture_rules = match &ws_images {
        Some(i) if i.enabled && state.storage.is_some() => format!(
            "圖片：可以用 redraw_picture 重畫一張圖（這份學習單還可以重畫 {} 次）。",
            i.regenerations_left
        ),
        _ => {
            "圖片：這個環境不能重畫圖，不要呼叫 redraw_picture；老師要求時請說明目前沒有開啟產圖。"
                .to_string()
        }
    };
    let prompt = prompts::latest_required(db, KEY_CHAT).await?;
    let vars: std::collections::HashMap<&'static str, String> = std::collections::HashMap::from([
        ("class_profile", crate::text::or_blank(&profile)),
        (
            "material_title",
            material
                .as_ref()
                .map_or("（沒有教材）".to_string(), run::material_title),
        ),
        (
            "paragraph_count",
            material
                .as_ref()
                .map_or(0, |m| m.paragraphs.len())
                .to_string(),
        ),
        (
            "passage_numbered",
            passage_text
                .clone()
                .unwrap_or_else(|| "（教師沒有提供教材）".to_string()),
        ),
        (
            "worksheet_title",
            ws.as_ref()
                .map_or("（還沒有學習單）".to_string(), |w| w.title.clone()),
        ),
        ("worksheet_compact", compact.clone()),
        ("picture_rules", picture_rules),
        ("answer_format_labels", ANSWER_FORMATS.join("、")),
    ]);
    let required = [
        ("passage_numbered", "教材"),
        ("worksheet_compact", "學習單目前的內容"),
        ("picture_rules", "圖片"),
    ];
    let mut system = prompts::render(&prompt.system_text, &vars);
    let mut prefix = prompts::render_user_with(&prompt, &vars, &required);
    let mut logged_vars = vars.clone();
    for (name, label) in [
        ("passage_numbered", "備課對話教材"),
        ("worksheet_compact", "由教材衍生的學習單內容"),
    ] {
        if let Some(v) = logged_vars.get(name).cloned() {
            logged_vars.insert(name, prompts::redacted(label, &v));
        }
    }
    let mut logged_system = prompts::render(&prompt.system_text, &logged_vars);
    let mut logged_prefix = prompts::render_user_with(&prompt, &logged_vars, &required);
    // The teacher's habits, as every step that writes for the teacher gets them.
    let mut memory_snapshot = Value::Null;
    if let Some(ctx) = crate::memory::habit_context(
        state,
        session.teacher_id,
        KEY_CHAT,
        &format!("{prefix}\n{request}"),
    )
    .await?
    {
        if let Some(a) = &ctx.applied {
            system.push_str(&a.system);
            prefix.push_str(&a.user);
            logged_system.push_str(&a.system);
            logged_prefix.push_str(&a.user);
        }
        memory_snapshot = crate::memory::snapshot_for(state, session.teacher_id, &ctx).await;
    }

    // ---- the recent turns and this turn's request
    let (_, recent_text) = run::recent_turns_n(db, session.id, turn.seq, RECENT_TURNS).await?;
    let recent_message = format!("先前的對話（最近 {RECENT_TURNS} 輪以內）：\n{recent_text}");
    let attached = super::images::load(state, Some(turn.id), None).await?;
    let request_text = if request.is_empty() {
        "（老師沒有寫字）".to_string()
    } else {
        format!("{request}{}", attached.note())
    };
    let mut messages = vec![
        ChatMessage::System(system),
        ChatMessage::User {
            text: prefix,
            images: Vec::new(),
        },
        ChatMessage::User {
            text: recent_message.clone(),
            images: Vec::new(),
        },
        ChatMessage::User {
            text: request_text.clone(),
            images: attached.images.clone(),
        },
    ];
    let mut logged: Vec<Value> = messages.iter().map(describe).collect();

    let specs = if can_edit { tools::specs() } else { Vec::new() };
    let tool_names: Vec<&'static str> = specs.iter().map(|t| t.name).collect();
    let fake_blocks = work
        .as_ref()
        .map_or(json!([]), |w| view::fake_blocks(&w.content, &w.labels));
    let calls = Calls {
        state,
        meta: InteractionMeta {
            teacher_id: session.teacher_id,
            module: KEY_CHAT,
            unit_id: None,
            lesson_id: None,
            scaffold_id: scaffold_id(db, SCAFFOLD).await?,
            prompt_version_id: Some(prompt.id),
        },
        ai: ai.clone(),
        base: json!({
            "inputs": {
                "request": request,
                "recent_turns": prompts::redacted("先前的對話", &recent_text),
                "passage_numbered": prompts::redacted("備課對話教材", &passage_text.clone().unwrap_or_default()),
                "worksheet_compact": prompts::redacted("由教材衍生的學習單內容", &compact),
            },
            "prep": {
                "session_id": session.id, "turn_id": turn.id, "kind": turn.kind,
                "material_id": session.material_id, "worksheet_id": ws.as_ref().map(|w| w.id),
                "class_context": class_snapshot, "attached_images": attached.record,
                "can_edit": can_edit,
            },
            "prompt": { "key": KEY_CHAT, "version_no": prompt.version_no,
                        "system": logged_system, "user": logged_prefix },
            "schema": SCHEMA_NAME,
            "teacher_memory": memory_snapshot,
        }),
        tool_names,
        paced: ai.kind == AiProviderKind::Fake && request.contains("FAKE_CHAT_PACED"),
    };

    // ---- the loop
    let mut tool_calls: u32 = 0;
    let mut model_calls: u32 = 0;
    let mut limit_hit = false;
    let mut final_answer: Option<(String, i64)> = None;
    let mut last_interaction: Option<i64> = None;
    while model_calls < MAX_MODEL_CALLS {
        model_calls += 1;
        let req = ChatRequest {
            messages: messages.clone(),
            tools: specs.clone(),
            tool_choice: if limit_hit {
                ToolChoice::None
            } else {
                ToolChoice::Auto
            },
            answer_schema_name: SCHEMA_NAME,
            answer_schema: ask_schema(),
            max_tokens: MAX_TOKENS,
            fake_context: json!({
                "task": KEY_CHAT, "request": request, "blocks": fake_blocks,
                "can_edit": can_edit,
            }),
        };
        let answered = match calls.call(model_calls, &req, &logged).await? {
            Ok(a) => a,
            Err(message) => return Ok(Err(message)),
        };
        last_interaction = Some(answered.interaction_id);
        let response = answered.response;
        if response.tool_calls.is_empty() || limit_hit {
            if let Some(text) = response.content.clone() {
                final_answer = Some((text, answered.interaction_id));
            }
            if !response.tool_calls.is_empty() {
                // Asked for tools although it was told to answer.
                limit_hit = true;
            }
            break;
        }
        let assistant = ChatMessage::Assistant {
            content: response.content.clone(),
            tool_calls: response.tool_calls.clone(),
        };
        logged.push(describe(&assistant));
        messages.push(assistant);
        for call in &response.tool_calls {
            let allowed = tool_calls < MAX_TOOL_CALLS;
            let message = match work.as_mut().filter(|_| allowed) {
                Some(w) => {
                    tool_calls += 1;
                    run_tool(
                        state,
                        progress,
                        w,
                        call,
                        tool_calls,
                        session.teacher_id,
                        turn.id,
                    )
                    .await
                }
                None => {
                    limit_hit = true;
                    log_tool(
                        state,
                        session.teacher_id,
                        turn.id,
                        call,
                        false,
                        "limit",
                        None,
                    )
                    .await;
                    "這一輪的工具次數已經用完，這個呼叫沒有執行。請直接用文字回答，說明做了什麼、還有什麼沒做。"
                        .to_string()
                }
            };
            let tool_message = ChatMessage::Tool {
                call_id: call.id.clone(),
                content: message,
            };
            logged.push(describe(&tool_message));
            messages.push(tool_message);
        }
    }
    if final_answer.is_none() {
        limit_hit = true;
    }
    progress
        .add(Step::new("answer", "整理回答").with_status(progress::StepStatus::Pending))
        .await;
    progress.start("answer").await;

    // ---- the answer
    let (answer, follow_ups) = match &final_answer {
        Some((text, _)) => parse_answer(text),
        None => (String::new(), Vec::new()),
    };

    // ---- apply
    let mut not_applied: Vec<NotApplied> = Vec::new();
    let mut changes: Vec<Change> = Vec::new();
    let mut version_no: Option<i32> = None;
    if let (Some(w), Some(ws)) = (work.as_mut(), &ws) {
        let text_changed = w.content != w.original;
        if text_changed || !w.redraws.is_empty() {
            match commit(
                state,
                ws,
                w,
                latest.as_ref().map_or(0, |v| v.version_no),
                turn.id,
                last_interaction,
                text_changed,
            )
            .await?
            {
                Ok(no) => version_no = no,
                Err(message) => return Ok(Err(message)),
            }
        }
        changes = view::changes(w);
        for ((_, _), (label, reason)) in &w.failed {
            not_applied.push(NotApplied {
                label: label.clone(),
                reason: reason.clone(),
            });
        }
        // The pictures: queued once the version is written.
        let mut queued = 0;
        for r in std::mem::take(&mut w.redraws) {
            let name = w.name(&r.block_id);
            match images::regenerate(state, ws, &r.slot_id, Some(&r.note)).await {
                Ok(_) => {
                    queued += 1;
                    let k = w
                        .content
                        .block(&r.block_id)
                        .and_then(|b| b.image_slots.iter().position(|s| s.id == r.slot_id))
                        .map_or(1, |p| p + 1);
                    changes.push(Change {
                        target: "picture".into(),
                        block_id: Some(r.block_id.clone()),
                        label: format!("{name}的圖"),
                        summary: format!("重畫第 {k} 張圖（畫好會自動出現）"),
                    });
                }
                Err(e) => not_applied.push(NotApplied {
                    label: format!("{name}的圖"),
                    reason: match e {
                        AppError::Internal(_) => "圖片這一步發生錯誤，沒有處理。".to_string(),
                        other => run::teacher_message(&other),
                    },
                }),
            }
        }
        if let (Some(no), true) = (version_no, queued > 0) {
            ws_store::snapshot_version_pictures(db, ws.id, no).await?;
        }
    }

    let mut answer = answer;
    if answer.trim().is_empty() {
        answer = if !changes.is_empty() {
            "已依你的要求處理，改了哪些地方列在下面。".to_string()
        } else if !not_applied.is_empty() {
            "這次沒有改成，原因列在下面。".to_string()
        } else {
            "這一輪處理完了，沒有修改學習單。".to_string()
        };
    }
    if limit_hit {
        answer = format!("{answer}\n\n{MSG_LIMIT}");
    }
    let result = ChatResult {
        answer,
        follow_ups,
        undoable: version_no.is_some(),
        changes,
        not_applied,
        version_no,
        tool_calls,
    };
    if let Err(e) = log_usage(
        db,
        session.teacher_id,
        "prep_chat_turn",
        json!({
            "turn_id": turn.id, "tool_calls": tool_calls, "model_calls": model_calls,
            "changed": !result.changes.is_empty(), "changes": result.changes.len(),
            "not_applied": result.not_applied.len(), "undone": false,
        }),
    )
    .await
    {
        tracing::warn!(error = %e, "could not log the prep chat turn");
    }
    progress.done("answer").await;
    Ok(Ok((Some(json!(result)), last_interaction)))
}

async fn log_tool(
    state: &AppState,
    teacher_id: i64,
    turn_id: i64,
    call: &ToolCall,
    ok: bool,
    code: &str,
    block_id: Option<&str>,
) {
    let r = log_usage(
        &state.db,
        teacher_id,
        "prep_chat_tool",
        json!({ "turn_id": turn_id, "tool": call.name, "block_id": block_id,
                "ok": ok, "reason_code": code }),
    )
    .await;
    if let Err(e) = r {
        tracing::warn!(error = %e, "could not log a prep chat tool call");
    }
}

/// Runs a tool call on the working copy, with its progress step and its log line.
async fn run_tool(
    state: &AppState,
    progress: &Arc<Tracker>,
    work: &mut Work<'_>,
    call: &ToolCall,
    n: u32,
    teacher_id: i64,
    turn_id: i64,
) -> String {
    let key = format!("tool{n}");
    let args: Value = serde_json::from_str(&call.arguments).unwrap_or(Value::Null);
    let label = work.target_of(&call.name, &args);
    progress.add(Step::new(&key, &label)).await;
    progress.start(&key).await;
    let result = work.exec(&call.name, &call.arguments);
    log_tool(
        state,
        teacher_id,
        turn_id,
        call,
        result.ok,
        result.code,
        result.block_id.as_deref(),
    )
    .await;
    if result.ok {
        progress.done(&key).await;
    } else {
        progress.fail(&key, "沒有通過檢查").await;
    }
    result.text
}

/// The model's last message: the JSON answer, or else its text as it is.
fn parse_answer(text: &str) -> (String, Vec<String>) {
    let raw: Option<RawAnswer> = serde_json::from_str(text.trim()).ok();
    let (paragraphs, follow_ups): (Vec<String>, Vec<String>) = match raw {
        Some(r) => (r.paragraphs, r.follow_ups),
        None => (text.split("\n\n").map(str::to_string).collect(), Vec::new()),
    };
    let joined = paragraphs
        .iter()
        .map(|p| clean_text(p, 800))
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    let answer = clip_chars(&joined, ANSWER_CHARS);
    let follow_ups = follow_ups
        .iter()
        .map(|f| clean_text(f, FOLLOW_UP_CHARS))
        .filter(|f| !f.is_empty())
        .take(FOLLOW_UPS_MAX)
        .collect();
    (answer, follow_ups)
}

// ---------------------------------------------------------------- one new version

/// Writes the turn's changes as ONE version of the worksheet (nothing when the text did not
/// change), after checking that the worksheet is still as the turn found it. Each changed block
/// goes to the block history with the block as it was left (so a later undo can tell it was
/// edited since). `Ok(Err(message))`: nothing was applied.
async fn commit(
    state: &AppState,
    ws: &Worksheet,
    work: &Work<'_>,
    base_version_no: i32,
    turn_id: i64,
    interaction_id: Option<i64>,
    text_changed: bool,
) -> AppResult<Result<Option<i32>, String>> {
    let db = &state.db;
    if ws_ai::jobs(state, ws.id).await?.1 {
        return fail(run::MSG_WORKSHEET_BUSY);
    }
    let mut tx = db.begin().await?;
    sqlx::query("SELECT id FROM worksheets WHERE id = $1 FOR UPDATE")
        .bind(ws.id)
        .execute(&mut *tx)
        .await?;
    let alive: Option<i64> =
        sqlx::query_scalar("SELECT id FROM worksheets WHERE id = $1 AND deleted_at IS NULL")
            .bind(ws.id)
            .fetch_optional(&mut *tx)
            .await?;
    let current: Option<i32> = sqlx::query_scalar(
        "SELECT MAX(version_no) FROM worksheet_versions WHERE worksheet_id = $1",
    )
    .bind(ws.id)
    .fetch_one(&mut *tx)
    .await?;
    if alive.is_none() {
        return fail(run::MSG_WORKSHEET_GONE);
    }
    if current != Some(base_version_no) {
        return fail(MSG_CHANGED_MEANWHILE);
    }
    if !text_changed {
        tx.commit().await?;
        return Ok(Ok(None));
    }
    let changed = tools::changed_block_ids(&work.original, &work.content);
    let mut block_ids: Vec<String> = work
        .content
        .blocks
        .iter()
        .filter(|b| changed.contains(&b.id))
        .map(|b| b.id.clone())
        .collect();
    block_ids.sort();
    let saved = ws_store::insert_version_from(
        &mut tx,
        ws.id,
        &work.content,
        "revise",
        Some(json!({
            "cause": "prep_turn", "turn_id": turn_id, "interaction_id": interaction_id,
            "block_ids": block_ids, "tool_chat": true,
        })),
    )
    .await?;
    let mut undo_blocks = Vec::new();
    for b in work
        .content
        .blocks
        .iter()
        .filter(|b| changed.contains(&b.id))
    {
        let before = work.original.block(&b.id).cloned();
        let history_id = match &before {
            Some(prev) => Some(
                sqlx::query_scalar::<_, i64>(
                    "INSERT INTO worksheet_block_history
                       (worksheet_id, block_id, block, interaction_id, version_no, after_block)
                     VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
                )
                .bind(ws.id)
                .bind(&b.id)
                .bind(sqlx::types::Json(prev))
                .bind(interaction_id)
                .bind(saved.version_no)
                .bind(sqlx::types::Json(b))
                .fetch_one(&mut *tx)
                .await?,
            ),
            None => None,
        };
        undo_blocks.push(UndoBlock {
            id: b.id.clone(),
            before,
            after: b.clone(),
            history_id,
        });
    }
    let ids = |c: &Content| c.blocks.iter().map(|b| b.id.clone()).collect::<Vec<_>>();
    let common = |a: &Content, b: &Content| {
        ids(a)
            .into_iter()
            .filter(|id| b.block(id).is_some())
            .collect::<Vec<_>>()
    };
    let undo = UndoRecord {
        worksheet_id: ws.id,
        version_no: saved.version_no,
        order_before: ids(&work.original),
        order_after: ids(&work.content),
        moved: common(&work.original, &work.content) != common(&work.content, &work.original),
        blocks: undo_blocks,
    };
    sqlx::query("UPDATE prep_turns SET chat_undo = $2 WHERE id = $1")
        .bind(turn_id)
        .bind(json!(undo))
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Ok(Some(saved.version_no)))
}

// ---------------------------------------------------------------- undo

/// Why the turn cannot be taken back as a whole now, if it cannot: a block it changed was
/// changed again (or is gone), or the order it set was set again.
fn undo_conflict(
    content: &Content,
    undo: &UndoRecord,
    names: &dyn Fn(&str) -> String,
) -> Option<String> {
    for b in &undo.blocks {
        match content.block(&b.id) {
            Some(now) if *now == b.after => {}
            Some(_) => {
                return Some(format!(
                    "{}在這一輪之後又改過了，所以不能整輪復原。可以到那一區單獨復原。",
                    names(&b.id)
                ))
            }
            None => {
                return Some(format!(
                    "{}已經不在學習單上了，所以不能整輪復原。",
                    names(&b.id)
                ))
            }
        }
    }
    if undo.moved {
        let current: Vec<&str> = content
            .blocks
            .iter()
            .map(|b| b.id.as_str())
            .filter(|id| undo.order_after.iter().any(|x| x == id))
            .collect();
        let left: Vec<&str> = undo
            .order_after
            .iter()
            .map(String::as_str)
            .filter(|id| content.block(id).is_some())
            .collect();
        if current != left {
            return Some("區塊的順序在這一輪之後又調整過，所以不能整輪復原。".to_string());
        }
    }
    None
}

/// Whether the turn's changes can still be taken back as a whole (`result.undoable`).
pub fn still_undoable(content: &Content, record: &Value) -> bool {
    match serde_json::from_value::<UndoRecord>(record.clone()) {
        Ok(u) => undo_conflict(content, &u, &|id| id.to_string()).is_none(),
        Err(_) => false,
    }
}

/// `POST /api/prep-sessions/:id/turns/:turn_id/undo`: puts every block the turn changed back as
/// it was, as one new version (`cause = "undo"`). 409 (with a reason) when a block was changed
/// since or nothing is left to undo. Pictures that were drawn stay (they can be switched back
/// in the picture's history).
pub async fn undo(
    state: &AppState,
    teacher_id: i64,
    session_id: i64,
    turn_id: i64,
) -> AppResult<()> {
    let db = &state.db;
    store::owned(db, session_id, teacher_id).await?;
    let turn = store::turns(db, session_id)
        .await?
        .into_iter()
        .find(|t| t.id == turn_id)
        .ok_or_else(|| AppError::not_found("找不到這一輪。"))?;
    if turn.kind != super::KIND_CHAT {
        return Err(AppError::bad_request(
            "這一輪不是對話，沒有可以復原的修改。",
        ));
    }
    let record: Option<UndoRecord> = turn
        .chat_undo
        .clone()
        .and_then(|v| serde_json::from_value(v).ok());
    let undoable = turn
        .result
        .as_ref()
        .and_then(|r| r["undoable"].as_bool())
        .unwrap_or(false);
    let Some(undo) = record.filter(|_| undoable && turn.status == super::STATUS_DONE) else {
        return Err(AppError::Conflict(MSG_NO_UNDO.to_string()));
    };
    let ws = match ws_store::owned(db, undo.worksheet_id, teacher_id).await {
        Ok(w) => w,
        Err(AppError::NotFound(_)) => {
            return Err(AppError::Conflict(
                "這份學習單已經刪除了，沒辦法復原。".into(),
            ))
        }
        Err(e) => return Err(e),
    };
    if ws_ai::jobs(state, ws.id).await?.1 {
        return Err(AppError::Conflict(
            "Manna 正在產生這份學習單，好了之後再復原。".to_string(),
        ));
    }
    let mut tx = db.begin().await?;
    sqlx::query("SELECT id FROM worksheets WHERE id = $1 FOR UPDATE")
        .bind(ws.id)
        .execute(&mut *tx)
        .await?;
    let latest = ws_store::latest(&mut *tx, ws.id).await?;
    let mut content = latest.content.clone();
    let labels = run::labels(&ws, &content);
    let names = |id: &str| -> String {
        match content.block(id) {
            Some(b) => run::block_name(labels.get(id).map_or("", String::as_str), &b.title),
            None => "有一區".to_string(),
        }
    };
    if let Some(why) = undo_conflict(&content, &undo, &names) {
        return Err(AppError::Conflict(why));
    }
    for u in &undo.blocks {
        match &u.before {
            Some(before) => {
                if let Some(slot) = content.blocks.iter_mut().find(|b| b.id == u.id) {
                    *slot = before.clone();
                }
            }
            None => content.blocks.retain(|b| b.id != u.id),
        }
    }
    if undo.moved {
        let rank = |id: &str| {
            undo.order_before
                .iter()
                .position(|x| x == id)
                .unwrap_or(usize::MAX)
        };
        content.blocks.sort_by_key(|b| rank(&b.id));
    }
    content.normalise().map_err(AppError::bad_request)?;
    let saved = ws_store::insert_version_from(
        &mut tx,
        ws.id,
        &content,
        "undo",
        Some(json!({
            "cause": "undo", "turn_id": turn_id, "from_version_no": latest.version_no,
            "undone_version_no": undo.version_no,
            "block_ids": undo.blocks.iter().map(|b| b.id.clone()).collect::<Vec<_>>(),
        })),
    )
    .await?;
    for u in &undo.blocks {
        if let Some(h) = u.history_id {
            ws_store::mark_undone(&mut *tx, h, saved.version_no).await?;
        }
    }
    sqlx::query(
        "UPDATE prep_turns
            SET result = jsonb_set(jsonb_set(result, '{undoable}', 'false'::jsonb),
                                   '{undone}', 'true'::jsonb)
          WHERE id = $1",
    )
    .bind(turn_id)
    .execute(&mut *tx)
    .await?;
    log_usage(
        &mut *tx,
        teacher_id,
        "prep_chat_undo",
        json!({ "turn_id": turn_id, "worksheet_id": ws.id, "version_no": saved.version_no,
                "undone_version_no": undo.version_no, "blocks": undo.blocks.len() }),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}
