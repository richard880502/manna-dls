use super::*;

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
        let mut note = super::super::checks::clean_markup(&g.note_zh);
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
            note = clip_chars(&note, GOAL_NOTE_MAX_CHARS - 1);
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
    let analysis = super::super::ai::analysis_summary(&input.version, paragraphs);
    let vars: HashMap<&'static str, String> = HashMap::from([
        ("school_track", track_label(&input.track).to_string()),
        ("class_profile", or_blank(&input.profile.text)),
        (
            "passage_numbered",
            super::super::numbered_passage(paragraphs),
        ),
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
            &super::super::ai::analysis_summary(&input.version, paragraphs),
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
    Ok(done(outcome, context))
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
}
