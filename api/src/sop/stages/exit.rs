use super::*;
use crate::worksheet::checks::{evaluate_exit_items, ExitCtx};
use crate::worksheet::schemas::{self as ws_schemas, RawExit, RawExitItem};

// ---------------------------------------------------------------- step 7: exit ticket

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ExitItemSuggestion {
    pub kind: ExitKind,
    pub text: String,
    /// Id of the step 3 objective a formative item checks (empty: none).
    pub objective_id: String,
    pub answer: String,
    pub bloom: Option<worksheet::Bloom>,
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
            bloom: i.bloom.as_deref().and_then(worksheet::Bloom::parse),
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

fn exit_label(kind: ExitKind) -> &'static str {
    EXIT_PURPOSES
        .iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, l)| *l)
        .unwrap_or(EXIT_PURPOSES[0].1)
}

/// What a 換一個 job tells the model on top of the exit-ticket prompt (`redo_note`).
fn exit_redo_note(redo: &super::super::ai::ItemRedo, kind: ExitKind) -> String {
    let label = exit_label(kind);
    let text_of = |v: &Value| clip_chars(v["text"].as_str().unwrap_or_default().trim(), 300);
    format!(
        "【這次只要 1 題】教師不要下面這一題，請另外想 1 題「{label}」，換一個不同的問法或角度：\n被換掉的題目：{}\n不要與下列題目重複：\n{}\nitems 只輸出 1 筆，purpose 一定是「{label}」{}。",
        text_of(&redo.declined),
        lines_or(
            redo.others.iter().map(|o| format!("- {}", text_of(o))).collect(),
            "（沒有）"
        ),
        if kind == ExitKind::Formative {
            "；objective_refs 填它檢核的目標代號，並附 sample_answer_en"
        } else {
            ""
        }
    )
}

/// 換一個 keeps the first usable item of the kind asked for that is not the one set aside or
/// another item of the ticket; the rest is reported for the repair call.
fn pick_one_item(
    eval: &mut crate::worksheet::checks::Eval<Vec<RawExitItem>>,
    kind: ExitKind,
    not_again: &[String],
) {
    let label = exit_label(kind);
    let Some(items) = eval.output.take() else {
        return;
    };
    let mut kept: Option<RawExitItem> = None;
    for item in items {
        if item.purpose != label {
            eval.problems
                .push(format!("這次只要「{label}」，purpose 一定是「{label}」。"));
            continue;
        }
        if not_again.contains(&super::super::checks::norm(&item.prompt_en).to_lowercase()) {
            eval.problems
                .push("這一題和被換掉的或其他題目一樣，請換一個問法。".to_string());
            continue;
        }
        if kept.is_none() {
            kept = Some(item);
        }
    }
    eval.output = kept.map(|item| vec![item]);
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
    let objectives = objective_texts(&codes);
    let passage = texts(paragraphs).join("\n");
    let count = worksheet::exit_count(content.options.minutes);
    let notes = notes_for_ai(state, input).await?;
    let task_prompts: Vec<String> = content
        .task_blocks
        .iter()
        .flat_map(|b| b.tasks.iter().map(|t| t.text.clone()))
        .filter(|t| !t.trim().is_empty())
        .collect();
    // 換一個: one item of the same kind again, different from the one set aside and the others.
    let redo = input.scope.item();
    let wanted: Option<ExitKind> = redo.map(|r| {
        serde_json::from_value::<ExitKind>(r.declined["kind"].clone()).unwrap_or_default()
    });
    let not_again: Vec<String> = redo
        .map(|r| {
            std::iter::once(&r.declined)
                .chain(&r.others)
                .filter_map(|v| v["text"].as_str())
                .map(|t| super::super::checks::norm(t).to_lowercase())
                .collect()
        })
        .unwrap_or_default();
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
        (
            "redo_note",
            match (redo, wanted) {
                (Some(r), Some(kind)) => exit_redo_note(r, kind),
                _ => String::new(),
            },
        ),
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
    let mut call = new_call(
        KEY_EXIT,
        SC_EXIT,
        ws_schemas::EXIT_SCHEMA_NAME,
        ws_schemas::exit_schema(),
        vars,
        json!({
            "task": "worksheet_exit_ticket", "exit_count": count, "objective_ids": code_names,
            "sop": true,
            // 換一個: the one kind asked for, worded differently from the one it replaces.
            "one_purpose": wanted.map(exit_label),
            "redo": redo.map(|_| job_id),
        }),
        worksheet::ai::EXIT_MAX_TOKENS,
        temperature,
        trace(
            input,
            json!({ "exit_count": count, "one_purpose": wanted.map(exit_label) }),
        ),
    );
    if redo.is_some() {
        call.required.push(("redo_note", "這次只要換一題"));
    }
    let outcome = run_call(
        state,
        input,
        job_id,
        PHASE_EXIT_REPAIR,
        call,
        |raw: &RawExit| {
            let mut eval = evaluate_exit_items(
                raw,
                &ExitCtx {
                    objective_ids: &code_names,
                    task_prompts: &task_prompts,
                    passage: &passage,
                    objectives: &objectives,
                },
                wanted.is_none(),
            );
            if let Some(kind) = wanted {
                pick_one_item(&mut eval, kind, &not_again);
            }
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
            eval_to_checked(eval, |items| {
                let mut out = to_exit_suggestion(&items, &code_ids);
                // One item says nothing about the ticket's coverage.
                if wanted.is_some() {
                    out.uncovered_objective_ids.clear();
                }
                out
            })
        },
    )
    .await?;
    Ok(done(outcome, context))
}
