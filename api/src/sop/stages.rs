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
use crate::worksheet::checks::{
    evaluate_exit, evaluate_scaffolds, evaluate_tasks, evaluate_warmup, Candidate, CleanWarmup,
    ExitCtx, ScaffoldsCtx, TasksCtx, WarmupCtx,
};
use crate::worksheet::schemas::{
    self as ws_schemas, RawBlock, RawExit, RawExitItem, RawScaffoldSet, RawScaffolds, RawTasks,
    RawWarmup,
};
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

pub const SC_GOALS: &str = "SC-SOP-GOALS";
pub const SC_TASKS: &str = "SC-SOP-TASKS";
pub const SC_WARMUP: &str = "SC-SOP-WARMUP";
pub const SC_EXIT: &str = "SC-SOP-EXIT";
pub const SC_ENGLISH: &str = "SC-SOP-ENGLISH";

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

/// The longest note of a paragraph goal (characters).
pub const GOAL_NOTE_MAX_CHARS: usize = 120;
/// Most objectives one paragraph can be tied to.
pub const GOAL_MAX_OBJECTIVES: usize = 3;
/// Most words of the student-facing English of an objective.
pub const ENGLISH_MAX_WORDS: usize = 25;

const BASE_TEMPERATURE_NOTE: &str = "";

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
    } else if message == MSG_ERROR {
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

// ---------------------------------------------------------------- step 4: goals

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawGoal {
    pub paragraph_no: i64,
    pub objective_codes: Vec<String>,
    pub note_zh: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawGoals {
    pub paragraphs: Vec<RawGoal>,
}

pub const GOALS_SCHEMA_NAME: &str = "sop_paragraph_goals";

pub fn goals_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "paragraphs": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "paragraph_no": { "type": "integer" },
                        "objective_codes": { "type": "array", "items": { "type": "string" } },
                        "note_zh": { "type": "string" }
                    },
                    "required": ["paragraph_no", "objective_codes", "note_zh"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["paragraphs"],
        "additionalProperties": false
    })
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GoalSuggestion {
    pub paragraph_id: String,
    pub paragraph_no: i32,
    /// Ids of step 3 objectives (the model's codes were mapped back and checked).
    pub objective_ids: Vec<String>,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct GoalsSuggestion {
    pub paragraphs: Vec<GoalSuggestion>,
    /// Paragraphs asked for that the model did not deliver (even after the repair call).
    pub missing_paragraph_ids: Vec<String>,
}

#[derive(Debug, Default)]
pub struct GoalsEval {
    pub kept: Vec<GoalSuggestion>,
    pub missing: Vec<i32>,
    pub problems: Vec<String>,
}

/// Checks a goals output: every paragraph asked for addressed once and in order; only codes of
/// objectives that exist; a note in Chinese within the limit; a paragraph that serves nothing
/// says so in its note. What cannot be right is left out; `problems` is what the repair call is
/// told.
pub fn evaluate_goals(
    raw: &RawGoals,
    paragraphs: &[Paragraph],
    codes: &[(String, String)],
    targets: &[i32],
) -> GoalsEval {
    let by_no: HashMap<i64, &Paragraph> = paragraphs.iter().map(|p| (i64::from(p.no), p)).collect();
    let id_of: HashMap<&str, &str> = codes
        .iter()
        .map(|(c, i)| (c.as_str(), i.as_str()))
        .collect();
    let mut eval = GoalsEval::default();
    let mut seen: HashSet<i64> = HashSet::new();
    for g in &raw.paragraphs {
        let Some(p) = by_no.get(&g.paragraph_no) else {
            eval.problems.push(format!(
                "paragraph_no {} 不是教材的段落（1 到 {}）。",
                g.paragraph_no,
                paragraphs.len()
            ));
            continue;
        };
        if !targets.contains(&p.no) {
            eval.problems.push(format!(
                "這次只需要輸出 {}，不要輸出 [P{}]。",
                targets
                    .iter()
                    .map(|n| format!("[P{n}]"))
                    .collect::<Vec<_>>()
                    .join("、"),
                p.no
            ));
            continue;
        }
        if !seen.insert(g.paragraph_no) {
            eval.problems
                .push(format!("[P{}] 重複了，每一段只能有一筆。", p.no));
            continue;
        }
        let mut ids: Vec<String> = Vec::new();
        let mut invalid: Vec<&str> = Vec::new();
        for code in &g.objective_codes {
            match id_of.get(code.trim()) {
                Some(id) => {
                    if !ids.iter().any(|x| x == id) {
                        ids.push((*id).to_string());
                    }
                }
                None => invalid.push(code.trim()),
            }
        }
        if !invalid.is_empty() {
            eval.problems.push(format!(
                "[P{}] 用了不在清單裡的目標代號：{}。只能逐字複製「教師確認的目標」的代號；沒有合適的目標就給空陣列。",
                p.no,
                invalid.join("、")
            ));
        }
        if ids.len() > GOAL_MAX_OBJECTIVES {
            eval.problems.push(format!(
                "[P{}] 對應了 {} 項目標，每一段最多 {GOAL_MAX_OBJECTIVES} 項。",
                p.no,
                ids.len()
            ));
            ids.truncate(GOAL_MAX_OBJECTIVES);
        }
        let mut note = super::checks::clean_markup(&g.note_zh);
        if note.is_empty() || !contains_cjk(&note) {
            eval.problems.push(format!(
                "[P{}] 的 note_zh 要用繁體中文寫一兩句話{}。",
                p.no,
                if ids.is_empty() {
                    "，說明這一段為什麼不對應目標"
                } else {
                    "，說明這一段要怎麼帶"
                }
            ));
            continue;
        }
        if note.chars().count() > GOAL_NOTE_MAX_CHARS {
            eval.problems.push(format!(
                "[P{}] 的 note_zh 有 {} 字，請縮短到 {GOAL_NOTE_MAX_CHARS} 字以內。",
                p.no,
                note.chars().count()
            ));
            note = clip_chars(&note, GOAL_NOTE_MAX_CHARS);
        }
        eval.kept.push(GoalSuggestion {
            paragraph_id: p.id.clone(),
            paragraph_no: p.no,
            objective_ids: ids,
            note,
        });
    }
    eval.kept.sort_by_key(|g| g.paragraph_no);
    eval.missing = targets
        .iter()
        .copied()
        .filter(|n| !eval.kept.iter().any(|g| g.paragraph_no == *n))
        .collect();
    if !eval.missing.is_empty() {
        eval.problems.push(format!(
            "沒有輸出這些段落：{}。每一段都要有一筆。",
            eval.missing
                .iter()
                .map(|n| format!("[P{n}]"))
                .collect::<Vec<_>>()
                .join("、")
        ));
    }
    eval
}

/// 「內容目標 1」「語言目標 2」: how the teacher sees the objectives numbered.
fn objective_labels(content: &SopContent) -> HashMap<String, String> {
    let mut counts = (0, 0);
    content
        .objectives
        .iter()
        .map(|o| {
            let n = match o.kind {
                ObjectiveType::Content => {
                    counts.0 += 1;
                    counts.0
                }
                ObjectiveType::Language => {
                    counts.1 += 1;
                    counts.1
                }
            };
            (o.id.clone(), format!("{} {n}", o.kind.label()))
        })
        .collect()
}

fn teacher_goals_text(content: &SopContent, paragraphs: &[Paragraph]) -> String {
    let labels = objective_labels(content);
    lines_or(
        paragraphs
            .iter()
            .filter_map(|p| {
                let g = content
                    .paragraph_goals
                    .iter()
                    .find(|g| g.paragraph_id == p.id)?;
                if g.objective_ids.is_empty() && g.note.trim().is_empty() {
                    return None;
                }
                let objectives: Vec<&str> = g
                    .objective_ids
                    .iter()
                    .filter_map(|id| labels.get(id).map(String::as_str))
                    .collect();
                Some(format!(
                    "[P{}] 對應：{}；筆記：{}",
                    p.no,
                    if objectives.is_empty() {
                        "（沒有選）".to_string()
                    } else {
                        objectives.join("、")
                    },
                    if g.note.trim().is_empty() {
                        "（沒有寫）"
                    } else {
                        g.note.trim()
                    }
                ))
            })
            .collect(),
        "（教師還沒有寫）",
    )
}

pub async fn run_goals(
    state: &AppState,
    job_id: i64,
    input: &JobInput,
    temperature: f32,
) -> AppResult<Done> {
    let paragraphs = &input.material.paragraphs;
    let content = &input.version.content;
    let codes = objective_codes(content);
    let code_ids: Vec<(String, String)> = codes
        .iter()
        .map(|(c, o)| (c.clone(), o.id.clone()))
        .collect();
    let all: Vec<i32> = paragraphs.iter().map(|p| p.no).collect();
    let targets: Vec<i32> = match input.scope.paragraph_id() {
        Some(id) => paragraphs
            .iter()
            .filter(|p| p.id == id)
            .map(|p| p.no)
            .collect(),
        None => all,
    };
    let only = if input.scope.paragraph_id().is_some() {
        format!(
            "這次只需要輸出的段落：{}。paragraphs 只輸出這些段落，每段一筆。",
            targets
                .iter()
                .map(|n| format!("[P{n}]"))
                .collect::<Vec<_>>()
                .join("、")
        )
    } else {
        String::new()
    };
    let analysis = super::ai::analysis_summary(&input.version, paragraphs);
    let vars: HashMap<&'static str, String> = HashMap::from([
        ("school_track", track_label(&input.track).to_string()),
        ("class_profile", or_blank(&input.profile.text)),
        ("passage_numbered", super::numbered_passage(paragraphs)),
        (
            "analysis_summary",
            if analysis.is_empty() {
                "（教師還沒有做教材分析）".to_string()
            } else {
                analysis
            },
        ),
        (
            "objectives_list",
            codes
                .iter()
                .map(|(c, o)| format!("{c}｜{}｜{}", o.kind.label(), o.text.trim()))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        ("teacher_goals", teacher_goals_text(content, paragraphs)),
        ("only_paragraphs", only),
    ]);
    let mut context = base_context(input);
    context
        .add(
            "你的教材分析",
            &super::ai::analysis_summary(&input.version, paragraphs),
        )
        .add("第 3 步的目標", &format!("{} 項", codes.len()));
    let context = context.into_vec();
    let mut call = new_call(
        KEY_GOALS,
        SC_GOALS,
        GOALS_SCHEMA_NAME,
        goals_schema(),
        vars,
        json!({
            "task": KEY_GOALS,
            "paragraphs": paragraphs.iter()
                .map(|p| json!({ "no": p.no, "text": p.text })).collect::<Vec<_>>(),
            "codes": code_ids.iter().map(|(c, _)| c.as_str()).collect::<Vec<_>>(),
            "only": if input.scope.paragraph_id().is_some() { json!(targets) } else { Value::Null },
        }),
        (1200 + 160 * targets.len().min(200) as u32).clamp(3000, 8000),
        temperature,
        trace(input, json!({ "targets": targets })),
    );
    call.required.push(("objectives_list", "教師確認的目標"));
    let check_targets = targets.clone();
    let outcome = run_call(
        state,
        input,
        job_id,
        PHASE_GOALS_REPAIR,
        call,
        move |raw: &RawGoals| {
            let eval = evaluate_goals(raw, paragraphs, &code_ids, &check_targets);
            let output = (!eval.kept.is_empty()).then(|| GoalsSuggestion {
                missing_paragraph_ids: eval
                    .missing
                    .iter()
                    .filter_map(|n| paragraphs.iter().find(|p| p.no == *n))
                    .map(|p| p.id.clone())
                    .collect(),
                paragraphs: eval.kept,
            });
            Checked {
                output,
                problems: eval.problems,
            }
        },
    )
    .await?;
    let _ = PHASE_GOALS;
    Ok(done(outcome, context))
}

// ---------------------------------------------------------------- step 5: tasks

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct TaskSuggestion {
    pub function: Option<super::InfoFunction>,
    pub text: String,
    pub answer_format: String,
    pub options: Vec<String>,
    pub answer: String,
    /// A verbatim piece of the paragraphs; named `evidence` so that deleting the material scrubs it.
    pub evidence: String,
    pub scaffolds: Scaffolds,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct BlockSuggestion {
    pub paragraph_ids: Vec<String>,
    pub paragraph_nos: Vec<i32>,
    pub title: String,
    /// 學生任務 and 作答方式 Manna pre-selected for the block (the generator's own labels).
    pub student_tasks: Vec<String>,
    pub answer_formats: Vec<String>,
    /// Ids of the step 3 objectives the block checks.
    pub objective_ids: Vec<String>,
    pub reason: String,
    pub tasks: Vec<TaskSuggestion>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct TasksSuggestion {
    pub blocks: Vec<BlockSuggestion>,
    pub missing_paragraph_ids: Vec<String>,
    /// False when the scaffolds call did not deliver: every task then has only the procedural
    /// steps the program makes.
    pub scaffolds_ok: bool,
}

/// The procedural steps the program makes for a block (the worksheet's own rule, spec §3.3).
pub fn auto_procedural_steps(
    paragraph_ids: &[String],
    student_tasks: &[String],
    answer_formats: &[String],
) -> Vec<String> {
    let block = worksheet::Block {
        kind: BlockKind::Activity,
        paragraph_ids: paragraph_ids.to_vec(),
        student_tasks: student_tasks.to_vec(),
        items: answer_formats
            .iter()
            .map(|f| worksheet::Item {
                answer_format: Some(f.clone()),
                ..worksheet::Item::default()
            })
            .collect(),
        ..worksheet::Block::default()
    };
    worksheet::assemble::auto_steps(&block)
}

/// `1. step` lines: how a procedural scaffold is written in the SOP's plain text field.
pub fn steps_text(steps: &[String]) -> String {
    steps
        .iter()
        .enumerate()
        .map(|(i, s)| format!("{}. {}", i + 1, s.trim()))
        .collect::<Vec<_>>()
        .join("\n")
}

fn support_text(items: &[(String, String)]) -> String {
    items
        .iter()
        .filter(|(_, c)| !c.trim().is_empty())
        .map(|(k, c)| format!("{k}：{}", c.trim()))
        .collect::<Vec<_>>()
        .join("\n")
}

fn block_detail(b: &RawBlock) -> String {
    format!(
        "[{}] {}（{}；{}；學生任務：{}）\n{}",
        b.block_id,
        b.title_en,
        b.info_function,
        nos_label(&b.paragraph_nos),
        if b.student_tasks.is_empty() {
            "未標示".to_string()
        } else {
            b.student_tasks.join("、")
        },
        b.items
            .iter()
            .enumerate()
            .map(|(i, it)| format!(
                "  {}. ({}) {}\n     參考答案：{}",
                i + 1,
                it.answer_format,
                clip_chars(&it.prompt_en, 300),
                clip_chars(&it.answer_en, 300)
            ))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

fn to_block_suggestion(
    b: &RawBlock,
    paragraphs: &[Paragraph],
    code_ids: &[(String, String)],
    scaffolds: Option<&RawScaffoldSet>,
) -> BlockSuggestion {
    let paragraph_ids: Vec<String> = b
        .paragraph_nos
        .iter()
        .filter_map(|n| paragraphs.iter().find(|p| i64::from(p.no) == *n))
        .map(|p| p.id.clone())
        .collect();
    // The SOP is one design: only the core items (the worksheet makes the other tiers itself).
    let core: Vec<&crate::worksheet::schemas::RawItem> = {
        let c: Vec<_> = b.items.iter().filter(|i| i.tier == "core").collect();
        if c.is_empty() {
            b.items.iter().collect()
        } else {
            c
        }
    };
    let function = info_function_from_label(&b.info_function);
    let mut formats: Vec<String> = Vec::new();
    for it in &core {
        if !formats.contains(&it.answer_format) {
            formats.push(it.answer_format.clone());
        }
    }
    let procedural = scaffolds
        .and_then(|s| {
            let steps: Vec<String> = s
                .procedural
                .iter()
                .flat_map(|p| p.steps_en.iter().cloned())
                .filter(|s| !s.trim().is_empty())
                .collect();
            (steps.len() >= 2).then_some(steps)
        })
        .unwrap_or_else(|| auto_procedural_steps(&paragraph_ids, &b.student_tasks, &formats));
    let verbal = scaffolds
        .map(|s| {
            support_text(
                &s.verbal
                    .iter()
                    .map(|x| (x.kind.clone(), x.content_en.clone()))
                    .collect::<Vec<_>>(),
            )
        })
        .unwrap_or_default();
    let instructional = scaffolds
        .map(|s| {
            support_text(
                &s.instructional
                    .iter()
                    .map(|x| (x.kind.clone(), x.content_en.clone()))
                    .collect::<Vec<_>>(),
            )
        })
        .unwrap_or_default();
    let shared = Scaffolds {
        verbal,
        instructional,
        procedural: steps_text(&procedural),
    };
    BlockSuggestion {
        paragraph_nos: b.paragraph_nos.iter().map(|n| *n as i32).collect(),
        paragraph_ids,
        title: b.title_en.clone(),
        student_tasks: b.student_tasks.clone(),
        answer_formats: formats,
        objective_ids: b
            .objective_refs
            .iter()
            .filter_map(|r| code_ids.iter().find(|(c, _)| c == r))
            .map(|(_, id)| id.clone())
            .collect(),
        reason: super::checks::clean_markup(&b.design_reason_zh),
        // The scaffolds belong to the block; every task of it shows them.
        tasks: core
            .iter()
            .map(|it| TaskSuggestion {
                function,
                text: it.prompt_en.clone(),
                answer_format: it.answer_format.clone(),
                options: it.options.clone(),
                answer: it.answer_en.clone(),
                evidence: it.evidence.clone().unwrap_or_default(),
                scaffolds: shared.clone(),
            })
            .collect(),
    }
}

fn existing_tasks_text(content: &SopContent, paragraphs: &[Paragraph]) -> String {
    lines_or(
        content
            .task_blocks
            .iter()
            .flat_map(|b| {
                let nos: Vec<i64> = b
                    .paragraph_ids
                    .iter()
                    .filter_map(|id| paragraphs.iter().find(|p| &p.id == id))
                    .map(|p| i64::from(p.no))
                    .collect();
                b.tasks
                    .iter()
                    .filter(|t| !t.text.trim().is_empty())
                    .map(move |t| {
                        format!(
                            "- {}{}{}",
                            nos_label(&nos),
                            t.function
                                .map(|f| format!("（{}）", worksheet::info_function_label(f)))
                                .unwrap_or_default(),
                            clip_chars(t.text.trim(), 300)
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .collect(),
        "（沒有）",
    )
}

pub async fn run_tasks(
    state: &AppState,
    job_id: i64,
    input: &JobInput,
    temperature: f32,
) -> AppResult<Done> {
    let paragraphs = &input.material.paragraphs;
    let content = &input.version.content;
    let codes = objective_codes(content);
    let code_ids: Vec<(String, String)> = codes
        .iter()
        .map(|(c, o)| (c.clone(), o.id.clone()))
        .collect();
    let code_names: Vec<String> = code_ids.iter().map(|(c, _)| c.clone()).collect();
    let passages = texts(paragraphs);
    let budget = worksheet::item_budget(content.options.minutes, paragraphs.len(), true);
    let habits = crate::memory::option_habits(&state.db, input.teacher_id).await?;
    let notes = notes_for_ai(state, input).await?;
    let mut vars = common_vars(input, &codes, notes.clone());
    vars.extend([
        ("material_types", "（未指定）".to_string()),
        ("minutes", "未指定（依教材份量判斷）".to_string()),
        ("item_budget", budget.to_string()),
        (
            "selected_student_tasks",
            "（沒有勾選，請依教材判斷）".to_string(),
        ),
        (
            "selected_answer_formats",
            "（沒有勾選，請依教材判斷）".to_string(),
        ),
        ("teacher_tasks", existing_tasks_text(content, paragraphs)),
        ("only_block", String::new()),
    ]);
    let mut context = base_context(input);
    context
        .add(
            "第 3 步的目標",
            &if codes.is_empty() {
                String::new()
            } else {
                format!("{} 項", codes.len())
            },
        )
        .add("你已經寫的問題或任務", &{
            let n: usize = content.task_blocks.iter().map(|b| b.tasks.len()).sum();
            if n == 0 {
                String::new()
            } else {
                format!("{n} 項")
            }
        });
    let context = context.into_vec();
    let fake_context = json!({
        "task": "worksheet_tasks",
        "image_slots": 0,
        "paragraphs": paragraphs.iter()
            .map(|p| json!({ "no": p.no, "text": p.text })).collect::<Vec<_>>(),
        "item_budget": budget,
        "selected_student_tasks": [],
        "selected_answer_formats": [],
        "objective_ids": code_names,
        "only": null,
        "run_no": 1,
        "sop": true,
    });
    let call = new_call(
        KEY_TASKS,
        SC_TASKS,
        ws_schemas::TASKS_SCHEMA_NAME,
        ws_schemas::tasks_schema(),
        vars.clone(),
        fake_context,
        worksheet::ai::tasks_max_tokens(paragraphs.len()),
        temperature,
        trace(input, json!({ "step": "tasks" })),
    );
    let check_codes = code_names_of(&code_ids);
    let tasks = run_call(
        state,
        input,
        job_id,
        PHASE_TASKS_REPAIR,
        call,
        |raw: &RawTasks| {
            eval_to_checked(
                evaluate_tasks(
                    raw,
                    &TasksCtx {
                        paragraphs: &passages,
                        objective_ids: &check_codes,
                        item_budget: budget,
                        only: None,
                        habits,
                    },
                ),
                |blocks| blocks,
            )
        },
    )
    .await?;
    let _ = PHASE_TASKS;
    let (raw_blocks, tasks_id) = match tasks {
        Ok(ok) => ok,
        Err(message) => return Ok(done::<TasksSuggestion>(Err(message), context)),
    };

    // ---- the scaffolds of every block (not fatal: the program's procedural steps stay)
    set_phase(&state.db, job_id, PHASE_SCAFFOLDS).await?;
    let names: Vec<String> = raw_blocks.iter().map(|b| b.block_id.clone()).collect();
    let answers: Vec<String> = raw_blocks
        .iter()
        .flat_map(|b| b.items.iter().map(|i| i.answer_en.clone()))
        .collect();
    let mut svars = common_vars(input, &codes, notes);
    svars.extend([
        ("verbal_kinds", VERBAL_KINDS.join("、")),
        ("instructional_kinds", INSTRUCTIONAL_KINDS.join("、")),
        ("procedural_kinds", PROCEDURAL_KINDS.join("、")),
        (
            "selected_supports",
            "（沒有勾選，由你依任務判斷）".to_string(),
        ),
        (
            "blocks_detail",
            raw_blocks
                .iter()
                .map(block_detail)
                .collect::<Vec<_>>()
                .join("\n\n"),
        ),
    ]);
    let scaffold_call = new_call(
        KEY_SCAFFOLDS,
        SC_TASKS,
        ws_schemas::SCAFFOLDS_SCHEMA_NAME,
        ws_schemas::scaffolds_schema(),
        svars,
        json!({
            "task": "worksheet_scaffolds",
            "targets": raw_blocks.iter().map(|b| json!({
                "target": b.block_id,
                "answers": b.items.iter().map(|i| i.answer_en.clone()).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "selected_kinds": [],
            "sop": true,
        }),
        worksheet::ai::scaffolds_max_tokens(raw_blocks.len()),
        temperature,
        trace(
            input,
            json!({ "step": "scaffolds", "tasks_interaction": tasks_id }),
        ),
    );
    let scaffolds = run_call(
        state,
        input,
        job_id,
        PHASE_SCAFFOLDS_REPAIR,
        scaffold_call,
        |raw: &RawScaffolds| {
            eval_to_checked(
                evaluate_scaffolds(
                    raw,
                    &ScaffoldsCtx {
                        targets: &names,
                        answers: &answers,
                    },
                ),
                |sets| sets,
            )
        },
    )
    .await?;
    let (sets, scaffolds_ok) = match scaffolds {
        Ok((sets, _)) => (sets, true),
        Err(_) => (Vec::new(), false),
    };
    let blocks: Vec<BlockSuggestion> = raw_blocks
        .iter()
        .map(|b| {
            let set = sets
                .iter()
                .find(|s| s.target.trim().eq_ignore_ascii_case(b.block_id.trim()));
            to_block_suggestion(b, paragraphs, &code_ids, set)
        })
        .collect();
    let covered: HashSet<&str> = blocks
        .iter()
        .flat_map(|b| b.paragraph_ids.iter().map(String::as_str))
        .collect();
    let missing_paragraph_ids = paragraphs
        .iter()
        .filter(|p| !covered.contains(p.id.as_str()))
        .map(|p| p.id.clone())
        .collect();
    let out = TasksSuggestion {
        blocks,
        missing_paragraph_ids,
        scaffolds_ok,
    };
    Ok(done(Ok((out, tasks_id)), context))
}

fn code_names_of(code_ids: &[(String, String)]) -> Vec<String> {
    code_ids.iter().map(|(c, _)| c.clone()).collect()
}

// ---------------------------------------------------------------- step 6: warm-up

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct WarmupActivitySuggestion {
    /// Title, instructions and the questions, in English, as one text.
    pub text: String,
    pub scaffolds: Scaffolds,
    pub purpose: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct VocabularySuggestion {
    pub word: String,
    /// From the word list, never from the model; `None`: not in the list.
    pub level: Option<i32>,
    pub paragraph_id: Option<String>,
    /// A verbatim sentence of the material (named `evidence`: scrubbed when the material is deleted).
    pub evidence: String,
    pub definition_en: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct WarmupSuggestion {
    pub background: Vec<WarmupActivitySuggestion>,
    pub vocabulary: Vec<VocabularySuggestion>,
    /// How many candidate words the code offered; 0: nothing to pick from.
    pub candidates_available: usize,
    /// Whether the CEEC word list was available to give levels.
    pub levels_available: bool,
}

/// `item_index` of the vocabulary group in decisions (the activities are 0, 1, …).
pub const VOCABULARY_ITEM_INDEX: i32 = 100;

fn clean_to_suggestion(
    w: &CleanWarmup,
    paragraphs: &[Paragraph],
    candidates_available: usize,
    levels_available: bool,
) -> WarmupSuggestion {
    let background = w
        .activities
        .iter()
        .map(|a| {
            let mut lines: Vec<String> = Vec::new();
            if !a.title.is_empty() {
                lines.push(a.title.clone());
            }
            if !a.instructions.is_empty() {
                lines.push(a.instructions.clone());
            }
            lines.extend(a.prompts.iter().map(|p| format!("- {p}")));
            let steps = auto_steps_for_warmup(!w.words.is_empty());
            WarmupActivitySuggestion {
                text: lines.join("\n"),
                scaffolds: Scaffolds {
                    verbal: String::new(),
                    instructional: String::new(),
                    procedural: steps_text(&steps),
                },
                purpose: a.purpose_zh.clone(),
            }
        })
        .collect();
    let vocabulary = w
        .words
        .iter()
        .map(|v| VocabularySuggestion {
            word: v.word.clone(),
            level: v.level.map(i32::from),
            paragraph_id: v
                .paragraph_no
                .and_then(|n| paragraphs.iter().find(|p| i64::from(p.no) == n))
                .map(|p| p.id.clone()),
            evidence: v.context.clone(),
            definition_en: v.definition.clone(),
        })
        .collect();
    WarmupSuggestion {
        background,
        vocabulary,
        candidates_available,
        levels_available,
    }
}

fn auto_steps_for_warmup(has_words: bool) -> Vec<String> {
    let block = worksheet::Block {
        kind: BlockKind::Warmup,
        vocabulary: if has_words {
            vec![worksheet::Vocab::default()]
        } else {
            Vec::new()
        },
        ..worksheet::Block::default()
    };
    worksheet::assemble::auto_steps(&block)
}

pub async fn run_warmup(
    state: &AppState,
    job_id: i64,
    input: &JobInput,
    temperature: f32,
) -> AppResult<Done> {
    let paragraphs = &input.material.paragraphs;
    let content = &input.version.content;
    let codes = objective_codes(content);
    let passages = texts(paragraphs);
    let teacher_words: Vec<String> = content
        .warmup
        .vocabulary
        .iter()
        .map(|v| v.word.trim().to_string())
        .filter(|w| !w.is_empty())
        .collect();
    // Candidates and their levels come from the word list (code); the model only picks.
    let lexicon = crate::vocabulary::load_lexicon(&state.db, None)
        .await?
        .filter(|l| !l.is_empty());
    let (candidates, _hard) =
        worksheet::ai::key_word_candidates(&passages, lexicon.as_ref(), &teacher_words);
    let count = worksheet::vocab_count(content.options.minutes);
    let notes = notes_for_ai(state, input).await?;
    let mut vars = common_vars(input, &codes, notes);
    vars.extend([
        (
            "vocab_count",
            format!("{}–{count}", count.saturating_sub(2).max(2)),
        ),
        (
            "teacher_warmup",
            lines_or(
                content
                    .warmup
                    .background
                    .iter()
                    .filter(|w| !w.text.trim().is_empty())
                    .map(|w| format!("- {}", clip_chars(w.text.trim(), 300)))
                    .collect(),
                "（沒有）",
            ),
        ),
        (
            "teacher_vocabulary",
            if teacher_words.is_empty() {
                "（沒有）".to_string()
            } else {
                teacher_words.join("、")
            },
        ),
        ("tasks_summary", existing_tasks_text(content, paragraphs)),
        (
            "vocab_candidates",
            lines_or(
                candidates
                    .iter()
                    .map(|c| {
                        format!(
                            "{}｜{}｜P{}",
                            c.word,
                            c.level
                                .map(|l| format!("第 {l} 級"))
                                .unwrap_or_else(|| "未對應到詞彙表".to_string()),
                            c.paragraph_no
                        )
                    })
                    .collect(),
                "（沒有候選字彙；key_vocabulary 請給空陣列）",
            ),
        ),
    ]);
    let mut context = base_context(input);
    context.add(
        "候選關鍵字（由詞彙表比對）",
        &if candidates.is_empty() {
            String::new()
        } else {
            format!("{} 個", candidates.len())
        },
    );
    let context = context.into_vec();
    let fake_context = json!({
        "task": "worksheet_warmup",
        "title": input.material.title,
        "vocab_count": count,
        "candidates": candidates.iter()
            .map(|c| json!({ "word": c.word, "paragraph_no": c.paragraph_no, "sentence": c.sentence }))
            .collect::<Vec<_>>(),
        "sop": true,
    });
    let call = new_call(
        KEY_WARMUP,
        SC_WARMUP,
        ws_schemas::WARMUP_SCHEMA_NAME,
        ws_schemas::warmup_schema(),
        vars,
        fake_context,
        worksheet::ai::WARMUP_MAX_TOKENS,
        temperature,
        trace(input, json!({ "candidates": candidates.len() })),
    );
    let levels_available = lexicon.is_some();
    let n_candidates = candidates.len();
    let outcome = run_call(
        state,
        input,
        job_id,
        PHASE_WARMUP_REPAIR,
        call,
        |raw: &RawWarmup| {
            let eval = evaluate_warmup(
                raw,
                &WarmupCtx {
                    paragraphs: &passages,
                    candidates: &candidates as &[Candidate],
                    vocab_count: count,
                },
            );
            eval_to_checked(eval, |w| {
                clean_to_suggestion(&w, paragraphs, n_candidates, levels_available)
            })
        },
    )
    .await?;
    let _ = PHASE_WARMUP;
    Ok(done(outcome, context))
}

// ---------------------------------------------------------------- step 7: exit ticket

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ExitItemSuggestion {
    pub kind: ExitKind,
    pub text: String,
    /// Id of the step 3 objective a formative item checks (empty: none).
    pub objective_id: String,
    pub answer: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct ExitSuggestion {
    pub items: Vec<ExitItemSuggestion>,
    /// Objectives no item checks (the teacher can add one).
    pub uncovered_objective_ids: Vec<String>,
}

fn exit_kind(label: &str) -> ExitKind {
    EXIT_PURPOSES
        .iter()
        .find(|(_, l)| *l == label)
        .map(|(k, _)| *k)
        .unwrap_or(ExitKind::Formative)
}

fn to_exit_suggestion(items: &[RawExitItem], code_ids: &[(String, String)]) -> ExitSuggestion {
    let id_of = |code: &str| {
        code_ids
            .iter()
            .find(|(c, _)| c == code)
            .map(|(_, id)| id.clone())
    };
    let mapped: Vec<ExitItemSuggestion> = items
        .iter()
        .map(|i| ExitItemSuggestion {
            kind: exit_kind(&i.purpose),
            text: i.prompt_en.clone(),
            objective_id: if exit_kind(&i.purpose) == ExitKind::Formative {
                i.objective_refs
                    .iter()
                    .find_map(|r| id_of(r))
                    .unwrap_or_default()
            } else {
                String::new()
            },
            answer: i.sample_answer_en.clone().unwrap_or_default(),
        })
        .collect();
    let checked: HashSet<&str> = items
        .iter()
        .flat_map(|i| i.objective_refs.iter().map(String::as_str))
        .collect();
    ExitSuggestion {
        items: mapped,
        uncovered_objective_ids: code_ids
            .iter()
            .filter(|(c, _)| !checked.contains(c.as_str()))
            .map(|(_, id)| id.clone())
            .collect(),
    }
}

pub async fn run_exit(
    state: &AppState,
    job_id: i64,
    input: &JobInput,
    temperature: f32,
) -> AppResult<Done> {
    let paragraphs = &input.material.paragraphs;
    let content = &input.version.content;
    let codes = objective_codes(content);
    let code_ids: Vec<(String, String)> = codes
        .iter()
        .map(|(c, o)| (c.clone(), o.id.clone()))
        .collect();
    let code_names = code_names_of(&code_ids);
    let passage = texts(paragraphs).join("\n");
    let count = worksheet::exit_count(content.options.minutes);
    let notes = notes_for_ai(state, input).await?;
    let task_prompts: Vec<String> = content
        .task_blocks
        .iter()
        .flat_map(|b| b.tasks.iter().map(|t| t.text.clone()))
        .filter(|t| !t.trim().is_empty())
        .collect();
    let mut vars = common_vars(input, &codes, notes);
    vars.extend([
        ("exit_count", count.to_string()),
        (
            "exit_minutes",
            if count <= 2 { "3" } else { "5" }.to_string(),
        ),
        (
            "teacher_exit",
            lines_or(
                content
                    .exit_ticket
                    .items
                    .iter()
                    .filter(|e| !e.text.trim().is_empty())
                    .map(|e| format!("- {}", clip_chars(e.text.trim(), 300)))
                    .collect(),
                "（沒有）",
            ),
        ),
        ("tasks_summary", existing_tasks_text(content, paragraphs)),
    ]);
    let mut context = base_context(input);
    context.add(
        "第 3 步的目標",
        &if codes.is_empty() {
            String::new()
        } else {
            format!("{} 項", codes.len())
        },
    );
    let context = context.into_vec();
    let call = new_call(
        KEY_EXIT,
        SC_EXIT,
        ws_schemas::EXIT_SCHEMA_NAME,
        ws_schemas::exit_schema(),
        vars,
        json!({ "task": "worksheet_exit_ticket", "exit_count": count, "objective_ids": code_names, "sop": true }),
        worksheet::ai::EXIT_MAX_TOKENS,
        temperature,
        trace(input, json!({ "exit_count": count })),
    );
    let outcome = run_call(
        state,
        input,
        job_id,
        PHASE_EXIT_REPAIR,
        call,
        |raw: &RawExit| {
            let mut eval = evaluate_exit(
                raw,
                &ExitCtx {
                    objective_ids: &code_names,
                    task_prompts: &task_prompts,
                    passage: &passage,
                },
            );
            // The SOP wants the formative item tied to an objective, with its answer.
            if let Some(items) = &eval.output {
                let formative = EXIT_PURPOSES[0].1;
                if !code_names.is_empty()
                    && items
                        .iter()
                        .any(|i| i.purpose == formative && i.objective_refs.is_empty())
                {
                    eval.problems.push(
                        "形成性評量的題目要在 objective_refs 填它檢核的目標代號。".to_string(),
                    );
                }
            }
            eval_to_checked(eval, |items| to_exit_suggestion(&items, &code_ids))
        },
    )
    .await?;
    let _ = PHASE_EXIT;
    Ok(done(outcome, context))
}

// ---------------------------------------------------------------- step 3: the English of an objective

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawEnglish {
    pub english: String,
    pub changed: bool,
    pub note_zh: String,
}

pub const ENGLISH_SCHEMA_NAME: &str = "sop_objective_english";

pub fn english_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "english": { "type": "string" },
            "changed": { "type": "boolean" },
            "note_zh": { "type": "string" }
        },
        "required": ["english", "changed", "note_zh"],
        "additionalProperties": false
    })
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct EnglishSuggestion {
    pub english: String,
    /// False when the model found nothing to fix (the same sentence comes back).
    pub changed: bool,
    pub note: String,
}

/// Checks the corrected English of an objective: one sentence in the 「I can …」 form, in
/// English only, short enough for students.
pub(crate) fn evaluate_english(raw: &RawEnglish, original: &str) -> Checked<EnglishSuggestion> {
    let mut problems: Vec<String> = Vec::new();
    let mut english = super::checks::clean_markup(&raw.english);
    english = english.split_whitespace().collect::<Vec<_>>().join(" ");
    if english.is_empty() {
        problems.push("english 是空的。".to_string());
    } else if contains_cjk(&english) {
        problems.push("english 只能用英文，不可以有中文。".to_string());
    } else {
        let lower = english.to_lowercase();
        if !lower.starts_with("i can ") {
            problems.push("english 必須以「I can 」開頭。".to_string());
        }
        let words = english.split_whitespace().count();
        if words > ENGLISH_MAX_WORDS || words < 4 {
            problems.push(format!(
                "english 需要是 4 到 {ENGLISH_MAX_WORDS} 個英文單字的一句話，現在有 {words} 個。"
            ));
        }
    }
    if !problems.is_empty() {
        return Checked {
            output: None,
            problems,
        };
    }
    // The opening is always written the same way.
    english = format!("I can {}", english[6..].trim_start());
    let same = super::checks::norm(&english) == super::checks::norm(original);
    let mut note = super::checks::clean_markup(&raw.note_zh);
    if note.is_empty() || !contains_cjk(&note) {
        note = if same {
            "這個英文版本已經很好，不需要改。".to_string()
        } else if original.trim().is_empty() {
            "依中文目標寫的英文版本。".to_string()
        } else {
            "修了文法與用字。".to_string()
        };
    }
    Checked {
        output: Some(EnglishSuggestion {
            changed: !same,
            english,
            note: clip_chars(&note, 80),
        }),
        problems,
    }
}

/// 「幫我修英文」: one short call, answered in the request (no job). Returns the `AiResult` JSON.
pub async fn suggest_english(
    state: &AppState,
    teacher_id: i64,
    lesson: &crate::teaching_context::LessonRef,
    kind: ObjectiveType,
    text: &str,
    english: &str,
) -> AppResult<Value> {
    if !state.ai.enabled() {
        return Ok(result_json(AiResult::<Value>::disabled()));
    }
    let vars: HashMap<&'static str, String> = HashMap::from([
        ("objective_type", kind.label().to_string()),
        ("objective_text", or_blank(text.trim())),
        ("student_facing_en", or_blank(english.trim())),
    ]);
    let mut call = new_call(
        KEY_ENGLISH,
        SC_ENGLISH,
        ENGLISH_SCHEMA_NAME,
        english_schema(),
        vars,
        json!({
            "task": "sop_objective_english",
            "english": english.trim(),
            "text": text.trim(),
        }),
        400,
        0.2,
        json!({ "sop": { "lesson_id": lesson.id } }),
    );
    call.required = vec![("objective_text", "中文目標")];
    // The teacher's own words are the input here, not the material: nothing is redacted.
    call.redact = Vec::new();
    let original = english.trim().to_string();
    let outcome = generation::call_in(
        state,
        teacher_id,
        (Some(lesson.unit_id), Some(lesson.id)),
        call,
        |_| Vec::new(),
        |raw: &RawEnglish| evaluate_english(raw, &original),
        generation::nothing,
    )
    .await?;
    let _ = BASE_TEMPERATURE_NOTE;
    Ok(match outcome {
        Ok((out, id)) => result_json(AiResult::shown(id, out, Vec::new())),
        Err(message) => failed_result(&message, Vec::new()),
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    fn paragraphs(n: i32) -> Vec<Paragraph> {
        (1..=n)
            .map(|i| Paragraph {
                id: format!("p{i}"),
                no: i,
                text: format!("Paragraph number {i} has some words."),
                page: None,
            })
            .collect()
    }

    fn codes() -> Vec<(String, String)> {
        vec![("O1".into(), "o-a".into()), ("O2".into(), "o-b".into())]
    }

    fn goal(no: i64, codes: &[&str], note: &str) -> RawGoal {
        RawGoal {
            paragraph_no: no,
            objective_codes: codes.iter().map(|c| c.to_string()).collect(),
            note_zh: note.to_string(),
        }
    }

    #[test]
    fn goals_map_codes_back_to_ids_in_paragraph_order() {
        let raw = RawGoals {
            paragraphs: vec![
                goal(2, &["O2"], "先找出轉折語，再比較前後兩句。"),
                goal(1, &["O1", "O2"], "用第一句帶出主旨。"),
            ],
        };
        let eval = evaluate_goals(&raw, &paragraphs(2), &codes(), &[1, 2]);
        assert!(eval.problems.is_empty(), "{:?}", eval.problems);
        assert_eq!(eval.kept[0].paragraph_no, 1);
        assert_eq!(eval.kept[0].objective_ids, ["o-a", "o-b"]);
        assert_eq!(eval.kept[1].objective_ids, ["o-b"]);
    }

    #[test]
    fn goals_drop_invented_codes_and_report_missing_paragraphs() {
        let raw = RawGoals {
            paragraphs: vec![goal(1, &["O1", "O9"], "用第一句帶出主旨。")],
        };
        let eval = evaluate_goals(&raw, &paragraphs(3), &codes(), &[1, 2, 3]);
        assert_eq!(eval.kept.len(), 1);
        assert_eq!(eval.kept[0].objective_ids, ["o-a"]);
        assert_eq!(eval.missing, [2, 3]);
        assert!(eval.problems.iter().any(|p| p.contains("O9")));
        assert!(eval.problems.iter().any(|p| p.contains("[P2]")));
    }

    #[test]
    fn goals_need_a_chinese_note_within_the_limit_and_a_paragraph_may_serve_none() {
        let raw = RawGoals {
            paragraphs: vec![
                goal(1, &[], "這一段只是帶入情境，不特別對應目標，快速讀過即可。"),
                goal(2, &["O1"], "Introduce the topic."),
                goal(3, &["O1"], &"字".repeat(GOAL_NOTE_MAX_CHARS + 20)),
            ],
        };
        let eval = evaluate_goals(&raw, &paragraphs(3), &codes(), &[1, 2, 3]);
        assert!(eval.kept[0].objective_ids.is_empty());
        assert_eq!(eval.kept.len(), 2, "the English note is dropped");
        assert_eq!(eval.missing, [2]);
        assert_eq!(eval.kept[1].note.chars().count(), GOAL_NOTE_MAX_CHARS);
        assert!(eval.problems.iter().any(|p| p.contains("縮短")));
    }

    #[test]
    fn goals_for_one_paragraph_refuse_other_paragraphs_and_duplicates() {
        let raw = RawGoals {
            paragraphs: vec![
                goal(2, &["O1"], "用這一段帶出因果。"),
                goal(2, &["O2"], "重複的一筆。"),
                goal(3, &["O1"], "不在這次要求裡。"),
            ],
        };
        let eval = evaluate_goals(&raw, &paragraphs(3), &codes(), &[2]);
        assert_eq!(eval.kept.len(), 1);
        assert_eq!(eval.kept[0].objective_ids, ["o-a"]);
        assert_eq!(eval.problems.len(), 2);
        assert!(eval.missing.is_empty());
    }

    #[test]
    fn at_most_three_objectives_per_paragraph() {
        let many: Vec<(String, String)> = (1..=5)
            .map(|i| (format!("O{i}"), format!("o{i}")))
            .collect();
        let raw = RawGoals {
            paragraphs: vec![goal(1, &["O1", "O2", "O3", "O4"], "四項都想帶。")],
        };
        let eval = evaluate_goals(&raw, &paragraphs(1), &many, &[1]);
        assert_eq!(eval.kept[0].objective_ids.len(), GOAL_MAX_OBJECTIVES);
        assert!(!eval.problems.is_empty());
    }

    fn english(e: &str) -> RawEnglish {
        RawEnglish {
            english: e.into(),
            changed: true,
            note_zh: String::new(),
        }
    }

    #[test]
    fn english_fix_keeps_the_i_can_form_and_reports_no_change() {
        let ok = evaluate_english(&english("i can explain why the stand was useful."), "");
        assert!(ok.problems.is_empty());
        let out = ok.output.unwrap();
        assert_eq!(out.english, "I can explain why the stand was useful.");
        assert!(out.changed);
        let same = evaluate_english(
            &english("I can explain why the stand was useful."),
            "I can  explain why the stand was useful.",
        );
        assert!(!same.output.unwrap().changed);
    }

    #[test]
    fn english_fix_rejects_chinese_other_openings_and_long_sentences() {
        for bad in [
            "我可以說明原因",
            "Students explain why it happened today.",
            "",
            &format!("I can {}", "word ".repeat(30)),
        ] {
            let c = evaluate_english(&english(bad), "I can x y z.");
            assert!(c.output.is_none(), "{bad}");
            assert!(!c.problems.is_empty());
        }
    }

    #[test]
    fn auto_steps_follow_the_blocks_tasks() {
        let steps = auto_procedural_steps(
            &["p1".to_string()],
            &["小組共同完成".to_string()],
            &["簡答題".to_string()],
        );
        assert!(steps.iter().any(|s| s.contains("group")));
        assert!(steps_text(&steps).starts_with("1. "));
    }
}
