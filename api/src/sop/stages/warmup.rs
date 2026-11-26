use super::*;
use crate::worksheet::checks::{evaluate_warmup, Candidate, CleanWarmup, WarmupCtx};
use crate::worksheet::schemas::{self as ws_schemas, RawWarmup};

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
    /// Part of speech in the evidence sentence (checked against the word list).
    pub pos: String,
    /// Short Traditional Chinese meaning of that sense ("" when none).
    pub meaning_zh: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct WarmupSuggestion {
    pub background: Vec<WarmupActivitySuggestion>,
    pub vocabulary: Vec<VocabularySuggestion>,
    /// How many candidate words the code offered; 0: nothing to pick from.
    pub candidates_available: usize,
    /// Whether the CEEC word list was available to give levels.
    pub levels_available: bool,
    /// Whether the material has other candidate words than this group's (and the teacher's): 換一個
    /// for the group can only be offered then.
    pub more_words: bool,
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
            pos: v.pos.clone(),
            meaning_zh: v.meaning_zh.clone(),
        })
        .collect();
    WarmupSuggestion {
        background,
        vocabulary,
        candidates_available,
        levels_available,
        more_words: false,
    }
}

/// What a 換一個 job without other candidate words says.
pub const MSG_NO_OTHER_WORDS: &str =
    "教材裡沒有其他候選字可以換了。可以自己新增關鍵字彙，或請 Manna 再設計一次暖身。";

/// What a 換一個 job tells the model on top of the warm-up prompt (`redo_note`).
fn warmup_redo_note(
    redo: &super::super::ai::ItemRedo,
    vocabulary: bool,
    declined_words: &[String],
) -> String {
    if vocabulary {
        return format!(
            "【這次只要換一組關鍵字彙】教師不要上一組字（{}），候選字彙清單已經拿掉它們，請從清單裡另外挑一組。background_activities 照常給 1 個（這次不會用到）。",
            declined_words.join("、")
        );
    }
    let text_of = |v: &Value| clip_chars(v["text"].as_str().unwrap_or_default().trim(), 400);
    format!(
        "【這次只要 1 個暖身活動】教師不要下面這個暖身活動，請另外設計 1 個不同的活動（換一種做法或角度）：\n被換掉的活動：\n{}\n不要與下列已有的活動重複：\n{}\nbackground_activities 只輸出 1 個；key_vocabulary 給空陣列。",
        text_of(&redo.declined),
        lines_or(
            redo.others.iter().map(|o| format!("- {}", text_of(o))).collect(),
            "（沒有）"
        )
    )
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
    // 換一個: one activity again, or another group of words.
    let redo = input.scope.item();
    let vocabulary_redo = redo.is_some_and(|r| r.index == VOCABULARY_ITEM_INDEX);
    let declined_words: Vec<String> = match redo {
        Some(r) if vocabulary_redo => r
            .declined
            .as_array()
            .map(|list| {
                list.iter()
                    .filter_map(|v| v["word"].as_str())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    // Candidates and their levels come from the word list (code); the model only picks. The
    // words of a group set aside are not offered again.
    let lexicon = crate::vocabulary::load_lexicon(&state.db, None)
        .await?
        .filter(|l| !l.is_empty());
    let (mut candidates, _hard) =
        worksheet::ai::key_word_candidates(&passages, lexicon.as_ref(), &teacher_words);
    // Every candidate but the teacher's own words (they come first): what another group could use.
    let all_words: Vec<String> = candidates
        .iter()
        .map(|c| c.word.to_lowercase())
        .filter(|w| !teacher_words.iter().any(|t| t.eq_ignore_ascii_case(w)))
        .collect();
    candidates.retain(|c| {
        !declined_words
            .iter()
            .any(|w| w.eq_ignore_ascii_case(&c.word))
    });
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
                    .map(worksheet::ai::candidate_line)
                    .collect(),
                "（沒有候選字彙；key_vocabulary 請給空陣列）",
            ),
        ),
        (
            "redo_note",
            redo.map(|r| warmup_redo_note(r, vocabulary_redo, &declined_words))
                .unwrap_or_default(),
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
    if vocabulary_redo && candidates.is_empty() {
        return Ok(done::<WarmupSuggestion>(
            Err(MSG_NO_OTHER_WORDS.to_string()),
            context,
        ));
    }
    let fake_context = json!({
        "task": "worksheet_warmup",
        "title": input.material.title,
        "vocab_count": count,
        "candidates": candidates.iter()
            .map(|c| json!({ "word": c.word, "paragraph_no": c.paragraph_no, "sentence": c.sentence, "pos": c.pos }))
            .collect::<Vec<_>>(),
        "sop": true,
        // A redo of an activity is worded differently from the one it replaces.
        "redo": if redo.is_some() && !vocabulary_redo { json!(job_id) } else { Value::Null },
    });
    let mut call = new_call(
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
    if redo.is_some() {
        call.required.push(("redo_note", "這次只要換一項"));
    }
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
                let mut out = clean_to_suggestion(&w, paragraphs, n_candidates, levels_available);
                out.more_words = all_words.iter().any(|word| {
                    !out.vocabulary
                        .iter()
                        .any(|v| v.word.eq_ignore_ascii_case(word))
                });
                // A redo keeps only what was asked for.
                if vocabulary_redo {
                    out.background.clear();
                } else if redo.is_some() {
                    out.background.truncate(1);
                    out.vocabulary.clear();
                }
                out
            })
        },
    )
    .await?;
    Ok(done(outcome, context))
}
