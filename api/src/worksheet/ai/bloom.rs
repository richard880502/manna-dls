//! 閱讀理解題 (`output = "comprehension"`) and 差異化學習單 (`output = "tiered"`, formerly 分層練習):
//! questions written per Bloom level and checked by a separate judge. Contract: docs/API.md
//! 「Phase 2.7」 and 「差異化學習單」.
//!
//! A run has two jobs. `bloom`: one model call per selected level (`worksheet_bloom_level`; for a
//! 差異化學習單 `worksheet_tiered_level`, which writes every question once with its three
//! versions), one after another in Bloom order, each with its own checks and one repair call; a
//! level starts with the questions the lower levels have already written, so it does not repeat
//! them.
//! `bloom_check`: one judge call (`worksheet_bloom_check`) classifies every question on its own;
//! the questions it rates too far from their target are written again once (one call per
//! affected level, the judge's reasons as the problems) and judged again; a question that still
//! misses keeps the judged level as its label, and a notice says so. The judge sees each
//! question once: a 差異化學習單 has the same questions, so the same Bloom distribution, in all
//! three versions (T2). The worksheet is then assembled in code: one block per level, in Bloom
//! order, with fixed titles and the written steps (R5).
//!
//! Regenerating one of those blocks re-runs its level call and judges that block only.

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use futures_util::stream::{self, StreamExt, TryStreamExt};
use serde_json::json;

use super::{
    close, empty_message, expire_stale, mark_done, mark_held, open_run, replace_block, run_step,
    skip, spawn_run, Run, RunInput, StepCall, StepInfo, StepResult, STEP_BLOOM, STEP_BLOOM_CHECK,
};
use crate::error::{AppError, AppResult};
use crate::routes::{clip_chars, log_usage, or_blank, track_label};
use crate::worksheet::assemble::{auto_steps, letter};
use crate::worksheet::bloom::{
    self as bl, Bloom, Output, RawBloomCheck, RawBloomLevel, CHECK_SCHEMA_NAME, LEVEL_SCHEMA_NAME,
};
use crate::worksheet::checks::{
    evaluate_bloom_check, evaluate_bloom_level, evaluate_tiered_level, BloomCtx, CleanBloomItem,
    CleanBloomLevel, CleanLinking, Judged,
};
use crate::worksheet::store;
use crate::worksheet::tiered::{self, RawTieredLevel};
use crate::worksheet::{
    Block, BlockKind, Chosen, Content, Item, Level, Material, Origin, Scaffold, ScaffoldType,
    Settings, GEN_OBJECTIVES, KIND_EXAMPLE, KIND_STEPS, KIND_WORD_LIST,
};
use crate::AppState;

pub const KEY_LEVEL: &str = "worksheet_bloom_level";
pub const KEY_TIERED: &str = "worksheet_tiered_level";
pub const KEY_CHECK: &str = "worksheet_bloom_check";
const SCAFFOLD_LEVEL: &str = "SC-WS-BLOOM";
const SCAFFOLD_TIERED: &str = "SC-WS-TIERED";
const SCAFFOLD_CHECK: &str = "SC-WS-BLOOM-CHECK";

const fn level_step(phase: &'static str, repair_phase: &'static str) -> StepInfo {
    StepInfo {
        key: KEY_LEVEL,
        module: KEY_LEVEL,
        scaffold: SCAFFOLD_LEVEL,
        schema_name: LEVEL_SCHEMA_NAME,
        phase,
        repair_phase,
    }
}

const fn tiered_step(phase: &'static str, repair_phase: &'static str) -> StepInfo {
    StepInfo {
        key: KEY_TIERED,
        module: KEY_TIERED,
        scaffold: SCAFFOLD_TIERED,
        schema_name: tiered::LEVEL_SCHEMA_NAME,
        phase,
        repair_phase,
    }
}

/// One per level, in Bloom order: the phase names the level being written.
const LEVEL_STEPS: [StepInfo; 6] = [
    level_step(
        "正在出『記憶』層次的題目",
        "『記憶』層次有題目沒有通過檢查，正在修正",
    ),
    level_step(
        "正在出『理解』層次的題目",
        "『理解』層次有題目沒有通過檢查，正在修正",
    ),
    level_step(
        "正在出『應用』層次的題目",
        "『應用』層次有題目沒有通過檢查，正在修正",
    ),
    level_step(
        "正在出『分析』層次的題目",
        "『分析』層次有題目沒有通過檢查，正在修正",
    ),
    level_step(
        "正在出『評鑑』層次的題目",
        "『評鑑』層次有題目沒有通過檢查，正在修正",
    ),
    level_step(
        "正在出『創造』層次的題目",
        "『創造』層次有題目沒有通過檢查，正在修正",
    ),
];

/// The same call, rewriting the questions the judge rated as another level.
const REDO_STEPS: [StepInfo; 6] = [
    level_step(
        "正在重出層次不符的『記憶』題目",
        "正在修正重出的『記憶』題目",
    ),
    level_step(
        "正在重出層次不符的『理解』題目",
        "正在修正重出的『理解』題目",
    ),
    level_step(
        "正在重出層次不符的『應用』題目",
        "正在修正重出的『應用』題目",
    ),
    level_step(
        "正在重出層次不符的『分析』題目",
        "正在修正重出的『分析』題目",
    ),
    level_step(
        "正在重出層次不符的『評鑑』題目",
        "正在修正重出的『評鑑』題目",
    ),
    level_step(
        "正在重出層次不符的『創造』題目",
        "正在修正重出的『創造』題目",
    ),
];

/// 差異化學習單: one per level, in Bloom order.
const TIERED_STEPS: [StepInfo; 6] = [
    tiered_step(
        "正在出『記憶』層次的題目與三個版本的支持",
        "『記憶』層次有題目沒有通過檢查，正在修正",
    ),
    tiered_step(
        "正在出『理解』層次的題目與三個版本的支持",
        "『理解』層次有題目沒有通過檢查，正在修正",
    ),
    tiered_step(
        "正在出『應用』層次的題目與三個版本的支持",
        "『應用』層次有題目沒有通過檢查，正在修正",
    ),
    tiered_step(
        "正在出『分析』層次的題目與三個版本的支持",
        "『分析』層次有題目沒有通過檢查，正在修正",
    ),
    tiered_step(
        "正在出『評鑑』層次的題目與三個版本的支持",
        "『評鑑』層次有題目沒有通過檢查，正在修正",
    ),
    tiered_step(
        "正在出『創造』層次的題目與三個版本的支持",
        "『創造』層次有題目沒有通過檢查，正在修正",
    ),
];

const TIERED_REDO_STEPS: [StepInfo; 6] = [
    tiered_step(
        "正在重出層次不符的『記憶』題目",
        "正在修正重出的『記憶』題目",
    ),
    tiered_step(
        "正在重出層次不符的『理解』題目",
        "正在修正重出的『理解』題目",
    ),
    tiered_step(
        "正在重出層次不符的『應用』題目",
        "正在修正重出的『應用』題目",
    ),
    tiered_step(
        "正在重出層次不符的『分析』題目",
        "正在修正重出的『分析』題目",
    ),
    tiered_step(
        "正在重出層次不符的『評鑑』題目",
        "正在修正重出的『評鑑』題目",
    ),
    tiered_step(
        "正在重出層次不符的『創造』題目",
        "正在修正重出的『創造』題目",
    ),
];

/// The level call of an output (`redo`: rewriting questions the judge rated as another level).
fn step_for(output: Output, b: Bloom, redo: bool) -> &'static StepInfo {
    match (output, redo) {
        (Output::Tiered, false) => &TIERED_STEPS[b.rank()],
        (Output::Tiered, true) => &TIERED_REDO_STEPS[b.rank()],
        (_, false) => &LEVEL_STEPS[b.rank()],
        (_, true) => &REDO_STEPS[b.rank()],
    }
}

/// The answer formats a level may use: a 差異化學習單 only uses the open ones.
fn formats_for(output: Output, b: Bloom, selected: &[String]) -> Vec<&'static str> {
    match output {
        Output::Tiered => tiered::level_formats(b, selected),
        _ => bl::level_formats(b, selected),
    }
}

const CHECK: StepInfo = StepInfo {
    key: KEY_CHECK,
    module: KEY_CHECK,
    scaffold: SCAFFOLD_CHECK,
    schema_name: CHECK_SCHEMA_NAME,
    phase: "正在檢查題目的 Bloom 層次",
    repair_phase: "正在重新檢查題目的 Bloom 層次",
};

const SKIPPED: &str = "前一步沒有成功，所以沒有執行。";

fn or_none(s: &str) -> String {
    if s.trim().is_empty() {
        "（沒有）".to_string()
    } else {
        s.trim().to_string()
    }
}

// ---------------------------------------------------------------- drafts

/// One question while it is made: the level it was written for and the level it is labelled.
#[derive(Debug, Clone)]
struct Draft {
    target: Bloom,
    label: Bloom,
    item: CleanBloomItem,
}

/// One level's block while it is made.
#[derive(Debug, Clone)]
struct LevelDraft {
    bloom: Bloom,
    /// Prompt version 1's one sentence ("" with the written steps of version 2).
    instructions: String,
    /// The written steps (R5); empty: the program writes them.
    steps: Vec<String>,
    example: String,
    linking: Option<CleanLinking>,
    items: Vec<Draft>,
    /// Why the level has no questions.
    failure: Option<String>,
}

impl LevelDraft {
    fn failed(bloom: Bloom, reason: String) -> Self {
        Self {
            bloom,
            instructions: String::new(),
            steps: Vec::new(),
            example: String::new(),
            linking: None,
            items: Vec::new(),
            failure: Some(reason),
        }
    }

    fn from_output(bloom: Bloom, out: CleanBloomLevel) -> Self {
        Self {
            bloom,
            instructions: out.instructions,
            steps: out.steps,
            example: out.example,
            linking: out.linking,
            items: out
                .items
                .into_iter()
                .map(|item| Draft {
                    target: bloom,
                    label: bloom,
                    item,
                })
                .collect(),
            failure: None,
        }
    }

    fn prompts(&self) -> impl Iterator<Item = &str> {
        self.items.iter().map(|d| d.item.item.prompt.as_str())
    }
}

// ---------------------------------------------------------------- the two calls

/// What the questions are for. The 閱讀理解題 and 差異化學習單 requests must never be identical
/// (the gateway would answer the second from its cache); they now use different prompts, and
/// this line still tells them apart.
fn purpose_line(output: Output) -> &'static str {
    match output {
        Output::Tiered => {
            "差異化學習單：同一份題目印成基礎版、一般版、進階版，三版只差在支持的多寡。"
        }
        _ => "閱讀理解題：全班同一份，依 Bloom 層次由淺到深。",
    }
}

/// The worksheet's objectives as the prompts see them (T1: the questions serve them), one per
/// line; 「（沒有）」 when there are none.
fn objectives_text(content: &Content) -> String {
    let lines: Vec<String> = content
        .objectives
        .iter()
        .filter_map(|o| {
            let both = [o.text.trim(), o.student_facing_en.trim()]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("｜");
            (!both.is_empty()).then(|| format!("- {}：{both}", o.kind.label()))
        })
        .collect();
    if lines.is_empty() {
        "（沒有）".to_string()
    } else {
        lines.join("\n")
    }
}

struct LevelRequest<'a> {
    /// What the block is for (the generation's settings: a regenerated block keeps them).
    output: Output,
    info: &'a StepInfo,
    bloom: Bloom,
    count: usize,
    formats: &'a [&'static str],
    /// Questions already written (not to be repeated).
    earlier: &'a [String],
    /// Rewriting questions the judge rated as another level: what to tell the model.
    redo: Option<String>,
}

async fn level_call(
    run: &Run,
    job_id: i64,
    r: LevelRequest<'_>,
) -> AppResult<StepResult<CleanBloomLevel>> {
    let output = r.output;
    let material = &run.input.worksheet.material;
    let codes = bl::paragraph_codes(material.paragraphs.len());
    let b = r.bloom;
    let evidence_rule = if b.needs_evidence() {
        "這個層次的每一題都必須填：從教材逐字複製一段連續文字（至少 5 個英文字）作為答案的依據；不可改寫，不可用「...」省略，也不可把不相鄰的兩處接在一起。平台會核對，不是逐字的題目會被退回。"
    } else {
        "這個層次的題目不需要原文依據，填空字串 \"\"。"
    };
    let earlier = if r.earlier.is_empty() {
        "（沒有）".to_string()
    } else {
        r.earlier
            .iter()
            .map(|p| format!("- {}", clip_chars(p, 200)))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let mut vars: HashMap<&'static str, String> = HashMap::from([
        ("school_track", track_label(&run.input.track).to_string()),
        ("class_profile", or_blank(&run.input.class_profile)),
        ("objectives", objectives_text(&run.input.content)),
        ("teacher_request", or_none(&run.request)),
        ("teacher_notes_for_ai", run.ai_notes.clone()),
        ("bloom_id", b.as_str().to_string()),
        ("bloom_label", b.label().to_string()),
        ("bloom_definition", b.definition().to_string()),
        ("bloom_stems", b.stems().to_string()),
        ("answer_formats", r.formats.join("、")),
        ("question_count", r.count.to_string()),
        ("purpose", purpose_line(output).to_string()),
        ("evidence_rule", evidence_rule.to_string()),
        ("earlier_questions", earlier),
        ("passage_numbered", material.numbered()),
        ("redo_note", r.redo.clone().unwrap_or_default()),
    ]);
    // 內容目標 and 語言目標, and both in `{{teacher_objective}}` for version 1.
    vars.extend(run.teacher.vars());
    let fake_context = json!({
        "task": if output == Output::Tiered { KEY_TIERED } else { KEY_LEVEL },
        "bloom": b.as_str(),
        "count": r.count,
        "formats": r.formats,
        "paragraphs": codes.iter().zip(&run.paragraphs)
            .map(|(code, text)| json!({ "code": code, "text": text })).collect::<Vec<_>>(),
        "run_no": run.run_no,
        "redo": r.redo.is_some(),
    });
    let passage = run.paragraphs.join("\n");
    let ctx = BloomCtx {
        bloom: b,
        formats: r.formats,
        passage: &passage,
        paragraph_codes: &codes,
        count: r.count,
    };
    if output == Output::Tiered {
        return run_step(
            run,
            r.info,
            job_id,
            StepCall {
                vars,
                schema: tiered::level_schema(r.formats, &codes, r.count),
                fake_context,
                max_tokens: tiered::level_max_tokens(r.count),
            },
            |raw: &RawTieredLevel| evaluate_tiered_level(raw, &ctx),
        )
        .await;
    }
    run_step(
        run,
        r.info,
        job_id,
        StepCall {
            vars,
            schema: bl::level_schema(r.formats, &codes, r.count),
            fake_context,
            max_tokens: bl::level_max_tokens(r.count),
        },
        |raw: &RawBloomLevel| evaluate_bloom_level(raw, &ctx),
    )
    .await
}

/// The question as the judge sees it.
fn question_text(code: &str, item: &CleanBloomItem) -> String {
    let it = &item.item;
    let mut text = format!(
        "{code}（{}）{}",
        it.answer_format,
        clip_chars(&it.prompt, 400)
    );
    if !it.options.is_empty() {
        text.push_str(&format!(
            "\n  選項：{}",
            it.options
                .iter()
                .enumerate()
                .map(|(i, o)| format!("({}) {}", letter(i), clip_chars(o, 120)))
                .collect::<Vec<_>>()
                .join(" ")
        ));
    }
    if !it.answer.trim().is_empty() {
        text.push_str(&format!("\n  參考答案：{}", clip_chars(&it.answer, 300)));
    }
    text
}

/// The judge: one call that classifies every question (`target` only reaches the fake model).
async fn check_call(
    run: &Run,
    job_id: i64,
    questions: &[(Bloom, CleanBloomItem)],
) -> AppResult<StepResult<Vec<Judged>>> {
    let ids: Vec<String> = (1..=questions.len()).map(|n| format!("Q{n}")).collect();
    let vars: HashMap<&'static str, String> = HashMap::from([
        ("passage_numbered", run.input.worksheet.material.numbered()),
        ("question_count", questions.len().to_string()),
        (
            "questions",
            ids.iter()
                .zip(questions)
                .map(|(id, (_, item))| question_text(id, item))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
    ]);
    let fake_context = json!({
        "task": KEY_CHECK,
        "items": ids.iter().zip(questions)
            .map(|(id, (target, _))| json!({ "id": id, "target": target.as_str() }))
            .collect::<Vec<_>>(),
    });
    run_step(
        run,
        &CHECK,
        job_id,
        StepCall {
            vars,
            schema: bl::check_schema(&ids),
            fake_context,
            max_tokens: bl::check_max_tokens(ids.len()),
        },
        |raw: &RawBloomCheck| evaluate_bloom_check(raw, &ids),
    )
    .await
}

// ---------------------------------------------------------------- the judge and its consequences

struct Checked {
    outcome: &'static str,
    interaction_id: Option<i64>,
    message: Option<String>,
    /// For `content.notices`, in question order.
    notices: Vec<String>,
}

fn redo_note(b: Bloom, wrong: &[(&CleanBloomItem, &Judged)]) -> String {
    format!(
        "\n\n【重新出題】下列題目原本要出「{}」層次，但審查判斷不是這個層次。請重新設計 {} 題真正屬於「{}」層次的題目來取代它們（不要只改幾個字）：\n{}",
        b.label(),
        wrong.len(),
        b.label(),
        wrong
            .iter()
            .map(|(item, j)| format!(
                "- {} → 審查判斷：{}（{}）",
                clip_chars(&item.item.prompt, 200),
                j.bloom.label(),
                j.reason
            ))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

/// Judges every question of `drafts`. Questions too far from their target are written again
/// once (one call per affected level) and the new ones judged again; a question that still
/// misses is labelled with the judged level and gets a notice. `first_no[i]` is the number of
/// the first question of `drafts[i]`; `outside` are the prompts of other blocks (not to be
/// repeated by the rewritten questions).
async fn judge_and_fix(
    run: &Run,
    job_id: i64,
    output: Output,
    drafts: &mut [LevelDraft],
    first_no: &[usize],
    selected_formats: &[String],
    outside: &[String],
) -> AppResult<Checked> {
    let refs: Vec<(usize, usize)> = drafts
        .iter()
        .enumerate()
        .flat_map(|(li, l)| (0..l.items.len()).map(move |ii| (li, ii)))
        .collect();
    if refs.is_empty() {
        return Ok(Checked {
            outcome: "skipped",
            interaction_id: None,
            message: Some("沒有題目可以檢查。".to_string()),
            notices: Vec::new(),
        });
    }
    let question = |drafts: &[LevelDraft], (li, ii): (usize, usize)| {
        let d = &drafts[li].items[ii];
        (d.target, d.item.clone())
    };
    let questions: Vec<(Bloom, CleanBloomItem)> =
        refs.iter().map(|r| question(drafts, *r)).collect();
    let (judge_id, judged) = match check_call(run, job_id, &questions).await? {
        StepResult::Shown {
            interaction_id,
            output,
        } => (interaction_id, output),
        StepResult::Failed {
            outcome,
            interaction_id,
            reason,
        } => {
            return Ok(Checked {
                outcome,
                interaction_id,
                message: Some(format!(
                    "{reason}，這一步沒有完成；題目標的是出題時的目標層次。"
                )),
                notices: vec![format!(
                "Bloom 層次的檢查沒有完成（{reason}）；題目標的是出題時的目標層次，可以自己確認。"
            )],
            })
        }
    };

    let mut notices: Vec<(usize, String)> = Vec::new();
    let relabel = |drafts: &mut [LevelDraft],
                   notices: &mut Vec<(usize, String)>,
                   (li, ii): (usize, usize),
                   judged: Bloom| {
        let d = &mut drafts[li].items[ii];
        d.label = judged;
        let no = first_no.get(li).copied().unwrap_or(1) + ii;
        notices.push((no, bl::relabel_notice(no, d.target, judged)));
    };

    // Too far from the target: per level, in order.
    let mut wrong: BTreeMap<usize, Vec<(usize, Judged)>> = BTreeMap::new();
    for j in judged {
        let (li, ii) = refs[j.index];
        if bl::is_mismatch(drafts[li].items[ii].target, j.bloom) {
            wrong.entry(li).or_default().push((ii, j));
        }
    }
    let mut redone: Vec<(usize, usize)> = Vec::new();
    for (li, list) in &wrong {
        let li = *li;
        let b = drafts[li].bloom;
        let earlier: Vec<String> = outside
            .iter()
            .cloned()
            .chain(drafts.iter().enumerate().flat_map(|(lj, l)| {
                l.items
                    .iter()
                    .enumerate()
                    .filter(move |(ij, _)| !(lj == li && list.iter().any(|(ii, _)| ii == ij)))
                    .map(|(_, d)| d.item.item.prompt.clone())
            }))
            .collect();
        let pairs: Vec<(&CleanBloomItem, &Judged)> = list
            .iter()
            .map(|(ii, j)| (&drafts[li].items[*ii].item, j))
            .collect();
        let note = redo_note(b, &pairs);
        let formats = formats_for(output, b, selected_formats);
        let result = level_call(
            run,
            job_id,
            LevelRequest {
                output,
                info: step_for(output, b, true),
                bloom: b,
                count: list.len(),
                formats: &formats,
                earlier: &earlier,
                redo: Some(note),
            },
        )
        .await?;
        let new_items = match result {
            StepResult::Shown { output, .. } => output.items,
            StepResult::Failed { .. } => Vec::new(),
        };
        for (k, (ii, j)) in list.iter().enumerate() {
            match new_items.get(k) {
                Some(new) => {
                    drafts[li].items[*ii].item = new.clone();
                    redone.push((li, *ii));
                }
                // Nothing to replace it with: it stays, labelled as judged.
                None => relabel(drafts, &mut notices, (li, *ii), j.bloom),
            }
        }
    }
    if !redone.is_empty() {
        let questions: Vec<(Bloom, CleanBloomItem)> =
            redone.iter().map(|r| question(drafts, *r)).collect();
        // A second judgement that fails leaves the new questions with their target.
        if let StepResult::Shown { output, .. } = check_call(run, job_id, &questions).await? {
            for j in output {
                let r = redone[j.index];
                if bl::is_mismatch(drafts[r.0].items[r.1].target, j.bloom) {
                    relabel(drafts, &mut notices, r, j.bloom);
                }
            }
        }
    }
    notices.sort_by_key(|(no, _)| *no);
    Ok(Checked {
        outcome: "shown",
        interaction_id: Some(judge_id),
        message: None,
        notices: notices.into_iter().map(|(_, n)| n).collect(),
    })
}

// ---------------------------------------------------------------- blocks

/// The scaffolds of a level's block: the written steps (R5; the program's own when the model
/// gave none), the worked example (閱讀理解題: every version; 差異化學習單: 基礎版 and 一般版,
/// support goes down) and the linking words, graded per version (T5).
fn level_scaffolds(
    block: &Block,
    draft: &LevelDraft,
    output: Output,
    levels: &[Level],
) -> Vec<Scaffold> {
    let steps = if draft.steps.is_empty() {
        auto_steps(block)
    } else {
        draft.steps.clone()
    };
    let mut out = vec![Scaffold {
        id: format!("{}-sp", block.id),
        kind_type: ScaffoldType::Procedural,
        kind: KIND_STEPS.to_string(),
        content: String::new(),
        steps,
        levels: levels.to_vec(),
        auto: draft.steps.is_empty(),
        origin: Origin::Ai,
    }];
    if !draft.example.trim().is_empty() {
        let shown: Vec<Level> = match output {
            Output::Tiered => levels
                .iter()
                .copied()
                .filter(|l| *l != Level::Advanced)
                .collect(),
            _ => levels.to_vec(),
        };
        if !shown.is_empty() {
            out.push(Scaffold {
                id: format!("{}-sx", block.id),
                kind_type: ScaffoldType::Verbal,
                kind: KIND_EXAMPLE.to_string(),
                content: draft.example.clone(),
                steps: Vec::new(),
                levels: shown,
                auto: false,
                origin: Origin::Ai,
            });
        }
    }
    if let Some(linking) = &draft.linking {
        for level in levels {
            let words = linking.words(*level);
            if words.is_empty() {
                continue;
            }
            let lead = if linking.function.trim().is_empty() {
                "Linking words".to_string()
            } else {
                format!("Linking words for {}", linking.function.trim())
            };
            out.push(Scaffold {
                id: format!("{}-sl{}", block.id, level.as_str()),
                kind_type: ScaffoldType::Verbal,
                kind: KIND_WORD_LIST.to_string(),
                content: format!("{lead}: {}", words.join(", ")),
                steps: Vec::new(),
                levels: vec![*level],
                auto: false,
                origin: Origin::Ai,
            });
        }
    }
    out
}

/// A level's block. Every question is in all of the worksheet's versions: a 閱讀理解題 has one
/// version (or the teacher's), a 差異化學習單 three, which differ only in each question's
/// `tiers` (T2).
fn level_block(
    id: &str,
    draft: &LevelDraft,
    material: &Material,
    output: Output,
    levels: &[Level],
) -> Block {
    let b = draft.bloom;
    let mut nos: Vec<usize> = draft
        .items
        .iter()
        .flat_map(|d| d.item.paragraph_nos.iter().copied())
        .collect();
    nos.sort_unstable();
    nos.dedup();
    let items = draft
        .items
        .iter()
        .enumerate()
        .map(|(k, d)| Item {
            id: format!("{id}-i{}", k + 1),
            answer_format: Some(d.item.item.answer_format.clone()),
            prompt: d.item.item.prompt.clone(),
            options: d.item.item.options.clone(),
            answer: d.item.item.answer.clone(),
            evidence: d.item.item.evidence.clone().unwrap_or_default(),
            purpose: None,
            levels: levels.to_vec(),
            origin: Origin::Ai,
            bloom: Some(d.label),
            hint: d.hint(),
            tiers: d
                .item
                .tiers
                .iter()
                .filter(|t| levels.contains(&t.level))
                .cloned()
                .collect(),
        })
        .collect();
    let mut block = Block {
        id: id.to_string(),
        kind: BlockKind::Activity,
        title: bl::block_title(b),
        instructions: match &draft.failure {
            Some(_) => String::new(),
            None => draft.instructions.trim().to_string(),
        },
        paragraph_ids: nos
            .iter()
            .filter_map(|no| material.id_of(*no as i64))
            .map(str::to_string)
            .collect(),
        student_tasks: b.student_tasks().iter().map(|t| t.to_string()).collect(),
        items,
        origin: if draft.failure.is_some() {
            Origin::Teacher
        } else {
            Origin::Ai
        },
        message: draft
            .failure
            .as_ref()
            .map(|reason| empty_message(&format!("『{}』層次的題目", b.label()), reason)),
        bloom: Some(b),
        ..Block::default()
    };
    if draft.failure.is_none() {
        block.scaffolds = level_scaffolds(&block, draft, output, levels);
    }
    block
}

impl Draft {
    fn hint(&self) -> String {
        self.item.hint.trim().to_string()
    }
}

/// What the system used for the generation: the versions (tiered: all three; otherwise the
/// selected ones or 一般版), the levels and counts actually generated, and why.
fn chosen_for(settings: &Settings, plan: &[(Bloom, usize)]) -> Chosen {
    let mut s = settings.clone();
    let mut auto: Vec<String> = Vec::new();
    let total: usize = plan.iter().map(|(_, n)| n).sum();
    let mut reasons = vec![bl::summary(plan)];
    match settings.output {
        Output::Tiered => s.levels = Level::ALL.to_vec(),
        _ if s.levels.is_empty() => {
            s.levels = vec![Level::Standard];
            auto.push("levels".to_string());
            reasons.push("版本：預設產生一般版。".to_string());
        }
        _ => {}
    }
    if settings.bloom_counts.is_some() {
        reasons.push(format!("題數：依指定的各層次題數，共 {total} 題。"));
    } else {
        if settings.bloom.is_empty() {
            reasons.push("層次：沒有指定，六個層次都出。".to_string());
        }
        reasons.push(match settings.question_count {
            None => format!(
                "題數：沒有指定，{}預設 {} 題。",
                settings.output.label(),
                settings.output.default_count()
            ),
            Some(_) => format!("題數：共 {total} 題，每個層次至少 1 題。"),
        });
    }
    if settings.output == Output::Tiered {
        reasons.push(tiered::TIERED_REASON.to_string());
    }
    s.bloom = plan.iter().map(|(b, _)| *b).collect();
    s.question_count = Some(total as i32);
    Chosen {
        settings: s,
        auto,
        item_budget: total,
        reasons,
    }
}

// ---------------------------------------------------------------- a whole generation

/// Creates the two jobs of a generation per Bloom level (or joins the run in progress) and
/// starts the work.
pub(super) async fn start_generation(state: &AppState, input: RunInput) -> AppResult<()> {
    let worksheet_id = input.worksheet.id;
    expire_stale(state, worksheet_id).await?;
    let Some((run_no, ids)) = open_run(
        &state.db,
        worksheet_id,
        input.worksheet.teacher_id,
        &[STEP_BLOOM, STEP_BLOOM_CHECK],
        None,
    )
    .await?
    else {
        return Ok(());
    };
    let state2 = state.clone();
    spawn_run(state, worksheet_id, run_no, async move {
        let hold = ids.get(STEP_BLOOM_CHECK).copied();
        let run = Run::new(&state2, input, run_no, hold).await?;
        generate(&run, &ids).await
    });
    Ok(())
}

async fn make_level(
    run: &Run,
    job_id: i64,
    selected_formats: &[String],
    b: Bloom,
    count: usize,
    done: &Mutex<Vec<(Bloom, Vec<String>)>>,
) -> AppResult<LevelDraft> {
    // The questions the lower levels have written so far.
    let earlier: Vec<String> = done
        .lock()
        .map(|d| {
            let mut lower: Vec<&(Bloom, Vec<String>)> = d.iter().filter(|(l, _)| *l < b).collect();
            lower.sort_by_key(|(l, _)| *l);
            lower
                .into_iter()
                .flat_map(|(_, p)| p.iter().cloned())
                .collect()
        })
        .unwrap_or_default();
    let output = run.input.worksheet.settings.output;
    let formats = formats_for(output, b, selected_formats);
    let result = level_call(
        run,
        job_id,
        LevelRequest {
            output,
            info: step_for(output, b, false),
            bloom: b,
            count,
            formats: &formats,
            earlier: &earlier,
            redo: None,
        },
    )
    .await?;
    Ok(match result {
        StepResult::Shown { output, .. } => {
            let draft = LevelDraft::from_output(b, output);
            if let Ok(mut d) = done.lock() {
                d.push((b, draft.prompts().map(str::to_string).collect()));
            }
            draft
        }
        StepResult::Failed { reason, .. } => LevelDraft::failed(b, reason),
    })
}

/// The number of the first question of each draft (questions numbered through the sheet).
fn first_numbers(drafts: &[LevelDraft], start: usize) -> Vec<usize> {
    let mut no = start;
    drafts
        .iter()
        .map(|d| {
            let first = no;
            no += d.items.len();
            first
        })
        .collect()
}

async fn generate(run: &Run, jobs: &HashMap<&'static str, i64>) -> AppResult<()> {
    let db = run.db();
    let ws = &run.input.worksheet;
    let material = &ws.material;
    let settings = &ws.settings;
    let output = settings.output;
    let plan = bl::plan(settings);
    let chosen = chosen_for(settings, &plan);
    let levels = chosen.settings.levels.clone();
    let level_job = jobs[STEP_BLOOM];
    let check_job = jobs[STEP_BLOOM_CHECK];

    // ---- one call per level, one after another in Bloom order: every level sees all the
    // questions the lower levels wrote (run concurrently, 理解 re-asked what 記憶 had just asked).
    let done: Mutex<Vec<(Bloom, Vec<String>)>> = Mutex::new(Vec::new());
    let done = &done;
    let formats = &settings.answer_formats;
    let mut drafts: Vec<LevelDraft> = stream::iter(plan.iter().copied())
        .map(move |(b, n)| make_level(run, level_job, formats, b, n, done))
        .buffered(1)
        .try_collect()
        .await?;
    let failed = drafts.iter().filter(|d| d.failure.is_some()).count();
    let (outcome, message) = match failed {
        0 => ("shown", None),
        n if n == drafts.len() => (
            "blocked",
            Some("所有層次都沒有產生成功；你可以按「產生」再試一次，或自己寫。".to_string()),
        ),
        n => (
            "shown",
            Some(format!(
                "{n} 個層次沒有產生成功；可以在那一區按「重新產生」，或自己寫。"
            )),
        ),
    };
    let total: usize = plan.iter().map(|(_, n)| n).sum();
    let context = run.context(&[
        ("Bloom 層次", bl::summary(&plan)),
        ("題數", format!("{total} 題")),
    ]);
    mark_done(db, level_job, outcome, message.as_deref(), None, &context).await?;

    // ---- the judge
    let first_no = first_numbers(&drafts, 1);
    let checked =
        judge_and_fix(run, check_job, output, &mut drafts, &first_no, formats, &[]).await?;
    let context = run.context(&[(
        "要檢查的題目",
        format!("{} 題", drafts.iter().map(|d| d.items.len()).sum::<usize>()),
    )]);
    mark_held(
        db,
        check_job,
        checked.outcome,
        checked.message.as_deref(),
        checked.interaction_id,
        &context,
    )
    .await?;

    // ---- assembly, in code: one block per level, in Bloom order
    let blocks: Vec<Block> = drafts
        .iter()
        .enumerate()
        .map(|(i, d)| {
            level_block(
                &format!("r{}b{}", run.run_no, i + 1),
                d,
                material,
                output,
                &levels,
            )
        })
        .collect();
    // A level that kept fewer questions than asked (the others failed their checks twice) says
    // so; before, the worksheet was silently one question short.
    let mut notices: Vec<String> = plan
        .iter()
        .zip(&drafts)
        .filter(|((_, n), d)| d.failure.is_none() && d.items.len() < *n)
        .map(|((b, n), d)| {
            format!(
                "『{}』層次要出 {n} 題，只留下 {} 題（其他題目沒有通過檢查）。可以在那一區按「重新產生」，或自己補。",
                b.label(),
                d.items.len()
            )
        })
        .collect();
    notices.extend(checked.notices);
    let latest = store::latest(db, ws.id).await?;
    let mut content = Content {
        objectives: run.objectives_for(&latest.content.objectives),
        // T1: every version of a 差異化學習單 starts with the shared objectives.
        show_objectives: output == Output::Tiered || chosen.settings.wants(GEN_OBJECTIVES),
        blocks,
        levels: levels.clone(),
        chosen: Some(chosen),
        notices,
    };
    content.normalise().map_err(|e| {
        AppError::Internal(anyhow::anyhow!(
            "worksheet assembled per Bloom level failed validation: {e}"
        ))
    })?;
    let saved = store::save_content(db, ws.id, &content, "generate").await?;
    store::set_generated(db, ws.id).await?;
    log_usage(
        db,
        ws.teacher_id,
        "worksheet_generated",
        json!({
            "worksheet_id": ws.id, "run_no": run.run_no, "version_no": saved.version_no,
            "blocks": content.blocks.len(), "levels": content.levels,
            "output": output.as_str(),
            "bloom": plan.iter().map(|(b, n)| json!([b.as_str(), n])).collect::<Vec<_>>(),
            "failed_blocks": failed, "relabelled": content.notices.len(),
        }),
    )
    .await?;
    Ok(())
}

// ---------------------------------------------------------------- one block again

/// The Bloom level a block was made for, when the worksheet was made per Bloom level: the
/// block's `bloom`, else its fixed title, else the level most of its questions carry.
pub(super) fn level_of_block(input: &RunInput, block: &Block) -> Option<Bloom> {
    let output = input
        .content
        .chosen
        .as_ref()
        .map_or(input.worksheet.settings.output, |c| c.settings.output);
    if !output.by_bloom() {
        return None;
    }
    block
        .bloom
        .or_else(|| bl::level_of_title(&block.title))
        .or_else(|| {
            let mut counts: BTreeMap<Bloom, usize> = BTreeMap::new();
            for b in block.items.iter().filter_map(|i| i.bloom) {
                *counts.entry(b).or_default() += 1;
            }
            counts
                .into_iter()
                .max_by_key(|(b, n)| (*n, std::cmp::Reverse(*b)))
                .map(|(b, _)| b)
        })
}

/// Creates the jobs for regenerating one level's block (or joins the run in progress).
pub(super) async fn start_block(
    state: &AppState,
    input: RunInput,
    old: Block,
    level: Bloom,
) -> AppResult<()> {
    let worksheet_id = input.worksheet.id;
    expire_stale(state, worksheet_id).await?;
    let Some((run_no, ids)) = open_run(
        &state.db,
        worksheet_id,
        input.worksheet.teacher_id,
        &[STEP_BLOOM, STEP_BLOOM_CHECK],
        Some(&old.id),
    )
    .await?
    else {
        return Ok(());
    };
    let state2 = state.clone();
    spawn_run(state, worksheet_id, run_no, async move {
        let hold = ids.get(STEP_BLOOM_CHECK).copied();
        let run = Run::new(&state2, input, run_no, hold).await?;
        regenerate(&run, &ids, old, level).await
    });
    Ok(())
}

async fn regenerate(
    run: &Run,
    jobs: &HashMap<&'static str, i64>,
    old: Block,
    b: Bloom,
) -> AppResult<()> {
    let db = run.db();
    let ws = &run.input.worksheet;
    let content = &run.input.content;
    // The settings of the generation this block belongs to.
    let settings = content
        .chosen
        .as_ref()
        .map_or_else(|| ws.settings.clone(), |c| c.settings.clone());
    let output = settings.output;
    let levels: Vec<Level> = if !content.levels.is_empty() {
        content.levels.clone()
    } else if output == Output::Tiered {
        Level::ALL.to_vec()
    } else if !settings.levels.is_empty() {
        settings.levels.clone()
    } else {
        vec![Level::Standard]
    };
    let count = if old.items.is_empty() {
        bl::plan(&settings)
            .into_iter()
            .find(|(l, _)| *l == b)
            .map_or(1, |(_, n)| n)
    } else {
        old.items.len()
    }
    .clamp(1, *bl::QUESTION_COUNT_RANGE.end() as usize);
    let formats = formats_for(output, b, &settings.answer_formats);
    let others: Vec<String> = content
        .blocks
        .iter()
        .filter(|x| x.id != old.id)
        .flat_map(|x| x.items.iter().map(|i| i.prompt.clone()))
        .collect();
    let result = level_call(
        run,
        jobs[STEP_BLOOM],
        LevelRequest {
            output,
            info: step_for(output, b, false),
            bloom: b,
            count,
            formats: &formats,
            earlier: &others,
            redo: None,
        },
    )
    .await?;
    let context = run.context(&[("Bloom 層次", b.label().to_string())]);
    let Ok((out, interaction_id)) = close(run, jobs[STEP_BLOOM], result, &context).await? else {
        skip(db, jobs.get(STEP_BLOOM_CHECK), SKIPPED).await?;
        return Ok(());
    };
    let mut drafts = vec![LevelDraft::from_output(b, out)];
    let position = content
        .blocks
        .iter()
        .position(|x| x.id == old.id)
        .unwrap_or(0);
    let before: usize = content.blocks[..position]
        .iter()
        .map(|x| x.items.len())
        .sum();
    let checked = judge_and_fix(
        run,
        jobs[STEP_BLOOM_CHECK],
        output,
        &mut drafts,
        &[before + 1],
        &settings.answer_formats,
        &others,
    )
    .await?;
    let context = run.context(&[("要檢查的題目", format!("{} 題", drafts[0].items.len()))]);
    mark_held(
        db,
        jobs[STEP_BLOOM_CHECK],
        checked.outcome,
        checked.message.as_deref(),
        checked.interaction_id,
        &context,
    )
    .await?;
    let block = level_block(&old.id, &drafts[0], &ws.material, output, &levels);
    replace_block(run, &old, block, interaction_id, checked.notices).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chosen_reports_the_distribution() {
        let s = Settings {
            output: Output::Comprehension,
            ..Settings::default()
        };
        let plan = bl::plan(&s);
        let c = chosen_for(&s, &plan);
        assert_eq!(
            c.reasons[0],
            "Bloom：記憶 2、理解 2、應用 1、分析 1、評鑑 1、創造 1"
        );
        assert_eq!(c.item_budget, 8);
        assert_eq!(c.settings.levels, [Level::Standard]);
        assert_eq!(c.settings.question_count, Some(8));
        assert!(c.settings.normalised().is_ok());
        let t = Settings {
            output: Output::Tiered,
            ..Settings::default()
        };
        let c = chosen_for(&t, &bl::plan(&t));
        assert_eq!(c.settings.levels, Level::ALL);
        assert!(c.reasons.iter().any(|r| r == tiered::TIERED_REASON));
        assert_eq!(first_numbers(&[], 1), Vec::<usize>::new());
    }
}
