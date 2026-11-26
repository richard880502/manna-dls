//! The AI stages of steps 3 (one objective again, the English of an objective), 4, 5, 6 and 7 of
//! the lesson SOP. Steps 2 and 3's first calls live in [`super::ai`].
//!
//! Every stage is a background job like steps 2 and 3 ([`super::ai::start`]); what is shared
//! here is the checked structured call (`application::generation::call_in`: one repair call, the
//! first call's usable output kept when the repair fails) and what each step adds to it:
//!
//! | step | prompt key | checks |
//! |---|---|---|
//! | 4 逐段整合目標 | `sop_paragraph_goals` (new, not validated against a real model) | [`evaluate_goals`] |
//! | 5 逐段問題或任務 | `sop_paragraph_tasks`, `sop_scaffolds` (the validated worksheet steps (c) and (e)) | `worksheet::checks::evaluate_tasks` and `evaluate_scaffolds` |
//! | 6 暖身 | `sop_warmup` (worksheet step (d)) | `worksheet::checks::evaluate_warmup`; candidates and levels come from code |
//! | 7 Exit ticket | `sop_exit_ticket` (worksheet step (f)) | `worksheet::checks::evaluate_exit` |
//! | 3 幫我修英文 | `sop_objective_english` (new) | [`evaluate_english`] |
//! | 5–7 幫我修英文 | `sop_text_english` (new) | [`evaluate_text_english`] |
//!
//! 換一個 (one suggestion again, `Scope::Item`) runs the same prompt and checks for that one
//! suggestion: the code adds what was set aside and what not to repeat (`only_block` in step 5,
//! `redo_note` in steps 6 and 7, appended where the prompt has no place for it), and the checks
//! that count a whole set (an exit ticket's 2–4 items and its kinds) are left out.
//!
//! The 作答方式 rules (選擇題 options, 配對題 minimum) and the teacher's confirmed option-count
//! habit are the worksheet's, because step 5 runs the worksheet's own checks. Procedural
//! scaffolds are always made by the program (`worksheet::assemble::auto_steps`).

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::ai::{result_json, set_phase, Done, JobInput, Swap};
use super::{ExitKind, Objective, ObjectiveType, Paragraph, Scaffolds, SopContent};
use crate::ai::pipeline::{AiContextItem, AiResult, ContextList};
use crate::application::generation::{
    self, call_in, Call, Checked, MSG_DISABLED, MSG_ERROR, MSG_TIMEOUT,
};
use crate::checks::contains_cjk;
use crate::error::AppResult;
use crate::teaching_context::track_label;
use crate::text::{clip_chars, or_blank};
use crate::worksheet::{
    self, info_function_from_label, student_tasks, BlockKind, ANSWER_FORMATS, EXIT_PURPOSES,
    INSTRUCTIONAL_KINDS, PROCEDURAL_KINDS, VERBAL_KINDS,
};
use crate::AppState;

pub const KEY_GOALS: &str = "sop_paragraph_goals";
pub const KEY_TASKS: &str = "sop_paragraph_tasks";
pub const KEY_SCAFFOLDS: &str = "sop_scaffolds";
pub const KEY_WARMUP: &str = "sop_warmup";
pub const KEY_EXIT: &str = "sop_exit_ticket";
pub const KEY_ENGLISH: &str = "sop_objective_english";
pub const KEY_TEXT_ENGLISH: &str = "sop_text_english";

pub const SC_GOALS: &str = "SC-SOP-GOALS";
pub const SC_TASKS: &str = "SC-SOP-TASKS";
pub const SC_WARMUP: &str = "SC-SOP-WARMUP";
pub const SC_EXIT: &str = "SC-SOP-EXIT";
pub const SC_ENGLISH: &str = "SC-SOP-ENGLISH";
pub const SC_TEXT_ENGLISH: &str = "SC-SOP-TEXT-ENGLISH";

pub const PHASE_GOALS: &str = "正在逐段想這一段可以帶到哪些目標";
pub const PHASE_GOALS_REPAIR: &str = "有幾段沒有通過檢查，正在重新確認";
pub const PHASE_TASKS: &str = "正在逐段設計問題或任務";
pub const PHASE_TASKS_REPAIR: &str = "有幾題沒有通過檢查，正在重新確認";
const PHASE_SCAFFOLDS: &str = "正在為每一段安排鷹架";
const PHASE_SCAFFOLDS_REPAIR: &str = "正在重新確認鷹架";
pub const PHASE_WARMUP: &str = "正在設計暖身，並從詞彙表挑關鍵字彙";
pub const PHASE_WARMUP_REPAIR: &str = "正在重新確認暖身與字彙";
pub const PHASE_EXIT: &str = "正在設計 Exit ticket";
pub const PHASE_EXIT_REPAIR: &str = "正在重新確認 Exit ticket";
pub const PHASE_SWAP: &str = "正在換一個目標";
pub const PHASE_REDO: &str = "正在重新建議這一段";
pub const PHASE_REDO_ANALYSIS: &str = "正在重新分析這一段";
pub const PHASE_REDO_OVERALL: &str = "正在換一個角度看全文";
pub const PHASE_REDO_BLOCK: &str = "正在重新設計這一個區塊";
pub const PHASE_REDO_WARMUP: &str = "正在換一個暖身活動";
pub const PHASE_REDO_VOCABULARY: &str = "正在換一組關鍵字彙";
pub const PHASE_REDO_EXIT: &str = "正在換一題";

/// The longest note of a paragraph goal (characters).
pub const GOAL_NOTE_MAX_CHARS: usize = 120;
/// Most objectives one paragraph can be tied to.
pub const GOAL_MAX_OBJECTIVES: usize = 3;
/// Most words of the student-facing English of an objective.
pub const ENGLISH_MAX_WORDS: usize = 25;

// ---------------------------------------------------------------- shared

/// `O1`, `O2`…: the codes the model sees for the objectives with text, and their ids.
pub fn objective_codes(content: &SopContent) -> Vec<(String, &Objective)> {
    content
        .objectives
        .iter()
        .filter(|o| !o.text.trim().is_empty() || !o.student_facing_en.trim().is_empty())
        .enumerate()
        .map(|(i, o)| (format!("O{}", i + 1), o))
        .collect()
}

fn objectives_summary(codes: &[(String, &Objective)]) -> String {
    if codes.is_empty() {
        return "（教師沒有提供目標；objective_refs 請給空陣列）".to_string();
    }
    codes
        .iter()
        .map(|(code, o)| {
            let mut both: Vec<&str> = [o.text.trim(), o.student_facing_en.trim()]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect();
            both.dedup();
            format!("{code}（{}）：{}", o.kind.label(), both.join("｜"))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn lines_or(lines: Vec<String>, empty: &str) -> String {
    if lines.is_empty() {
        empty.to_string()
    } else {
        lines.join("\n")
    }
}

fn nos_label(nos: &[i64]) -> String {
    nos.iter()
        .map(|n| format!("[P{n}]"))
        .collect::<Vec<_>>()
        .join("")
}

/// A result the page can show when a call produced nothing usable.
fn status_of(message: &str) -> &'static str {
    if message == MSG_TIMEOUT {
        "timeout"
    } else if message == MSG_ERROR
        || message == crate::ai::pipeline::MSG_QUOTA
        || message == crate::ai::pipeline::MSG_BUSY
    {
        "error"
    } else if message == MSG_DISABLED {
        "disabled"
    } else {
        "blocked"
    }
}

fn failed_result(message: &str, context: Vec<AiContextItem>) -> Value {
    json!({
        "interaction_id": null, "status": status_of(message), "message": message,
        "output": null, "sources": [], "context": context
    })
}

fn done<T: Serialize>(outcome: Result<(T, i64), String>, context: Vec<AiContextItem>) -> Done {
    match outcome {
        Ok((output, id)) => (
            result_json(AiResult::shown(id, output, Vec::new()).with_context(context)),
            Some(id),
        ),
        Err(message) => (failed_result(&message, context), None),
    }
}

/// What every stage's request records (ids only, never text).
fn trace(input: &JobInput, extra: Value) -> Value {
    json!({
        "sop": {
            "version_no": input.version.version_no,
            "material_id": input.material.id,
            "material_version_no": input.material.version_no,
            "paragraph_ids": input.material.paragraphs.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            "slot": input.scope.slot(),
        },
        "class_context": input.profile.snapshot(),
        "extra": extra,
    })
}

fn base_context(input: &JobInput) -> ContextList {
    let mut context = ContextList::default();
    let name = if input.material.title.trim().is_empty() {
        "未命名教材".to_string()
    } else {
        input.material.title.trim().to_string()
    };
    context
        .add(
            "教材",
            &format!(
                "{name}（{} 段，{} 字）",
                input.material.paragraphs.len(),
                input.material.word_count
            ),
        )
        .add(&input.profile.context_label(), &input.profile.text);
    context
}

fn texts(paragraphs: &[Paragraph]) -> Vec<String> {
    paragraphs.iter().map(|p| p.text.clone()).collect()
}

fn common_vars(
    input: &JobInput,
    codes: &[(String, &Objective)],
    notes: String,
) -> HashMap<&'static str, String> {
    HashMap::from([
        ("school_track", track_label(&input.track).to_string()),
        ("class_profile", or_blank(&input.profile.text)),
        ("objectives_summary", objectives_summary(codes)),
        (
            "passage_numbered",
            super::numbered_passage(&input.material.paragraphs),
        ),
        ("student_task_labels", student_tasks().join("、")),
        ("answer_format_labels", ANSWER_FORMATS.join("、")),
        ("teacher_notes_for_ai", notes),
    ])
}

async fn notes_for_ai(state: &AppState, input: &JobInput) -> AppResult<String> {
    let notes = crate::materials::annotations::shared_with_ai(
        &state.db,
        input.teacher_id,
        input.material.id,
    )
    .await?;
    Ok(crate::materials::annotations::ai_context_block(&notes))
}

#[allow(clippy::too_many_arguments)]
fn new_call(
    key: &'static str,
    scaffold: &'static str,
    schema_name: &'static str,
    schema: Value,
    vars: HashMap<&'static str, String>,
    fake_context: Value,
    max_tokens: u32,
    temperature: f32,
    trace: Value,
) -> Call {
    Call {
        key,
        scaffold,
        schema_name,
        schema,
        vars,
        required: vec![("class_profile", "班級特性")],
        redact: vec![
            ("passage_numbered", "課例教材"),
            ("vocab_candidates", "由教材衍生的內容"),
            ("blocks_detail", "由教材衍生的內容"),
            ("tasks_summary", "由教材衍生的內容"),
            ("teacher_tasks", "由教材衍生的內容"),
            // 換一個: what was set aside and what not to repeat.
            ("only_block", "由教材衍生的內容"),
            ("redo_note", "由教材衍生的內容"),
        ],
        fake_context,
        max_tokens,
        temperature,
        trace,
        progress: None,
        images: Vec::new(),
    }
}

/// One checked call of a job. `repair_phase` is what the page says while the repair call runs.
async fn run_call<R, T>(
    state: &AppState,
    input: &JobInput,
    job_id: i64,
    repair_phase: &'static str,
    call: Call,
    check: impl Fn(&R) -> Checked<T>,
) -> AppResult<Result<(T, i64), String>>
where
    R: serde::de::DeserializeOwned,
{
    let db = state.db.clone();
    call_in(
        state,
        input.teacher_id,
        (Some(input.lesson.unit_id), Some(input.lesson.id)),
        call,
        |_| Vec::new(),
        check,
        async move |_id: i64| -> AppResult<()> {
            set_phase(&db, job_id, repair_phase).await?;
            Ok(())
        },
    )
    .await
}

fn eval_to_checked<T, U>(
    eval: crate::worksheet::checks::Eval<T>,
    map: impl FnOnce(T) -> U,
) -> Checked<U> {
    Checked {
        output: eval.output.map(map),
        problems: eval.problems,
    }
}

fn code_names_of(code_ids: &[(String, String)]) -> Vec<String> {
    code_ids.iter().map(|(c, _)| c.clone()).collect()
}

// ---------------------------------------------------------------- step 3: one objective again

/// What a 換一個 job tells the model on top of the objectives prompt.
pub fn swap_note(swap: &Swap) -> String {
    format!(
        "\n\n【這次只要 1 項】教師不要下面這一項建議，請另外想「1 項{}」，換一個不同的角度或做法：\n被換掉的建議：{}\n教師已經有、或其他建議裡已經有的目標（不要重複）：\n{}\nobjectives 只輸出 1 筆，siop_type 一定是「{}」。",
        swap.kind.label(),
        clip_chars(&swap.declined, 300),
        if swap.avoid.is_empty() {
            "（沒有）".to_string()
        } else {
            swap.avoid
                .iter()
                .map(|a| format!("- {}", clip_chars(a, 200)))
                .collect::<Vec<_>>()
                .join("\n")
        },
        swap.kind.label()
    )
}

/// What a run for one kind tells the model on top of the objectives prompt: the 內容目標 are
/// decided first; the 語言目標 are then derived from the ones the teacher decided on.
pub fn kind_note(kind: ObjectiveType, decided_content: &[String]) -> String {
    match kind {
        ObjectiveType::Content => "\n\n【這次只要內容目標】請推薦 2–3 項內容目標，objectives 裡的 siop_type 一定是「內容目標」，不要寫語言目標。語言目標要等教師決定好內容目標之後，才會另外請你依內容目標推導。".to_string(),
        ObjectiveType::Language => format!(
            "\n\n【這次只要語言目標】教師已經決定的內容目標：\n{}\n請依這些內容目標，推薦 2–3 項語言目標，objectives 裡的 siop_type 一定是「語言目標」，不要重寫內容目標。語言目標是從內容目標推出來的：先想學生要達成上面的內容目標，必須用英語讀、聽、說、寫裡的哪些能力，做出什麼看得到、聽得到的成果（例如口頭說明、寫出句子、標出線索），再寫成語言目標；reason_zh 要說明它服務哪一項內容目標、讓學生做出什麼成果，老師看到這個成果就知道學生有沒有學會內容。",
            decided_content
                .iter()
                .map(|c| format!("- {}", clip_chars(c, 300)))
                .collect::<Vec<_>>()
                .join("\n")
        ),
    }
}

mod goals;
mod tasks;
use tasks::existing_tasks_text;
mod english;
mod exit;
mod text_english;
mod warmup;

pub use english::*;
pub use exit::*;
pub use goals::*;
pub use tasks::*;
pub use text_english::*;
pub use warmup::*;
