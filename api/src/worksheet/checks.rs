//! Automatic checks of the four worksheet steps, ported from
//! research/sop-validation/chain/checks.py (docs/LLM_VALIDATION.md section 5).
//!
//! Every `evaluate_*` does two things at once: it reports which checks failed (a failure sends
//! the output back to the model once, with the problems listed), and it returns the output with
//! the offending parts taken out. After the repair call the cleaned output is what the teacher
//! gets, so one bad item never costs the whole step; only an output with nothing usable left is
//! blocked.

use std::collections::{BTreeMap, HashSet};

use super::bloom::{Bloom, RawBloomCheck, RawBloomLevel, HINT_CHARS, REASON_CHARS};
use super::schemas::{
    RawBlock, RawExit, RawExitItem, RawItem, RawScaffoldSet, RawScaffolds, RawTasks, RawWarmup,
    TARGET_WARMUP, TIER_CHALLENGE, TIER_CORE, TIER_SUPPORT,
};
use super::{
    info_function_from_label, student_tasks, ANSWER_FORMATS, EXIT_PURPOSES, FMT_BLANK, FMT_CHOICE,
    FMT_MATCHING, FMT_SHORT, FMT_TRUE_FALSE, INSTRUCTIONAL_KINDS, PROCEDURAL_KINDS, VERBAL_KINDS,
};
use crate::checks::CheckResult;
use crate::routes::clip_chars;
use crate::sop::checks::{clean_markup, english_only, is_verbatim, locate_verbatim, norm};
use crate::syllables;

pub struct Eval<T> {
    /// The output with everything that failed taken out; `None` when nothing usable is left.
    pub output: Option<T>,
    pub results: Vec<CheckResult>,
    /// What to tell the model in the repair call.
    pub problems: Vec<String>,
    /// What the code changed in `output` (logged in `check_results.repairs`).
    pub repairs: Vec<String>,
}

impl<T> Eval<T> {
    pub fn needs_repair(&self) -> bool {
        self.results.iter().any(|r| !r.passed)
    }
}

/// Collects failures per check, in the order the checks are declared.
struct Tally {
    names: Vec<&'static str>,
    failures: BTreeMap<&'static str, Vec<String>>,
    /// Problems the code fixed completely: reported, but no reason for a repair call.
    fixed: BTreeMap<&'static str, Vec<String>>,
    problems: Vec<String>,
    repairs: Vec<String>,
}

fn joined(list: &[String]) -> String {
    list.iter()
        .take(5)
        .map(|s| clip_chars(s, 160))
        .collect::<Vec<_>>()
        .join("；")
}

impl Tally {
    fn new(names: &[&'static str]) -> Self {
        Self {
            names: names.to_vec(),
            failures: BTreeMap::new(),
            fixed: BTreeMap::new(),
            problems: Vec::new(),
            repairs: Vec::new(),
        }
    }

    /// Records a failed check, what to tell the model and what the code does about it.
    fn fail(&mut self, name: &'static str, problem: String, repair: &str) {
        debug_assert!(self.names.contains(&name), "undeclared check {name}");
        self.failures.entry(name).or_default().push(problem.clone());
        if !repair.is_empty() {
            self.repairs
                .push(format!("{repair}：{}", clip_chars(&problem, 160)));
        }
        self.problems.push(problem);
    }

    /// Records a problem the code has fully repaired (`repair` says how). The check still
    /// passes: on its own it never costs a repair call, but if one is made for another reason
    /// the model is told about this too.
    fn fixed(&mut self, name: &'static str, problem: String, repair: &str) {
        debug_assert!(self.names.contains(&name), "undeclared check {name}");
        let note = format!("{repair}：{}", clip_chars(&problem, 160));
        self.fixed.entry(name).or_default().push(note.clone());
        self.repairs.push(note);
        self.problems.push(problem);
    }

    fn finish<T>(self, output: Option<T>) -> Eval<T> {
        let results = self
            .names
            .iter()
            .map(
                |name| match (self.failures.get(name), self.fixed.get(name)) {
                    (Some(list), _) => CheckResult::fail(name, joined(list)),
                    (None, Some(list)) => CheckResult::repaired(name, joined(list)),
                    (None, None) => CheckResult::pass(name),
                },
            )
            .collect();
        Eval {
            output,
            results,
            problems: self.problems,
            repairs: self.repairs,
        }
    }
}

fn word_count(s: &str) -> usize {
    s.split_whitespace().count()
}

fn clean(s: &str) -> String {
    clean_markup(s)
}

fn clean_all(list: &[String]) -> Vec<String> {
    list.iter()
        .map(|s| clean(s))
        .filter(|s| !s.is_empty())
        .collect()
}

/// Shorter pieces of a spliced quote are fragments, not evidence.
pub const MIN_EVIDENCE_WORDS: usize = 5;

/// The longest piece of a quote that is word for word in `passage` and at least five words
/// long ([`MIN_EVIDENCE_WORDS`]). The quote is cut where the model joined pieces: at "..." / "…"
/// and at sentence ends.
pub fn verbatim_piece(quote: &str, passage: &str) -> Option<String> {
    let mut pieces: Vec<String> = Vec::new();
    for part in quote.replace('…', "...").split("...") {
        // Where sentences may end; every run of consecutive sentences is a candidate (so an
        // abbreviation such as "Mrs." does not cut a sentence in two).
        let mut cuts = vec![0];
        let chars: Vec<(usize, char)> = part.char_indices().collect();
        for (k, (i, c)) in chars.iter().enumerate() {
            let next_is_space = chars.get(k + 1).is_none_or(|(_, n)| n.is_whitespace());
            if matches!(c, '.' | '!' | '?' | ';') && next_is_space {
                cuts.push(i + c.len_utf8());
            }
        }
        if cuts.last() != Some(&part.len()) {
            cuts.push(part.len());
        }
        for a in 0..cuts.len() {
            for b in a + 1..cuts.len() {
                pieces.push(part[cuts[a]..cuts[b]].to_string());
            }
        }
    }
    pieces
        .into_iter()
        .map(|p| {
            p.trim()
                .trim_matches(|c: char| c == '"' || c == '\'' || c == '“' || c == '”')
                .trim()
                .to_string()
        })
        .filter(|p| word_count(p) >= MIN_EVIDENCE_WORDS)
        .filter_map(|p| locate_verbatim(&p, passage))
        .max_by_key(|p| p.chars().count())
}

/// An answer of four words or more that appears word for word in `text`.
pub(crate) fn contains_answer(text: &str, answer: &str) -> bool {
    let a = norm(answer).to_lowercase();
    word_count(&a) >= 4 && norm(text).to_lowercase().contains(&a)
}

// ---------------------------------------------------------------- (c) tasks

pub struct TasksCtx<'a> {
    pub paragraphs: &'a [String],
    pub objective_ids: &'a [String],
    pub item_budget: usize,
    /// A single-block regeneration: the paragraph numbers the one block has to cover.
    pub only: Option<&'a [i64]>,
}

const TASK_CHECKS: [&str; 9] = [
    "labels_from_spec_sets",
    "english_only",
    "evidence_verbatim",
    "items_wellformed",
    "answer_not_in_prompt",
    "objective_refs_valid",
    "all_paragraphs_covered",
    "blocks_in_paragraph_order",
    "at_least_3_info_functions",
];
const TASK_CHECKS_WITH_BUDGET: [&str; 10] = [
    "labels_from_spec_sets",
    "english_only",
    "evidence_verbatim",
    "items_wellformed",
    "answer_not_in_prompt",
    "objective_refs_valid",
    "all_paragraphs_covered",
    "blocks_in_paragraph_order",
    "at_least_3_info_functions",
    "item_count_within_budget",
];

static OPTION_LETTER: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"^\s*(?:\(([A-Ha-h])\)|([A-Ha-h])[.)．、:])\s*(.*)$").expect("valid regex")
});

/// Puts a choice item in the form the checks expect, before checking it. The model often writes
/// options as 「A. …」 and the answer as "B" (real-model run: an exit ticket failed this way):
/// letters are stripped when every option carries them in order, and an answer that is a letter,
/// or an option with its letter, or an option in another case, becomes that option exactly.
pub(crate) fn normalize_choice(options: &mut [String], answer: &mut String) {
    let lettered: Vec<Option<(char, String)>> = options
        .iter()
        .map(|o| {
            OPTION_LETTER.captures(o).map(|c| {
                let l = c
                    .get(1)
                    .or(c.get(2))
                    .map_or('?', |m| m.as_str().chars().next().unwrap_or('?'));
                (l.to_ascii_uppercase(), c[3].trim().to_string())
            })
        })
        .collect();
    let in_order = !options.is_empty()
        && lettered.iter().enumerate().all(|(i, l)| {
            l.as_ref()
                .is_some_and(|(c, rest)| *c == (b'A' + i as u8) as char && !rest.is_empty())
        });
    if in_order {
        for (o, l) in options.iter_mut().zip(lettered) {
            if let Some((_, rest)) = l {
                *o = rest;
            }
        }
    }
    let a = answer.trim();
    if options.iter().any(|o| o.trim() == a) {
        return;
    }
    let bare = a.trim_matches(|c: char| c == '(' || c == ')' || c == '.' || c == ' ');
    if bare.len() == 1 {
        if let Some(c) = bare.chars().next().filter(char::is_ascii_alphabetic) {
            let idx = (c.to_ascii_uppercase() as u8).wrapping_sub(b'A') as usize;
            if let Some(o) = options.get(idx) {
                *answer = o.clone();
                return;
            }
        }
    }
    let text = OPTION_LETTER
        .captures(a)
        .map_or(a.to_string(), |c| c[3].trim().to_string());
    let key = |s: &str| norm(s).to_lowercase().trim_end_matches('.').to_string();
    if let Some(o) = options.iter().find(|o| key(o) == key(&text)) {
        *answer = o.clone();
    }
}

fn item_wellformed(it: &RawItem) -> Result<(), &'static str> {
    match it.answer_format.as_str() {
        f if f == FMT_CHOICE => {
            let distinct: HashSet<&str> = it.options.iter().map(|o| o.trim()).collect();
            if it.options.len() != 4 || distinct.len() != 4 {
                Err("選擇題要有 4 個不重複的選項")
            } else if !it.options.iter().any(|o| o.trim() == it.answer_en.trim()) {
                Err("選擇題的 answer_en 必須與其中一個選項完全相同")
            } else {
                Ok(())
            }
        }
        f if f == FMT_TRUE_FALSE => {
            if matches!(it.answer_en.trim(), "True" | "False") {
                Ok(())
            } else {
                Err("是非題的 answer_en 必須是 True 或 False")
            }
        }
        f if f == FMT_BLANK => {
            if it.prompt_en.contains("___") {
                Ok(())
            } else {
                Err("填空題的 prompt_en 要用 ______ 表示空格")
            }
        }
        f if f == FMT_MATCHING => {
            if it.options.len() >= 3 {
                Ok(())
            } else {
                Err("配對題的 options 至少要 3 個")
            }
        }
        _ => Ok(()),
    }
}

fn paragraph_label(nos: &[i64]) -> String {
    match nos {
        [] => "Whole text".to_string(),
        [one] => format!("Paragraph {one}"),
        [first, .., last] if (*last - *first) as usize + 1 == nos.len() => {
            format!("Paragraphs {first}–{last}")
        }
        many => format!(
            "Paragraphs {}",
            many.iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

pub fn default_block_title(nos: &[i64], function_label: &str) -> String {
    let verb = match function_label {
        "發現資訊" => "Find",
        "整理資訊" => "Organize",
        "分析資訊" => "Analyze",
        "比較資訊" => "Compare",
        "創造資訊" => "Create",
        _ => "Read and answer",
    };
    format!("{}: {verb}", paragraph_label(nos))
}

pub fn evaluate_tasks(raw: &RawTasks, ctx: &TasksCtx<'_>) -> Eval<Vec<RawBlock>> {
    let n = ctx.paragraphs.len() as i64;
    let passage = ctx.paragraphs.join("\n");
    let all_tasks = student_tasks();
    let mut t = Tally::new(if ctx.only.is_some() {
        &TASK_CHECKS
    } else {
        &TASK_CHECKS_WITH_BUDGET
    });
    let mut blocks: Vec<RawBlock> = Vec::new();

    for (bi, b) in raw.blocks.iter().enumerate() {
        let name = if b.block_id.trim().is_empty() {
            format!("B{}", bi + 1)
        } else {
            b.block_id.trim().to_string()
        };
        if info_function_from_label(&b.info_function).is_none() {
            t.fail(
                "labels_from_spec_sets",
                format!(
                    "{name} 的 info_function「{}」不在允許的清單內。",
                    b.info_function
                ),
                "整個區塊拿掉",
            );
            continue;
        }
        let mut block = b.clone();
        block.title_en = clean(&b.title_en);
        block.instructions_en = clean(&b.instructions_en);
        block.design_reason_zh = clean(&b.design_reason_zh);
        block.student_tasks = b
            .student_tasks
            .iter()
            .filter(|label| {
                let ok = all_tasks.contains(&label.as_str());
                if !ok {
                    t.fail(
                        "labels_from_spec_sets",
                        format!("{name} 的 student_tasks「{label}」不在允許的清單內。"),
                        "拿掉這個標籤",
                    );
                }
                ok
            })
            .cloned()
            .collect();
        block.student_tasks.dedup();
        block.paragraph_nos = match ctx.only {
            Some(only) => only.to_vec(),
            None => {
                let mut nos: Vec<i64> = b
                    .paragraph_nos
                    .iter()
                    .copied()
                    .filter(|p| (1..=n).contains(p))
                    .collect();
                nos.sort_unstable();
                nos.dedup();
                nos
            }
        };
        if block.paragraph_nos.is_empty() {
            t.fail(
                "all_paragraphs_covered",
                format!("{name} 的 paragraph_nos 沒有對應到教材的段落（1 到 {n}）。"),
                "整個區塊拿掉",
            );
            continue;
        }
        if !english_only(&block.title_en) || block.title_en.is_empty() {
            if !block.title_en.is_empty() {
                t.fail(
                    "english_only",
                    format!("{name} 的 title_en 含有中文，必須用英文。"),
                    "改用預設標題",
                );
            }
            block.title_en = default_block_title(&block.paragraph_nos, &block.info_function);
        }
        if !english_only(&block.instructions_en) {
            t.fail(
                "english_only",
                format!("{name} 的 instructions_en 含有中文，必須用英文。"),
                "清空作答指示",
            );
            block.instructions_en = String::new();
        }
        let before = block.objective_refs.len();
        block
            .objective_refs
            .retain(|r| ctx.objective_ids.contains(r));
        if block.objective_refs.len() != before {
            t.fail(
                "objective_refs_valid",
                format!(
                    "{name} 的 objective_refs 只能使用這些代號：{}。",
                    if ctx.objective_ids.is_empty() {
                        "（沒有提供目標，請給空陣列）".to_string()
                    } else {
                        ctx.objective_ids.join("、")
                    }
                ),
                "拿掉不存在的目標代號",
            );
        }

        let mut items: Vec<RawItem> = Vec::new();
        for (ii, it) in b.items.iter().enumerate() {
            let iname = format!("{name} 第 {} 題", ii + 1);
            let mut item = it.clone();
            item.prompt_en = clean(&it.prompt_en);
            item.answer_en = clean(&it.answer_en);
            item.options = clean_all(&it.options);
            // A label outside the fixed set (a provider that ignores the schema) is dropped.
            item.bloom = it
                .bloom
                .as_deref()
                .and_then(Bloom::parse)
                .map(|b| b.as_str().to_string());
            if !ANSWER_FORMATS.contains(&item.answer_format.as_str()) {
                t.fail(
                    "labels_from_spec_sets",
                    format!(
                        "{iname} 的 answer_format「{}」不在允許的清單內。",
                        it.answer_format
                    ),
                    "拿掉這一題",
                );
                continue;
            }
            let texts = [&item.prompt_en, &item.answer_en];
            if item.prompt_en.is_empty()
                || !texts.iter().all(|s| english_only(s))
                || !item.options.iter().all(|s| english_only(s))
            {
                t.fail(
                    "english_only",
                    format!(
                        "{iname} 的題目、選項或答案含有中文（或題目是空的）；學生看得到的欄位只能用英文。"
                    ),
                    "拿掉這一題",
                );
                continue;
            }
            if item.answer_format == FMT_CHOICE {
                normalize_choice(&mut item.options, &mut item.answer_en);
            }
            if let Err(why) = item_wellformed(&item) {
                t.fail(
                    "items_wellformed",
                    format!("{iname}（{}）：{why}。", item.answer_format),
                    "拿掉這一題",
                );
                continue;
            }
            if contains_answer(&item.prompt_en, &item.answer_en) {
                t.fail(
                    "answer_not_in_prompt",
                    format!("{iname} 的題幹直接寫出了答案。"),
                    "拿掉這一題",
                );
                continue;
            }
            // Evidence is only a note for the teacher: one that is not verbatim is cut down to
            // its longest verbatim piece, or dropped, in code (as the SOP analysis drops it).
            // It never costs the step a repair call on its own.
            item.evidence = match it.evidence.as_deref().map(clean) {
                Some(e) if e.is_empty() => None,
                Some(e) if is_verbatim(&e, &passage) => locate_verbatim(&e, &passage),
                Some(e) => {
                    let piece = verbatim_piece(&e, &passage);
                    t.fixed(
                        "evidence_verbatim",
                        format!(
                            "{iname} 的 evidence「{}」不是教材的連續片段。請逐字複製一段連續文字，不可用「...」省略或拼接；找不到就填 null。",
                            clip_chars(&e, 120)
                        ),
                        if piece.is_some() {
                            "只留下其中逐字的一段"
                        } else {
                            "清空這一題的依據"
                        },
                    );
                    piece
                }
                None => None,
            };
            if ![TIER_CORE, TIER_SUPPORT, TIER_CHALLENGE].contains(&item.tier.as_str()) {
                item.tier = TIER_CORE.to_string();
            }
            items.push(item);
        }
        if items.is_empty() {
            t.repairs
                .push(format!("{name} 沒有剩下可用的題目，整個區塊拿掉"));
            continue;
        }
        // Every version shows the core items: a block needs at least one.
        if !items.iter().any(|i| i.tier == TIER_CORE) {
            items[0].tier = TIER_CORE.to_string();
        }
        for tier in [TIER_SUPPORT, TIER_CHALLENGE] {
            let mut seen = false;
            items.retain(|i| {
                if i.tier != tier {
                    return true;
                }
                let keep = !seen;
                seen = true;
                keep
            });
        }
        block.items = items;
        blocks.push(block);
    }

    if ctx.only.is_some() {
        blocks.truncate(1);
    } else {
        let covered: HashSet<i64> = blocks
            .iter()
            .flat_map(|b| b.paragraph_nos.clone())
            .collect();
        let missing: Vec<String> = (1..=n)
            .filter(|p| !covered.contains(p))
            .map(|p| format!("[P{p}]"))
            .collect();
        if !missing.is_empty() && !blocks.is_empty() {
            t.fail(
                "all_paragraphs_covered",
                format!(
                    "這些段落沒有被任何區塊涵蓋：{}。每一段都要有區塊。",
                    missing.join("、")
                ),
                "",
            );
        }
        // A closing whole-text block (first and last paragraph) may follow the others.
        let closing = blocks
            .last()
            .is_some_and(|b| n > 1 && b.paragraph_nos.contains(&1) && b.paragraph_nos.contains(&n));
        let ordered = if closing {
            blocks.len() - 1
        } else {
            blocks.len()
        };
        let firsts: Vec<i64> = blocks[..ordered]
            .iter()
            .map(|b| b.paragraph_nos[0])
            .collect();
        if firsts.windows(2).any(|w| w[0] > w[1]) {
            t.fail(
                "blocks_in_paragraph_order",
                "區塊沒有依段落順序排列。".to_string(),
                "依段落順序重新排列區塊",
            );
            blocks[..ordered].sort_by_key(|b| b.paragraph_nos[0]);
        }
        let functions: HashSet<&str> = blocks.iter().map(|b| b.info_function.as_str()).collect();
        if blocks.len() >= 3 && functions.len() < 3 {
            t.fail(
                "at_least_3_info_functions",
                format!(
                    "整份設計只用了 {} 種資訊處理功能；三個以上區塊時至少要 3 種。",
                    functions.len()
                ),
                "",
            );
        }
        let limit = (ctx.item_budget * 3).div_ceil(2).max(blocks.len());
        let core = |blocks: &[RawBlock]| -> usize {
            blocks
                .iter()
                .flat_map(|b| &b.items)
                .filter(|i| i.tier == TIER_CORE)
                .count()
        };
        let total = core(&blocks);
        if total > limit {
            t.fail(
                "item_count_within_budget",
                format!(
                    "核心題（tier = core）共 {total} 題，超過這份學習單的時間能做完的量；請減少到大約 {} 題。",
                    ctx.item_budget
                ),
                "從題數最多的區塊末端刪去多的核心題",
            );
            while core(&blocks) > limit {
                let Some(fullest) = blocks
                    .iter_mut()
                    .filter(|b| b.items.iter().filter(|i| i.tier == TIER_CORE).count() > 1)
                    .max_by_key(|b| b.items.iter().filter(|i| i.tier == TIER_CORE).count())
                else {
                    break;
                };
                if let Some(pos) = fullest.items.iter().rposition(|i| i.tier == TIER_CORE) {
                    fullest.items.remove(pos);
                }
            }
        }
    }

    for (bi, b) in blocks.iter_mut().enumerate() {
        b.block_id = format!("B{}", bi + 1);
        for (ii, it) in b.items.iter_mut().enumerate() {
            it.item_id = format!("B{}-{}", bi + 1, ii + 1);
        }
    }
    let output = if blocks.is_empty() {
        None
    } else {
        Some(blocks)
    };
    t.finish(output)
}

/// Paragraph numbers (from 1) that no block covers.
pub fn uncovered_paragraphs(blocks: &[RawBlock], paragraphs: usize) -> Vec<i64> {
    let covered: HashSet<i64> = blocks
        .iter()
        .flat_map(|b| b.paragraph_nos.clone())
        .collect();
    (1..=paragraphs as i64)
        .filter(|p| !covered.contains(p))
        .collect()
}

// ---------------------------------------------------------------- (d) warm-up

/// A word the model may pick as a key word: found in the material by the code, with its level
/// from the word list (never from the model).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub word: String,
    pub level: Option<u8>,
    pub paragraph_no: i64,
    /// The sentence of the material the word appears in.
    pub sentence: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CleanActivity {
    pub title: String,
    pub instructions: String,
    pub prompts: Vec<String>,
    pub student_tasks: Vec<String>,
    pub answer_format: Option<String>,
    pub purpose_zh: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CleanWord {
    pub word: String,
    pub level: Option<u8>,
    pub definition: String,
    pub context: String,
    pub paragraph_no: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CleanWarmup {
    pub activities: Vec<CleanActivity>,
    pub words: Vec<CleanWord>,
}

pub struct WarmupCtx<'a> {
    pub paragraphs: &'a [String],
    pub candidates: &'a [Candidate],
    pub vocab_count: usize,
}

const WARMUP_CHECKS: [&str; 5] = [
    "background_1_2_activities",
    "english_only",
    "labels_from_spec_sets",
    "vocab_only_from_candidates",
    "context_sentence_verbatim",
];

pub fn evaluate_warmup(raw: &RawWarmup, ctx: &WarmupCtx<'_>) -> Eval<CleanWarmup> {
    let mut t = Tally::new(&WARMUP_CHECKS);
    let all_tasks = student_tasks();
    let mut activities = Vec::new();
    for (i, a) in raw.background_activities.iter().enumerate() {
        let name = format!("background_activities 第 {} 個", i + 1);
        let title = clean(&a.title_en);
        let instructions = clean(&a.instructions_en);
        let prompts = clean_all(&a.prompts_en);
        let english = english_only(&title)
            && english_only(&instructions)
            && prompts.iter().all(|p| english_only(p));
        if !english || (prompts.is_empty() && instructions.is_empty()) {
            t.fail(
                "english_only",
                format!("{name} 含有中文或沒有內容；題目與指示只能用英文。"),
                "拿掉這個活動",
            );
            continue;
        }
        let student_tasks: Vec<String> = a
            .student_tasks
            .iter()
            .filter(|l| all_tasks.contains(&l.as_str()))
            .cloned()
            .collect();
        let format_ok = ANSWER_FORMATS.contains(&a.answer_format.as_str());
        if student_tasks.len() != a.student_tasks.len() || !format_ok {
            t.fail(
                "labels_from_spec_sets",
                format!("{name} 的 student_tasks 或 answer_format 用了清單以外的標籤。"),
                "拿掉清單以外的標籤",
            );
        }
        // A warm-up activity has prompts but no options: as 選擇題 or 配對題 it would print a
        // question with nothing to choose from (real-model run), so it becomes 簡答題.
        let answer_format = format_ok.then(|| {
            if a.answer_format == FMT_CHOICE || a.answer_format == FMT_MATCHING {
                t.fixed(
                    "labels_from_spec_sets",
                    format!(
                        "{name} 的 answer_format 是「{}」，但暖身活動沒有選項。",
                        a.answer_format
                    ),
                    "改成簡答題",
                );
                FMT_SHORT.to_string()
            } else {
                a.answer_format.clone()
            }
        });
        activities.push(CleanActivity {
            title,
            instructions,
            prompts,
            student_tasks,
            answer_format,
            purpose_zh: clean(&a.purpose_zh),
        });
    }
    if activities.is_empty() {
        t.fail(
            "background_1_2_activities",
            "background_activities 需要 1–2 個可用的活動。".to_string(),
            "",
        );
    }
    activities.truncate(2);

    let mut words: Vec<CleanWord> = Vec::new();
    for v in &raw.key_vocabulary {
        let word = v.word.trim();
        let Some(candidate) = ctx
            .candidates
            .iter()
            .find(|c| c.word.eq_ignore_ascii_case(word))
        else {
            t.fail(
                "vocab_only_from_candidates",
                format!("關鍵字「{word}」不在候選字彙清單裡；只能從清單挑，寫法要完全相同。"),
                "拿掉這個字",
            );
            continue;
        };
        if words.iter().any(|w| w.word == candidate.word) {
            continue;
        }
        let definition = clean(&v.student_friendly_definition_en);
        if !english_only(&definition) {
            t.fail(
                "english_only",
                format!("關鍵字「{word}」的解釋含有中文，必須用簡單的英文。"),
                "清空這個字的解釋",
            );
        }
        let sentence = clean(&v.context_sentence);
        let paragraph = usize::try_from(v.paragraph_no)
            .ok()
            .and_then(|p| p.checked_sub(1))
            .and_then(|p| ctx.paragraphs.get(p));
        let verbatim = paragraph.is_some_and(|p| is_verbatim(&sentence, p));
        let (context, paragraph_no) = if verbatim {
            (sentence, Some(v.paragraph_no))
        } else {
            // The code found the word's sentence in the material itself, so this is fully
            // repaired here (gemma drops the quotation marks of dialogue, for one).
            t.fixed(
                "context_sentence_verbatim",
                format!(
                    "關鍵字「{word}」的 context_sentence 不是 [P{}] 的原句；請逐字複製含有這個字的句子。",
                    v.paragraph_no
                ),
                "改用程式在教材中找到的句子",
            );
            (candidate.sentence.clone(), Some(candidate.paragraph_no))
        };
        words.push(CleanWord {
            word: candidate.word.clone(),
            level: candidate.level,
            definition: if english_only(&definition) {
                definition
            } else {
                String::new()
            },
            context,
            paragraph_no,
        });
    }
    words.truncate(ctx.vocab_count.max(1));
    let output = if activities.is_empty() && words.is_empty() {
        None
    } else {
        Some(CleanWarmup { activities, words })
    };
    t.finish(output)
}

// ---------------------------------------------------------------- (e) scaffolds

pub struct ScaffoldsCtx<'a> {
    /// `warm_up` and the block ids scaffolds were asked for.
    pub targets: &'a [String],
    /// Reference answers of the main tasks: none of them may appear in a scaffold.
    pub answers: &'a [String],
}

const SCAFFOLD_CHECKS: [&str; 5] = [
    "targets_match_blocks_and_warmup",
    "each_target_has_procedural",
    "kinds_from_spec_sets",
    "english_only",
    "no_answer_leak",
];

/// `text` without its parenthesised Chinese glosses ("routine (常規)", "routine（常規）"), when
/// that leaves English only; `None` when there is Chinese elsewhere too.
pub fn without_cjk_glosses(text: &str) -> Option<String> {
    let mut out = String::new();
    let mut rest = text;
    while let Some(open) = rest.find(['(', '（']) {
        let after = &rest[open..];
        let close = after.find([')', '）'])?;
        let inner = &after[after.chars().next()?.len_utf8()..close];
        out.push_str(&rest[..open]);
        if english_only(inner) {
            out.push_str(&after[..close + after[close..].chars().next()?.len_utf8()]);
        } else {
            // Drop the space before the gloss too.
            while out.ends_with(' ') {
                out.pop();
            }
        }
        rest = &after[close + after[close..].chars().next()?.len_utf8()..];
    }
    out.push_str(rest);
    let out = out.trim().to_string();
    (!out.is_empty() && english_only(&out)).then_some(out)
}

/// A target id the way the model may spell it: "warm_up", "Warm-up" and "warmup" are one,
/// as are "B1", "b1" and "[B1]".
fn target_key(s: &str) -> String {
    s.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// The requested target the model meant, if any.
pub fn canonical_target(written: &str, targets: &[String]) -> Option<String> {
    let key = target_key(written);
    targets
        .iter()
        .find(|t| !key.is_empty() && target_key(t) == key)
        .cloned()
}

pub fn evaluate_scaffolds(raw: &RawScaffolds, ctx: &ScaffoldsCtx<'_>) -> Eval<Vec<RawScaffoldSet>> {
    let mut t = Tally::new(&SCAFFOLD_CHECKS);
    let mut sets: Vec<RawScaffoldSet> = Vec::new();
    for s in &raw.scaffolds {
        let written = s.target.trim().to_string();
        let target = match canonical_target(&written, ctx.targets) {
            Some(target) => target,
            // The prompt names the warm-up's target even when the warm-up failed and was not
            // sent: a set for it is simply not needed.
            None if target_key(&written) == target_key(TARGET_WARMUP) => {
                t.fixed(
                    "targets_match_blocks_and_warmup",
                    format!("這次沒有暖身，不需要 target「{written}」。"),
                    "拿掉這一筆",
                );
                continue;
            }
            None => {
                t.fail(
                    "targets_match_blocks_and_warmup",
                    format!(
                        "target「{written}」不在要設計的區塊裡；只能用：{}。",
                        ctx.targets.join("、")
                    ),
                    "拿掉這一筆",
                );
                continue;
            }
        };
        if sets.iter().any(|x| x.target == target) {
            t.fixed(
                "targets_match_blocks_and_warmup",
                format!("target「{target}」重複了。"),
                "只留第一筆",
            );
            continue;
        }
        let mut support = |list: &[super::schemas::RawSupport], kinds: &[&str], what: &str| {
            let mut out = Vec::new();
            for sup in list {
                let mut content = clean(&sup.content_en);
                // A word bank with Chinese glosses, "behavior (行為), routine (常規)": the
                // glosses go, the English stays.
                if !english_only(&content) {
                    if let Some(english) = without_cjk_glosses(&content) {
                        t.fixed(
                            "english_only",
                            format!(
                                "{target} 的{what}「{}」在括號裡加了中文；鷹架只能用英文。",
                                sup.kind
                            ),
                            "拿掉括號裡的中文",
                        );
                        content = english;
                    }
                }
                if !kinds.contains(&sup.kind.as_str()) {
                    t.fail(
                        "kinds_from_spec_sets",
                        format!("{target} 的{what}種類「{}」不在允許的清單內。", sup.kind),
                        "拿掉這個鷹架",
                    );
                } else if content.is_empty() || !english_only(&content) {
                    t.fail(
                        "english_only",
                        format!(
                            "{target} 的{what}「{}」含有中文或沒有內容；鷹架只能用英文。",
                            sup.kind
                        ),
                        "拿掉這個鷹架",
                    );
                } else if ctx.answers.iter().any(|a| contains_answer(&content, a)) {
                    t.fail(
                        "no_answer_leak",
                        format!(
                            "{target} 的{what}「{}」直接寫出了參考答案；鷹架不可以給答案。",
                            sup.kind
                        ),
                        "拿掉這個鷹架",
                    );
                } else {
                    out.push(super::schemas::RawSupport {
                        kind: sup.kind.clone(),
                        content_en: content,
                    });
                }
            }
            out.truncate(2);
            out
        };
        let verbal = support(&s.verbal, &VERBAL_KINDS, "語言鷹架");
        let instructional = support(&s.instructional, &INSTRUCTIONAL_KINDS, "教學鷹架");
        let mut procedural = Vec::new();
        for p in &s.procedural {
            let mut steps = clean_all(&p.steps_en);
            steps.truncate(5);
            if !PROCEDURAL_KINDS.contains(&p.kind.as_str()) {
                t.fail(
                    "kinds_from_spec_sets",
                    format!("{target} 的程序鷹架種類「{}」不在允許的清單內。", p.kind),
                    "拿掉這個鷹架",
                );
            } else if steps.is_empty() || !steps.iter().all(|s| english_only(s)) {
                t.fail(
                    "english_only",
                    format!("{target} 的程序鷹架含有中文或沒有步驟；steps_en 只能用英文。"),
                    "拿掉這個鷹架",
                );
            } else if steps
                .iter()
                .any(|s| ctx.answers.iter().any(|a| contains_answer(s, a)))
            {
                t.fail(
                    "no_answer_leak",
                    format!("{target} 的程序鷹架步驟裡寫出了參考答案。"),
                    "拿掉這個鷹架",
                );
            } else {
                procedural.push(super::schemas::RawProcedure {
                    kind: p.kind.clone(),
                    steps_en: steps,
                });
            }
        }
        procedural.truncate(2);
        if procedural.is_empty() {
            t.fail(
                "each_target_has_procedural",
                format!(
                    "{target} 沒有可用的程序鷹架；每個區塊至少要 1 個，steps_en 列出 2–5 個步驟。"
                ),
                "由程式產生分步指示",
            );
        }
        sets.push(RawScaffoldSet {
            target,
            verbal,
            instructional,
            procedural,
        });
    }
    let missing: Vec<&str> = ctx
        .targets
        .iter()
        .filter(|target| !sets.iter().any(|s| &s.target == *target))
        .map(String::as_str)
        .collect();
    if !missing.is_empty() {
        t.fail(
            "targets_match_blocks_and_warmup",
            format!(
                "這些區塊沒有鷹架：{}。每個 target 都要有一筆。",
                missing.join("、")
            ),
            "缺的區塊由程式產生分步指示",
        );
    }
    let output = if sets.is_empty() { None } else { Some(sets) };
    t.finish(output)
}

// ---------------------------------------------------------------- (f) exit ticket

pub struct ExitCtx<'a> {
    pub objective_ids: &'a [String],
    /// Prompts of the main tasks: the exit ticket must not repeat them.
    pub task_prompts: &'a [String],
}

const EXIT_CHECKS: [&str; 8] = [
    "labels_from_spec_sets",
    "english_only",
    "items_wellformed",
    "not_a_repeat_of_main_task",
    "item_count_2_4",
    "has_formative_item",
    "has_reflection_or_question_item",
    "formative_has_sample_answer",
];

pub fn evaluate_exit(raw: &RawExit, ctx: &ExitCtx<'_>) -> Eval<Vec<RawExitItem>> {
    let mut t = Tally::new(&EXIT_CHECKS);
    let all_tasks = student_tasks();
    let prompts: HashSet<String> = ctx
        .task_prompts
        .iter()
        .map(|p| norm(p).to_lowercase())
        .collect();
    let formative = EXIT_PURPOSES[0].1;
    let mut items: Vec<RawExitItem> = Vec::new();
    for (i, it) in raw.items.iter().enumerate() {
        let name = format!("第 {} 題", i + 1);
        if !EXIT_PURPOSES.iter().any(|(_, l)| *l == it.purpose)
            || !ANSWER_FORMATS.contains(&it.answer_format.as_str())
        {
            t.fail(
                "labels_from_spec_sets",
                format!("{name} 的 purpose 或 answer_format 不在允許的清單內。"),
                "拿掉這一題",
            );
            continue;
        }
        let mut item = it.clone();
        item.prompt_en = clean(&it.prompt_en);
        item.options = clean_all(&it.options);
        item.sample_answer_en = it
            .sample_answer_en
            .as_deref()
            .map(clean)
            .filter(|s| !s.is_empty());
        if item.answer_format == FMT_CHOICE {
            let mut answer = item.sample_answer_en.clone().unwrap_or_default();
            normalize_choice(&mut item.options, &mut answer);
            if item.sample_answer_en.is_some() {
                item.sample_answer_en = Some(answer);
            }
        }
        let english = english_only(&item.prompt_en)
            && item.options.iter().all(|o| english_only(o))
            && item.sample_answer_en.as_deref().is_none_or(english_only);
        if item.prompt_en.is_empty() || !english {
            t.fail(
                "english_only",
                format!("{name} 含有中文或題目是空的；題目只能用英文。"),
                "拿掉這一題",
            );
            continue;
        }
        if item.answer_format == FMT_CHOICE && item.options.len() < 2 {
            t.fail(
                "items_wellformed",
                format!("{name} 是選擇題，options 要列出選項。"),
                "拿掉這一題",
            );
            continue;
        }
        if prompts.contains(&norm(&item.prompt_en).to_lowercase()) {
            t.fail(
                "not_a_repeat_of_main_task",
                format!("{name} 與主要任務的題目重複。"),
                "拿掉這一題",
            );
            continue;
        }
        let before = item.student_tasks.len();
        item.student_tasks
            .retain(|l| all_tasks.contains(&l.as_str()));
        if item.student_tasks.len() != before {
            t.fail(
                "labels_from_spec_sets",
                format!("{name} 的 student_tasks 用了清單以外的標籤。"),
                "拿掉清單以外的標籤",
            );
        }
        item.objective_refs
            .retain(|r| ctx.objective_ids.contains(r));
        if item.purpose == formative && item.sample_answer_en.is_none() {
            t.fail(
                "formative_has_sample_answer",
                format!("{name} 是形成性評量，需要 sample_answer_en。"),
                "",
            );
        }
        items.push(item);
    }
    if !(2..=4).contains(&items.len()) {
        t.fail(
            "item_count_2_4",
            format!(
                "Exit ticket 需要 2–4 題可用的題目，現在有 {} 題。",
                items.len()
            ),
            if items.len() > 4 {
                "只留前 4 題"
            } else {
                ""
            },
        );
    }
    items.truncate(4);
    if !items.iter().any(|i| i.purpose == formative) {
        t.fail(
            "has_formative_item",
            "至少要有 1 題形成性評量。".to_string(),
            "",
        );
    }
    if !items.iter().any(|i| i.purpose != formative) {
        t.fail(
            "has_reflection_or_question_item",
            "至少要有 1 題學生反思或蒐集問題。".to_string(),
            "",
        );
    }
    for (i, it) in items.iter_mut().enumerate() {
        it.item_id = format!("E{}", i + 1);
    }
    let output = if items.is_empty() { None } else { Some(items) };
    t.finish(output)
}

/// Whether `target` is the warm-up's scaffold target.
pub fn is_warmup_target(target: &str) -> bool {
    target == TARGET_WARMUP
}

// ---------------------------------------------------------------- a block made from a template

/// One block generated from a template section (instructions may be Chinese: the template's
/// instruction language decides, so there is no English-only check here).
pub struct SectionCtx<'a> {
    /// The new material, paragraphs joined.
    pub passage: &'a str,
    pub item_count: usize,
    pub needs_external: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CleanSectionItem {
    pub answer_format: String,
    pub prompt: String,
    pub options: Vec<String>,
    pub answer: String,
    /// The answer's verbatim support in the material, when there is one (teacher version).
    pub evidence: Option<String>,
    /// The Bloom level the model gave the item (template blocks, prompt version 2); `None`
    /// when it gave none or a value outside the six.
    pub bloom: Option<Bloom>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CleanSection {
    pub title: String,
    pub instructions: String,
    pub scaffold: String,
    pub items: Vec<CleanSectionItem>,
}

const SECTION_CHECKS: [&str; 7] = [
    "labels_from_spec_sets",
    "items_wellformed",
    "answer_not_in_prompt",
    "item_count_within_template",
    "has_items",
    "no_invented_material",
    "syllables_match_dictionary",
];

/// Text that names a resource from outside the material: a web address or a video site.
fn names_a_resource(text: &str) -> bool {
    let t = text.to_lowercase();
    [
        "http://", "https://", "www.", "youtube", "youtu.be", "ted.com", ".com/", ".org/",
    ]
    .iter()
    .any(|k| t.contains(k))
}

fn described(corrections: &[syllables::Correction]) -> String {
    corrections
        .iter()
        .map(|c| format!("{} → {}", c.written, c.dictionary))
        .collect::<Vec<_>>()
        .join("、")
}

/// Puts the dictionary's syllable division in an item's answer (the model gets these wrong:
/// "dra-ma-tic" for dra-mat-ic). A choice item's answer has to stay one of its options, so it
/// moves to the option the dictionary agrees with; when no option does, the item is dropped.
/// Returns whether the item is kept.
fn syllable_answer_from_dictionary(
    item: &mut CleanSectionItem,
    iname: &str,
    t: &mut Tally,
) -> bool {
    if item.answer_format == FMT_CHOICE {
        if matches!(syllables::check(item.answer.trim()), Some(Err(_))) {
            let right = item
                .options
                .iter()
                .find(|o| matches!(syllables::check(o.trim()), Some(Ok(()))))
                .cloned();
            match right {
                Some(option) => {
                    t.fixed(
                        "syllables_match_dictionary",
                        format!("{iname} 的答案 {} 與字典不同，改選 {option}", item.answer),
                        "依字典改正",
                    );
                    item.answer = option;
                }
                None => {
                    t.fail(
                        "syllables_match_dictionary",
                        format!("{iname} 的選項裡沒有字典的音節劃分（{}）。", item.answer),
                        "拿掉這一題",
                    );
                    return false;
                }
            }
        }
        return true;
    }
    let (fixed, corrections) = syllables::correct(&item.answer);
    if !corrections.is_empty() {
        t.fixed(
            "syllables_match_dictionary",
            format!("{iname} 的音節劃分與字典不同：{}", described(&corrections)),
            "依字典改正",
        );
        item.answer = fixed;
    }
    true
}

/// The checks every generated item shares: a prompt, a well-formed choice item (a bare letter
/// answer "B" names the option in that position; at least 2 distinct options; the answer is one
/// of them), a matching item with options, and no answer in the stem. Records what failed;
/// returns whether the item is kept.
fn item_checks(item: &mut CleanSectionItem, iname: &str, t: &mut Tally) -> bool {
    if item.prompt.is_empty() {
        t.fail(
            "items_wellformed",
            format!("{iname} 的題目是空的。"),
            "拿掉這一題",
        );
        return false;
    }
    if item.answer_format == FMT_CHOICE {
        normalize_choice(&mut item.options, &mut item.answer);
        let distinct: HashSet<&str> = item.options.iter().map(|o| o.trim()).collect();
        let letter = item
            .answer
            .trim()
            .trim_matches(|c| c == '(' || c == ')' || c == '.');
        let matches = item.options.iter().any(|o| o.trim() == item.answer.trim());
        if !matches && letter.len() == 1 {
            if let Some(c) = letter.chars().next().filter(char::is_ascii_alphabetic) {
                let idx = (c.to_ascii_uppercase() as u8 - b'A') as usize;
                if let Some(option) = item.options.get(idx) {
                    item.answer = option.clone();
                }
            }
        }
        if item.options.len() < 2 || distinct.len() != item.options.len() {
            t.fail(
                "items_wellformed",
                format!("{iname}（選擇題）要有至少 2 個不重複的選項。"),
                "拿掉這一題",
            );
            return false;
        }
        if !item.options.iter().any(|o| o.trim() == item.answer.trim()) {
            t.fail(
                "items_wellformed",
                format!("{iname}（選擇題）的 answer 必須與其中一個選項完全相同。"),
                "拿掉這一題",
            );
            return false;
        }
    }
    if item.answer_format == FMT_MATCHING && item.options.len() < 2 {
        t.fail(
            "items_wellformed",
            format!("{iname}（配對題）的 options 至少要 2 個。"),
            "拿掉這一題",
        );
        return false;
    }
    if contains_answer(&item.prompt, &item.answer) {
        t.fail(
            "answer_not_in_prompt",
            format!("{iname} 的題幹直接寫出了答案。"),
            "拿掉這一題",
        );
        return false;
    }
    true
}

/// Checks one generated template block: fixed answer-format labels, well-formed closed items
/// (a choice item's answer is one of its options), no answer in the stem, at most the
/// section's item count, at least one item, and, for a block that needs the teacher's own
/// material, no named or invented resource. An answer quoted from the material gets that quote
/// as its evidence (material grounding for the teacher version).
pub fn evaluate_section(
    raw: &crate::worksheet::template::RawSectionOut,
    ctx: &SectionCtx<'_>,
) -> Eval<CleanSection> {
    let mut t = Tally::new(&SECTION_CHECKS);
    let mut out = CleanSection {
        title: clean(&raw.title),
        instructions: clean(&raw.instructions),
        scaffold: clean(&raw.scaffold),
        items: Vec::new(),
    };
    let syllable_section = syllables::mentions_syllables(&out.title)
        || syllables::mentions_syllables(&out.instructions);
    section_text_checks(
        [
            ("title", &mut out.title),
            ("instructions", &mut out.instructions),
            ("scaffold", &mut out.scaffold),
        ],
        syllable_section,
        ctx,
        &mut t,
    );
    for (ii, it) in raw.items.iter().enumerate() {
        let iname = format!("第 {} 題", ii + 1);
        if let Some(item) = section_item(it, &iname, syllable_section, ctx, &mut t) {
            out.items.push(item);
        }
    }
    if out.items.len() > ctx.item_count {
        t.fixed(
            "item_count_within_template",
            format!(
                "交出 {} 題，版型這一區是 {} 題。",
                out.items.len(),
                ctx.item_count
            ),
            "只留前面幾題",
        );
        out.items.truncate(ctx.item_count);
    }
    if out.items.is_empty() {
        t.fail(
            "has_items",
            "沒有交出可以用的題目；這一區至少要有 1 題。".to_string(),
            "",
        );
        return t.finish(None);
    }
    t.finish(Some(out))
}

/// The checks of a section's own text: in a section about syllables, the dictionary's division
/// in the instructions and scaffold (an example there, "e.g., be-hav-ior", is a division too);
/// in a section that needs the teacher's material, no named or invented resource (cleared).
fn section_text_checks<const N: usize>(
    fields: [(&str, &mut String); N],
    syllable_section: bool,
    ctx: &SectionCtx<'_>,
    t: &mut Tally,
) {
    for (what, field) in fields {
        if syllable_section && what != "title" {
            let (fixed, corrections) = syllables::correct(field);
            if !corrections.is_empty() {
                t.fixed(
                    "syllables_match_dictionary",
                    format!("{what} 的音節劃分與字典不同：{}", described(&corrections)),
                    "依字典改正",
                );
                *field = fixed;
            }
        }
        if ctx.needs_external && names_a_resource(field) {
            t.fail(
                "no_invented_material",
                format!("{what} 寫出了外部素材（網址或影片網站）；這一區的素材要由教師提供，不可以寫出或編造。"),
                "清空這個欄位",
            );
            field.clear();
        }
    }
}

/// One generated item, cleaned and checked like every item of a template section (fixed
/// answer-format label, well-formed, no answer in the stem, no invented resource, the
/// dictionary's syllables); `None` when it failed (recorded in `t`). An answer quoted from the
/// material gets that quote as its evidence.
fn section_item(
    it: &crate::worksheet::template::RawOutItem,
    iname: &str,
    syllable_section: bool,
    ctx: &SectionCtx<'_>,
    t: &mut Tally,
) -> Option<CleanSectionItem> {
    let format = it.answer_format.trim().to_string();
    if !ANSWER_FORMATS.contains(&format.as_str()) {
        t.fail(
            "labels_from_spec_sets",
            format!(
                "{iname} 的 answer_format「{}」不在允許的清單內。",
                it.answer_format
            ),
            "拿掉這一題",
        );
        return None;
    }
    let mut item = CleanSectionItem {
        answer_format: format,
        prompt: clean(&it.prompt),
        options: clean_all(&it.options),
        answer: clean(&it.answer),
        evidence: None,
        // A label outside the fixed set (a provider that ignores the schema) is dropped.
        bloom: it.bloom.as_deref().and_then(Bloom::parse),
    };
    if !item_checks(&mut item, iname, t) {
        return None;
    }
    if ctx.needs_external {
        let texts = std::iter::once(&item.prompt).chain(item.options.iter());
        if texts.into_iter().any(|x| names_a_resource(x)) {
            t.fail(
                "no_invented_material",
                format!(
                    "{iname} 寫出了外部素材（網址或影片網站）；題目只能留空位給教師提供的素材。"
                ),
                "拿掉這一題",
            );
            return None;
        }
    } else {
        item.evidence = verbatim_piece(&item.answer, ctx.passage);
    }
    let about_syllables = syllable_section || syllables::mentions_syllables(&item.prompt);
    if about_syllables && !syllable_answer_from_dictionary(&mut item, iname, t) {
        return None;
    }
    Some(item)
}

// ---------------------------------------------------------------- one block revised (逐區修改)

const REVISION_CHECKS: [&str; 5] = [
    "labels_from_spec_sets",
    "items_wellformed",
    "answer_not_in_prompt",
    "no_invented_material",
    "syllables_match_dictionary",
];

/// What `worksheet_block_revise` wrote for one block, before it is merged with the block.
pub struct RevisionDraft<'a> {
    /// The new title and instructions; `None` when the model left them unchanged.
    pub title: Option<&'a str>,
    pub instructions: Option<&'a str>,
    /// The block's title and instructions as they will read (to tell a section about syllables).
    pub effective_title: &'a str,
    pub effective_instructions: &'a str,
    /// The items the model modified or added, with the name the model knows them by ("Q2",
    /// "新增的第 1 題"). Items it kept are the teacher's and are never checked here.
    pub items: Vec<(String, &'a crate::worksheet::template::RawOutItem)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CleanRevision {
    pub title: Option<String>,
    pub instructions: Option<String>,
    /// One entry per item of the draft, in order: the cleaned item, or `None` when it failed.
    pub items: Vec<Option<CleanSectionItem>>,
}

/// Checks a block revision the way [`evaluate_section`] checks a template block, item by item,
/// keeping the position of every item so the caller can tell which change failed (and keep the
/// original there). There is no item-count or has-items check: the kept items are the block's.
pub fn evaluate_revision(draft: &RevisionDraft<'_>, ctx: &SectionCtx<'_>) -> Eval<CleanRevision> {
    let mut t = Tally::new(&REVISION_CHECKS);
    let mut title = draft.title.map(clean);
    let mut instructions = draft.instructions.map(clean);
    let syllable_section =
        syllables::mentions_syllables(title.as_deref().unwrap_or(draft.effective_title))
            || syllables::mentions_syllables(
                instructions
                    .as_deref()
                    .unwrap_or(draft.effective_instructions),
            );
    let mut scratch = (String::new(), String::new());
    section_text_checks(
        [
            ("title", title.as_mut().unwrap_or(&mut scratch.0)),
            (
                "instructions",
                instructions.as_mut().unwrap_or(&mut scratch.1),
            ),
        ],
        syllable_section,
        ctx,
        &mut t,
    );
    let items = draft
        .items
        .iter()
        .map(|(name, it)| section_item(it, name, syllable_section, ctx, &mut t))
        .collect();
    t.finish(Some(CleanRevision {
        title,
        instructions,
        items,
    }))
}

// ---------------------------------------------------------------- questions of one Bloom level

/// One call of `worksheet_bloom_level`: the level, the formats it may use, the material and how
/// many questions were asked for.
pub struct BloomCtx<'a> {
    pub bloom: Bloom,
    pub formats: &'a [&'static str],
    /// The material, paragraphs joined.
    pub passage: &'a str,
    /// "P1"…, in material order.
    pub paragraph_codes: &'a [String],
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CleanBloomItem {
    /// Format, prompt, options, answer and the verbatim evidence (if any).
    pub item: CleanSectionItem,
    /// 1-based paragraph numbers the question is about.
    pub paragraph_nos: Vec<usize>,
    pub hint: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CleanBloomLevel {
    pub instructions: String,
    pub items: Vec<CleanBloomItem>,
}

const BLOOM_CHECKS: [&str; 7] = [
    "labels_from_spec_sets",
    "items_wellformed",
    "answer_not_in_prompt",
    "evidence_verbatim",
    "item_count_within_plan",
    "has_items",
    "syllables_match_dictionary",
];

/// Checks the questions of one level the way a template block is checked (fixed labels, the
/// level's formats only, well-formed closed items, no answer in the stem, the dictionary's
/// syllables), plus: a fill-in item has a blank, and a remember / understand question quotes
/// the text word for word as its evidence (cut down to its longest verbatim piece of five
/// words or more; with none the question fails). Other levels need no evidence: one that is
/// not verbatim is cleared. More questions than asked for are cut.
pub fn evaluate_bloom_level(raw: &RawBloomLevel, ctx: &BloomCtx<'_>) -> Eval<CleanBloomLevel> {
    let mut t = Tally::new(&BLOOM_CHECKS);
    let mut out = CleanBloomLevel {
        instructions: clean(&raw.instructions),
        items: Vec::new(),
    };
    let needs_evidence = ctx.bloom.needs_evidence();
    for (ii, it) in raw.items.iter().enumerate() {
        let iname = format!("第 {} 題", ii + 1);
        let format = it.answer_format.trim().to_string();
        if !ctx.formats.contains(&format.as_str()) {
            t.fail(
                "labels_from_spec_sets",
                format!(
                    "{iname} 的 answer_format「{}」不能用在這個層次；只能用：{}。",
                    it.answer_format,
                    ctx.formats.join("、")
                ),
                "拿掉這一題",
            );
            continue;
        }
        let mut item = CleanSectionItem {
            answer_format: format,
            prompt: clean(&it.prompt),
            options: clean_all(&it.options),
            answer: clean(&it.answer),
            evidence: None,
            // The level is the call's, not the item's (see `CleanBloomItem`).
            bloom: None,
        };
        if item.answer_format == FMT_TRUE_FALSE && item.options.is_empty() {
            item.options = vec!["True".to_string(), "False".to_string()];
        }
        if !item_checks(&mut item, &iname, &mut t) {
            continue;
        }
        if item.answer_format == FMT_BLANK && !item.prompt.contains("___") {
            t.fail(
                "items_wellformed",
                format!("{iname}（填空題）的 prompt 要用 ______ 表示空格。"),
                "拿掉這一題",
            );
            continue;
        }
        let evidence = clean(&it.evidence);
        item.evidence = if evidence.is_empty() {
            None
        } else if let Some(span) = locate_verbatim(&evidence, ctx.passage) {
            Some(span)
        } else {
            verbatim_piece(&evidence, ctx.passage)
        };
        if needs_evidence {
            match &item.evidence {
                None => {
                    t.fail(
                        "evidence_verbatim",
                        if evidence.is_empty() {
                            format!("{iname} 沒有 evidence；這個層次的每一題都要從教材逐字複製一段連續文字當作依據。")
                        } else {
                            format!(
                                "{iname} 的 evidence「{}」不是教材的連續片段。請逐字複製一段連續文字（至少 5 個英文字），不可改寫，不可用「...」省略或拼接。",
                                clip_chars(&evidence, 120)
                            )
                        },
                        "拿掉這一題",
                    );
                    continue;
                }
                Some(piece) if *piece != evidence => t.fixed(
                    "evidence_verbatim",
                    format!(
                        "{iname} 的 evidence「{}」不完全是教材的連續片段。",
                        clip_chars(&evidence, 120)
                    ),
                    "只留下其中逐字的一段",
                ),
                Some(_) => {}
            }
        } else if !evidence.is_empty() && item.evidence.as_deref() != Some(evidence.as_str()) {
            t.fixed(
                "evidence_verbatim",
                format!(
                    "{iname} 的 evidence「{}」不是教材的連續片段。",
                    clip_chars(&evidence, 120)
                ),
                if item.evidence.is_some() {
                    "只留下其中逐字的一段"
                } else {
                    "清空這一題的依據"
                },
            );
        }
        if syllables::mentions_syllables(&item.prompt)
            && !syllable_answer_from_dictionary(&mut item, &iname, &mut t)
        {
            continue;
        }
        let mut paragraph_nos: Vec<usize> = it
            .paragraph_ids
            .iter()
            .filter_map(|code| ctx.paragraph_codes.iter().position(|c| c == code.trim()))
            .map(|i| i + 1)
            .collect();
        paragraph_nos.sort_unstable();
        paragraph_nos.dedup();
        out.items.push(CleanBloomItem {
            item,
            paragraph_nos,
            hint: clean(&it.hint).chars().take(HINT_CHARS).collect(),
        });
    }
    if out.items.len() > ctx.count {
        t.fixed(
            "item_count_within_plan",
            format!("交出 {} 題，這個層次要 {} 題。", out.items.len(), ctx.count),
            "只留前面幾題",
        );
        out.items.truncate(ctx.count);
    }
    if out.items.is_empty() {
        t.fail(
            "has_items",
            "沒有交出可以用的題目；至少要有 1 題。".to_string(),
            "",
        );
        return t.finish(None);
    }
    t.finish(Some(out))
}

/// One question's level as the judge classified it.
#[derive(Debug, Clone, PartialEq)]
pub struct Judged {
    /// Index into the ids the judge was given.
    pub index: usize,
    pub bloom: Bloom,
    pub reason: String,
}

const BLOOM_CHECK_CHECKS: [&str; 2] = ["ids_valid", "every_item_judged"];

/// Checks the judge's output: every id it was given, once, with a level from the fixed set.
/// Unknown ids and levels are dropped, a repeated id keeps its first judgement; a question left
/// out is a failure (one repair call), after which it simply stays unjudged.
pub fn evaluate_bloom_check(raw: &RawBloomCheck, ids: &[String]) -> Eval<Vec<Judged>> {
    let mut t = Tally::new(&BLOOM_CHECK_CHECKS);
    let mut out: Vec<Judged> = Vec::new();
    for j in &raw.items {
        let Some(index) = ids.iter().position(|id| id == j.id.trim()) else {
            t.fail(
                "ids_valid",
                format!("「{}」不是要判斷的題號。", clip_chars(&j.id, 20)),
                "拿掉這一筆",
            );
            continue;
        };
        let Some(bloom) = Bloom::parse(&j.bloom) else {
            t.fail(
                "ids_valid",
                format!(
                    "{} 的 bloom「{}」不是六個層次之一。",
                    ids[index],
                    clip_chars(&j.bloom, 20)
                ),
                "拿掉這一筆",
            );
            continue;
        };
        if out.iter().any(|o| o.index == index) {
            t.fixed(
                "ids_valid",
                format!("{} 判斷了兩次。", ids[index]),
                "只留第一次的判斷",
            );
            continue;
        }
        out.push(Judged {
            index,
            bloom,
            reason: clean(&j.reason).chars().take(REASON_CHARS).collect(),
        });
    }
    let missing: Vec<&str> = ids
        .iter()
        .enumerate()
        .filter(|(i, _)| !out.iter().any(|o| o.index == *i))
        .map(|(_, id)| id.as_str())
        .collect();
    if !missing.is_empty() {
        t.fail(
            "every_item_judged",
            format!(
                "這些題目沒有判斷：{}。每一題都要有一筆。",
                missing.join("、")
            ),
            "",
        );
    }
    out.sort_by_key(|j| j.index);
    if out.is_empty() {
        return t.finish(None);
    }
    t.finish(Some(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worksheet::schemas::{RawProcedure, RawSupport};
    use crate::worksheet::{FMT_PARAGRAPH, FMT_SHORT};

    fn out_item(
        format: &str,
        prompt: &str,
        options: &[&str],
        answer: &str,
    ) -> crate::worksheet::template::RawOutItem {
        crate::worksheet::template::RawOutItem {
            answer_format: format.into(),
            prompt: prompt.into(),
            options: options.iter().map(|o| o.to_string()).collect(),
            answer: answer.into(),
            bloom: None,
        }
    }

    #[test]
    fn choice_letters_are_stripped_and_letter_answers_become_the_option() {
        let mut options: Vec<String> = [
            "A. To stop fishing",
            "B. To use fewer bags",
            "C. To build a museum",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let mut answer = "B".to_string();
        normalize_choice(&mut options, &mut answer);
        assert_eq!(
            options,
            ["To stop fishing", "To use fewer bags", "To build a museum"]
        );
        assert_eq!(answer, "To use fewer bags");

        let mut options: Vec<String> = ["(A) red", "(B) blue"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let mut answer = "(B) Blue.".to_string();
        normalize_choice(&mut options, &mut answer);
        assert_eq!(answer, "blue");

        // Options that merely start with a capital "A" word are left alone.
        let mut options: Vec<String> = ["A lot of people", "Few people"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let mut answer = "few people".to_string();
        normalize_choice(&mut options, &mut answer);
        assert_eq!(options, ["A lot of people", "Few people"]);
        assert_eq!(answer, "Few people");
    }

    #[test]
    fn syllable_answers_come_from_the_dictionary() {
        let passage = paragraphs().join("\n");
        let raw = crate::worksheet::template::RawSectionOut {
            title: "Task I. Syllable Separation and Oral Reading".into(),
            instructions: "Separate the words into syllables (e.g., be-ha-vior).".into(),
            scaffold: "".into(),
            items: vec![
                out_item(FMT_BLANK, "dramatic", &[], "dra-ma-tic"),
                out_item(FMT_BLANK, "invisible", &[], "in-vis-i-ble"),
                out_item(FMT_BLANK, "Kaohsiung", &[], "Kao-hsi-ung"),
                out_item(
                    FMT_CHOICE,
                    "Which division is right?",
                    &["au-to-ma-tic", "au-to-mat-ic"],
                    "au-to-ma-tic",
                ),
                out_item(
                    FMT_CHOICE,
                    "Which division is right?",
                    &["pat-ience", "pati-ence"],
                    "pat-ience",
                ),
            ],
        };
        let ctx = SectionCtx {
            passage: &passage,
            item_count: 5,
            needs_external: false,
        };
        let e = evaluate_section(&raw, &ctx);
        let out = e.output.unwrap();
        assert_eq!(
            out.instructions,
            "Separate the words into syllables (e.g., be-hav-ior)."
        );
        let answers: Vec<&str> = out.items.iter().map(|i| i.answer.as_str()).collect();
        assert_eq!(
            answers,
            ["dra-mat-ic", "in-vis-i-ble", "Kao-hsi-ung", "au-to-mat-ic"],
            "corrected, kept, unknown word left alone, choice moved; no right option: dropped"
        );
        let result = e
            .results
            .iter()
            .find(|r| r.name == "syllables_match_dictionary")
            .unwrap();
        assert!(!result.passed, "the dropped choice item fails the check");

        // Outside a syllable section an answer with a hyphen is not touched.
        let other = crate::worksheet::template::RawSectionOut {
            title: "Paragraph 1: Details".into(),
            instructions: "Answer.".into(),
            scaffold: "".into(),
            items: vec![out_item(FMT_BLANK, "Word?", &[], "dra-ma-tic")],
        };
        let out = evaluate_section(&other, &ctx).output.unwrap();
        assert_eq!(out.items[0].answer, "dra-ma-tic");
    }

    #[test]
    fn template_sections_are_checked_and_grounded() {
        let passage = paragraphs().join("\n");
        let raw = crate::worksheet::template::RawSectionOut {
            title: "Paragraph 1: Details".into(),
            instructions: "請閱讀第一段並回答。".into(),
            scaffold: "".into(),
            items: vec![
                out_item(
                    FMT_CHOICE,
                    "Which sentence is in paragraph 1?",
                    &[P1, "No.", "Yes."],
                    "A",
                ),
                out_item(FMT_CHOICE, "Pick one.", &["a", "a"], "a"),
                out_item("探究", "Explore.", &[], "x"),
                out_item("簡答題", &format!("Why? {P2}"), &[], P2),
                out_item("簡答題", "What happened first?", &[], "Something happened."),
                out_item("簡答題", "And then?", &[], "More."),
            ],
        };
        let e = evaluate_section(
            &raw,
            &SectionCtx {
                passage: &passage,
                item_count: 1,
                needs_external: false,
            },
        );
        assert!(e.needs_repair());
        let out = e.output.unwrap();
        // Chinese instructions are fine here (the template's language decides).
        assert_eq!(out.instructions, "請閱讀第一段並回答。");
        assert_eq!(out.items.len(), 1, "capped at item_count");
        assert_eq!(out.items[0].answer, P1, "a bare letter names the option");
        assert_eq!(out.items[0].evidence.as_deref(), Some(P1));

        let external = crate::worksheet::template::RawSectionOut {
            title: "Watch https://youtube.com/x".into(),
            instructions: "Watch the video your teacher gives you.".into(),
            scaffold: "".into(),
            items: vec![
                out_item(
                    "簡答題",
                    "See www.ted.com/talk and answer: ______",
                    &[],
                    "x",
                ),
                out_item(
                    "簡答題",
                    "After the material, write one key point: ______",
                    &[],
                    P1,
                ),
            ],
        };
        let e = evaluate_section(
            &external,
            &SectionCtx {
                passage: &passage,
                item_count: 3,
                needs_external: true,
            },
        );
        let out = e.output.unwrap();
        assert_eq!(out.title, "");
        assert_eq!(out.items.len(), 1);
        assert_eq!(out.items[0].evidence, None);
        let empty = crate::worksheet::template::RawSectionOut {
            items: Vec::new(),
            ..external
        };
        let e = evaluate_section(
            &empty,
            &SectionCtx {
                passage: &passage,
                item_count: 3,
                needs_external: true,
            },
        );
        assert!(e.output.is_none() && e.needs_repair());
    }

    const P1: &str = "Mrs. Lin put an umbrella stand outside her shop on a rainy Monday.";
    const P2: &str =
        "Her neighbors laughed at the idea. They were sure every umbrella would be gone.";
    const P3: &str = "In the first month, twelve umbrellas did disappear. However, something unexpected followed.";

    fn paragraphs() -> Vec<String> {
        vec![P1.to_string(), P2.to_string(), P3.to_string()]
    }

    fn item(format: &str, prompt: &str, answer: &str) -> RawItem {
        RawItem {
            item_id: "x".into(),
            answer_format: format.into(),
            tier: "core".into(),
            prompt_en: prompt.into(),
            options: vec![],
            answer_en: answer.into(),
            evidence: None,
            bloom: None,
        }
    }

    fn block(nos: &[i64], function: &str, items: Vec<RawItem>) -> RawBlock {
        RawBlock {
            block_id: "B".into(),
            paragraph_nos: nos.to_vec(),
            title_en: "Read".into(),
            info_function: function.into(),
            student_tasks: vec!["理解內容".into()],
            objective_refs: vec![],
            instructions_en: "Answer.".into(),
            items,
            design_reason_zh: "理由".into(),
            image_slots: Vec::new(),
        }
    }

    fn failed(e: &[CheckResult]) -> Vec<&'static str> {
        e.iter().filter(|r| !r.passed).map(|r| r.name).collect()
    }

    fn tasks_ctx<'a>(paragraphs: &'a [String], objectives: &'a [String]) -> TasksCtx<'a> {
        TasksCtx {
            paragraphs,
            objective_ids: objectives,
            item_budget: 6,
            only: None,
        }
    }

    #[test]
    fn good_tasks_pass_and_get_sequential_ids() {
        let p = paragraphs();
        let raw = RawTasks {
            blocks: vec![
                block(
                    &[1],
                    "發現資訊",
                    vec![item("簡答題", "Where was the stand?", "Outside her shop.")],
                ),
                block(
                    &[2],
                    "分析資訊",
                    vec![item(
                        "簡答題",
                        "Why did they laugh?",
                        "They expected theft.",
                    )],
                ),
                block(
                    &[3],
                    "創造資訊",
                    vec![item("段落寫作", "Write your own ending.", "Answers vary.")],
                ),
            ],
        };
        let e = evaluate_tasks(&raw, &tasks_ctx(&p, &[]));
        assert!(!e.needs_repair(), "{:?}", failed(&e.results));
        let blocks = e.output.unwrap();
        assert_eq!(blocks[2].block_id, "B3");
        assert_eq!(blocks[2].items[0].item_id, "B3-1");
    }

    #[test]
    fn tasks_drop_what_fails_and_say_why() {
        let p = paragraphs();
        let mut cjk = item("簡答題", "請回答 Why?", "Because.");
        cjk.tier = "support".into();
        let mut spliced = item(
            "簡答題",
            "What happened in the first month?",
            "Twelve disappeared.",
        );
        spliced.evidence = Some("In the first month ... unexpected followed.".into());
        let mut mcq = item("選擇題", "Pick one.", "A");
        mcq.options = vec!["A".into(), "B".into()];
        let leak = item(
            "簡答題",
            "They were sure every umbrella would be gone. What were they sure about?",
            "every umbrella would be gone",
        );
        let mut labelled = block(
            &[2],
            "發現資訊",
            vec![item("簡答題", "Who laughed?", "The neighbors."), leak],
        );
        labelled.student_tasks.push("探究".into());
        labelled.objective_refs = vec!["O9".into()];
        let raw = RawTasks {
            blocks: vec![
                block(
                    &[1],
                    "發現資訊",
                    vec![item("簡答題", "Where?", "Outside."), cjk, mcq],
                ),
                labelled,
                block(&[3], "發現資訊", vec![spliced]),
                block(&[3], "高層次思考", vec![item("簡答題", "Q?", "A.")]),
            ],
        };
        let objectives = vec!["O1".to_string()];
        let e = evaluate_tasks(&raw, &tasks_ctx(&p, &objectives));
        let names = failed(&e.results);
        for expected in [
            "labels_from_spec_sets",
            "english_only",
            "items_wellformed",
            "answer_not_in_prompt",
            "objective_refs_valid",
            "at_least_3_info_functions",
        ] {
            assert!(names.contains(&expected), "{expected} in {names:?}");
        }
        let blocks = e.output.unwrap();
        assert_eq!(
            blocks.len(),
            3,
            "the block with a label outside the set is gone"
        );
        assert_eq!(blocks[0].items.len(), 1);
        assert_eq!(blocks[1].student_tasks, vec!["理解內容".to_string()]);
        assert!(blocks[1].objective_refs.is_empty());
        assert_eq!(blocks[1].items.len(), 1);
        // The spliced evidence has no verbatim piece of five words: cleared in code, reported
        // as repaired (not as a failure).
        assert_eq!(blocks[2].items[0].evidence, None);
        let evidence = e
            .results
            .iter()
            .find(|r| r.name == "evidence_verbatim")
            .unwrap();
        assert!(evidence.passed);
        assert!(evidence
            .detail
            .as_deref()
            .unwrap()
            .contains("清空這一題的依據"));
        assert!(!e.repairs.is_empty() && !e.problems.is_empty());
    }

    #[test]
    fn spliced_evidence_is_cut_to_its_verbatim_piece_without_a_repair_call() {
        let p = paragraphs();
        let passage = p.join("\n");
        // Two sentences that are not next to each other (the model's habit), and a "..." splice.
        assert_eq!(
            verbatim_piece(
                "Mrs. Lin put an umbrella stand outside her shop on a rainy Monday. However, something unexpected followed.",
                &passage
            )
            .as_deref(),
            Some("Mrs. Lin put an umbrella stand outside her shop on a rainy Monday.")
        );
        assert_eq!(
            verbatim_piece(
                "In the first month, twelve umbrellas did disappear … something unexpected",
                &passage
            )
            .as_deref(),
            Some("In the first month, twelve umbrellas did disappear")
        );
        assert_eq!(
            verbatim_piece("Her neighbors ... the idea.", &passage),
            None
        );
        assert_eq!(
            verbatim_piece("Nothing like the text at all here.", &passage),
            None
        );

        let mut spliced = item(
            "簡答題",
            "What happened in the first month?",
            "Twelve went.",
        );
        spliced.evidence = Some(
            "In the first month, twelve umbrellas did disappear. ... something unexpected followed."
                .into(),
        );
        let raw = RawTasks {
            blocks: vec![
                block(&[1], "發現資訊", vec![item("簡答題", "Where?", "Outside.")]),
                block(&[2], "分析資訊", vec![spliced]),
                block(
                    &[3],
                    "創造資訊",
                    vec![item("段落寫作", "Write.", "Varies.")],
                ),
            ],
        };
        let e = evaluate_tasks(&raw, &tasks_ctx(&p, &[]));
        assert!(!e.needs_repair(), "{:?}", failed(&e.results));
        let blocks = e.output.unwrap();
        assert_eq!(
            blocks[1].items[0].evidence.as_deref(),
            Some("In the first month, twelve umbrellas did disappear.")
        );
        assert!(e.repairs.iter().any(|r| r.contains("只留下其中逐字的一段")));
        // If a repair call is made for another reason, the model hears about it too.
        assert!(e.problems.iter().any(|p| p.contains("evidence")));
    }

    #[test]
    fn tasks_report_uncovered_paragraphs_and_trim_to_the_budget() {
        let p = paragraphs();
        let many: Vec<RawItem> = (0..12)
            .map(|i| item("簡答題", &format!("Question {i}?"), "Yes."))
            .collect();
        let raw = RawTasks {
            blocks: vec![
                block(&[2], "發現資訊", many),
                block(&[1], "分析資訊", vec![item("簡答題", "Q?", "A.")]),
            ],
        };
        let mut ctx = tasks_ctx(&p, &[]);
        ctx.item_budget = 4;
        let e = evaluate_tasks(&raw, &ctx);
        let names = failed(&e.results);
        assert!(names.contains(&"all_paragraphs_covered"));
        assert!(names.contains(&"blocks_in_paragraph_order"));
        assert!(names.contains(&"item_count_within_budget"));
        let blocks = e.output.unwrap();
        assert_eq!(blocks[0].paragraph_nos, vec![1]);
        let core: usize = blocks.iter().map(|b| b.items.len()).sum();
        assert_eq!(core, 6, "1.5 times the budget of 4");
        assert_eq!(uncovered_paragraphs(&blocks, 3), vec![3]);
    }

    #[test]
    fn warmup_keeps_only_candidates_and_levels_from_data() {
        let p = paragraphs();
        let candidates = vec![
            Candidate {
                word: "disappear".into(),
                level: Some(2),
                paragraph_no: 3,
                sentence: "In the first month, twelve umbrellas did disappear.".into(),
            },
            Candidate {
                word: "unexpected".into(),
                level: Some(4),
                paragraph_no: 3,
                sentence: "However, something unexpected followed.".into(),
            },
        ];
        let raw = RawWarmup {
            background_activities: vec![RawBackground {
                title_en: "Before you read".into(),
                instructions_en: "Talk to a partner.".into(),
                prompts_en: vec!["Have you ever lent something to a stranger?".into()],
                student_tasks: vec!["討論／分享".into(), "合作活動".into()],
                answer_format: "簡答題".into(),
                purpose_zh: "連結個人經驗".into(),
            }],
            key_vocabulary: vec![
                RawKeyWord {
                    word: "Disappear".into(),
                    paragraph_no: 3,
                    context_sentence: "twelve umbrellas did disappear".into(),
                    student_friendly_definition_en: "to go away".into(),
                },
                RawKeyWord {
                    word: "unexpected".into(),
                    paragraph_no: 1,
                    context_sentence: "made up sentence".into(),
                    student_friendly_definition_en: "沒想到的".into(),
                },
                RawKeyWord {
                    word: "trust".into(),
                    paragraph_no: 1,
                    context_sentence: "x".into(),
                    student_friendly_definition_en: "belief".into(),
                },
            ],
        };
        let e = evaluate_warmup(
            &raw,
            &WarmupCtx {
                paragraphs: &p,
                candidates: &candidates,
                vocab_count: 6,
            },
        );
        let names = failed(&e.results);
        for expected in [
            "labels_from_spec_sets",
            "vocab_only_from_candidates",
            "english_only",
        ] {
            assert!(names.contains(&expected), "{expected} in {names:?}");
        }
        // A context sentence that is not verbatim is replaced by the one the code found:
        // repaired, not a reason for another call.
        let context = e
            .results
            .iter()
            .find(|r| r.name == "context_sentence_verbatim")
            .unwrap();
        assert!(context.passed && context.detail.is_some());
        let w = e.output.unwrap();
        assert_eq!(
            w.activities[0].student_tasks,
            vec!["討論／分享".to_string()]
        );
        assert_eq!(w.words.len(), 2);
        assert_eq!(
            (w.words[0].word.as_str(), w.words[0].level),
            ("disappear", Some(2))
        );
        assert_eq!(
            w.words[1].context,
            "However, something unexpected followed."
        );
        assert_eq!(w.words[1].definition, "");
    }
    use crate::worksheet::schemas::{RawBackground, RawKeyWord};

    #[test]
    fn scaffolds_need_a_procedure_and_must_not_give_the_answer() {
        let targets = vec!["warm_up".to_string(), "B1".to_string(), "B2".to_string()];
        let answers = vec!["because a stranger once shared his umbrella".to_string()];
        let steps = |s: &[&str]| RawProcedure {
            kind: "清楚的分步指示及完成順序".into(),
            steps_en: s.iter().map(|x| x.to_string()).collect(),
        };
        let raw = RawScaffolds {
            scaffolds: vec![
                RawScaffoldSet {
                    target: "warm_up".into(),
                    verbal: vec![],
                    instructional: vec![],
                    procedural: vec![steps(&["Read.", "Talk."])],
                },
                RawScaffoldSet {
                    target: "B1".into(),
                    verbal: vec![
                        RawSupport {
                            kind: "句型框架".into(),
                            content_en: "She started it ___ because ___.".into(),
                        },
                        RawSupport {
                            kind: "作答示例".into(),
                            content_en:
                                "She did it because a stranger once shared his umbrella with her."
                                    .into(),
                        },
                        RawSupport {
                            kind: "自創".into(),
                            content_en: "x".into(),
                        },
                    ],
                    instructional: vec![RawSupport {
                        kind: "圖像組織".into(),
                        content_en: "請畫表格".into(),
                    }],
                    procedural: vec![],
                },
                RawScaffoldSet {
                    target: "B9".into(),
                    verbal: vec![],
                    instructional: vec![],
                    procedural: vec![steps(&["Go."])],
                },
            ],
        };
        let e = evaluate_scaffolds(
            &raw,
            &ScaffoldsCtx {
                targets: &targets,
                answers: &answers,
            },
        );
        let names = failed(&e.results);
        for expected in SCAFFOLD_CHECKS {
            assert!(names.contains(&expected), "{expected} in {names:?}");
        }
        let sets = e.output.unwrap();
        assert_eq!(sets.len(), 2);
        assert_eq!(sets[1].verbal.len(), 1);
        assert!(sets[1].instructional.is_empty() && sets[1].procedural.is_empty());
    }

    #[test]
    fn chinese_glosses_are_taken_out_of_a_word_bank() {
        assert_eq!(
            without_cjk_glosses("Word Bank: behavior (行為), automatic（自動的）, effort (try)")
                .as_deref(),
            Some("Word Bank: behavior, automatic, effort (try)")
        );
        assert_eq!(without_cjk_glosses("請用這個句型 (frame)"), None);
        assert_eq!(without_cjk_glosses("(行為)"), None);
        assert_eq!(without_cjk_glosses("unclosed (行為"), None);

        let raw = RawScaffolds {
            scaffolds: vec![RawScaffoldSet {
                target: "B1".into(),
                verbal: vec![RawSupport {
                    kind: "字詞表".into(),
                    content_en: "Word Bank: habit (習慣), routine (常規)".into(),
                }],
                instructional: vec![],
                procedural: vec![RawProcedure {
                    kind: "清楚的分步指示及完成順序".into(),
                    steps_en: vec!["Read.".into(), "Write.".into()],
                }],
            }],
        };
        let e = evaluate_scaffolds(
            &raw,
            &ScaffoldsCtx {
                targets: &["B1".to_string()],
                answers: &[],
            },
        );
        assert!(!e.needs_repair(), "{:?}", failed(&e.results));
        assert_eq!(
            e.output.unwrap()[0].verbal[0].content_en,
            "Word Bank: habit, routine"
        );
    }

    #[test]
    fn scaffold_targets_are_matched_however_they_are_spelled() {
        let set = |target: &str| RawScaffoldSet {
            target: target.into(),
            verbal: vec![],
            instructional: vec![],
            procedural: vec![RawProcedure {
                kind: "清楚的分步指示及完成順序".into(),
                steps_en: vec!["Read.".into(), "Write.".into()],
            }],
        };
        assert_eq!(
            canonical_target("Warm-up", &["warm_up".into()]).as_deref(),
            Some("warm_up")
        );
        assert_eq!(
            canonical_target("[b2]", &["B1".into(), "B2".into()]).as_deref(),
            Some("B2")
        );
        assert_eq!(canonical_target("", &["B1".into()]), None);

        // The warm-up failed, so only the blocks were sent; the model still wrote a warm-up
        // set (the prompt describes its target) and spelled the blocks its own way.
        let targets = vec!["B1".to_string(), "B2".to_string()];
        let raw = RawScaffolds {
            scaffolds: vec![set("warmup"), set("b1"), set("[B2]"), set("B2")],
        };
        let e = evaluate_scaffolds(
            &raw,
            &ScaffoldsCtx {
                targets: &targets,
                answers: &[],
            },
        );
        assert!(!e.needs_repair(), "{:?}", failed(&e.results));
        let sets = e.output.unwrap();
        let names: Vec<&str> = sets.iter().map(|s| s.target.as_str()).collect();
        assert_eq!(names, ["B1", "B2"]);
        let check = e
            .results
            .iter()
            .find(|r| r.name == "targets_match_blocks_and_warmup")
            .unwrap();
        assert!(check.detail.as_deref().unwrap().contains("沒有暖身"));

        // With the warm-up requested, "warm-up" is its set; a made-up target still fails.
        let targets = vec!["warm_up".to_string(), "B1".to_string()];
        let raw = RawScaffolds {
            scaffolds: vec![set("Warm-up"), set("B1"), set("B7")],
        };
        let e = evaluate_scaffolds(
            &raw,
            &ScaffoldsCtx {
                targets: &targets,
                answers: &[],
            },
        );
        assert_eq!(failed(&e.results), ["targets_match_blocks_and_warmup"]);
        let sets = e.output.unwrap();
        assert_eq!(sets[0].target, "warm_up");
        assert_eq!(sets.len(), 2);
    }

    #[test]
    fn exit_ticket_needs_both_purposes() {
        let ex = |purpose: &str, prompt: &str, sample: Option<&str>| RawExitItem {
            item_id: "x".into(),
            purpose: purpose.into(),
            prompt_en: prompt.into(),
            answer_format: "簡答題".into(),
            options: vec![],
            student_tasks: vec!["學習反思".into()],
            objective_refs: vec!["O1".into(), "O7".into()],
            sample_answer_en: sample.map(str::to_string),
        };
        let objectives = vec!["O1".to_string()];
        let prompts = vec!["Why did they laugh?".to_string()];
        let ctx = ExitCtx {
            objective_ids: &objectives,
            task_prompts: &prompts,
        };
        let good = RawExit {
            estimated_minutes: 3,
            items: vec![
                ex(
                    "形成性評量",
                    "Explain the main idea in one sentence.",
                    Some("Trust grows when it is given."),
                ),
                ex("學生反思", "What was hard today?", None),
            ],
        };
        let e = evaluate_exit(&good, &ctx);
        assert!(!e.needs_repair(), "{:?}", failed(&e.results));
        let items = e.output.unwrap();
        assert_eq!(items[0].objective_refs, vec!["O1".to_string()]);
        assert_eq!(items[1].item_id, "E2");

        let bad = RawExit {
            estimated_minutes: 3,
            items: vec![
                ex("形成性評量", "why did they laugh?", None),
                ex("形成性評量", "今天學到什麼？", None),
            ],
        };
        let e = evaluate_exit(&bad, &ctx);
        let names = failed(&e.results);
        for expected in [
            "english_only",
            "not_a_repeat_of_main_task",
            "item_count_2_4",
            "has_formative_item",
            "has_reflection_or_question_item",
        ] {
            assert!(names.contains(&expected), "{expected} in {names:?}");
        }
        assert!(e.output.is_none());
    }

    fn bloom_item(
        format: &str,
        prompt: &str,
        options: &[&str],
        answer: &str,
        evidence: &str,
    ) -> crate::worksheet::bloom::RawBloomItem {
        crate::worksheet::bloom::RawBloomItem {
            answer_format: format.into(),
            prompt: prompt.into(),
            options: options.iter().map(|o| o.to_string()).collect(),
            answer: answer.into(),
            evidence: evidence.into(),
            paragraph_ids: vec!["P2".into(), "P9".into()],
            hint: "Look at paragraph 2.".into(),
        }
    }

    #[test]
    fn remember_questions_must_quote_the_text() {
        let passage = [P1, P2, P3].join("\n");
        let codes = crate::worksheet::bloom::paragraph_codes(3);
        let ctx = |bloom, count| BloomCtx {
            bloom,
            formats: bloom_formats(bloom),
            passage: &passage,
            paragraph_codes: &codes,
            count,
        };
        fn bloom_formats(b: Bloom) -> &'static [&'static str] {
            b.formats()
        }
        let raw = RawBloomLevel {
            instructions: "Answer the questions.".into(),
            items: vec![
                // verbatim
                bloom_item(
                    FMT_SHORT,
                    "Where did Mrs. Lin put the stand?",
                    &[],
                    "Outside her shop.",
                    P1,
                ),
                // not in the text at all: fails, repair
                bloom_item(
                    FMT_SHORT,
                    "What day was it?",
                    &[],
                    "Monday.",
                    "It was a sunny Tuesday in the city.",
                ),
                // a spliced quote with a verbatim piece: cut down, no repair needed for it
                bloom_item(
                    FMT_SHORT,
                    "What happened?",
                    &[],
                    "Umbrellas disappeared.",
                    "In the first month, twelve umbrellas did disappear ... and so on",
                ),
                // a format the level may not use
                bloom_item(FMT_PARAGRAPH, "Write about it.", &[], "Answers vary.", P1),
                // a fill-in item without a blank
                bloom_item(
                    FMT_BLANK,
                    "Mrs. Lin put an umbrella stand",
                    &[],
                    "outside",
                    P1,
                ),
            ],
        };
        let e = evaluate_bloom_level(&raw, &ctx(Bloom::Remember, 8));
        assert!(e.needs_repair());
        let failed: Vec<&str> = e
            .results
            .iter()
            .filter(|r| !r.passed)
            .map(|r| r.name)
            .collect();
        assert_eq!(
            failed,
            [
                "labels_from_spec_sets",
                "items_wellformed",
                "evidence_verbatim"
            ]
        );
        let out = e.output.unwrap();
        assert_eq!(out.items.len(), 2);
        assert_eq!(out.items[0].item.evidence.as_deref(), Some(P1));
        assert_eq!(
            out.items[1].item.evidence.as_deref(),
            Some("In the first month, twelve umbrellas did disappear")
        );
        assert_eq!(out.items[0].paragraph_nos, [2], "unknown codes are dropped");
        assert_eq!(out.items[0].hint, "Look at paragraph 2.");

        // Analyze needs no evidence: one that is not verbatim is cleared without a repair.
        let raw = RawBloomLevel {
            instructions: String::new(),
            items: vec![
                bloom_item(
                    FMT_SHORT,
                    "Why does the writer mention the first month?",
                    &[],
                    "To show the change.",
                    "made up words that are not there",
                ),
                bloom_item(
                    FMT_CHOICE,
                    "What is the purpose of paragraph 3?",
                    &["To warn", "To surprise", "To list", "To end"],
                    "B",
                    "",
                ),
                bloom_item(FMT_SHORT, "Third question?", &[], "x", ""),
            ],
        };
        let e = evaluate_bloom_level(&raw, &ctx(Bloom::Analyze, 2));
        assert!(!e.needs_repair(), "{:?}", e.results);
        let out = e.output.unwrap();
        assert_eq!(out.items.len(), 2, "cut to the count");
        assert_eq!(out.items[0].item.evidence, None);
        assert_eq!(
            out.items[1].item.answer, "To surprise",
            "a letter names its option"
        );
        assert!(e.repairs.iter().any(|r| r.contains("只留前面幾題")));

        // Nothing usable: no output.
        let raw = RawBloomLevel {
            instructions: String::new(),
            items: vec![bloom_item("探究", "x", &[], "y", "")],
        };
        assert!(evaluate_bloom_level(&raw, &ctx(Bloom::Create, 1))
            .output
            .is_none());
    }

    #[test]
    fn the_judge_must_judge_every_question_once() {
        use crate::worksheet::bloom::RawJudgement;
        let ids: Vec<String> = ["Q1", "Q2", "Q3"].iter().map(|s| s.to_string()).collect();
        let j = |id: &str, bloom: &str| RawJudgement {
            id: id.into(),
            bloom: bloom.into(),
            reason: "理由".into(),
        };
        let all = RawBloomCheck {
            items: vec![
                j("Q2", "analyze"),
                j("Q1", "remember"),
                j("Q3", "create"),
                j("Q3", "apply"),
            ],
        };
        let e = evaluate_bloom_check(&all, &ids);
        assert!(!e.needs_repair());
        let out = e.output.unwrap();
        assert_eq!(
            out.iter().map(|x| (x.index, x.bloom)).collect::<Vec<_>>(),
            [
                (0, Bloom::Remember),
                (1, Bloom::Analyze),
                (2, Bloom::Create)
            ]
        );
        let partial = RawBloomCheck {
            items: vec![j("Q1", "remember"), j("Q7", "apply"), j("Q2", "thinking")],
        };
        let e = evaluate_bloom_check(&partial, &ids);
        assert!(e.needs_repair());
        assert_eq!(e.output.unwrap().len(), 1);
        assert!(e.problems.iter().any(|p| p.contains("Q2、Q3")));
        assert!(evaluate_bloom_check(&RawBloomCheck { items: vec![] }, &ids)
            .output
            .is_none());
    }
}
