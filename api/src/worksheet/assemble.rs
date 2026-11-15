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
    info_function_from_label, Audience, Block, BlockKind, Content, Item, Level, Material, Origin,
    Scaffold, ScaffoldType, Settings, Vocab, EXIT_PURPOSES, FMT_BLANK, FMT_CHOICE, FMT_MATCHING,
    FMT_ORGANIZER, FMT_PARAGRAPH, FMT_TABLE, FMT_TRUE_FALSE, GEN_EXAMPLE, GEN_FRAMES, GEN_GUIDING,
    GEN_WORD_LIST, KIND_EXAMPLE, KIND_FRAMES, KIND_GUIDING, KIND_STEPS, KIND_WORD_LIST,
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
            })
        })
        .collect();
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
        teacher_notes: raw.design_reason_zh.clone(),
        origin: Origin::Ai,
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
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Sheet {
    pub title: String,
    pub level: Level,
    pub audience: Audience,
    pub objectives: Vec<String>,
    pub blocks: Vec<SheetBlock>,
}

pub fn support_heading(kind: &str) -> &'static str {
    match kind {
        "引導問題" => "Guiding questions",
        "字詞表" => "Word bank",
        "句型框架" => "Sentence frames",
        "作答示例" => "Example",
        "改述或重述" => "In other words",
        "思考放聲" => "Think aloud",
        "提示與追問" => "Hints",
        "透過上下文解釋詞義" => "Words in context",
        "圖像組織" => "Organizer",
        "部分完成的大綱或內容" => "Outline",
        "調整文本" => "Simplified text",
        "標示關鍵概念或字彙" => "Key words",
        _ => "Support",
    }
}

fn letter(index: usize) -> String {
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
        let items = b
            .items
            .iter()
            .filter(|i| shown(&i.levels))
            .map(|i| {
                no += 1;
                SheetItem {
                    no,
                    answer_format: i.answer_format.clone(),
                    prompt: i.prompt.trim().to_string(),
                    options: i.options.clone(),
                    lines: answer_lines(i.answer_format.as_deref(), level),
                    answer: (teacher && !i.answer.trim().is_empty())
                        .then(|| i.answer.trim().to_string()),
                    evidence: (teacher && !i.evidence.trim().is_empty())
                        .then(|| i.evidence.trim().to_string()),
                }
            })
            .collect();
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
                })
                .collect(),
            supports: scaffolds
                .iter()
                .filter(|s| s.kind_type != ScaffoldType::Procedural && !s.content.trim().is_empty())
                .map(|s| SheetSupport {
                    kind_type: s.kind_type,
                    kind: s.kind.clone(),
                    heading: support_heading(&s.kind),
                    content: s.content.trim().to_string(),
                })
                .collect(),
            items,
            teacher_notes: (teacher && !b.teacher_notes.trim().is_empty())
                .then(|| b.teacher_notes.trim().to_string()),
        });
    }
    blocks.retain(has_content);
    let mut lettered = 0;
    for b in &mut blocks {
        if b.kind != BlockKind::ExitTicket {
            b.label = letter(lettered);
            lettered += 1;
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
        for it in &b.items {
            push(format!("{name} · 第 {} 題", it.no), &it.prompt);
            for o in &it.options {
                push(format!("{name} · 第 {} 題選項", it.no), o);
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
    fn group_work_gets_its_own_steps() {
        let b = Block {
            student_tasks: vec!["小組共同完成".into()],
            paragraph_ids: vec!["p1".into()],
            ..Block::default()
        };
        assert!(auto_steps(&b).iter().any(|s| s.contains("group")));
    }
}
