//! Assembly in code (docs/LLM_VALIDATION.md section 10: the worksheet is put together by the
//! program, not by the model). The checked outputs of the steps become blocks; the order
//! (warm-up first, activity blocks in paragraph order, exit ticket last), the item numbers, the
//! differences between the versions and the student / teacher split are all decided here, so
//! they cannot go wrong the way a model-written sheet can: an answer has no place to appear in
//! a student sheet.

use std::collections::HashMap;

use serde::Serialize;

use super::checks::{CleanWarmup, CleanWord};
use super::schemas::{
    RawBlock, RawExitItem, RawScaffoldSet, TARGET_WARMUP, TIER_CHALLENGE, TIER_SUPPORT,
};
use super::tiered::{self, SupportKind};
use super::{
    info_function_from_label, uses_choices, Audience, Block, BlockKind, Bloom, Content, Item,
    Level, Material, Origin, Practice, Scaffold, ScaffoldType, Settings, Vocab, EXIT_PURPOSES,
    FMT_BLANK, FMT_CHOICE, FMT_MATCHING, FMT_ORGANIZER, FMT_PARAGRAPH, FMT_TABLE, FMT_TRUE_FALSE,
    GEN_EXAMPLE, GEN_FRAMES, GEN_GUIDING, GEN_WORD_LIST, KIND_EXAMPLE, KIND_FRAMES, KIND_GUIDING,
    KIND_STEPS, KIND_WORD_LIST,
};
use crate::sop::ExitKind;

pub const WARMUP_ID: &str = "warmup";
pub const EXIT_ID: &str = "exit";

// ---------------------------------------------------------------- version rules (spec §5.4)

/// Which versions an item of a tier appears in. Core items are the shared core tasks; support
/// items exist only in 基礎版 and challenge items only in 進階版.
pub fn item_levels(tier: &str, selected: &[Level]) -> Vec<Level> {
    let wanted: &[Level] = match tier {
        TIER_SUPPORT => &[Level::Basic],
        TIER_CHALLENGE => &[Level::Advanced],
        _ => &Level::ALL,
    };
    selected
        .iter()
        .copied()
        .filter(|l| wanted.contains(l))
        .collect()
}

/// The scaffold kinds the teacher asked for in 產生設定.
pub fn selected_kinds(settings: &Settings) -> Vec<&'static str> {
    [
        (GEN_GUIDING, KIND_GUIDING),
        (GEN_EXAMPLE, KIND_EXAMPLE),
        (GEN_WORD_LIST, KIND_WORD_LIST),
        (GEN_FRAMES, KIND_FRAMES),
    ]
    .into_iter()
    .filter(|(option, _)| settings.wants(option))
    .map(|(_, kind)| kind)
    .collect()
}

/// Which versions a scaffold appears in: support is greatest in 基礎版 and least in 進階版.
/// Procedural scaffolds stay everywhere; word lists, sentence frames and whatever the teacher
/// ticked stay in 基礎版 and 一般版; the rest only in 基礎版. A kind the teacher ticked is never
/// lost: it appears at least in the lowest selected version. `None`: nowhere, leave it out.
pub fn scaffold_levels(
    kind_type: ScaffoldType,
    kind: &str,
    selected: &[Level],
    ticked: &[&str],
) -> Option<Vec<Level>> {
    if kind_type == ScaffoldType::Procedural {
        return Some(selected.to_vec());
    }
    let asked = ticked.contains(&kind);
    let wanted: &[Level] = if asked || kind == KIND_WORD_LIST || kind == KIND_FRAMES {
        &[Level::Basic, Level::Standard]
    } else {
        &[Level::Basic]
    };
    let levels: Vec<Level> = selected
        .iter()
        .copied()
        .filter(|l| wanted.contains(l))
        .collect();
    if !levels.is_empty() {
        Some(levels)
    } else if asked {
        selected.first().map(|l| vec![*l])
    } else {
        None
    }
}

/// Lines to answer on: more room in the higher versions, none for closed formats.
pub fn answer_lines(answer_format: Option<&str>, level: Level) -> u8 {
    let base = match level {
        Level::Basic => 2,
        Level::Standard => 3,
        Level::Advanced => 4,
    };
    match answer_format {
        Some(f)
            if [
                FMT_BLANK,
                FMT_CHOICE,
                FMT_TRUE_FALSE,
                FMT_MATCHING,
                FMT_TABLE,
                FMT_ORGANIZER,
            ]
            .contains(&f) =>
        {
            0
        }
        Some(f) if f == FMT_PARAGRAPH => base + 2,
        _ => base,
    }
}

// ---------------------------------------------------------------- procedural scaffolds (always there)

/// Step-by-step instructions made by the program from what the block asks for. Used whenever
/// the model did not deliver a usable procedural scaffold, so every block has one (spec §3.3).
pub fn auto_steps(block: &Block) -> Vec<String> {
    let has = |label: &str| block.student_tasks.iter().any(|t| t == label);
    let format = |f: &str| {
        block
            .items
            .iter()
            .any(|i| i.answer_format.as_deref() == Some(f))
    };
    let mut steps = Vec::new();
    match block.kind {
        BlockKind::Warmup => {
            steps.push("Think about the questions on your own for one minute.".to_string());
            steps.push("Share your ideas with a partner.".to_string());
            if !block.vocabulary.is_empty() {
                steps.push("Look at the key words. Circle the ones you already know.".to_string());
            }
        }
        BlockKind::ExitTicket => {
            steps.push("Answer on your own. Do not look back at your notes.".to_string());
            steps.push("Hand in your answers before you leave.".to_string());
        }
        BlockKind::Activity => {
            steps.push(if block.paragraph_ids.is_empty() {
                "Read the questions first.".to_string()
            } else {
                "Read the paragraph(s) on your own. Then read the questions.".to_string()
            });
            if format(FMT_TABLE) || format(FMT_ORGANIZER) {
                steps.push(
                    "Underline the information you need, then fill in the organizer.".to_string(),
                );
            } else {
                steps.push("Underline the words in the text that help you answer.".to_string());
            }
            if has("小組共同完成") {
                steps.push(
                    "Work in a group of four. Decide who writes, who checks and who reports."
                        .to_string(),
                );
                steps.push("Agree on one answer for the group and write it down.".to_string());
            } else if has("討論／分享") {
                steps.push(
                    "Write your own answer first. Then compare it with a partner.".to_string(),
                );
            } else {
                steps.push("Write your answers.".to_string());
            }
            if format(FMT_PARAGRAPH) || has("寫作") {
                steps.push(
                    "Read your writing once more and check it against the instructions."
                        .to_string(),
                );
            } else {
                steps.push("Check your answers against the text.".to_string());
            }
        }
    }
    steps
}

fn auto_procedural(block: &Block, levels: &[Level]) -> Scaffold {
    Scaffold {
        id: format!("{}-sp", block.id),
        kind_type: ScaffoldType::Procedural,
        kind: KIND_STEPS.to_string(),
        content: String::new(),
        steps: auto_steps(block),
        levels: levels.to_vec(),
        auto: true,
        origin: Origin::Ai,
        heading: None,
    }
}

/// Puts the scaffolds of one target on its block. Without a usable procedural scaffold from the
/// model, the program adds its own.
pub fn attach_scaffolds(
    block: &mut Block,
    set: Option<&RawScaffoldSet>,
    levels: &[Level],
    ticked: &[&str],
) {
    block.scaffolds.clear();
    if let Some(set) = set {
        for (i, p) in set.procedural.iter().enumerate() {
            block.scaffolds.push(Scaffold {
                id: format!("{}-sp{}", block.id, i + 1),
                kind_type: ScaffoldType::Procedural,
                kind: p.kind.clone(),
                content: String::new(),
                steps: p.steps_en.clone(),
                levels: levels.to_vec(),
                auto: true,
                origin: Origin::Ai,
                heading: None,
            });
        }
        let supports = set
            .verbal
            .iter()
            .map(|s| (ScaffoldType::Verbal, "sv", s))
            .chain(
                set.instructional
                    .iter()
                    .map(|s| (ScaffoldType::Instructional, "si", s)),
            );
        for (i, (kind_type, prefix, s)) in supports.enumerate() {
            if let Some(shown) = scaffold_levels(kind_type, &s.kind, levels, ticked) {
                block.scaffolds.push(Scaffold {
                    id: format!("{}-{prefix}{}", block.id, i + 1),
                    kind_type,
                    kind: s.kind.clone(),
                    content: s.content_en.clone(),
                    steps: Vec::new(),
                    levels: shown,
                    auto: false,
                    origin: Origin::Ai,
                    heading: None,
                });
            }
        }
    }
    if !block
        .scaffolds
        .iter()
        .any(|s| s.kind_type == ScaffoldType::Procedural)
    {
        let auto = auto_procedural(block, levels);
        block.scaffolds.insert(0, auto);
    }
}

// ---------------------------------------------------------------- sentence frames are not questions

/// What a question that was only a sentence frame asks once the frame is in the block's box.
pub const FRAME_TASK: &str = "Use the sentence frame above to write your answer.";

/// A generated question that is only a sentence frame ([`super::checks::sentence_frame_only`],
/// still so after the repair call) is a language scaffold, not a question: the frame goes into
/// the block's 句型框架 scaffold, printed un-numbered in the 「Sentence frames」 box above the
/// questions, in the versions the question was in. Right after an open question (簡答題,
/// 段落寫作) it is that question's frame and its number goes; otherwise the question stays and
/// asks to use the frame ([`FRAME_TASK`], a 簡答題 so it prints lines). A greeting-led frame
/// wrapped in a task (「Write … using the frame: Hi! …」, [`super::checks::greeting_frame`]) is
/// lifted the same way, without the wrapper. The teacher's own questions are never touched.
/// Returns how many frames were moved.
pub fn lift_frames(block: &mut Block, passage: &str) -> usize {
    let mut frames: Vec<String> = Vec::new();
    let mut levels: Vec<Level> = Vec::new();
    let mut items: Vec<Item> = Vec::new();
    for mut item in std::mem::take(&mut block.items) {
        let frame = if item.origin != Origin::Ai {
            None
        } else if super::checks::sentence_frame_only(&item.prompt, passage) {
            Some(item.prompt.trim().to_string())
        } else {
            super::checks::greeting_frame(&item.prompt, passage)
        };
        let Some(frame) = frame else {
            items.push(item);
            continue;
        };
        frames.push(frame);
        levels.extend(item.levels.iter().copied());
        let open = |i: &Item| {
            matches!(
                i.answer_format.as_deref(),
                Some(f) if f == super::FMT_SHORT || f == FMT_PARAGRAPH
            )
        };
        if items.last().is_some_and(open) {
            continue;
        }
        item.prompt = FRAME_TASK.to_string();
        if !open(&item) {
            item.answer_format = Some(super::FMT_SHORT.to_string());
        }
        items.push(item);
    }
    block.items = items;
    if frames.is_empty() {
        return 0;
    }
    let levels: Vec<Level> = Level::ALL
        .into_iter()
        .filter(|l| levels.contains(l))
        .collect();
    let content = frames.join("\n");
    match block.scaffolds.iter_mut().find(|s| {
        s.kind_type == ScaffoldType::Verbal && s.kind == KIND_FRAMES && s.levels == levels
    }) {
        Some(s) => s.content = format!("{}\n{content}", s.content.trim_end()),
        None => block.scaffolds.push(Scaffold {
            id: format!("{}-sf1", block.id),
            kind_type: ScaffoldType::Verbal,
            kind: KIND_FRAMES.to_string(),
            content,
            steps: Vec::new(),
            levels,
            auto: false,
            origin: Origin::Ai,
            heading: None,
        }),
    }
    frames.len()
}

// ---------------------------------------------------------------- one term per worksheet

/// Names of one idea a worksheet must not mix (a teacher found "informative writing" and
/// "expository writing" on one sheet; a real-model run wrote "informational text" for a sample
/// that says "informative writing"). Each group is one idea; its words are recognised before
/// one of [`TERM_NOUNS`] ("informative writing", "Expository texts").
const TERM_GROUPS: [&[&str]; 1] = [&["informative", "expository", "informational"]];
const TERM_NOUNS: &str =
    r"writing|texts?|essays?|articles?|paragraphs?|passages?|genre|style|structure|features?";

static TERM_PATTERNS: std::sync::LazyLock<Vec<regex::Regex>> = std::sync::LazyLock::new(|| {
    TERM_GROUPS
        .iter()
        .map(|group| {
            regex::Regex::new(&format!(
                r"(?i)\b({})(\s+(?:{TERM_NOUNS}))\b",
                group.join("|")
            ))
            .expect("valid regex")
        })
        .collect()
});

/// Every text of a block the student or teacher reads, except quotes from the material.
fn block_texts(b: &mut Block) -> Vec<&mut String> {
    let mut out: Vec<&mut String> = vec![&mut b.title, &mut b.instructions];
    for it in &mut b.items {
        out.push(&mut it.prompt);
        out.push(&mut it.answer);
        out.push(&mut it.hint);
        out.extend(it.options.iter_mut());
    }
    for s in &mut b.scaffolds {
        out.push(&mut s.content);
        out.extend(s.steps.iter_mut());
    }
    out
}

/// The same texts, to read.
fn block_strings(b: &Block) -> Vec<&str> {
    let mut out = vec![b.title.as_str(), b.instructions.as_str()];
    for it in &b.items {
        out.extend([it.prompt.as_str(), it.answer.as_str(), it.hint.as_str()]);
        out.extend(it.options.iter().map(String::as_str));
    }
    for s in &b.scaffolds {
        out.push(s.content.as_str());
        out.extend(s.steps.iter().map(String::as_str));
    }
    out
}

/// The texts of `blocks` ([`block_texts`]), one per line: a reference for [`unify_terms`].
pub fn blocks_text(blocks: &[Block]) -> String {
    blocks
        .iter()
        .flat_map(block_strings)
        .filter(|t| !t.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// How often each word of `group` names the idea in `text` (lower case).
fn term_counts(pattern: &regex::Regex, text: &str) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for c in pattern.captures_iter(text) {
        *counts.entry(c[1].to_lowercase()).or_insert(0) += 1;
    }
    counts
}

/// One name per idea across `blocks` ([`TERM_GROUPS`]): when the blocks use two words of a
/// group (or, with `mixed_only` false, a word another than the reference's), the one the first
/// of `reference` uses wins (a whole sheet: the template, then the material; one block made
/// again: the other blocks); with none there, the one the blocks use most (the first used on a
/// tie). The others are replaced, keeping a capital first letter. Blocks written in parallel
/// cannot see each other, so the model alone cannot keep this. Returns how many words were
/// replaced.
pub fn unify_terms(blocks: &mut [Block], reference: &[&str], mixed_only: bool) -> usize {
    let mut replaced = 0;
    for pattern in TERM_PATTERNS.iter() {
        let mut order: Vec<String> = Vec::new();
        let mut used: HashMap<String, usize> = HashMap::new();
        for b in blocks.iter() {
            for text in block_strings(b) {
                for c in pattern.captures_iter(text) {
                    let w = c[1].to_lowercase();
                    if !order.contains(&w) {
                        order.push(w.clone());
                    }
                    *used.entry(w).or_insert(0) += 1;
                }
            }
        }
        if order.is_empty() || (mixed_only && order.len() < 2) {
            continue;
        }
        let chosen = reference
            .iter()
            .find_map(|text| {
                let counts = term_counts(pattern, text);
                counts
                    .iter()
                    .max_by_key(|(w, n)| {
                        (**n, std::cmp::Reverse(order.iter().position(|o| o == *w)))
                    })
                    .map(|(w, _)| w.clone())
            })
            .unwrap_or_else(|| {
                order
                    .iter()
                    .max_by_key(|w| {
                        (
                            used[*w],
                            std::cmp::Reverse(order.iter().position(|o| o == *w)),
                        )
                    })
                    .cloned()
                    .unwrap_or_default()
            });
        for b in blocks.iter_mut() {
            for text in block_texts(b) {
                let new = pattern.replace_all(text, |c: &regex::Captures| {
                    let w = &c[1];
                    if w.to_lowercase() == chosen {
                        return format!("{w}{}", &c[2]);
                    }
                    replaced += 1;
                    let word = if w.starts_with(char::is_uppercase) {
                        let mut chars = chosen.chars();
                        chars
                            .next()
                            .map(|f| f.to_uppercase().chain(chars).collect::<String>())
                            .unwrap_or_default()
                    } else {
                        chosen.clone()
                    };
                    format!("{word}{}", &c[2])
                });
                if new != *text {
                    *text = new.into_owned();
                }
            }
        }
    }
    replaced
}

// ---------------------------------------------------------------- blocks from step outputs

pub fn activity_block(
    id: &str,
    raw: &RawBlock,
    material: &Material,
    levels: &[Level],
    objective_ids: &HashMap<String, String>,
) -> Block {
    let items = raw
        .items
        .iter()
        .enumerate()
        .filter_map(|(i, it)| {
            let shown = item_levels(&it.tier, levels);
            (!shown.is_empty()).then(|| Item {
                id: format!("{id}-i{}", i + 1),
                answer_format: Some(it.answer_format.clone()),
                prompt: it.prompt_en.clone(),
                options: it.options.clone(),
                answer: it.answer_en.clone(),
                evidence: it.evidence.clone().unwrap_or_default(),
                purpose: None,
                levels: shown,
                origin: Origin::Ai,
                bloom: it.bloom.as_deref().and_then(Bloom::parse),
                hint: String::new(),
                tiers: Vec::new(),
                table: it.table.as_ref().map(|t| t.to_item_table()),
                kept_options: Vec::new(),
                organizer: it
                    .organizer
                    .as_ref()
                    .map(|o| super::organizer::from_raw(o, false).0),
            })
        })
        .collect();
    // Q4: the block's stage, said to the teacher first in its notes.
    let practice = raw.practice.as_deref().and_then(Practice::from_label);
    let teacher_notes = match practice {
        Some(p) if raw.design_reason_zh.trim().is_empty() => p.note().to_string(),
        Some(p) => format!("{}\n{}", p.note(), raw.design_reason_zh),
        None => raw.design_reason_zh.clone(),
    };
    Block {
        id: id.to_string(),
        kind: BlockKind::Activity,
        title: raw.title_en.clone(),
        instructions: raw.instructions_en.clone(),
        paragraph_ids: raw
            .paragraph_nos
            .iter()
            .filter_map(|no| material.id_of(*no))
            .map(str::to_string)
            .collect(),
        info_function: info_function_from_label(&raw.info_function),
        student_tasks: raw.student_tasks.clone(),
        objective_ids: raw
            .objective_refs
            .iter()
            .filter_map(|r| objective_ids.get(r).cloned())
            .collect(),
        items,
        teacher_notes,
        origin: Origin::Ai,
        // Cleaned here (never a reason for a repair call); none unless pictures are on.
        image_slots: super::images::with_ids(
            super::images::clean_slots(&raw.image_slots, super::images::MAX_SLOTS_PER_TASK_BLOCK).0,
            id,
        ),
        practice,
        ..Block::default()
    }
}

fn vocab_of(w: &CleanWord, material: &Material) -> Vocab {
    Vocab {
        word: w.word.clone(),
        level: w.level,
        definition: w.definition.clone(),
        context: w.context.clone(),
        paragraph_id: w
            .paragraph_no
            .and_then(|no| material.id_of(no))
            .map(str::to_string),
        task_word: false,
    }
}

pub fn warmup_block(warmup: &CleanWarmup, material: &Material, levels: &[Level]) -> Block {
    let mut items = Vec::new();
    let mut student_tasks: Vec<String> = Vec::new();
    let mut notes = Vec::new();
    for a in &warmup.activities {
        for p in &a.prompts {
            items.push(Item {
                id: format!("{WARMUP_ID}-i{}", items.len() + 1),
                answer_format: a.answer_format.clone(),
                prompt: p.clone(),
                levels: levels.to_vec(),
                origin: Origin::Ai,
                ..Item::default()
            });
        }
        for t in &a.student_tasks {
            if !student_tasks.contains(t) {
                student_tasks.push(t.clone());
            }
        }
        if !a.purpose_zh.is_empty() {
            notes.push(a.purpose_zh.clone());
        }
    }
    let first = warmup.activities.first();
    Block {
        id: WARMUP_ID.to_string(),
        kind: BlockKind::Warmup,
        title: "Warm-up".to_string(),
        instructions: first.map(|a| a.instructions.clone()).unwrap_or_default(),
        student_tasks,
        vocabulary: warmup.words.iter().map(|w| vocab_of(w, material)).collect(),
        items,
        teacher_notes: notes.join("；"),
        origin: Origin::Ai,
        ..Block::default()
    }
}

pub fn exit_block(
    items: &[RawExitItem],
    levels: &[Level],
    objective_ids: &HashMap<String, String>,
) -> Block {
    let purpose = |label: &str| -> ExitKind {
        EXIT_PURPOSES
            .iter()
            .find(|(_, l)| *l == label)
            .map(|(k, _)| *k)
            .unwrap_or_default()
    };
    let mut student_tasks: Vec<String> = Vec::new();
    let mut objectives: Vec<String> = Vec::new();
    for it in items {
        for t in &it.student_tasks {
            if !student_tasks.contains(t) {
                student_tasks.push(t.clone());
            }
        }
        for r in &it.objective_refs {
            if let Some(id) = objective_ids.get(r) {
                if !objectives.contains(id) {
                    objectives.push(id.clone());
                }
            }
        }
    }
    Block {
        id: EXIT_ID.to_string(),
        kind: BlockKind::ExitTicket,
        title: "Exit ticket".to_string(),
        instructions: "Answer these before you leave.".to_string(),
        student_tasks,
        objective_ids: objectives,
        items: items
            .iter()
            .enumerate()
            .map(|(i, it)| Item {
                id: format!("{EXIT_ID}-i{}", i + 1),
                answer_format: Some(it.answer_format.clone()),
                prompt: it.prompt_en.clone(),
                options: it.options.clone(),
                answer: it.sample_answer_en.clone().unwrap_or_default(),
                evidence: String::new(),
                purpose: Some(purpose(&it.purpose)),
                levels: levels.to_vec(),
                origin: Origin::Ai,
                ..Item::default()
            })
            .collect(),
        origin: Origin::Ai,
        ..Block::default()
    }
}

/// An empty block standing in for a step that did not deliver: it says why and can be written
/// by hand or regenerated.
pub fn placeholder(kind: BlockKind, id: &str, message: &str, material: &Material) -> Block {
    Block {
        id: id.to_string(),
        kind,
        title: match kind {
            BlockKind::Warmup => "Warm-up".to_string(),
            BlockKind::ExitTicket => "Exit ticket".to_string(),
            BlockKind::Activity => "Main tasks".to_string(),
        },
        paragraph_ids: if kind == BlockKind::Activity {
            material.paragraphs.iter().map(|p| p.id.clone()).collect()
        } else {
            Vec::new()
        },
        origin: Origin::Teacher,
        message: Some(message.to_string()),
        ..Block::default()
    }
}

/// The scaffold target a block was sent to the model as.
pub fn target_of(block: &Block) -> String {
    if block.kind == BlockKind::Warmup {
        TARGET_WARMUP.to_string()
    } else {
        block.id.clone()
    }
}

// ---------------------------------------------------------------- projection: one version, one audience

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SheetWord {
    pub word: String,
    pub level: Option<u8>,
    pub definition: String,
    /// A word of the instructions whose `definition` is a Chinese hint (SIOP V2).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub task_word: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SheetSupport {
    #[serde(rename = "type")]
    pub kind_type: ScaffoldType,
    pub kind: String,
    /// English heading printed on the sheet (the kind labels themselves are Chinese): the
    /// scaffold's own heading when it has one (a template block prints the sample's, e.g.
    /// "Directions", "Sentence Frame for Sharing"), otherwise the kind's ([`support_heading`]).
    pub heading: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SheetItem {
    /// The question's id in the worksheet (notes on a question are anchored to it).
    pub id: String,
    pub no: usize,
    pub answer_format: Option<String>,
    pub prompt: String,
    /// The choices printed under the question: only for the formats that use them
    /// ([`uses_choices`]), without empty ones.
    pub options: Vec<String>,
    pub lines: u8,
    /// 表格整理 only: the table printed for the answer (absent for every other format).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table: Option<SheetTable>,
    /// Absent from the student version: the key itself is not serialised.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
    /// Teacher version only: the question's Bloom level (`null` when it has none). Absent from
    /// the student version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bloom: Option<Option<Bloom>>,
    /// 基礎版 only (both audiences): the hint printed under the question (a question without
    /// versions of its own; see `support`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    /// 差異化學習單 (both audiences): this version's support under the question; absent when the
    /// version gives none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub support: Option<SheetItemSupport>,
    /// 差異化學習單, teacher version only: how to score this version's answer (評分說明).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scoring: Option<String>,
    /// 圖像組織: the organizer this version prints (the student's empty or partly pre-filled, the
    /// teacher's with the answers), drawn instead of answer lines; absent for every other
    /// question and for an organizer question without one (which prints an empty box).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub organizer: Option<super::organizer::SheetOrganizer>,
}

/// The answer table of a 表格整理 question as printed: a heading row (when any heading is
/// filled in) over `rows` empty rows, one cell per column.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SheetTable {
    /// The headings across the top. A table with row labels has one more column than the
    /// question's: the first (empty heading) holds the labels.
    pub columns: Vec<String>,
    pub rows: u8,
    /// What each row asks for, printed in the first column (`rows` of them; empty: free rows).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub row_labels: Vec<String>,
}

impl SheetTable {
    /// The table as printed: headings and labels trimmed, a first column for the labels when
    /// any row has one.
    pub fn of(t: &super::ItemTable) -> Self {
        let mut columns: Vec<String> = t.columns.iter().map(|c| c.trim().to_string()).collect();
        let mut row_labels: Vec<String> = t
            .row_labels
            .iter()
            .take(usize::from(t.rows))
            .map(|l| l.trim().to_string())
            .collect();
        if row_labels.iter().all(String::is_empty) {
            row_labels.clear();
        } else {
            row_labels.resize(usize::from(t.rows), String::new());
            columns.insert(0, String::new());
        }
        Self {
            columns,
            rows: t.rows,
            row_labels,
        }
    }

    /// Whether the first row prints headings (a table without any prints only empty rows).
    pub fn has_headings(&self) -> bool {
        self.columns.iter().any(|c| !c.is_empty())
    }
}

/// One version's support under a question of a 差異化學習單.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SheetItemSupport {
    pub kind: SupportKind,
    /// English heading printed above it: 「Sentence frame」, 「Word bank」…
    pub heading: &'static str,
    /// The lines (a cloze or frame has 「___」 blanks).
    pub lines: Vec<String>,
    /// The word bank, in alphabetical order (a cloze's words in blank order would give the
    /// answer away). Never with a sentence starter (T3: a bank is more support than a starter);
    /// a bank the version keeps from another form stays in the worksheet, unprinted.
    pub word_bank: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SheetBlock {
    pub id: String,
    pub kind: BlockKind,
    /// "A", "B" …; empty for the exit ticket.
    pub label: String,
    pub title: String,
    pub instructions: String,
    pub steps: Vec<String>,
    pub vocabulary: Vec<SheetWord>,
    pub supports: Vec<SheetSupport>,
    pub items: Vec<SheetItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub teacher_notes: Option<String>,
    /// Teacher version only: 「Bloom：記憶 2、理解 1」 for the questions of this version, when any
    /// of them has a Bloom level.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bloom_summary: Option<String>,
    /// The block's pictures, printed after its instructions (filled in by
    /// `images::decorate`; absent when there are none).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<SheetImage>,
    /// The block's original paragraphs, printed when this version also prints a simplified text
    /// (T8: the rewrite is an aid, it never replaces the original). Empty otherwise.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub original_text: Vec<String>,
    /// The block's Glossary in this version (詞彙級別檢查「提供字義」), printed at the end of the
    /// block; absent when there is none.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub glosses: Vec<SheetGloss>,
}

/// One printed Glossary entry: 「abandon (v.) 拋棄 — to leave and never come back」.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SheetGloss {
    pub word: String,
    pub pos: String,
    pub meaning_zh: String,
    pub definition_en: String,
}

impl SheetGloss {
    /// The entry after its word: 「(v.) 拋棄 — to leave …」.
    pub fn text(&self) -> String {
        let mut out = String::new();
        if !self.pos.is_empty() {
            out.push_str(&format!("({}) ", self.pos));
        }
        out.push_str(&self.meaning_zh);
        if !self.definition_en.is_empty() {
            out.push_str(&format!(" — {}", self.definition_en));
        }
        out
    }
}

/// One picture of a printed block: the picture, or an empty box for the teacher to fill.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SheetImage {
    pub slot_id: String,
    /// queued | running | done | failed | skipped (removed slots are not printed)
    pub status: String,
    /// With `done`: `GET /api/worksheets/:id/images/:image_id`.
    pub image_id: Option<i64>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub alt: String,
    /// Printed under the picture ("" for none).
    pub caption: String,
    /// Made by the picture model (the teacher version labels it).
    pub ai_generated: bool,
    /// Teacher version only: what the picture is for (printed in an empty box).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
    /// Teacher version only: why there is no picture.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// The scaffold kind of a simplified text (「調整文本」, printed as 「Simplified text」).
pub const KIND_SIMPLIFIED: &str = "調整文本";

/// T8: a version that prints a simplified text keeps the original paragraphs beside it. Fills
/// `original_text` of every such block with the paragraphs it covers (all of them when it names
/// none). Applied by everything that prints a sheet and has the material (the sheet, the
/// exports, the kit); the projection itself does not carry the material.
pub fn keep_original_text(sheet: &mut Sheet, content: &Content, material: &Material) {
    for sb in &mut sheet.blocks {
        if !sb.supports.iter().any(|s| s.kind == KIND_SIMPLIFIED) {
            continue;
        }
        let ids: Vec<&str> = content
            .block(&sb.id)
            .map(|b| b.paragraph_ids.iter().map(String::as_str).collect())
            .unwrap_or_default();
        sb.original_text = material
            .paragraphs
            .iter()
            .filter(|p| ids.is_empty() || ids.contains(&p.id.as_str()))
            .map(|p| p.text.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect();
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Sheet {
    pub title: String,
    pub level: Level,
    pub audience: Audience,
    /// The neutral code this version carries on paper, 「A」「B」「C」 (T9: students never see the
    /// level's name); `null` when the worksheet has only one version.
    pub version_code: Option<String>,
    /// Teacher version of a 差異化學習單 only: why the versions differ (分層說明).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tiering_note: Option<String>,
    pub objectives: Vec<String>,
    pub blocks: Vec<SheetBlock>,
    /// 字詞表放在學習單最後: the word tables of the blocks, as one table printed after the last
    /// block (a word listed in two blocks once). Empty when the tables stay in their blocks.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub end_words: Vec<SheetWord>,
}

/// One version's support, as printed; `None` when it gives none.
fn item_support(tier: &super::ItemTier) -> Option<SheetItemSupport> {
    if tier.support_kind == SupportKind::None {
        return None;
    }
    let lines: Vec<String> = tier
        .support
        .iter()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    let mut word_bank: Vec<String> = if tier.support_kind == SupportKind::SentenceStarter {
        Vec::new()
    } else {
        tier.word_bank
            .iter()
            .map(|w| w.trim().to_string())
            .filter(|w| !w.is_empty())
            .collect()
    };
    word_bank.sort_by_key(|w| w.to_lowercase());
    (!lines.is_empty() || !word_bank.is_empty()).then(|| SheetItemSupport {
        kind: tier.support_kind,
        heading: tier.support_kind.heading(),
        lines,
        word_bank,
    })
}

/// The printed heading of each scaffold kind; any other kind prints under [`SUPPORT_FALLBACK`].
pub const SUPPORT_HEADINGS: [(&str, &str); 12] = [
    (KIND_GUIDING, "Guiding questions"),
    (KIND_WORD_LIST, "Word bank"),
    (KIND_FRAMES, "Sentence frames"),
    (KIND_EXAMPLE, "Example"),
    ("改述或重述", "In other words"),
    ("思考放聲", "Think aloud"),
    ("提示與追問", "Hints"),
    ("透過上下文解釋詞義", "Words in context"),
    ("圖像組織", "Organizer"),
    ("部分完成的大綱或內容", "Outline"),
    ("調整文本", "Simplified text"),
    ("標示關鍵概念或字彙", "Key words"),
];
pub const SUPPORT_FALLBACK: &str = "Support";

pub fn support_heading(kind: &str) -> &'static str {
    SUPPORT_HEADINGS
        .iter()
        .find(|(k, _)| *k == kind)
        .map_or(SUPPORT_FALLBACK, |(_, heading)| heading)
}

/// A support's text without a leading copy of the heading it is printed under. The model often
/// starts a word bank with 「Word bank:」, which under the heading 「Word bank」 reads twice
/// (「Word bank: Word Bank: reason, …」). The heading goes, in any case, when a colon follows it
/// or it stands alone on the first line; a heading word that begins a sentence stays
/// ("Example (a different question): …", "Outline the main idea …").
pub fn without_heading<'a>(content: &'a str, heading: &str) -> &'a str {
    let content = content.trim();
    let starts_with_heading = content
        .get(..heading.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(heading));
    if !starts_with_heading {
        return content;
    }
    let after = content[heading.len()..].trim_start_matches([' ', '\t']);
    if let Some(rest) = after.strip_prefix([':', '：']) {
        rest.trim()
    } else if after.is_empty() || after.starts_with(['\n', '\r']) {
        after.trim()
    } else {
        content
    }
}

/// The label printed before the `index`-th choice of a question: 「(A)」「(B)」… (the sheet's
/// convention: DOCX, PDF, the editor and the preview all print it; options are stored without it).
pub fn option_label(index: usize) -> String {
    format!("({})", (b'A' + (index % 26) as u8) as char)
}

/// A letter label at the start of a choice the model (or a teacher) wrote: 「(A) …」「（A）…」,
/// 「A. …」「A) …」「A: …」 (a space after these) or 「A．…」「A、…」「A：…」. The web uses the same
/// pattern (`OPTION_LABEL` in web/src/lib/worksheet.ts).
static OPTION_LABEL: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(
        r"^[ \t]*(?:[(（][ \t]*([A-Za-z])[ \t]*[)）][ \t]*|([A-Za-z])(?:[.):][ \t]+|[．、：][ \t]*))([^ \t][\s\S]*)$",
    )
    .expect("valid regex")
});

/// The letter and the text of a choice that starts with a letter label, if it does.
fn split_option_label(text: &str) -> Option<(char, &str)> {
    let c = OPTION_LABEL.captures(text)?;
    let letter = c.get(1).or(c.get(2))?.as_str().chars().next()?;
    Some((letter.to_ascii_uppercase(), c.get(3)?.as_str()))
}

/// The `index`-th choice without a letter label of its own (「A. Paris」 as the first choice is
/// 「Paris」), so that it is not lettered twice. Only the label of its own position goes: 「B. 1066」
/// as the first choice, or 「A.D. 1066」 (no space after the dot), stays as it is.
pub fn without_option_label(text: &str, index: usize) -> &str {
    let own = (b'A' + (index % 26) as u8) as char;
    match split_option_label(text) {
        Some((letter, rest)) if letter == own => rest,
        _ => text,
    }
}

/// The `index`-th choice as printed: 「(A) Paris」.
pub fn printed_option(text: &str, index: usize) -> String {
    format!(
        "{} {}",
        option_label(index),
        without_option_label(text, index)
    )
}

/// The answer of a 選擇題 as the teacher version prints it: with the label of the choice it is
/// (「(B) London」), so the key matches the letters on the sheet. Answers are stored as the
/// choice's text (checks.rs); one written with a label of its own, or in another case, is still
/// recognised. Any other answer, and the answer of every other format, prints as it is.
pub fn printed_answer(format: Option<&str>, answer: &str, options: &[String]) -> String {
    if format != Some(FMT_CHOICE) {
        return answer.to_string();
    }
    let bare = split_option_label(answer)
        .map_or(answer, |(_, rest)| rest)
        .trim();
    options
        .iter()
        .enumerate()
        .map(|(i, o)| (i, without_option_label(o, i).trim()))
        .find(|(_, o)| !o.is_empty() && o.to_lowercase() == bare.to_lowercase())
        .map_or_else(
            || answer.to_string(),
            |(i, o)| format!("{} {o}", option_label(i)),
        )
}

pub(crate) fn letter(index: usize) -> String {
    let c = (b'A' + (index % 26) as u8) as char;
    if index < 26 {
        c.to_string()
    } else {
        format!("{c}{}", index / 26 + 1)
    }
}

/// Whether a projected block has anything to print besides its heading. A step that failed
/// leaves an empty block with a message for the editor; on paper it would only be a heading
/// over nothing, so it is left out of the sheet.
fn has_content(b: &SheetBlock) -> bool {
    !(b.items.is_empty()
        && b.steps.is_empty()
        && b.vocabulary.is_empty()
        && b.supports.is_empty()
        && b.instructions.is_empty()
        && b.teacher_notes.is_none())
}

/// The sheet one audience gets for one version. Switched-off blocks and blocks with nothing to
/// print in this version are left out, items and scaffolds follow their `levels`, items are
/// numbered through the whole sheet. Answers, evidence and teacher notes exist only for
/// [`Audience::Teacher`].
pub fn project(title: &str, content: &Content, level: Level, audience: Audience) -> Sheet {
    let teacher = audience == Audience::Teacher;
    let mut no = 0;
    let mut blocks = Vec::new();
    for b in content.blocks.iter().filter(|b| b.enabled) {
        // Lettered below, once the blocks that print are known.
        let label = String::new();
        let shown = |levels: &[Level]| levels.contains(&level);
        // A block of one Bloom level whose questions are all in other versions (分層練習 made
        // before the 差異化學習單 redesign) is not part of this one.
        if b.bloom.is_some() && !b.items.is_empty() && !b.items.iter().any(|i| shown(&i.levels)) {
            continue;
        }
        let items = b
            .items
            .iter()
            .filter(|i| shown(&i.levels))
            .map(|i| {
                no += 1;
                // A 差異化學習單 question: this version's support, answer and scoring note.
                let tier = i.tier(level);
                let answer = tier.map_or(i.answer.trim(), |t| t.answer.trim());
                let organizer = i.printed_organizer().map(|o| o.project(level, teacher));
                SheetItem {
                    id: i.id.clone(),
                    no,
                    answer_format: i.answer_format.clone(),
                    prompt: i.prompt.trim().to_string(),
                    options: if uses_choices(i.answer_format.as_deref()) {
                        i.options
                            .iter()
                            .map(|o| o.trim().to_string())
                            .filter(|o| !o.is_empty())
                            .collect()
                    } else {
                        Vec::new()
                    },
                    lines: answer_lines(i.answer_format.as_deref(), level),
                    table: (i.answer_format.as_deref() == Some(FMT_TABLE))
                        .then(|| SheetTable::of(&i.table_or_default())),
                    answer: (teacher && !answer.is_empty()).then(|| answer.to_string()),
                    evidence: (teacher && !i.evidence.trim().is_empty())
                        .then(|| i.evidence.trim().to_string()),
                    bloom: teacher.then_some(i.bloom),
                    hint: (tier.is_none() && level == Level::Basic && !i.hint.trim().is_empty())
                        .then(|| i.hint.trim().to_string()),
                    support: tier.and_then(item_support),
                    scoring: tier
                        .filter(|t| teacher && !t.scoring.trim().is_empty())
                        .map(|t| t.scoring.trim().to_string()),
                    organizer,
                }
            })
            .collect::<Vec<SheetItem>>();
        let bloom_summary = teacher
            .then(|| {
                let counts: Vec<(Bloom, usize)> = Bloom::ALL
                    .into_iter()
                    .map(|bl| {
                        let n = items.iter().filter(|it| it.bloom == Some(Some(bl))).count();
                        (bl, n)
                    })
                    .filter(|(_, n)| *n > 0)
                    .collect();
                (!counts.is_empty()).then(|| super::bloom::summary(&counts))
            })
            .flatten();
        let scaffolds: Vec<&Scaffold> = b.scaffolds.iter().filter(|s| shown(&s.levels)).collect();
        blocks.push(SheetBlock {
            id: b.id.clone(),
            kind: b.kind,
            label,
            title: b.title.trim().to_string(),
            instructions: b.instructions.trim().to_string(),
            // A procedural scaffold with a heading of its own is a box (below).
            steps: scaffolds
                .iter()
                .filter(|s| s.kind_type == ScaffoldType::Procedural && s.own_heading().is_none())
                .flat_map(|s| s.steps.iter())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            vocabulary: b
                .vocabulary
                .iter()
                .filter(|v| !v.word.trim().is_empty())
                .map(|v| SheetWord {
                    word: v.word.trim().to_string(),
                    level: v.level,
                    definition: v.definition.trim().to_string(),
                    task_word: v.task_word,
                })
                .collect(),
            // The one place a support's heading meets its text: every printed form (preview,
            // 複製, DOCX, PDF) shows the heading once.
            supports: scaffolds
                .iter()
                .filter(|s| s.kind_type != ScaffoldType::Procedural || s.own_heading().is_some())
                .filter_map(|s| {
                    let heading = s.own_heading().unwrap_or_else(|| support_heading(&s.kind));
                    let text = if s.kind_type == ScaffoldType::Procedural {
                        s.steps
                            .iter()
                            .map(|l| l.trim())
                            .filter(|l| !l.is_empty())
                            .collect::<Vec<_>>()
                            .join("\n")
                    } else {
                        s.content.clone()
                    };
                    let content = without_heading(&text, heading);
                    (!content.is_empty()).then(|| SheetSupport {
                        kind_type: s.kind_type,
                        kind: s.kind.clone(),
                        heading: heading.to_string(),
                        content: content.to_string(),
                    })
                })
                .collect(),
            items,
            teacher_notes: (teacher && !b.teacher_notes.trim().is_empty())
                .then(|| b.teacher_notes.trim().to_string()),
            bloom_summary,
            images: Vec::new(),
            original_text: Vec::new(),
            glosses: b
                .glosses
                .iter()
                .filter(|g| shown(&g.levels) && !g.word.trim().is_empty())
                .map(|g| SheetGloss {
                    word: g.word.trim().to_string(),
                    pos: g.pos.trim().to_string(),
                    meaning_zh: g.meaning_zh.trim().to_string(),
                    definition_en: g.definition_en.trim().to_string(),
                })
                .collect(),
        });
    }
    blocks.retain(has_content);
    let mut lettered = 0;
    for b in &mut blocks {
        if b.kind != BlockKind::ExitTicket {
            b.label = letter(lettered);
            lettered += 1;
            // 「Part B · Look Closer」 follows the printed letter.
            if let Some((_, name)) = super::bloom::split_part_title(&b.title) {
                b.title = format!("Part {} · {name}", b.label);
            }
        }
    }
    let differentiated = content
        .blocks
        .iter()
        .any(|b| b.enabled && b.items.iter().any(|i| !i.tiers.is_empty()));
    // 字詞表放在學習單最後: the tables leave their blocks (a block that only had a table was kept
    // above) and print once, after the last block.
    let mut end_words: Vec<SheetWord> = Vec::new();
    if content.vocabulary_at_end {
        for b in &mut blocks {
            for w in std::mem::take(&mut b.vocabulary) {
                let key = w.word.to_lowercase();
                if !end_words.iter().any(|x| x.word.to_lowercase() == key) {
                    end_words.push(w);
                }
            }
        }
    }
    Sheet {
        title: title.trim().to_string(),
        level,
        audience,
        version_code: tiered::version_code(&content.levels, &content.student_order, level),
        tiering_note: (teacher && differentiated).then(|| tiered::TIERED_REASON.to_string()),
        objectives: if content.show_objectives {
            content
                .objectives
                .iter()
                .map(|o| o.student_facing_en.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect()
        } else {
            Vec::new()
        },
        blocks,
        end_words,
    }
}

/// The name of a block in the teacher's words, e.g. 「A 暖身」.
pub fn block_name(b: &SheetBlock) -> String {
    let kind = match b.kind {
        BlockKind::Warmup => "暖身",
        BlockKind::Activity => "活動",
        BlockKind::ExitTicket => "Exit ticket",
    };
    if b.label.is_empty() {
        kind.to_string()
    } else {
        format!("{} {kind}", b.label)
    }
}

/// What kind of printed text a line of the vocabulary check is: says whether 換字 may change it
/// (詞彙級別檢查: the questions, instructions and options may change; the title, the objectives,
/// the key words, word banks, simplified texts and captions stay as they are).
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UnitKind {
    Title,
    Objective,
    BlockTitle,
    Instructions,
    Step,
    /// 字詞表: the key words the lesson teaches.
    KeyWord,
    Support,
    /// 調整文本: a rewrite of the reading text (T8).
    SimplifiedText,
    Caption,
    Question,
    Option,
    /// 表格整理: a question's table column headings.
    TableHeading,
    /// 圖像組織: an organizer's title and its section labels (column headings, row labels, box
    /// names): changed like the question they belong to, as table headings are.
    OrganizerHeading,
    /// 圖像組織: a mind map's centre word and the text pre-filled in a cell (and, in the teacher
    /// version, a cell's answer): what the question is about and its answers, never changed.
    OrganizerContent,
    Hint,
    ItemSupport,
    /// A word bank (a question's, or a 字詞表 support): the words to answer with.
    WordBank,
}

impl UnitKind {
    /// Whether 換字 may change text of this kind.
    pub fn replaceable(self) -> bool {
        matches!(
            self,
            Self::BlockTitle
                | Self::Instructions
                | Self::Step
                | Self::Support
                | Self::Question
                | Self::Option
                | Self::TableHeading
                | Self::OrganizerHeading
                | Self::Hint
                | Self::ItemSupport
        )
    }
}

/// One printed text of a sheet (for the vocabulary check).
#[derive(Debug, Clone, PartialEq)]
pub struct TextUnit {
    /// Where it is, in the teacher's words: 「C 活動 · 第 4 題」.
    pub place: String,
    /// Line breaks inside the text become spaces.
    pub text: String,
    pub kind: UnitKind,
}

/// Every printed text of a sheet, one unit per line, with where it is and what kind of text it
/// is. The Glossary is not part of it: it explains words, it is not reading load.
pub fn text_units(sheet: &Sheet) -> Vec<TextUnit> {
    let mut lines: Vec<TextUnit> = Vec::new();
    let mut push = |place: String, text: &str, kind: UnitKind| {
        // One unit per line: line breaks inside a text become spaces.
        let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if !flat.is_empty() {
            lines.push(TextUnit {
                place,
                text: flat,
                kind,
            });
        }
    };
    push("標題".to_string(), &sheet.title, UnitKind::Title);
    for (i, o) in sheet.objectives.iter().enumerate() {
        push(format!("學習目標第 {} 項", i + 1), o, UnitKind::Objective);
    }
    for b in &sheet.blocks {
        let name = block_name(b);
        push(format!("{name} · 標題"), &b.title, UnitKind::BlockTitle);
        push(
            format!("{name} · 作答指示"),
            &b.instructions,
            UnitKind::Instructions,
        );
        for (i, s) in b.steps.iter().enumerate() {
            push(format!("{name} · 步驟 {}", i + 1), s, UnitKind::Step);
        }
        for v in &b.vocabulary {
            push(
                format!("{name} · 字詞表"),
                &format!("{} {}", v.word, v.definition),
                UnitKind::KeyWord,
            );
        }
        for s in &b.supports {
            let kind = match s.kind.as_str() {
                KIND_SIMPLIFIED => UnitKind::SimplifiedText,
                // A word bank lists the words to answer with.
                KIND_WORD_LIST => UnitKind::WordBank,
                _ => UnitKind::Support,
            };
            push(format!("{name} · {}", s.kind), &s.content, kind);
        }
        for img in &b.images {
            push(format!("{name} · 圖說"), &img.caption, UnitKind::Caption);
        }
        for it in &b.items {
            push(
                format!("{name} · 第 {} 題", it.no),
                &it.prompt,
                UnitKind::Question,
            );
            for o in &it.options {
                push(format!("{name} · 第 {} 題選項", it.no), o, UnitKind::Option);
            }
            if let Some(t) = &it.table {
                push(
                    format!("{name} · 第 {} 題表格", it.no),
                    &t.columns.join(" "),
                    UnitKind::TableHeading,
                );
            }
            if let Some(h) = &it.hint {
                push(format!("{name} · 第 {} 題提示", it.no), h, UnitKind::Hint);
            }
            if let Some(o) = &it.organizer {
                // In the order the organizer prints them (as `SheetOrganizer::texts`).
                let place = format!("{name} · 第 {} 題圖像組織", it.no);
                push(format!("{place}標題"), &o.title, UnitKind::OrganizerHeading);
                push(
                    format!("{place}中心詞"),
                    &o.center,
                    UnitKind::OrganizerContent,
                );
                for s in &o.sections {
                    push(format!("{place}標籤"), &s.label, UnitKind::OrganizerHeading);
                    for c in &s.cells {
                        push(format!("{place}預填"), &c.given, UnitKind::OrganizerContent);
                        if let Some(a) = &c.answer {
                            push(format!("{place}答案"), a, UnitKind::OrganizerContent);
                        }
                    }
                }
            }
            if let Some(s) = &it.support {
                for line in &s.lines {
                    push(
                        format!("{name} · 第 {} 題支持", it.no),
                        line,
                        UnitKind::ItemSupport,
                    );
                }
                if !s.word_bank.is_empty() {
                    push(
                        format!("{name} · 第 {} 題字庫", it.no),
                        &s.word_bank.join(", "),
                        UnitKind::WordBank,
                    );
                }
            }
        }
    }
    for v in &sheet.end_words {
        push(
            "字詞表".to_string(),
            &format!("{} {}", v.word, v.definition),
            UnitKind::KeyWord,
        );
    }
    lines
}

/// Every printed text of a sheet, one unit per line, with where it is (for the vocabulary
/// check: positions are reported by line).
pub fn text_lines(sheet: &Sheet) -> Vec<(String, String)> {
    text_units(sheet)
        .into_iter()
        .map(|u| (u.place, u.text))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same cases as `printedOption` / `printedAnswer` in
    /// web/src/__tests__/worksheetPrint.test.ts: the preview and the exports letter alike.
    #[test]
    fn choices_print_lettered_once() {
        for (text, index, printed) in [
            ("Paris", 0, "(A) Paris"),
            ("London", 1, "(B) London"),
            ("A. Paris", 0, "(A) Paris"),
            ("(A) Paris", 0, "(A) Paris"),
            ("（B）London", 1, "(B) London"),
            ("( c ) Rome", 2, "(C) Rome"),
            ("b) London", 1, "(B) London"),
            ("D: Tokyo", 3, "(D) Tokyo"),
            ("C．Rome", 2, "(C) Rome"),
            ("C、Rome", 2, "(C) Rome"),
            // Not its own position, no space after the dot, or nothing after the label: kept.
            ("B. London", 0, "(A) B. London"),
            ("A.D. 1066", 0, "(A) A.D. 1066"),
            ("A lot of rain", 0, "(A) A lot of rain"),
            ("A.", 0, "(A) A."),
            ("(A)", 0, "(A) (A)"),
            ("True", 0, "(A) True"),
        ] {
            assert_eq!(printed_option(text, index), printed, "{text:?} at {index}");
        }
        assert_eq!(option_label(0), "(A)");
        assert_eq!(option_label(11), "(L)");
    }

    #[test]
    fn a_choice_answer_prints_with_its_letter() {
        let options: Vec<String> = ["Paris", "London", "Rome", "Tokyo"]
            .map(String::from)
            .to_vec();
        let lettered: Vec<String> = ["A. Paris", "B. London", "C. Rome", "D. Tokyo"]
            .map(String::from)
            .to_vec();
        for (format, answer, opts, printed) in [
            (Some(FMT_CHOICE), "London", &options, "(B) London"),
            (Some(FMT_CHOICE), " london ", &options, "(B) London"),
            (Some(FMT_CHOICE), "B. London", &options, "(B) London"),
            (Some(FMT_CHOICE), "(D) Tokyo", &options, "(D) Tokyo"),
            (Some(FMT_CHOICE), "Rome", &lettered, "(C) Rome"),
            // Not one of the choices (or only a letter): as written.
            (Some(FMT_CHOICE), "Berlin", &options, "Berlin"),
            (Some(FMT_CHOICE), "B", &options, "B"),
            // Other formats keep their answer.
            (Some(FMT_TRUE_FALSE), "True", &options, "True"),
            (Some("簡答題"), "London", &options, "London"),
            (None, "London", &options, "London"),
        ] {
            assert_eq!(
                printed_answer(format, answer, opts),
                printed,
                "{format:?} {answer:?}"
            );
        }
    }

    #[test]
    fn a_frame_written_as_a_question_prints_in_the_sentence_frames_box() {
        const FRAME: &str = "Hi! In the 3rd paragraph, the author describes ______. After reading it carefully, I identified ______ as one of the main characteristics of narrative writing.";
        let frame = |id: &str, format: &str| Item {
            prompt: FRAME.into(),
            ..item(id, &Level::ALL, format)
        };
        let question = |id: &str, format: &str, prompt: &str| Item {
            prompt: prompt.into(),
            ..item(id, &Level::ALL, format)
        };
        let content = |blocks: Vec<Block>| Content {
            blocks,
            levels: Level::ALL.to_vec(),
            ..Content::default()
        };

        // After an open question: the frame is that question's, and its number goes.
        let mut after_open = Block {
            id: "b1".into(),
            enabled: true,
            items: vec![
                question("q1", "簡答題", "What makes paragraph 3 a narrative?"),
                frame("q2", FMT_BLANK),
                question("q3", FMT_CHOICE, "Which word is a verb?"),
            ],
            ..Block::default()
        };
        assert_eq!(lift_frames(&mut after_open, ""), 1);
        let ids: Vec<&str> = after_open.items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, ["q1", "q3"]);
        let s = &after_open.scaffolds[0];
        assert_eq!(
            (s.kind_type, s.kind.as_str(), s.content.as_str()),
            (ScaffoldType::Verbal, KIND_FRAMES, FRAME)
        );
        assert_eq!(s.levels, Level::ALL);

        // Otherwise the question stays (an exit ticket keeps its count) and asks for the frame.
        let mut alone = Block {
            id: "exit".into(),
            kind: BlockKind::ExitTicket,
            enabled: true,
            items: vec![
                frame("e1", FMT_BLANK),
                question("e2", "簡答題", "Which part was the hardest for you?"),
            ],
            ..Block::default()
        };
        assert_eq!(lift_frames(&mut alone, ""), 1);
        assert_eq!(alone.items[0].prompt, FRAME_TASK);
        assert_eq!(alone.items[0].answer_format.as_deref(), Some("簡答題"));
        assert_eq!(alone.items[0].answer, "Answer e1", "its answer stays");

        // The teacher's own frame is hers.
        let mut mine = Block {
            id: "b2".into(),
            enabled: true,
            items: vec![Item {
                origin: Origin::Teacher,
                ..frame("t1", FMT_BLANK)
            }],
            ..Block::default()
        };
        assert_eq!(lift_frames(&mut mine, ""), 0);
        assert!(mine.scaffolds.is_empty());

        // On paper: one box, not numbered; the questions numbered 1, 2, 3 through the sheet.
        let sheet = project(
            "T",
            &content(vec![after_open, alone]),
            Level::Standard,
            Audience::Student,
        );
        let nos: Vec<usize> = sheet
            .blocks
            .iter()
            .flat_map(|b| b.items.iter().map(|i| i.no))
            .collect();
        assert_eq!(nos, [1, 2, 3, 4]);
        let prompts: Vec<&str> = sheet
            .blocks
            .iter()
            .flat_map(|b| b.items.iter().map(|i| i.prompt.as_str()))
            .collect();
        assert!(prompts.iter().all(|p| !p.contains("I identified")));
        for b in &sheet.blocks {
            assert_eq!(b.supports.len(), 1, "{}", b.id);
            assert_eq!(b.supports[0].heading, "Sentence frames");
            assert_eq!(b.supports[0].content, FRAME);
        }
    }

    fn item(id: &str, levels: &[Level], format: &str) -> Item {
        Item {
            id: id.into(),
            answer_format: Some(format.into()),
            prompt: format!("Prompt {id}"),
            answer: format!("Answer {id}"),
            evidence: "Evidence".into(),
            levels: levels.to_vec(),
            origin: Origin::Ai,
            ..Item::default()
        }
    }

    fn content() -> Content {
        let all = Level::ALL.to_vec();
        let mut warm = Block {
            id: WARMUP_ID.into(),
            kind: BlockKind::Warmup,
            title: "Warm-up".into(),
            items: vec![item("w1", &all, "簡答題")],
            vocabulary: vec![Vocab {
                word: "trust".into(),
                level: Some(2),
                definition: "belief".into(),
                ..Vocab::default()
            }],
            ..Block::default()
        };
        attach_scaffolds(&mut warm, None, &all, &[]);
        let mut act = Block {
            id: "b1".into(),
            title: "Paragraph 1: Find".into(),
            items: vec![
                item("s", &[Level::Basic], "簡答題"),
                item("c", &all, "段落寫作"),
                item("x", &[Level::Advanced], "選擇題"),
            ],
            teacher_notes: "設計理由".into(),
            ..Block::default()
        };
        let set = RawScaffoldSet {
            target: "b1".into(),
            verbal: vec![
                crate::worksheet::schemas::RawSupport {
                    kind: KIND_FRAMES.into(),
                    content_en: "I think ___ because ___.".into(),
                },
                crate::worksheet::schemas::RawSupport {
                    kind: "提示與追問".into(),
                    content_en: "Look at line 2.".into(),
                },
            ],
            instructional: vec![],
            procedural: vec![],
        };
        attach_scaffolds(&mut act, Some(&set), &all, &[]);
        let exit = Block {
            id: EXIT_ID.into(),
            kind: BlockKind::ExitTicket,
            title: "Exit ticket".into(),
            items: vec![item("e1", &all, "簡答題")],
            ..Block::default()
        };
        Content {
            blocks: vec![warm, act, exit],
            levels: all,
            ..Content::default()
        }
    }

    #[test]
    fn word_tables_print_once_at_the_end_when_asked() {
        let mut c = content();
        c.blocks[1].vocabulary = vec![
            Vocab {
                word: "Trust".into(),
                definition: "again".into(),
                ..Vocab::default()
            },
            Vocab {
                word: "gather".into(),
                definition: "meet".into(),
                ..Vocab::default()
            },
        ];
        let in_blocks = project("T", &c, Level::Standard, Audience::Student);
        assert!(in_blocks.end_words.is_empty());
        assert_eq!(in_blocks.blocks[0].vocabulary.len(), 1);
        c.vocabulary_at_end = true;
        let at_end = project("T", &c, Level::Standard, Audience::Student);
        assert!(at_end.blocks.iter().all(|b| b.vocabulary.is_empty()));
        let words: Vec<&str> = at_end.end_words.iter().map(|w| w.word.as_str()).collect();
        assert_eq!(
            words,
            ["trust", "gather"],
            "a word listed twice prints once"
        );
        assert_eq!(
            at_end.blocks.len(),
            in_blocks.blocks.len(),
            "no block is lost"
        );
        assert!(text_units(&at_end)
            .iter()
            .any(|u| u.place == "字詞表" && u.kind == UnitKind::KeyWord));
    }

    #[test]
    fn blocks_of_failed_steps_are_not_printed_and_letters_close_up() {
        let material = Material::default();
        let mut c = content();
        // The warm-up step failed: the editor keeps an empty block with a message.
        c.blocks[0] = placeholder(BlockKind::Warmup, WARMUP_ID, "暖身沒有產生成功", &material);
        c.blocks.insert(
            2,
            placeholder(
                BlockKind::Activity,
                "r1b2",
                "主要任務沒有產生成功",
                &material,
            ),
        );
        for audience in [Audience::Student, Audience::Teacher] {
            let sheet = project("T", &c, Level::Standard, audience);
            let kinds: Vec<BlockKind> = sheet.blocks.iter().map(|b| b.kind).collect();
            assert_eq!(kinds, [BlockKind::Activity, BlockKind::ExitTicket]);
            assert_eq!(
                sheet.blocks[0].label, "A",
                "letters skip what is not printed"
            );
            assert_eq!(sheet.blocks[0].items[0].no, 1);
        }
        // A block that only has items in another version is left out of this one alone.
        let mut c = content();
        c.blocks[1].items.retain(|i| i.levels == [Level::Advanced]);
        c.blocks[1].scaffolds.clear();
        c.blocks[1].teacher_notes.clear();
        let standard = project("T", &c, Level::Standard, Audience::Student);
        assert_eq!(standard.blocks.len(), 2);
        let advanced = project("T", &c, Level::Advanced, Audience::Student);
        assert_eq!(advanced.blocks.len(), 3);
        assert_eq!(advanced.blocks[1].label, "B");
        // Instructions alone are worth printing (a discussion task without items).
        let mut c = content();
        c.blocks[1].items.clear();
        c.blocks[1].scaffolds.clear();
        c.blocks[1].instructions = "Discuss with a partner.".into();
        assert_eq!(
            project("T", &c, Level::Standard, Audience::Student)
                .blocks
                .len(),
            3
        );
    }

    #[test]
    fn versions_share_the_core_and_differ_in_support() {
        let c = content();
        let sheet = |level| project("T", &c, level, Audience::Student);
        let (basic, standard, advanced) = (
            sheet(Level::Basic),
            sheet(Level::Standard),
            sheet(Level::Advanced),
        );
        let prompts = |s: &Sheet| -> Vec<String> {
            s.blocks[1].items.iter().map(|i| i.prompt.clone()).collect()
        };
        assert_eq!(prompts(&basic), ["Prompt s", "Prompt c"]);
        assert_eq!(prompts(&standard), ["Prompt c"]);
        assert_eq!(prompts(&advanced), ["Prompt c", "Prompt x"]);
        // Support: basic ≥ standard ≥ advanced; the procedural steps are everywhere.
        assert_eq!(basic.blocks[1].supports.len(), 2);
        assert_eq!(standard.blocks[1].supports.len(), 1);
        assert_eq!(advanced.blocks[1].supports.len(), 0);
        for s in [&basic, &standard, &advanced] {
            assert!(!s.blocks[1].steps.is_empty());
            assert!(
                !s.blocks[0].steps.is_empty(),
                "the program adds steps when the model gave none"
            );
        }
        // More room to answer in the higher versions.
        assert_eq!(basic.blocks[1].items[1].lines, 4);
        assert_eq!(advanced.blocks[1].items[0].lines, 6);
        assert_eq!(advanced.blocks[1].items[1].lines, 0);
    }

    #[test]
    fn glosses_print_in_their_versions_and_are_not_analysed() {
        let mut c = content();
        c.blocks[1].glosses = vec![crate::worksheet::Gloss {
            word: "abandon".into(),
            pos: "v.".into(),
            meaning_zh: "拋棄".into(),
            definition_en: String::new(),
            levels: vec![Level::Basic],
            origin: Origin::Ai,
        }];
        let basic = project("T", &c, Level::Basic, Audience::Student);
        assert_eq!(basic.blocks[1].glosses[0].text(), "(v.) 拋棄");
        let teacher = project("T", &c, Level::Basic, Audience::Teacher);
        assert_eq!(teacher.blocks[1].glosses.len(), 1, "both audiences");
        let standard = project("T", &c, Level::Standard, Audience::Student);
        assert!(standard.blocks[1].glosses.is_empty());
        assert!(!serde_json::to_string(&standard)
            .unwrap()
            .contains("glosses"));
        assert!(text_units(&basic).iter().all(|u| !u.text.contains("拋棄")));
        // What 換字 may change.
        let kinds: Vec<UnitKind> = text_units(&basic).iter().map(|u| u.kind).collect();
        assert_eq!(kinds[0], UnitKind::Title);
        assert!(kinds.contains(&UnitKind::KeyWord) && kinds.contains(&UnitKind::Question));
        assert!(!UnitKind::KeyWord.replaceable() && !UnitKind::WordBank.replaceable());
        assert!(UnitKind::Question.replaceable() && UnitKind::Option.replaceable());
        assert!(UnitKind::TableHeading.replaceable());
        assert!(
            UnitKind::OrganizerHeading.replaceable() && !UnitKind::OrganizerContent.replaceable()
        );
    }

    #[test]
    fn answers_exist_only_in_the_teacher_version() {
        let c = content();
        let student =
            serde_json::to_value(project("T", &c, Level::Standard, Audience::Student)).unwrap();
        let text = student.to_string();
        assert!(
            !text.contains("Answer") && !text.contains("Evidence") && !text.contains("設計理由")
        );
        assert!(!text.contains("\"answer\"") && !text.contains("teacher_notes"));
        let teacher = project("T", &c, Level::Standard, Audience::Teacher);
        assert_eq!(
            teacher.blocks[1].items[0].answer.as_deref(),
            Some("Answer c")
        );
        assert_eq!(teacher.blocks[1].teacher_notes.as_deref(), Some("設計理由"));
    }

    #[test]
    fn order_letters_numbers_and_switches() {
        let mut c = content();
        let s = project("T", &c, Level::Standard, Audience::Student);
        assert_eq!(
            s.blocks
                .iter()
                .map(|b| b.label.as_str())
                .collect::<Vec<_>>(),
            ["A", "B", ""]
        );
        assert_eq!(
            s.blocks
                .iter()
                .flat_map(|b| b.items.iter().map(|i| i.no))
                .collect::<Vec<_>>(),
            [1, 2, 3]
        );
        c.blocks[0].enabled = false;
        c.blocks[2].enabled = false;
        let s = project("T", &c, Level::Standard, Audience::Student);
        assert_eq!(s.blocks.len(), 1);
        assert_eq!(
            (s.blocks[0].label.as_str(), s.blocks[0].items[0].no),
            ("A", 1)
        );
        let lines = text_lines(&s);
        assert!(lines
            .iter()
            .any(|(place, text)| place == "A 活動 · 第 1 題" && text == "Prompt c"));
    }

    #[test]
    fn a_ticked_kind_is_kept_in_the_lowest_selected_version() {
        let only_advanced = [Level::Advanced];
        assert_eq!(
            scaffold_levels(
                ScaffoldType::Verbal,
                KIND_GUIDING,
                &only_advanced,
                &[KIND_GUIDING]
            ),
            Some(vec![Level::Advanced])
        );
        assert_eq!(
            scaffold_levels(ScaffoldType::Verbal, KIND_GUIDING, &only_advanced, &[]),
            None
        );
        assert_eq!(
            scaffold_levels(ScaffoldType::Procedural, KIND_STEPS, &only_advanced, &[]),
            Some(vec![Level::Advanced])
        );
        assert_eq!(
            item_levels("support", &[Level::Standard]),
            Vec::<Level>::new()
        );
    }

    #[test]
    fn bloom_tags_are_for_teachers_and_hints_for_the_basic_version() {
        let mut c = content();
        c.blocks[1].title = "Part B · Look Closer".into();
        c.blocks[1].items[1].bloom = Some(Bloom::Analyze);
        c.blocks[1].items[1].hint = "Look at the second sentence.".into();
        c.blocks[1].items[0].hint = "Start with: The writer …".into();
        for level in Level::ALL {
            let student = project("T", &c, level, Audience::Student);
            let json = serde_json::to_value(&student).unwrap().to_string();
            assert!(!json.contains("\"bloom\"") && !json.contains("bloom_summary"));
            let hints: Vec<&SheetItem> = student
                .blocks
                .iter()
                .flat_map(|b| &b.items)
                .filter(|i| i.hint.is_some())
                .collect();
            if level == Level::Basic {
                assert_eq!(hints.len(), 2);
                assert!(json.contains("Look at the second sentence."));
            } else {
                assert!(hints.is_empty(), "{level:?}");
                assert!(!json.contains("Look at the second sentence."));
            }
        }
        // The teacher version tags every question (null when it has no level) and sums up
        // each block; hints still only in 基礎版.
        let teacher = project("T", &c, Level::Standard, Audience::Teacher);
        let item = &teacher.blocks[1].items[0];
        assert_eq!(item.bloom, Some(Some(Bloom::Analyze)));
        assert_eq!(item.hint, None);
        assert_eq!(teacher.blocks[0].items[0].bloom, Some(None));
        assert_eq!(
            teacher.blocks[1].bloom_summary.as_deref(),
            Some("Bloom：分析 1")
        );
        assert_eq!(teacher.blocks[0].bloom_summary, None);
        let json = serde_json::to_value(&teacher).unwrap();
        assert_eq!(
            json["blocks"][0]["items"][0]["bloom"],
            serde_json::Value::Null
        );
        assert!(json["blocks"][0]["items"][0].get("bloom").is_some());
        assert_eq!(json["blocks"][1]["items"][0]["bloom"], "analyze");
        let basic = project("T", &c, Level::Basic, Audience::Teacher);
        assert_eq!(
            basic.blocks[1].items[0].hint.as_deref(),
            Some("Start with: The writer …")
        );
        // The Part letter follows the printed letter.
        assert_eq!(teacher.blocks[1].title, "Part B · Look Closer");
        c.blocks[0].enabled = false;
        let s = project("T", &c, Level::Standard, Audience::Student);
        assert_eq!(s.blocks[0].title, "Part A · Look Closer");
        assert!(
            text_lines(&project("T", &c, Level::Basic, Audience::Student))
                .iter()
                .any(
                    |(place, text)| place.ends_with("題提示") && text == "Start with: The writer …"
                )
        );
    }

    /// A 差異化學習單 block: one question with its three versions, plus a question stored before
    /// the redesign (no versions of its own, a hint).
    fn differentiated() -> Content {
        use crate::worksheet::ItemTier;
        let tier = |level, kind, support: &[&str], bank: &[&str], answer: &str| ItemTier {
            level,
            support_kind: kind,
            model_sentence: "The neighbors thought the umbrellas would be gone.".into(),
            support: support.iter().map(|s| s.to_string()).collect(),
            word_bank: bank.iter().map(|s| s.to_string()).collect(),
            answer: answer.into(),
            scoring: format!("{} 評分", level.label()),
        };
        let all = Level::ALL.to_vec();
        Content {
            blocks: vec![Block {
                id: "r1b1".into(),
                title: "Understand the Text".into(),
                bloom: Some(Bloom::Understand),
                items: vec![
                    Item {
                        id: "q1".into(),
                        answer_format: Some("簡答題".into()),
                        prompt: "What did the neighbors think?".into(),
                        answer: "They were sure every umbrella would be gone.".into(),
                        levels: all.clone(),
                        bloom: Some(Bloom::Understand),
                        tiers: vec![
                            tier(
                                Level::Basic,
                                SupportKind::SentenceFrame,
                                &["The neighbors thought the ___ would be ___."],
                                &["umbrellas", "gone"],
                                "umbrellas, gone",
                            ),
                            tier(
                                Level::Standard,
                                SupportKind::SentenceStarter,
                                &["The neighbors thought that …"],
                                &[],
                                "They thought the umbrellas would be gone.",
                            ),
                            tier(
                                Level::Advanced,
                                SupportKind::None,
                                &[],
                                &[],
                                "They were sure every umbrella would be gone.",
                            ),
                        ],
                        ..Item::default()
                    },
                    Item {
                        id: "old".into(),
                        answer_format: Some("簡答題".into()),
                        prompt: "An older question?".into(),
                        answer: "Old answer.".into(),
                        levels: all.clone(),
                        hint: "Start with: I think".into(),
                        ..Item::default()
                    },
                ],
                ..Block::default()
            }],
            levels: all,
            ..Content::default()
        }
    }

    #[test]
    fn the_three_versions_share_the_questions_and_differ_in_support() {
        let c = differentiated();
        let sheet = |level, audience| project("T", &c, level, audience);
        let prompts = |s: &Sheet| -> Vec<String> {
            s.blocks
                .iter()
                .flat_map(|b| b.items.iter().map(|i| i.prompt.clone()))
                .collect()
        };
        let basic = sheet(Level::Basic, Audience::Student);
        let standard = sheet(Level::Standard, Audience::Student);
        let advanced = sheet(Level::Advanced, Audience::Student);
        assert_eq!(prompts(&basic), prompts(&standard));
        assert_eq!(prompts(&standard), prompts(&advanced));
        // Support: 基礎版 a frame with its word bank in alphabetical order, 一般版 a starter,
        // 進階版 none.
        let s = basic.blocks[0].items[0].support.as_ref().unwrap();
        assert_eq!(
            (s.kind, s.heading),
            (SupportKind::SentenceFrame, "Sentence frame")
        );
        assert_eq!(s.word_bank, ["gone", "umbrellas"]);
        let s = standard.blocks[0].items[0].support.as_ref().unwrap();
        assert_eq!(s.lines, ["The neighbors thought that …"]);
        assert_eq!(advanced.blocks[0].items[0].support, None);
        // The older question keeps its hint in 基礎版 only.
        assert_eq!(
            basic.blocks[0].items[1].hint.as_deref(),
            Some("Start with: I think")
        );
        assert_eq!(standard.blocks[0].items[1].hint, None);
        assert_eq!(basic.blocks[0].items[0].hint, None);
        // Students: a neutral code, no answers, no scoring notes, no 分層說明.
        let codes: Vec<Option<String>> = [&basic, &standard, &advanced]
            .iter()
            .map(|s| s.version_code.clone())
            .collect();
        assert_eq!(
            codes,
            [Some("A".into()), Some("B".into()), Some("C".into())]
        );
        for s in [&basic, &standard, &advanced] {
            let json = serde_json::to_value(s).unwrap().to_string();
            assert!(!json.contains("\"answer\"") && !json.contains("scoring"));
            assert!(!json.contains("tiering_note") && !json.contains("評分"));
            assert!(!json.contains("model_sentence"));
        }
        // Teachers: this version's answer and scoring note, and the 分層說明.
        let t = sheet(Level::Basic, Audience::Teacher);
        assert_eq!(
            t.blocks[0].items[0].answer.as_deref(),
            Some("umbrellas, gone")
        );
        assert_eq!(t.blocks[0].items[0].scoring.as_deref(), Some("基礎版 評分"));
        assert_eq!(t.blocks[0].items[1].answer.as_deref(), Some("Old answer."));
        assert!(t.tiering_note.is_some());
        let t = sheet(Level::Advanced, Audience::Teacher);
        assert_eq!(
            t.blocks[0].items[0].answer.as_deref(),
            Some("They were sure every umbrella would be gone.")
        );
        // The same Bloom distribution in every version.
        let summaries: Vec<Option<String>> = Level::ALL
            .iter()
            .map(|l| sheet(*l, Audience::Teacher).blocks[0].bloom_summary.clone())
            .collect();
        assert!(summaries.iter().all(|x| *x == summaries[0]));
        // The vocabulary check reads the printed support too.
        assert!(text_lines(&basic)
            .iter()
            .any(|(place, text)| place.ends_with("題字庫") && text == "gone, umbrellas"));
        // One version only: no code, no note.
        let mut single = differentiated();
        single.levels = vec![Level::Standard];
        let s = project("T", &single, Level::Standard, Audience::Student);
        assert_eq!(s.version_code, None);
    }

    #[test]
    fn a_simplified_text_keeps_the_original_paragraphs() {
        let material = Material::pasted(
            None,
            0,
            "T",
            "First paragraph of the text.\n\nSecond paragraph of the text.",
            "",
        )
        .0;
        let mut c = content();
        c.blocks[1].paragraph_ids = vec![material.paragraphs[1].id.clone()];
        c.blocks[1].scaffolds.push(Scaffold {
            id: "b1-si9".into(),
            kind_type: ScaffoldType::Instructional,
            kind: KIND_SIMPLIFIED.into(),
            content: "An easier way to say it.".into(),
            levels: vec![Level::Basic],
            origin: Origin::Ai,
            ..Scaffold::default()
        });
        let mut basic = project("T", &c, Level::Basic, Audience::Student);
        keep_original_text(&mut basic, &c, &material);
        assert_eq!(
            basic.blocks[1].original_text,
            ["Second paragraph of the text."]
        );
        assert!(basic.blocks[1]
            .supports
            .iter()
            .any(|s| s.heading == "Simplified text"));
        // A version without the simplified text prints no original either.
        let mut standard = project("T", &c, Level::Standard, Audience::Student);
        keep_original_text(&mut standard, &c, &material);
        assert!(standard.blocks.iter().all(|b| b.original_text.is_empty()));
    }

    #[test]
    fn a_support_does_not_repeat_its_heading() {
        let headings = SUPPORT_HEADINGS
            .iter()
            .map(|(_, h)| *h)
            .chain([SUPPORT_FALLBACK]);
        for heading in headings {
            for content in [
                format!("{heading}: reason, result"),
                format!("{}: reason, result", heading.to_uppercase()),
                format!("{}:reason, result", heading.to_lowercase()),
                format!("  {heading} ：reason, result"),
                format!("{heading}\nreason, result"),
                "reason, result".to_string(),
            ] {
                assert_eq!(
                    without_heading(&content, heading),
                    "reason, result",
                    "{heading}: {content:?}"
                );
            }
            assert_eq!(without_heading(&format!("{heading}:"), heading), "");
            assert_eq!(without_heading(heading, heading), "");
            // The heading word starting a sentence, or a longer word, is part of the text.
            let sentence = format!("{heading} helps you answer.");
            assert_eq!(without_heading(&sentence, heading), sentence);
            let plural = format!("{heading}s: reason");
            assert_eq!(without_heading(&plural, heading), plural);
        }
        let example = "Example (a different question): Q: Why? A: Because.";
        assert_eq!(without_heading(example, "Example"), example);
        // Only the support's own heading goes; a different label is its text.
        assert_eq!(
            without_heading("Word bank: reason", "Key words"),
            "Word bank: reason"
        );
        // Non-ASCII text where the heading would end does not panic.
        assert_eq!(without_heading("字詞表：理由", "Hints"), "字詞表：理由");
    }

    #[test]
    fn the_sheet_prints_a_support_heading_once() {
        let all = Level::ALL.to_vec();
        let mut b = Block {
            id: "b1".into(),
            title: "Paragraph 1: Find".into(),
            items: vec![item("c", &all, "簡答題")],
            ..Block::default()
        };
        let support = |kind: &str, content: &str| crate::worksheet::schemas::RawSupport {
            kind: kind.into(),
            content_en: content.into(),
        };
        let set = RawScaffoldSet {
            target: "b1".into(),
            verbal: vec![
                support(KIND_WORD_LIST, "Word Bank: reason, result, however"),
                support(KIND_FRAMES, "Sentence frames:\nI think ___ because ___."),
                // Nothing but the heading: nothing to print.
                support(KIND_GUIDING, "Guiding questions:"),
            ],
            instructional: vec![],
            procedural: vec![],
        };
        attach_scaffolds(&mut b, Some(&set), &all, &[KIND_GUIDING]);
        let c = Content {
            blocks: vec![b],
            levels: all,
            ..Content::default()
        };
        let sheet = project("T", &c, Level::Basic, Audience::Student);
        let printed: Vec<(&str, &str)> = sheet.blocks[0]
            .supports
            .iter()
            .map(|s| (s.heading.as_str(), s.content.as_str()))
            .collect();
        assert_eq!(
            printed,
            [
                ("Word bank", "reason, result, however"),
                ("Sentence frames", "I think ___ because ___."),
            ]
        );
        // The vocabulary check reads the same printed text.
        assert!(text_lines(&sheet)
            .iter()
            .any(|(_, text)| text == "reason, result, however"));
    }

    #[test]
    fn row_labels_become_a_first_column_and_an_old_table_prints_as_before() {
        use crate::worksheet::ItemTable;
        let t = ItemTable {
            columns: vec!["Path A".into(), " Path B ".into()],
            rows: 3,
            row_labels: vec![" Claim ".into(), "Supporting detail".into()],
        };
        let s = SheetTable::of(&t);
        // A corner cell for the labels, then the headings; every row has a label (or none).
        assert_eq!(s.columns, ["", "Path A", "Path B"]);
        assert_eq!(s.row_labels, ["Claim", "Supporting detail", ""]);
        assert!(s.has_headings());
        // No labels (or only blank ones): the table the sheet always printed.
        for labels in [vec![], vec![" ".to_string()]] {
            let old = SheetTable::of(&ItemTable {
                row_labels: labels,
                ..t.clone()
            });
            assert_eq!(old.columns, ["Path A", "Path B"]);
            assert!(old.row_labels.is_empty());
        }
        // A stored table from before row labels existed still loads.
        let stored: ItemTable = serde_json::from_str(r#"{"columns":["A","B"],"rows":3}"#).unwrap();
        assert!(stored.row_labels.is_empty());
        assert!(!serde_json::to_string(&stored)
            .unwrap()
            .contains("row_labels"));
    }

    #[test]
    fn each_format_prints_its_own_answer_space() {
        let q = |id: &str, format: &str| Item {
            id: id.into(),
            answer_format: Some(format.into()),
            prompt: "Complete the table: Who? / Why? (Question 1)".into(),
            options: vec!["A choice".into(), " ".into(), "Another".into()],
            levels: vec![Level::Standard],
            ..Item::default()
        };
        let mut own = q("own", FMT_TABLE);
        own.table = Some(crate::worksheet::ItemTable {
            columns: vec![" Problem ".into(), "Solution".into(), String::new()],
            rows: 5,
            row_labels: Vec::new(),
        });
        let mut legacy = q("legacy", "");
        legacy.answer_format = None;
        let c = Content {
            blocks: vec![Block {
                id: "b1".into(),
                items: vec![
                    q("choice", FMT_CHOICE),
                    q("short", "簡答題"),
                    q("table", FMT_TABLE),
                    own,
                    q("organizer", FMT_ORGANIZER),
                    legacy,
                ],
                ..Block::default()
            }],
            levels: vec![Level::Standard],
            ..Content::default()
        };
        let s = project("T", &c, Level::Standard, Audience::Student);
        let items = &s.blocks[0].items;
        // Choices print for the formats that use them (and for a question without a format),
        // without the empty ones; the other formats keep them unprinted.
        assert_eq!(items[0].options, ["A choice", "Another"]);
        assert_eq!(items[5].options, ["A choice", "Another"]);
        for it in &items[1..5] {
            assert!(it.options.is_empty(), "{}", it.id);
        }
        // A table: the prompt's columns until the teacher sets her own; nothing else has one.
        assert_eq!(
            items[2].table,
            Some(SheetTable {
                columns: vec!["Who?".into(), "Why?".into()],
                rows: crate::worksheet::TABLE_ROWS,
                row_labels: Vec::new(),
            })
        );
        let t = items[3].table.as_ref().unwrap();
        assert_eq!(t.columns, ["Problem", "Solution", ""]);
        assert_eq!(t.rows, 5);
        assert!(t.has_headings());
        assert_eq!((items[2].lines, items[4].lines, items[1].lines), (0, 0, 3));
        for i in [0, 1, 4, 5] {
            assert_eq!(items[i].table, None);
        }
        assert!(text_lines(&s)
            .iter()
            .any(|(place, text)| place.ends_with("第 4 題表格") && text == "Problem Solution"));
    }

    #[test]
    fn a_starter_never_prints_a_word_bank() {
        let mut c = differentiated();
        // 一般版 switched from a frame to a starter keeps its bank (for the way back) unprinted.
        c.blocks[0].items[0].tiers[1].word_bank = vec!["umbrellas".into()];
        let standard = project("T", &c, Level::Standard, Audience::Student);
        let s = standard.blocks[0].items[0].support.as_ref().unwrap();
        assert_eq!(s.kind, SupportKind::SentenceStarter);
        assert!(s.word_bank.is_empty());
        // A bank still prints with a frame, a cloze and a word bank.
        for kind in [
            SupportKind::SentenceFrame,
            SupportKind::Cloze,
            SupportKind::WordBank,
        ] {
            c.blocks[0].items[0].tiers[1].support_kind = kind;
            let s = project("T", &c, Level::Standard, Audience::Student);
            assert_eq!(
                s.blocks[0].items[0].support.as_ref().unwrap().word_bank,
                ["umbrellas"]
            );
        }
    }

    #[test]
    fn group_work_gets_its_own_steps() {
        let b = Block {
            student_tasks: vec!["小組共同完成".into()],
            paragraph_ids: vec!["p1".into()],
            ..Block::default()
        };
        assert!(auto_steps(&b).iter().any(|s| s.contains("group")));
    }

    #[test]
    fn a_frame_wrapped_in_a_writing_task_is_lifted_too() {
        const FRAME: &str = "Hi! In Paragraph 2, the author describes ______. I identified ______ as the main idea.";
        let mut b = Block {
            id: "b1".into(),
            enabled: true,
            items: vec![
                Item {
                    prompt: "Which sentence is the topic sentence?".into(),
                    ..item("q1", &Level::ALL, FMT_CHOICE)
                },
                Item {
                    prompt: format!("Write a short analysis paragraph using the frame: {FRAME}"),
                    ..item("q2", &Level::ALL, FMT_PARAGRAPH)
                },
            ],
            ..Block::default()
        };
        assert_eq!(lift_frames(&mut b, ""), 1);
        // Not after an open question: it stays and asks for the frame (#71), without the wrapper.
        assert_eq!(b.items[1].prompt, FRAME_TASK);
        assert_eq!(b.scaffolds[0].content, FRAME);
    }

    #[test]
    fn a_support_prints_under_its_own_heading() {
        let support = |id: &str, kind_type, kind: &str, heading: Option<&str>| Scaffold {
            id: id.into(),
            kind_type,
            kind: kind.into(),
            levels: Level::ALL.to_vec(),
            heading: heading.map(str::to_string),
            ..Scaffold::default()
        };
        let mut c = content();
        c.blocks[1].scaffolds = vec![
            // Unheaded steps print as the block's numbered steps, as before.
            Scaffold {
                steps: vec!["Read the paragraph.".into()],
                ..support("b1-sp", ScaffoldType::Procedural, KIND_STEPS, None)
            },
            // A template's Directions box prints as a box of lines under the sample's heading.
            Scaffold {
                steps: vec![
                    "Identify the common idea.".into(),
                    "Underline the topic sentence.".into(),
                ],
                ..support(
                    "b1-s1",
                    ScaffoldType::Procedural,
                    KIND_STEPS,
                    Some("Directions"),
                )
            },
            Scaffold {
                content:
                    "Sentence Frame for Sharing: Hi! In Paragraph 1, the author describes ___."
                        .into(),
                ..support(
                    "b1-s2",
                    ScaffoldType::Verbal,
                    KIND_FRAMES,
                    Some(" Sentence Frame for Sharing "),
                )
            },
            Scaffold {
                content: "The author describes ___ to show ___.".into(),
                ..support("b1-s3", ScaffoldType::Verbal, KIND_FRAMES, None)
            },
        ];
        c.normalise().unwrap();
        assert_eq!(
            c.blocks[1].scaffolds[2].heading.as_deref(),
            Some("Sentence Frame for Sharing"),
            "trimmed"
        );
        let sheet = project("T", &c, Level::Standard, Audience::Student);
        let b = sheet
            .blocks
            .iter()
            .find(|b| b.id == c.blocks[1].id)
            .unwrap();
        assert_eq!(b.steps, ["Read the paragraph."]);
        let printed: Vec<(&str, &str)> = b
            .supports
            .iter()
            .map(|s| (s.heading.as_str(), s.content.as_str()))
            .collect();
        assert_eq!(
            printed,
            [
                (
                    "Directions",
                    "Identify the common idea.\nUnderline the topic sentence."
                ),
                (
                    "Sentence Frame for Sharing",
                    "Hi! In Paragraph 1, the author describes ___."
                ),
                ("Sentence frames", "The author describes ___ to show ___."),
            ]
        );
        // A heading longer than a heading is refused.
        let mut long = c.clone();
        long.blocks[1].scaffolds[1].heading = Some("H".repeat(81));
        assert!(long.normalise().is_err());
    }

    #[test]
    fn a_sheet_uses_one_name_for_one_idea() {
        let block = |id: &str, instructions: &str, prompt: &str| Block {
            id: id.into(),
            instructions: instructions.into(),
            items: vec![Item {
                prompt: prompt.into(),
                ..item(&format!("{id}-i1"), &Level::ALL, "簡答題")
            }],
            ..Block::default()
        };
        let sheet = || {
            vec![
                block(
                    "b1",
                    "Read paragraph 1 of this informative writing.",
                    "Name one feature of informative writing.",
                ),
                block(
                    "b2",
                    "Expository writing explains facts.",
                    "Which expository text features do you see? Informative texts use facts.",
                ),
            ]
        };
        // The template's word wins.
        let mut blocks = sheet();
        let n = unify_terms(
            &mut blocks,
            &["Features of expository writing", "informative"],
            true,
        );
        assert_eq!(n, 3);
        assert_eq!(
            blocks[0].instructions,
            "Read paragraph 1 of this expository writing."
        );
        assert_eq!(
            blocks[1].items[0].prompt,
            "Which expository text features do you see? Expository texts use facts."
        );
        // Without one there, the material's; without that either, the most used (capitals kept).
        let mut blocks = sheet();
        unify_terms(&mut blocks, &["", "An informative essay about bees."], true);
        assert_eq!(
            blocks[1].instructions,
            "Informative writing explains facts."
        );
        let mut blocks = sheet();
        unify_terms(&mut blocks, &[], true);
        assert_eq!(
            blocks[1].instructions,
            "Informative writing explains facts."
        );
        // A third name of the same idea.
        let mut three = vec![block(
            "b3",
            "Informational texts give facts.",
            "Name one feature of informative writing.",
        )];
        unify_terms(&mut three, &["informative writing"], true);
        assert_eq!(three[0].instructions, "Informative texts give facts.");
        // One word throughout: nothing to do; one block made again follows the others.
        let mut one = [sheet().remove(0)];
        assert_eq!(unify_terms(&mut one, &["expository texts"], true), 0);
        assert_eq!(unify_terms(&mut one, &["expository texts"], false), 2);
        assert_eq!(
            one[0].items[0].prompt,
            "Name one feature of expository writing."
        );
    }
}
