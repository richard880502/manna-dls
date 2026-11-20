use super::*;
use crate::worksheet::checks::{evaluate_scaffolds, evaluate_tasks, ScaffoldsCtx, TasksCtx};
use crate::worksheet::schemas::{
    self as ws_schemas, RawBlock, RawScaffoldSet, RawScaffolds, RawTasks,
};

// ---------------------------------------------------------------- step 5: tasks

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct TaskSuggestion {
    pub function: Option<super::super::InfoFunction>,
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
        reason: super::super::checks::clean_markup(&b.design_reason_zh),
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

pub(super) fn existing_tasks_text(content: &SopContent, paragraphs: &[Paragraph]) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

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
