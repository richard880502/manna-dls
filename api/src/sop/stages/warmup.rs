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
                    .map(worksheet::ai::candidate_line)
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
            .map(|c| json!({ "word": c.word, "paragraph_no": c.paragraph_no, "sentence": c.sentence, "pos": c.pos }))
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
    Ok(done(outcome, context))
}
