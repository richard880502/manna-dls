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
use super::{
    info_function_from_label, Audience, Block, BlockKind, Bloom, Content, Item, Level, Material,
    Origin, Practice, Scaffold, ScaffoldType, Settings, Vocab, EXIT_PURPOSES, FMT_BLANK,
    FMT_CHOICE, FMT_MATCHING, FMT_ORGANIZER, FMT_PARAGRAPH, FMT_TABLE, FMT_TRUE_FALSE, GEN_EXAMPLE,
    GEN_FRAMES, GEN_GUIDING, GEN_WORD_LIST, KIND_EXAMPLE, KIND_FRAMES, KIND_GUIDING, KIND_STEPS,
    KIND_WORD_LIST,
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
    /// English heading printed on the sheet (the kind labels themselves are Chinese).
    pub heading: &'static str,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SheetItem {
    /// The question's id in the worksheet (notes on a question are anchored to it).
    pub id: String,
    pub no: usize,
    pub answer_format: Option<String>,
    pub prompt: String,
    pub options: Vec<String>,
    pub lines: u8,
    /// Absent from the student version: the key itself is not serialised.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
    /// Teacher version only: the question's Bloom level (`null` when it has none). Absent from
    /// the student version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bloom: Option<Option<Bloom>>,
    /// 基礎版 only (both audiences): the hint printed under the question.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
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

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Sheet {
    pub title: String,
    pub level: Level,
    pub audience: Audience,
    pub objectives: Vec<String>,
    pub blocks: Vec<SheetBlock>,
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
        // A block of one Bloom level whose questions are all in other versions (分層練習) is
        // not part of this one.
        if b.bloom.is_some() && !b.items.is_empty() && !b.items.iter().any(|i| shown(&i.levels)) {
            continue;
        }
        let items = b
            .items
            .iter()
            .filter(|i| shown(&i.levels))
            .map(|i| {
                no += 1;
                SheetItem {
                    id: i.id.clone(),
                    no,
                    answer_format: i.answer_format.clone(),
                    prompt: i.prompt.trim().to_string(),
                    options: i.options.clone(),
                    lines: answer_lines(i.answer_format.as_deref(), level),
                    answer: (teacher && !i.answer.trim().is_empty())
                        .then(|| i.answer.trim().to_string()),
                    evidence: (teacher && !i.evidence.trim().is_empty())
                        .then(|| i.evidence.trim().to_string()),
                    bloom: teacher.then_some(i.bloom),
                    hint: (level == Level::Basic && !i.hint.trim().is_empty())
                        .then(|| i.hint.trim().to_string()),
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
            steps: scaffolds
                .iter()
                .filter(|s| s.kind_type == ScaffoldType::Procedural)
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
                .filter(|s| s.kind_type != ScaffoldType::Procedural)
                .filter_map(|s| {
                    let heading = support_heading(&s.kind);
                    let content = without_heading(&s.content, heading);
                    (!content.is_empty()).then(|| SheetSupport {
                        kind_type: s.kind_type,
                        kind: s.kind.clone(),
                        heading,
                        content: content.to_string(),
                    })
                })
                .collect(),
            items,
            teacher_notes: (teacher && !b.teacher_notes.trim().is_empty())
                .then(|| b.teacher_notes.trim().to_string()),
            bloom_summary,
            images: Vec::new(),
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
    Sheet {
        title: title.trim().to_string(),
        level,
        audience,
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

/// Every printed text of a sheet, one unit per line, with where it is (for the vocabulary
/// check: positions are reported by line).
pub fn text_lines(sheet: &Sheet) -> Vec<(String, String)> {
    let mut lines: Vec<(String, String)> = Vec::new();
    let mut push = |place: String, text: &str| {
        // One unit per line: line breaks inside a text become spaces.
        let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if !flat.is_empty() {
            lines.push((place, flat));
        }
    };
    push("標題".to_string(), &sheet.title);
    for (i, o) in sheet.objectives.iter().enumerate() {
        push(format!("學習目標第 {} 項", i + 1), o);
    }
    for b in &sheet.blocks {
        let name = block_name(b);
        push(format!("{name} · 標題"), &b.title);
        push(format!("{name} · 作答指示"), &b.instructions);
        for (i, s) in b.steps.iter().enumerate() {
            push(format!("{name} · 步驟 {}", i + 1), s);
        }
        for v in &b.vocabulary {
            push(
                format!("{name} · 字詞表"),
                &format!("{} {}", v.word, v.definition),
            );
        }
        for s in &b.supports {
            push(format!("{name} · {}", s.kind), &s.content);
        }
        for img in &b.images {
            push(format!("{name} · 圖說"), &img.caption);
        }
        for it in &b.items {
            push(format!("{name} · 第 {} 題", it.no), &it.prompt);
            for o in &it.options {
                push(format!("{name} · 第 {} 題選項", it.no), o);
            }
            if let Some(h) = &it.hint {
                push(format!("{name} · 第 {} 題提示", it.no), h);
            }
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

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
            .map(|s| (s.heading, s.content.as_str()))
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
    fn group_work_gets_its_own_steps() {
        let b = Block {
            student_tasks: vec!["小組共同完成".into()],
            paragraph_ids: vec!["p1".into()],
            ..Block::default()
        };
        assert!(auto_steps(&b).iter().any(|s| s.contains("group")));
    }
}
