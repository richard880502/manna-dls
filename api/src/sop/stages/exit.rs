use super::*;
use crate::worksheet::checks::{evaluate_exit, ExitCtx};
use crate::worksheet::schemas::{self as ws_schemas, RawExit, RawExitItem};

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
    Ok(done(outcome, context))
}
