//! Deterministic outputs of the fake provider for the four worksheet steps (tests, CI, local
//! demos). Everything is derived from `fake_context`, so the same request gives the same sheet.
//!
//! Markers, written anywhere in the material (see `ai::fake` for the generic ones):
//!
//! | marker                   | effect                                                              |
//! |--------------------------|---------------------------------------------------------------------|
//! | `FAKE_WS_CJK`            | tasks: call 1 has a Chinese prompt in one item; the repair call does not |
//! | `FAKE_WS_CJK_ALWAYS`     | tasks: both calls have it (the item is dropped)                     |
//! | `FAKE_WS_BAD_LABEL`      | tasks: call 1 uses a student-task label outside the spec's set      |
//! | `FAKE_WS_SPLICE_ALWAYS`  | tasks: one evidence is spliced with "..." in both calls (it is cleared) |
//! | `FAKE_WS_WARMUP_BLOCK`   | warm-up: both calls are unusable                                    |
//! | `FAKE_WS_NO_PROCEDURAL`  | scaffolds: no procedural scaffold (the program adds its own)        |
//! | `FAKE_WS_LEAK`           | scaffolds: call 1 puts a reference answer into a scaffold           |
//! | `FAKE_WS_TARGET_ALIAS`   | scaffolds: targets spelled "Warm-up" / "b1", plus a "warmup" set nobody asked for |
//! | `FAKE_WS_EXIT_PARTIAL`   | exit ticket: only the first objective is checked, in both calls (E1: a notice says which are not) |
//! | `FAKE_WS_EXIT_FRAME`     | exit ticket: on call 1 the first question is only a sentence frame ([`EXIT_FRAME`]; repaired by the second) |
//! | `FAKE_WS_EXIT_FRAME_ALWAYS` | exit ticket: the same in both calls (the frame goes into the block's Sentence frames box) |
//! | `FAKE_WS_PRACTICE_OFF`   | tasks: every block is 獨立應用 on the first attempt (Q4: repaired by the second) |
//! | `FAKE_WS_PRACTICE_ALWAYS`| tasks: the same in both calls (the server relabels the blocks in order) |
//! | `FAKE_WS_NO_BLANK`       | tasks: the first 填空題 has its whole sentence and no blank on call 1 (repaired by the second) |
//! | `FAKE_WS_NO_BLANK_ALWAYS`| tasks: the same in both calls (the server blanks the answer out of the sentence) |
//! | `FAKE_WS_BLANK_LOST`     | tasks: the first 填空題 has no blank and not its answer either, in both calls (the server makes it a 簡答題) |
//! | `FAKE_WS_FRAME_STEPS`    | scaffolds: on call 1 every 句型框架 has two directions and a 「Use this frame:」 lead (repaired by the second) |
//! | `FAKE_WS_FRAME_STEPS_ALWAYS` | scaffolds: the same in both calls (the directions join the block's steps, the lead goes) |
//! | `FAKE_BLOCK`             | every step: output with nothing usable                              |
//! | `FAKE_RUNAWAY`           | every step: call 1 runs to `max_tokens` in whitespace (see `ai::fake`) |
//! | `FAKE_SLOW`              | each call waits three seconds first (handled in `sop::ai`)          |
//! | `FAKE_PACED`             | each call waits 400 ms first (handled in `sop::ai`; tests that watch the progress) |
//!
//! `FAKE_WS_SPLICE_ALWAYS` evidence has no verbatim piece of five words, so it is cleared.
//!
//! Templates (我的學習單版型), see [`template_outline`], [`template_details`] and
//! [`template_section`]:
//!
//! | marker                   | effect                                                              |
//! |--------------------------|---------------------------------------------------------------------|
//! | `## <title>` lines       | extraction: each is one outline item; `[n]` in it → `per_paragraph`, "video"/「影片」 → `needs_external_material` |
//! | `FAKE_TPL_BAD_LABEL`     | extraction: details use labels outside the fixed sets and `item_count` 40 (the server drops / clamps them) |
//! | `FAKE_BLOCK`             | extraction: unreadable output in both calls (the template fails); in a section's title: that block fails |
//! | `FAKE_RUNAWAY`           | every call: the first attempt runs to `max_tokens` (the retry succeeds) |
//! | `FAKE_TPL_BAD_BLOOM`     | generation: every block's first item has a Bloom level outside the six (the server drops it) |
//! | `<heading>: <content>` lines | extraction: in a `## ` section, a line whose heading is Directions, Sentence Frame(s) for Sharing, Sentence Frames, Sentence Starters, Word Bank, Hints, Guiding Questions or Example is one of its support boxes |
//! | `FAKE_TPL_FRAME_AS_ITEM` | extraction: no support boxes; a section's sharing frame is the 4th of 4 question patterns (`item_count` 4), as before supports existed (the server moves it into a box, `item_count` 3) |
//! | `FAKE_TPL_BAD_SHAPES`    | generation, call 1: a 是非題 asking an opinion first, the sharing frame as two questions at the end (one wrapped in "Write … using the frame:"), and directions with a "Use this frame:" lead in the first box of frames (or the scaffold) while the Directions and sharing boxes stay empty (repaired by the second) |
//! | `FAKE_TPL_BAD_SHAPES_ALWAYS` | generation: the same in both calls (the server splits, lifts and rewrites) |
//! | `FAKE_TPL_TERMS`         | generation: odd paragraphs say "expository writing", even ones "informative writing" in the instructions (the server keeps one) |
//!
//! Generated items carry a Bloom level (prompt version 2): a writing item `create`, the others
//! `remember`, `understand`, `analyze` in turn; the scaffold is an array of lines. A section with
//! support boxes gets `supports` instead: one entry per box with lines for its role.
//!
//! Picture slots (only when the request asks for `image_slots`, i.e. pictures are on): the tasks
//! step gives the first block one `scene` slot; every template block gets one, except a block
//! that needs the teacher's material. Markers:
//!
//! | marker                   | effect                                                              |
//! |--------------------------|---------------------------------------------------------------------|
//! | `FAKE_IMG_PHOTO`         | tasks: the second block gets a slot asking for a map, whose picture fails (an empty box) |
//! | `FAKE_IMG_FAIL`, `FAKE_IMG_BLOCKED`, `FAKE_IMG_SLOW`, `FAKE_IMG_TIMEOUT` | copied into every slot's description, for the fake picture provider (`worksheet::images::client`) |
//!
//! 閱讀理解題 (per Bloom level), see [`bloom_level`] and [`bloom_check`]:
//!
//! | marker                   | effect                                                              |
//! |--------------------------|---------------------------------------------------------------------|
//! | `FAKE_BLOOM_MISMATCH`    | judge: the first evaluate / create question is rated `understand`, in both rounds (it is relabelled with a notice) |
//! | `FAKE_BLOOM_NO_EVIDENCE` | level call: the first remember question's evidence is not in the text, on the first attempt only (it is repaired) |
//! | `FAKE_BLOCK`             | level call: only labels outside the level's formats (the level fails) |
//!
//! 逐區修改 (`worksheet_block_revise`), see [`block_revise`]: the first question a note points at
//! (or the block's first) gets " (easier r<round>)" after its prompt and Bloom level `remember`;
//! every other question is `keep: true` with empty text. Markers, in a note or the instruction:
//!
//! | marker                   | effect                                                              |
//! |--------------------------|---------------------------------------------------------------------|
//! | `FAKE_REVISE_TOUCH_ALL`  | every question is changed (" (touched)"), the title too (the code keeps what the notes do not cover) |
//! | `FAKE_REVISE_ADD`        | a new question is added at the end                                  |
//! | `FAKE_REVISE_DROP`       | the last question is left out (removed when the scope allows it, otherwise kept) |
//! | `FAKE_REVISE_BAD`        | the change uses an answer format outside the fixed labels in both calls (the question stays as it was) |
//! | `FAKE_REVISE_BAD_ONCE`   | the same on the first call only (the retry is fine)                 |
//! | `FAKE_REVISE_NOTHING`    | every question kept; the reply asks a question (nothing to propose) |
//! | `FAKE_REVISE_PIC_ADD`    | also proposes a new picture for the block (picture_actions)         |
//! | `FAKE_REVISE_PIC_REDRAW` | also proposes redrawing the block's first picture ("no text in the picture") |
//! | `FAKE_REVISE_PIC_REMOVE` | also proposes removing the block's last picture                     |
//! | `FAKE_REVISE_PIC_BAD`    | also writes two invalid picture actions (unknown slot, another block) |
//! | `FAKE_REVISE_PIC_ONLY`   | every question kept, only the picture actions                       |
//!
//! 差異化學習單 (`worksheet_tiered_level`): see [`tiered_level`] (`FAKE_TIER_LOCATION`,
//! `FAKE_TIER_FLAT`, `FAKE_BLOCK`).
//!
//! 圖像組織: every 圖像組織 question (tasks, template blocks, 閱讀理解題, 差異化學習單) carries an
//! organizer, the types in turn by question number ([`fake_organizer`]): labels for the type,
//! answers from the paragraph's words, the first cell pre-filled (差異化學習單: the first in every
//! version, the second in 基礎版 only). Marker:
//!
//! | marker                   | effect                                                              |
//! |--------------------------|---------------------------------------------------------------------|
//! | `FAKE_ORG_BAD`           | tasks and 閱讀理解題: every organizer has the type `fishbone` and eight sections of three cells, all pre-filled (the server falls back to a sequence, cuts it to six boxes of two cells and leaves one to write) |

use serde_json::{json, Value};

use super::schemas::TARGET_WARMUP;
use super::{
    task_for_function, Practice, FMT_BLANK, FMT_CHOICE, FMT_MATCHING, FMT_ORGANIZER, FMT_PARAGRAPH,
    FMT_SHORT, FMT_TABLE, FMT_TRUE_FALSE, INFO_FUNCTIONS, KIND_EXAMPLE, KIND_FRAMES, KIND_GUIDING,
    KIND_STEPS, KIND_WORD_LIST,
};

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// The picture markers present in the request, for the slot descriptions.
fn picture_markers(has: &dyn Fn(&str) -> bool) -> String {
    [
        "FAKE_IMG_FAIL",
        "FAKE_IMG_BLOCKED",
        "FAKE_IMG_SLOW",
        "FAKE_IMG_TIMEOUT",
    ]
    .into_iter()
    .filter(|m| has(m))
    .map(|m| format!(" {m}"))
    .collect()
}

/// One `scene` slot about `label`.
fn scene_slot(label: &str, sentence: &str, markers: &str) -> Value {
    json!({
        "kind": "scene",
        "purpose_zh": format!("幫助學生想像{label}的情境"),
        "prompt_en": format!("A simple classroom scene showing what happens in {label}: {}{markers}",
                             sentence.chars().take(160).collect::<String>()),
        "alt_en": format!("A picture of what happens in {label}."),
        "caption": format!("Look at the picture of {label}."),
    })
}

/// The first sentence of `text`, a verbatim piece of it, without markers.
fn first_sentence(text: &str) -> String {
    let text = text.trim();
    for (i, c) in text.char_indices() {
        if matches!(c, '.' | '!' | '?') && i >= 12 {
            return text[..i + c.len_utf8()].trim().to_string();
        }
    }
    text.to_string()
}

fn spliced(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let head = words.iter().take(3).copied().collect::<Vec<_>>().join(" ");
    let tail = words
        .iter()
        .rev()
        .take(2)
        .rev()
        .copied()
        .collect::<Vec<_>>()
        .join(" ");
    format!("{head} ... {tail}")
}

/// One item of a format about paragraph(s) `label`, built from its first sentence.
fn item(format: &str, label: &str, sentence: &str, k: usize, tier: &str) -> Value {
    let (prompt, options, answer, evidence): (String, Vec<String>, String, Option<String>) = match format {
        f if f == FMT_CHOICE => (
            format!("Which sentence comes from {label}? (Question {k})"),
            vec![
                sentence.to_string(),
                "The writer never mentions this topic.".to_string(),
                "The story takes place on another planet.".to_string(),
                "Nobody in the text says anything.".to_string(),
            ],
            sentence.to_string(),
            Some(sentence.to_string()),
        ),
        f if f == FMT_TRUE_FALSE => (
            format!("{label} is written as a list of numbers only. (Question {k})"),
            vec!["True".to_string(), "False".to_string()],
            "False".to_string(),
            Some(sentence.to_string()),
        ),
        f if f == FMT_BLANK => {
            let words: Vec<&str> = sentence.split_whitespace().collect();
            let last = words.last().copied().unwrap_or("word");
            let answer = last.trim_matches(|c: char| !c.is_alphanumeric()).to_string();
            let stem = words[..words.len().saturating_sub(1)].join(" ");
            (
                format!("Complete the sentence from {label}: {stem} ______ (Question {k})"),
                Vec::new(),
                answer,
                Some(sentence.to_string()),
            )
        }
        f if f == FMT_MATCHING => (
            format!("Match the parts of {label} with their roles: 1. the opening  2. the middle  3. the ending (Question {k})"),
            vec!["introduces the topic".to_string(), "adds details".to_string(), "closes the idea".to_string()],
            "1-introduces the topic; 2-adds details; 3-closes the idea".to_string(),
            None,
        ),
        f if f == FMT_TABLE => (
            format!("Complete the table for {label}: Who? / What happened? / Why? (Question {k})"),
            Vec::new(),
            "Who: the main character. What: the key event. Why: the reason given in the text.".to_string(),
            Some(sentence.to_string()),
        ),
        f if f == FMT_ORGANIZER => (
            format!("Complete the organizer for {label}. (Question {k})"),
            Vec::new(),
            "Cause, event and result in the order of the text.".to_string(),
            None,
        ),
        f if f == FMT_PARAGRAPH => (
            format!("Write a short paragraph: what would you do in the situation described in {label}? (Question {k})"),
            Vec::new(),
            "Answers will vary. Look for a clear opinion and one reason from the text.".to_string(),
            None,
        ),
        _ => (
            format!("What is the first thing {label} tells the reader? Answer in your own words. (Question {k})"),
            Vec::new(),
            sentence.to_string(),
            Some(sentence.to_string()),
        ),
    };
    let mut out = json!({
        "item_id": format!("X-{k}"), "answer_format": format, "tier": tier,
        "prompt_en": prompt, "options": options, "answer_en": answer, "evidence": evidence,
    });
    if format == FMT_ORGANIZER {
        out["organizer"] = fake_organizer(k, label, sentence, false);
    }
    out
}

/// The organizer types in the order fake questions use them.
const FAKE_ORGANIZERS: [(&str, &[&str]); 8] = [
    ("t_chart", &["Before", "After"]),
    ("cause_effect", &["Cause", "Effect", "Effect"]),
    ("sequence", &["First", "Next", "Finally"]),
    ("venn", &["The writer", "Both", "The neighbors"]),
    ("mind_map", &["Who", "What", "Why", "How"]),
    (
        "story_map",
        &["Setting", "Characters", "Problem", "Events", "Solution"],
    ),
    ("problem_solution", &["Problem", "Solution", "Result"]),
    (
        "kwl",
        &["What I Know", "What I Want to Know", "What I Learned"],
    ),
];

/// The organizer of fake question `k` (from 1) about `label`: the type by `k`, answers made of
/// the sentence's words, the first cell pre-filled (`tiered`: in every version, the second
/// section's in 基礎版 only).
pub fn fake_organizer(k: usize, label: &str, sentence: &str, tiered: bool) -> Value {
    let (kind, labels) = FAKE_ORGANIZERS[(k.max(1) - 1) % FAKE_ORGANIZERS.len()];
    let words: Vec<String> = sentence
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
        .filter(|w| w.len() > 2)
        .collect();
    let word = |n: usize| {
        words
            .get(n % words.len().max(1))
            .cloned()
            .unwrap_or_else(|| "idea".to_string())
    };
    let per = match kind {
        "cause_effect" | "sequence" | "problem_solution" => 1,
        _ => 2,
    };
    let sections: Vec<Value> = labels
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let cells: Vec<Value> = (0..per)
                .map(|j| {
                    let answer = if kind == "kwl" && i < 2 {
                        String::new()
                    } else {
                        format!("{} in {label}", word(i * per + j))
                    };
                    let first = i == 0 && j == 0 && !answer.is_empty();
                    if tiered {
                        let prefill = match (i, j) {
                            _ if first => "all",
                            (1, 0) => "basic",
                            _ => "none",
                        };
                        json!({ "answer": answer, "prefill_in": prefill })
                    } else {
                        json!({ "answer": answer, "prefilled": first })
                    }
                })
                .collect();
            json!({ "label": l, "cells": cells })
        })
        .collect();
    json!({
        "type": kind,
        "title": format!("Organize {label}"),
        "center": if kind == "mind_map" { word(0) } else { String::new() },
        "sections": sections,
    })
}

/// `FAKE_ORG_BAD`: a type outside the eight, too many sections and cells, every cell pre-filled.
fn bad_organizer() -> Value {
    let cell = json!({ "answer": "a fact", "prefilled": true });
    json!({
        "type": "fishbone", "title": "Too big", "center": "",
        "sections": (1..=8)
            .map(|i| json!({ "label": format!("Bone {i}"), "cells": [cell, cell, cell] }))
            .collect::<Vec<_>>(),
    })
}

/// The `choice_options` habit in `teacher_habits` (added to the request by
/// [`crate::memory::apply`] only while the PI switch is on).
fn choice_habit(ctx: &Value) -> Option<usize> {
    ctx["teacher_habits"]
        .as_array()?
        .iter()
        .find(|h| h["kind"] == "choice_options")?["value"]
        .as_str()?
        .parse()
        .ok()
}

pub fn tasks(ctx: &Value, attempt: i32, has: &dyn Fn(&str) -> bool) -> Value {
    let invalid = has("FAKE_BLOCK");
    let cjk = has("FAKE_WS_CJK_ALWAYS") || (has("FAKE_WS_CJK") && attempt == 1);
    let bad_label = has("FAKE_WS_BAD_LABEL") && attempt == 1;
    let splice = has("FAKE_WS_SPLICE_ALWAYS");
    let practice_off =
        has("FAKE_WS_PRACTICE_ALWAYS") || (has("FAKE_WS_PRACTICE_OFF") && attempt == 1);
    let no_blank = has("FAKE_WS_NO_BLANK_ALWAYS") || (has("FAKE_WS_NO_BLANK") && attempt == 1);
    let blank_lost = has("FAKE_WS_BLANK_LOST");
    let mut blank_marked = false;
    let paragraphs = ctx["paragraphs"].as_array().cloned().unwrap_or_default();
    let only: Option<Vec<i64>> = ctx["only"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_i64).collect());
    let budget = ctx["item_budget"].as_u64().unwrap_or(6).max(1) as usize;
    let selected_tasks = strings(&ctx["selected_student_tasks"]);
    let selected_formats = strings(&ctx["selected_answer_formats"]);
    let objectives = strings(&ctx["objective_ids"]);
    // A different wording for every run, the way a regeneration differs from the first sheet.
    let run_no = ctx["run_no"].as_i64().unwrap_or(1);
    let slots = ctx["image_slots"].as_u64().unwrap_or(0) > 0;
    let markers = picture_markers(has);

    // One block per paragraph; a single-block regeneration covers exactly the asked paragraphs.
    let groups: Vec<Vec<i64>> = match &only {
        Some(nos) => vec![nos.clone()],
        None => paragraphs
            .iter()
            .filter_map(|p| p["no"].as_i64())
            .map(|no| vec![no])
            .collect(),
    };
    let n = groups.len().max(1);
    let default_formats = [FMT_SHORT, FMT_CHOICE, FMT_BLANK, FMT_TABLE];
    let mut k = 0usize;
    let blocks: Vec<Value> = groups
        .iter()
        .enumerate()
        .map(|(bi, nos)| {
            let text = paragraphs
                .iter()
                .find(|p| p["no"].as_i64() == nos.first().copied())
                .and_then(|p| p["text"].as_str())
                .unwrap_or_default();
            let sentence = first_sentence(text);
            let label = match nos.as_slice() {
                [one] => format!("paragraph {one}"),
                many => format!(
                    "paragraphs {}",
                    many.iter().map(i64::to_string).collect::<Vec<_>>().join(" and ")
                ),
            };
            // Find first, then analyse, organise, compare; the last block creates.
            let function = if n >= 3 && bi == n - 1 {
                INFO_FUNCTIONS[4]
            } else {
                INFO_FUNCTIONS[(bi + (run_no as usize - 1)) % 4]
            };
            let core = (budget / n + usize::from(bi < budget % n)).max(1);
            let format_at = |i: usize| -> &str {
                if selected_formats.is_empty() {
                    default_formats[(bi + i) % default_formats.len()]
                } else {
                    selected_formats[(bi + i) % selected_formats.len()].as_str()
                }
            };
            let mut items: Vec<Value> = Vec::new();
            for i in 0..core {
                k += 1;
                items.push(item(format_at(i), &label, &sentence, k, "core"));
            }
            k += 1;
            let mut support = item(FMT_SHORT, &label, &sentence, k, "support");
            support["prompt_en"] = json!(format!("Underline the key sentence in {label}. Copy it here. (Question {k})"));
            items.push(support);
            // The first 填空題 of the sheet without its blank (markers above).
            if (no_blank || blank_lost) && !blank_marked {
                if let Some(it) = items.iter_mut().find(|i| i["answer_format"] == FMT_BLANK) {
                    let prompt = it["prompt_en"].as_str().unwrap_or_default().to_string();
                    let answer = it["answer_en"].as_str().unwrap_or_default().to_string();
                    it["prompt_en"] = json!(if blank_lost {
                        prompt.replace(" ______", "")
                    } else {
                        prompt.replace("______", &answer)
                    });
                    blank_marked = true;
                }
            }
            k += 1;
            let mut challenge = item(FMT_SHORT, &label, &sentence, k, "challenge");
            challenge["prompt_en"] = json!(format!(
                "Do you agree with the idea in {label}? Give two reasons, one of them from the text. (Question {k})"
            ));
            challenge["answer_en"] = json!("Answers will vary. Both reasons should be explained.");
            challenge["evidence"] = Value::Null;
            items.push(challenge);
            // Bloom labels (prompt version 2): core from the function, support remember,
            // challenge evaluate.
            let core_bloom = match function.1 {
                "發現資訊" => "remember",
                "整理資訊" => "understand",
                "創造資訊" => "create",
                _ => "analyze",
            };
            for it in &mut items {
                it["bloom"] = json!(match it["tier"].as_str() {
                    Some("support") => "remember",
                    Some("challenge") => "evaluate",
                    _ => core_bloom,
                });
            }
            if run_no > 1 {
                for it in &mut items {
                    let p = it["prompt_en"].as_str().unwrap_or_default().to_string();
                    it["prompt_en"] = json!(format!("{p} [draft {run_no}]"));
                }
            }
            // A confirmed option-count habit the request carries (teacher memory on), followed
            // the way a model would (the answer is the first option).
            if let Some(n) = choice_habit(ctx) {
                for it in items.iter_mut().filter(|it| it["answer_format"] == FMT_CHOICE) {
                    if let Some(options) = it["options"].as_array_mut() {
                        options.truncate(n);
                        while options.len() < n {
                            options.push(json!(format!(
                                "None of the other answers ({}).",
                                options.len() + 1
                            )));
                        }
                    }
                }
            }
            if invalid {
                for it in &mut items {
                    it["prompt_en"] = json!("請用中文回答這一題。");
                }
            }
            if has("FAKE_ORG_BAD") {
                for it in &mut items {
                    if it.get("organizer").is_some() {
                        it["organizer"] = bad_organizer();
                    }
                }
            }
            if bi == 0 {
                if cjk {
                    items[core]["prompt_en"] = json!("請找出關鍵句 Find the key sentence.");
                }
                if splice && !invalid {
                    items[0]["evidence"] = json!(spliced(text));
                }
            }
            let mut student_tasks: Vec<String> = if selected_tasks.is_empty() {
                vec![task_for_function(function.0).to_string()]
            } else {
                selected_tasks.iter().take(2).cloned().collect()
            };
            if bad_label && bi == 0 {
                student_tasks.push("探究".to_string());
            }
            let image_slots: Vec<Value> = match (slots, bi) {
                (false, _) => Vec::new(),
                (true, 0) => vec![scene_slot(&label, &sentence, &markers)],
                (true, 1) if has("FAKE_IMG_PHOTO") => vec![json!({
                    "kind": "scene",
                    "purpose_zh": "讓學生看出故事發生的地方",
                    "prompt_en": format!("FAKE_IMG_FAIL A map of the town in {label}."),
                    "alt_en": "A map of the town.",
                    "caption": "",
                })],
                _ => Vec::new(),
            };
            let mut block = json!({
                "block_id": format!("B{}", bi + 1),
                "paragraph_nos": nos,
                "title_en": format!(
                    "{}{}: {}",
                    label[..1].to_uppercase(),
                    &label[1..],
                    match function.1 { "發現資訊" => "Find", "整理資訊" => "Organize",
                                       "分析資訊" => "Analyze", "比較資訊" => "Compare", _ => "Create" }
                ),
                "info_function": function.1,
                // Q4: in order, unless a marker writes every block as 獨立應用.
                "practice": if practice_off {
                    Practice::Independent.label()
                } else {
                    Practice::for_position(bi, n).label()
                },
                "student_tasks": student_tasks,
                "objective_refs": objectives.iter().take(1).collect::<Vec<_>>(),
                "instructions_en": format!("Read {label} again, then answer."),
                "items": items,
                "design_reason_zh": format!("先確認{}的關鍵訊息，再往「{}」推進。", if nos.len() > 1 { "這幾段" } else { "這一段" }, function.1),
            });
            if slots {
                block["image_slots"] = json!(image_slots);
            }
            block
        })
        .collect();
    json!({ "blocks": blocks })
}

pub fn warmup(ctx: &Value, _attempt: i32, has: &dyn Fn(&str) -> bool) -> Value {
    if has("FAKE_BLOCK") || has("FAKE_WS_WARMUP_BLOCK") {
        return json!({
            "background_activities": [{
                "title_en": "暖身", "instructions_en": "請和同學討論。", "prompts_en": ["你借過傘嗎？"],
                "student_tasks": ["討論／分享"], "answer_format": FMT_SHORT, "purpose_zh": "連結經驗",
            }],
            "key_vocabulary": [{
                "word": "zzzunlisted", "paragraph_no": 1, "context_sentence": "Not in the text.",
                "student_friendly_definition_en": "a made-up word",
            }],
        });
    }
    let count = ctx["vocab_count"].as_u64().unwrap_or(6) as usize;
    let title = ctx["title"].as_str().unwrap_or_default();
    let words: Vec<Value> = ctx["candidates"]
        .as_array()
        .map(|c| c.iter().take(count).cloned().collect())
        .unwrap_or_default();
    let key_vocabulary: Vec<Value> = words
        .iter()
        .map(|c| {
            json!({
                "word": c["word"], "paragraph_no": c["paragraph_no"],
                "context_sentence": c["sentence"],
                "student_friendly_definition_en": "an important idea in this text",
            })
        })
        .collect();
    let about = if title.trim().is_empty() {
        "the title of today's text".to_string()
    } else {
        format!("the title \"{}\"", title.trim())
    };
    json!({
        "background_activities": [{
            "title_en": "Before you read",
            "instructions_en": "Think on your own first. Then tell a partner.",
            "prompts_en": [
                format!("Look at {about}. What do you think the text is about?"),
                "Have you ever been in a similar situation? Tell your partner in two sentences.",
            ],
            "student_tasks": ["推論／預測", "討論／分享"],
            "answer_format": FMT_SHORT,
            "purpose_zh": "看標題預測並連結個人經驗，喚醒背景知識。",
        }],
        "key_vocabulary": key_vocabulary,
    })
}

pub fn scaffolds(ctx: &Value, attempt: i32, has: &dyn Fn(&str) -> bool) -> Value {
    let invalid = has("FAKE_BLOCK");
    let no_procedural = has("FAKE_WS_NO_PROCEDURAL");
    let leak = has("FAKE_WS_LEAK") && attempt == 1;
    let frame_steps =
        has("FAKE_WS_FRAME_STEPS_ALWAYS") || (has("FAKE_WS_FRAME_STEPS") && attempt == 1);
    let selected = strings(&ctx["selected_kinds"]);
    let content_of = |kind: &str| -> &'static str {
        match kind {
            k if k == KIND_FRAMES && frame_steps => "Identify the main idea in the first sentence. Use one detail from the text as evidence.\nUse this frame: The writer says ___ in order to ___.",
            k if k == KIND_GUIDING => "Who is the paragraph about? What happens first? What changes at the end?",
            k if k == KIND_EXAMPLE => "Example (a different question): Q: Why was the street empty? A: The street was empty because the shops had closed.",
            k if k == KIND_WORD_LIST => "Word bank: reason, result, however, because, in the end",
            _ => "The writer says ___ in order to ___. I think ___ because ___.",
        }
    };
    let mut leaked = false;
    let sets: Vec<Value> = ctx["targets"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|t| {
            let target = t["target"].as_str().unwrap_or_default();
            let warm = target == TARGET_WARMUP;
            let kinds: Vec<&str> = if selected.is_empty() {
                vec![KIND_FRAMES]
            } else {
                selected.iter().map(String::as_str).take(2).collect()
            };
            let mut verbal: Vec<Value> = kinds
                .iter()
                .map(|k| json!({ "kind": k, "content_en": content_of(k) }))
                .collect();
            if invalid {
                verbal = vec![json!({ "kind": KIND_FRAMES, "content_en": "請用這個句型回答。" })];
            }
            let long_answer = strings(&t["answers"])
                .into_iter()
                .find(|a| a.split_whitespace().count() >= 4);
            if leak && !leaked {
                if let Some(answer) = long_answer {
                    leaked = true;
                    verbal.push(json!({ "kind": "提示與追問", "content_en": format!("Hint: the answer is: {answer}") }));
                }
            }
            let steps: Vec<&str> = if warm {
                vec!["Step 1: Think about the questions on your own.", "Step 2: Share your ideas with a partner."]
            } else {
                vec![
                    "Step 1: Read the paragraph on your own.",
                    "Step 2: Underline the words that help you answer.",
                    "Step 3: Write your answers, then check with a partner.",
                ]
            };
            json!({
                "target": target,
                "verbal": verbal,
                "instructional": if warm || invalid { json!([]) } else {
                    json!([{ "kind": "標示關鍵概念或字彙", "content_en": "Circle the key words in each question before you answer." }])
                },
                "procedural": if no_procedural || invalid { json!([]) } else {
                    json!([{ "kind": KIND_STEPS, "steps_en": steps }])
                },
            })
        })
        .collect();
    let mut sets = sets;
    if has("FAKE_WS_TARGET_ALIAS") {
        // The model's own spellings ("Warm-up", "b1"), and a warm-up set nobody asked for.
        let asked_warmup = sets.iter().any(|s| s["target"] == TARGET_WARMUP);
        for s in &mut sets {
            let alias = match s["target"].as_str().unwrap_or_default() {
                TARGET_WARMUP => "Warm-up".to_string(),
                other => other.to_lowercase(),
            };
            s["target"] = json!(alias);
        }
        if !asked_warmup {
            if let Some(first) = sets.first().cloned() {
                let mut extra = first;
                extra["target"] = json!("warmup");
                sets.insert(0, extra);
            }
        }
    }
    json!({ "scaffolds": sets })
}

/// The sentence frame a teacher found printed as a numbered question (`FAKE_WS_EXIT_FRAME`).
pub const EXIT_FRAME: &str = "Hi! In the 3rd paragraph, the author describes ______. After reading it carefully, I identified ______ as one of the main characteristics of narrative writing, as seen in the text where ______.";

pub fn exit_ticket(ctx: &Value, attempt: i32, has: &dyn Fn(&str) -> bool) -> Value {
    if has("FAKE_BLOCK") {
        return json!({ "estimated_minutes": 3, "items": [{
            "item_id": "E1", "purpose": "形成性評量", "prompt_en": "今天學到什麼？",
            "answer_format": FMT_SHORT, "options": [], "student_tasks": ["學習反思"],
            "objective_refs": [], "sample_answer_en": null,
        }] });
    }
    let count = ctx["exit_count"].as_u64().unwrap_or(3) as usize;
    let objectives = strings(&ctx["objective_ids"]);
    // E1: the formative item checks every objective, unless the marker leaves some out.
    let checked = if has("FAKE_WS_EXIT_PARTIAL") {
        1
    } else {
        objectives.len()
    };
    let mut items = vec![
        json!({
            "item_id": "E1", "purpose": "形成性評量",
            "prompt_en": "In one or two sentences, explain the most important idea of today's text in your own words.",
            "answer_format": FMT_SHORT, "options": [], "student_tasks": ["理解內容"],
            "objective_refs": objectives.iter().take(checked).collect::<Vec<_>>(),
            "sample_answer_en": "A clear statement of the main idea with one detail from the text.",
        }),
        json!({
            "item_id": "E2", "purpose": "學生反思",
            "prompt_en": "Which part of today's worksheet was the hardest for you? Why?",
            "answer_format": FMT_SHORT, "options": [], "student_tasks": ["學習反思"],
            "objective_refs": [], "sample_answer_en": null,
        }),
        json!({
            "item_id": "E3", "purpose": "蒐集問題",
            "prompt_en": "Write one question you still have about the text.",
            "answer_format": FMT_SHORT, "options": [], "student_tasks": ["自我評量"],
            "objective_refs": [], "sample_answer_en": null,
        }),
    ];
    if has("FAKE_WS_EXIT_FRAME_ALWAYS") || (has("FAKE_WS_EXIT_FRAME") && attempt == 1) {
        items[0]["prompt_en"] = json!(EXIT_FRAME);
    }
    items.truncate(count.clamp(2, 3));
    json!({ "estimated_minutes": if count <= 2 { 3 } else { 5 }, "items": items })
}

#[allow(dead_code)]
const ALL_FORMATS_COVERED: [&str; 8] = [
    FMT_BLANK,
    FMT_CHOICE,
    FMT_TRUE_FALSE,
    FMT_MATCHING,
    FMT_SHORT,
    FMT_TABLE,
    FMT_ORGANIZER,
    FMT_PARAGRAPH,
];

// ---------------------------------------------------------------- templates (我的學習單版型)

/// Call A: every `## <title>` line of the worksheet is one outline item (three generic ones
/// when there are none).
pub fn template_outline(ctx: &Value, _attempt: i32, has: &dyn Fn(&str) -> bool) -> Value {
    if has("FAKE_BLOCK") {
        return json!({ "not_an_outline": true });
    }
    let text = ctx["text"].as_str().unwrap_or_default();
    let mut outline: Vec<Value> = text
        .lines()
        .filter_map(|l| l.trim().strip_prefix("## "))
        .map(|title| {
            json!({ "title_pattern": title.trim(), "per_paragraph": title.contains("[n]") })
        })
        .collect();
    if outline.is_empty() {
        outline = ["Warm-up: [主題]", "Reading: [主題]", "Exit ticket"]
            .iter()
            .map(|t| json!({ "title_pattern": t, "per_paragraph": false }))
            .collect();
    }
    json!({
        "worksheet_kind": "課文深度閱讀",
        "instruction_language": "英文",
        "header_fields": ["Name", "No", "Class"],
        "shows_objectives": text.contains("Objective"),
        "includes_passage_text": false,
        "outline": outline,
    })
}

/// The labelled support boxes the fake extraction recognises: a line 「<heading>: <content>」 in
/// a section of the sample, with one of these headings (any case), and the role it gets.
const FAKE_SUPPORT_HEADINGS: [(&str, &str); 9] = [
    ("directions", "directions"),
    ("sentence frame for sharing", "sharing_frame"),
    ("sentence frames for sharing", "sharing_frame"),
    ("sentence frames", "sentence_frame"),
    ("sentence starters", "sentence_frame"),
    ("word bank", "word_bank"),
    ("hints", "hints"),
    ("guiding questions", "guiding_questions"),
    ("example", "example"),
];

/// The support boxes written in the section of the sample titled `title` (its lines up to the
/// next `## ` line): `{ heading, role, pattern }`, the heading as written.
fn fake_supports(text: &str, title: &str) -> Vec<Value> {
    let mut inside = false;
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(t) = line.strip_prefix("## ") {
            inside = t.trim() == title;
            continue;
        }
        if !inside {
            continue;
        }
        let Some((head, content)) = line.split_once(':') else {
            continue;
        };
        let key = head.trim().to_lowercase();
        if let Some((_, role)) = FAKE_SUPPORT_HEADINGS.iter().find(|(h, _)| *h == key) {
            out.push(json!({ "heading": head.trim(), "role": role, "pattern": content.trim() }));
        }
    }
    out
}

fn kind_of_role(role: &str) -> &'static str {
    match role {
        "directions" => KIND_STEPS,
        "sentence_frame" | "sharing_frame" => KIND_FRAMES,
        "word_bank" => KIND_WORD_LIST,
        "hints" => "提示與追問",
        "example" => KIND_EXAMPLE,
        _ => KIND_GUIDING,
    }
}

/// Call B: one section per outline item, its design guessed from the title. Support boxes are
/// read from the section's lines ([`fake_supports`]); `FAKE_TPL_FRAME_AS_ITEM` reads them the
/// way extraction did before supports existed: no boxes, the sharing frame as the fourth of four
/// question patterns (the server moves it back into a box).
pub fn template_details(ctx: &Value, _attempt: i32, has: &dyn Fn(&str) -> bool) -> Value {
    if has("FAKE_BLOCK") {
        return json!({ "sections": "unreadable" });
    }
    let bad = has("FAKE_TPL_BAD_LABEL");
    let frame_as_item = has("FAKE_TPL_FRAME_AS_ITEM");
    let text = ctx["text"].as_str().unwrap_or_default();
    let outline = ctx["outline"].as_array().cloned().unwrap_or_default();
    let sections: Vec<Value> = outline
        .iter()
        .map(|o| {
            let title = o["title_pattern"].as_str().unwrap_or_default();
            let supports = fake_supports(text, title);
            let lower = title.to_lowercase();
            let per_paragraph = o["per_paragraph"] == true;
            let external = lower.contains("video") || title.contains("影片");
            let block_kind = if lower.starts_with("warm") {
                "warmup"
            } else if lower.contains("exit") || lower.contains("reflection") {
                "exit_ticket"
            } else {
                "activity"
            };
            let format = if lower.contains("vocab") {
                FMT_BLANK
            } else if lower.contains("writ") {
                FMT_PARAGRAPH
            } else if lower.contains("choice") {
                FMT_CHOICE
            } else {
                FMT_SHORT
            };
            let mut tasks = vec!["理解內容"];
            let mut formats = vec![format];
            if bad {
                tasks.push("探究");
                formats.push("SDGs");
            }
            let mut scaffolds: Vec<&str> = supports
                .iter()
                .map(|s| kind_of_role(s["role"].as_str().unwrap_or_default()))
                .collect();
            if scaffolds.is_empty() {
                scaffolds.push(KIND_GUIDING);
            }
            if bad {
                scaffolds.push("高層次思考");
            }
            let mut item_count = if bad { 40 } else if format == FMT_PARAGRAPH { 1 } else { 3 };
            let mut patterns = vec!["What does [人物] do in paragraph [n]?".to_string()];
            let mut supports = supports;
            let sharing = supports
                .iter()
                .position(|s| s["role"] == "sharing_frame");
            if let (true, Some(at)) = (frame_as_item, sharing) {
                patterns = vec![
                    "What does [人物] do in paragraph [n]?".to_string(),
                    "Which sentence is the topic sentence of paragraph [n]?".to_string(),
                    "Is the topic sentence the first sentence? (True/False)".to_string(),
                    supports[at]["pattern"].as_str().unwrap_or_default().to_string(),
                ];
                item_count = 4;
                supports.clear();
            }
            json!({
                "title_pattern": title,
                "purpose": "（測試）讓學生依段落找出細節並說明作者的寫法。",
                "block_kind": block_kind,
                "student_tasks": tasks,
                "answer_formats": formats,
                "item_count": item_count,
                "text_scope": if per_paragraph { "逐段" } else if external { "課文外" } else { "全文" },
                "scaffolds": scaffolds,
                "supports": supports,
                "instruction_style": "英文祈使句，一句話",
                "item_patterns": patterns,
                "needs_external_material": external,
            })
        })
        .collect();
    json!({
        "sections": sections,
        "generation_options": if bad { vec![GEN_GUIDING_LABEL, "SEL"] } else { vec![GEN_GUIDING_LABEL] },
        "style_notes": ["每段先問細節再問寫作手法", "指示語都是一句英文"],
    })
}

const GEN_GUIDING_LABEL: &str = "提供引導問題";

/// One block from a template section: `item_count + 1` items (the server keeps `item_count`)
/// of the section's first answer format, about the target paragraph (or the first one). A
/// section that needs the teacher's own material gets placeholder items only.
pub fn template_section(ctx: &Value, attempt: i32, has: &dyn Fn(&str) -> bool) -> Value {
    let section = &ctx["section"];
    let title = section["title_pattern"].as_str().unwrap_or("Section");
    if has("FAKE_BLOCK") {
        return json!({ "title": title, "instructions": "", "scaffold": "", "items": [] });
    }
    let paragraphs: Vec<String> = strings(&ctx["paragraphs"]);
    let paragraph = ctx["paragraph"].as_u64().map(|p| p as usize);
    let no = paragraph.unwrap_or(1);
    let text = paragraphs
        .get(no.saturating_sub(1))
        .cloned()
        .unwrap_or_default();
    let sentence = first_sentence(&text);
    let label = match paragraph {
        Some(p) => format!("paragraph {p}"),
        None => "the text".to_string(),
    };
    let format = strings(&section["answer_formats"])
        .into_iter()
        .next()
        .unwrap_or_else(|| FMT_SHORT.to_string());
    let count = (ctx["item_count"].as_u64().unwrap_or(3) as usize + 1).min(12);
    let external = section["needs_external_material"] == true;
    let run_no = ctx["run_no"].as_i64().unwrap_or(1);
    let bloom = |k: usize| {
        if k == 1 && has("FAKE_TPL_BAD_BLOOM") {
            "comprehend"
        } else if format == FMT_PARAGRAPH {
            "create"
        } else {
            ["remember", "understand", "analyze"][(k - 1) % 3]
        }
    };
    let mut items: Vec<Value> = (1..=count)
        .map(|k| {
            if external {
                return json!({
                    "answer_format": format,
                    "prompt": format!("After you study the material your teacher gives you, write down key point {k}: ______"),
                    "options": [],
                    "answer": "Answers depend on the material the teacher provides.",
                    "bloom": "understand",
                });
            }
            let raw = item(&format, &label, &sentence, k, TIER_CORE_LABEL);
            let mut out = json!({
                "answer_format": format,
                "prompt": format!("{} [v{run_no}.{attempt}]", raw["prompt_en"].as_str().unwrap_or_default()),
                "options": raw["options"],
                "answer": raw["answer_en"],
                "bloom": bloom(k),
            });
            if let Some(o) = raw.get("organizer") {
                out["organizer"] = o.clone();
            }
            out
        })
        .collect();
    let bad = !external
        && (has("FAKE_TPL_BAD_SHAPES_ALWAYS") || (has("FAKE_TPL_BAD_SHAPES") && attempt == 1));
    let directions_in_frames = [
        "Identify the common idea in the first sentence.",
        "Look for the sentence that introduces the paragraph's main point.",
        "Use this frame: The author describes ______ to show ______.",
    ];
    if bad {
        // What a real model wrote (docs/API.md「提示框」): an opinion as a 是非題 first, the sharing
        // frame twice as a question at the end (once wrapped in a writing task).
        let keep = (ctx["item_count"].as_u64().unwrap_or(3) as usize).saturating_sub(1);
        items.truncate(keep);
        items.insert(
            0,
            json!({
                "answer_format": FMT_TRUE_FALSE,
                "prompt": "Do you think the first sentence serves as a topic sentence?",
                "options": ["True", "False"], "answer": "True", "bloom": "analyze",
            }),
        );
        let frame = format!("Hi! In Paragraph {no}, the author describes ______. After reading it carefully, I identified ______ as one of the main characteristics of informative writing.");
        items.push(json!({
            "answer_format": FMT_PARAGRAPH,
            "prompt": format!("Write a short analysis paragraph using the frame: {frame}"),
            "options": [], "answer": "Answers will vary.", "bloom": "analyze",
        }));
        items.push(json!({
            "answer_format": FMT_SHORT, "prompt": frame, "options": [],
            "answer": "Answers will vary.", "bloom": "analyze",
        }));
    }
    // `FAKE_TPL_TERMS`: odd paragraphs call the genre by another name.
    let genre = if has("FAKE_TPL_TERMS") && no % 2 == 1 {
        " Notice how this expository writing is built."
    } else if has("FAKE_TPL_TERMS") {
        " Notice how this informative writing is built."
    } else {
        ""
    };
    let mut out = json!({
        "title": title.replace("[n]", &no.to_string()),
        "instructions": format!("Read {label} and answer the questions.{genre}"),
        "items": items,
    });
    let boxes = section["supports"].as_array().cloned().unwrap_or_default();
    if boxes.is_empty() {
        out["scaffold"] = if external {
            json!([])
        } else if bad {
            json!(directions_in_frames)
        } else {
            json!(["Look for the key words in the paragraph first."])
        };
    } else {
        let words: Vec<String> = sentence
            .split(|c: char| !c.is_alphabetic())
            .filter(|w| w.chars().count() > 4)
            .take(3)
            .map(str::to_lowercase)
            .collect();
        let mut frames_written = false;
        out["supports"] = boxes
            .iter()
            .map(|b| {
                let role = b["role"].as_str().unwrap_or_default();
                let lines: Vec<String> = match role {
                    // The bad output put the steps among the frames and left these empty.
                    "directions" if bad => Vec::new(),
                    "sharing_frame" if bad => Vec::new(),
                    "sentence_frame" if bad && !frames_written => {
                        frames_written = true;
                        directions_in_frames.iter().map(|l| l.to_string()).collect()
                    }
                    "directions" => vec![
                        "Underline the sentence that tells the main idea.".to_string(),
                        "Circle two details that support it.".to_string(),
                    ],
                    "sentence_frame" => vec!["The author describes ______ to show ______.".to_string()],
                    "sharing_frame" => vec![format!(
                        "Hi! In Paragraph {no}, the author describes ______. I identified ______ as the main idea."
                    )],
                    "word_bank" => vec![words.join(", ")],
                    "hints" => vec!["Look at the first sentence of the paragraph.".to_string()],
                    "guiding_questions" => vec!["What is the paragraph mostly about?".to_string()],
                    _ => vec!["Example: The author describes the shop to show the setting.".to_string()],
                };
                json!({ "heading": b["heading"], "lines": lines })
            })
            .collect::<Vec<_>>()
            .into();
    }
    if ctx["image_slots"].as_u64().unwrap_or(0) > 0 {
        out["image_slots"] = if external {
            json!([])
        } else {
            json!([scene_slot(&label, &sentence, &picture_markers(has))])
        };
    }
    out
}

const TIER_CORE_LABEL: &str = "core";

// ---------------------------------------------------------------- per Bloom level

/// `count` questions of one level, cycling through its formats and the paragraphs, each about
/// the paragraph's first sentence. Remember / understand questions quote it as evidence; every
/// question has a hint; then three written steps and an example (prompt version 2, R5). A redo
/// (questions the judge rated as another level) is marked "redo".
pub fn bloom_level(ctx: &Value, attempt: i32, has: &dyn Fn(&str) -> bool) -> Value {
    let bloom = ctx["bloom"].as_str().unwrap_or("remember");
    let count = ctx["count"].as_u64().unwrap_or(1).max(1) as usize;
    let formats = strings(&ctx["formats"]);
    let run_no = ctx["run_no"].as_i64().unwrap_or(1);
    let redo = ctx["redo"] == true;
    let paragraphs: Vec<(String, String)> = ctx["paragraphs"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|p| {
                    (
                        p["code"].as_str().unwrap_or("P1").to_string(),
                        p["text"].as_str().unwrap_or_default().to_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    if has("FAKE_BLOCK") || formats.is_empty() || paragraphs.is_empty() {
        return json!({ "items": [{
            "answer_format": "探究", "prompt": "請討論。", "options": [], "answer": "",
            "evidence": "", "paragraph_ids": [], "hint": "",
        }], "steps": [], "example": "" });
    }
    let quotes = matches!(bloom, "remember" | "understand");
    let no_evidence = has("FAKE_BLOOM_NO_EVIDENCE") && attempt == 1 && bloom == "remember" && !redo;
    let tag = if redo {
        format!("[{bloom} v{run_no} redo]")
    } else {
        format!("[{bloom} v{run_no}]")
    };
    let items: Vec<Value> = (0..count)
        .map(|k| {
            let (code, text) = &paragraphs[k % paragraphs.len()];
            let sentence = first_sentence(text);
            let label = format!("paragraph {}", k % paragraphs.len() + 1);
            let format = formats[k % formats.len()].as_str();
            let raw = item(format, &label, &sentence, k + 1, "core");
            let evidence = if no_evidence && k == 0 {
                "This line was never written in the story at all.".to_string()
            } else if quotes {
                sentence.clone()
            } else {
                String::new()
            };
            let mut out = json!({
                "answer_format": format,
                "prompt": format!("{} {tag}", raw["prompt_en"].as_str().unwrap_or_default()),
                "options": raw["options"],
                "answer": raw["answer_en"],
                "evidence": evidence,
                "paragraph_ids": [code],
                "hint": "I think the answer is … because …",
            });
            if let Some(o) = raw.get("organizer") {
                out["organizer"] = if has("FAKE_ORG_BAD") {
                    bad_organizer()
                } else {
                    o.clone()
                };
            }
            out
        })
        .collect();
    json!({
        "items": items,
        "steps": [
            "Read the questions first.",
            "Read the text again and underline the words that help you.",
            "Write your answers, then check them against the text."
        ],
        "example": "Q: Who is the story about? A: It is about a shop owner and her neighbors.",
    })
}

/// The judge rates every question at its target level (`fake_context` carries the targets;
/// the real model never sees them).
pub fn bloom_check(ctx: &Value, _attempt: i32, has: &dyn Fn(&str) -> bool) -> Value {
    let mismatch = has("FAKE_BLOOM_MISMATCH");
    let mut flagged = false;
    let items: Vec<Value> = ctx["items"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|it| {
            let target = it["target"].as_str().unwrap_or("remember");
            let mut bloom = target;
            if mismatch && !flagged && matches!(target, "evaluate" | "create") {
                bloom = "understand";
                flagged = true;
            }
            json!({ "id": it["id"], "bloom": bloom, "reason": "（測試）依題目要學生做的事判斷。" })
        })
        .collect();
    json!({ "items": items })
}

/// `worksheet_block_revise` (see the module documentation for the markers).
pub fn block_revise(ctx: &Value, attempt: i32, has: &dyn Fn(&str) -> bool) -> Value {
    let items = ctx["items"].as_array().cloned().unwrap_or_default();
    let round = ctx["round"].as_i64().unwrap_or(1);
    let target = ctx["targets"][0].as_str().map(str::to_string).or_else(|| {
        items
            .first()
            .and_then(|i| i["code"].as_str())
            .map(str::to_string)
    });
    let pictures = block_pictures(ctx, has);
    if has("FAKE_REVISE_PIC_ONLY") {
        let kept: Vec<Value> = items.iter().map(kept_item).collect();
        return json!({
            "title": "", "instructions": "", "items": kept,
            "picture_actions": pictures,
            "reply": "已調整這一區的圖片。（測試）",
        });
    }
    if has("FAKE_REVISE_NOTHING") || target.is_none() && !has("FAKE_REVISE_ADD") {
        let kept: Vec<Value> = items.iter().map(kept_item).collect();
        return json!({
            "title": "", "instructions": "", "items": kept,
            "picture_actions": pictures,
            "reply": "你想把這一區改成哪一種題型？（測試）",
        });
    }
    let bad = has("FAKE_REVISE_BAD_ONCE") && attempt == 1
        || has("FAKE_REVISE_BAD") && !has("FAKE_REVISE_BAD_ONCE");
    let all = has("FAKE_REVISE_TOUCH_ALL");
    let mut out: Vec<Value> = Vec::new();
    let last = items.len().saturating_sub(1);
    for (k, it) in items.iter().enumerate() {
        if has("FAKE_REVISE_DROP") && k == last {
            continue;
        }
        let code = it["code"].as_str().unwrap_or_default();
        let is_target = target.as_deref() == Some(code);
        if !(is_target || all) {
            out.push(kept_item(it));
            continue;
        }
        let tag = if is_target {
            format!(" (easier r{round})")
        } else {
            " (touched)".to_string()
        };
        out.push(json!({
            "source": code,
            "keep": false,
            "answer_format": if bad && is_target { "連連看" } else { it["answer_format"].as_str().unwrap_or(FMT_SHORT) },
            "prompt": format!("{}{tag}", it["prompt"].as_str().unwrap_or_default().trim()),
            "options": it["options"].clone(),
            "answer": it["answer"].clone(),
            "bloom": "remember",
        }));
    }
    if has("FAKE_REVISE_ADD") {
        out.push(json!({
            "source": "new",
            "keep": false,
            "answer_format": FMT_SHORT,
            "prompt": "Which part of the story would you tell a friend? (added)",
            "options": [],
            "answer": "Answers will vary.",
            "bloom": "apply",
        }));
    }
    json!({
        "title": if all { format!("{} (touched)", ctx["title"].as_str().unwrap_or_default()) } else { String::new() },
        "instructions": "",
        "items": out,
        "picture_actions": pictures,
        "reply": format!("已把 {} 改簡單一點。（測試）", target.unwrap_or_default()),
    })
}

/// The picture actions of the `FAKE_REVISE_PIC_*` markers, for the block the request is about
/// (`ctx["pictures"]` are its pictures).
fn block_pictures(ctx: &Value, has: &dyn Fn(&str) -> bool) -> Vec<Value> {
    let block_id = ctx["block_id"].as_str().unwrap_or_default();
    let slots: Vec<String> = ctx["pictures"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|p| p["slot_id"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let pic = |action: &str, block: &str, slot: &str, purpose: &str, prompt: &str| {
        json!({
            "action": action, "block_id": block, "slot_id": slot, "kind": "scene",
            "purpose_zh": purpose, "prompt_en": prompt, "caption": "",
        })
    };
    let mut out = Vec::new();
    if has("FAKE_REVISE_PIC_ADD") {
        out.push(pic(
            "add",
            block_id,
            "",
            "蜜蜂停在花上",
            "A honey bee sitting on a yellow flower in a sunny garden",
        ));
    }
    if has("FAKE_REVISE_PIC_REDRAW") {
        if let Some(s) = slots.first() {
            out.push(pic("redraw", "", s, "不要有文字", "no text in the picture"));
        }
    }
    if has("FAKE_REVISE_PIC_REMOVE") {
        if let Some(s) = slots.last() {
            out.push(pic("remove", "", s, "", ""));
        }
    }
    if has("FAKE_REVISE_PIC_BAD") {
        out.push(pic("redraw", "", "no-such-slot", "x", "x"));
        out.push(pic("add", "other-block", "", "別區", "A bee"));
    }
    out
}

/// A question kept as it is: only its code, the flag and the labels.
fn kept_item(it: &Value) -> Value {
    json!({
        "source": it["code"],
        "keep": true,
        "answer_format": it["answer_format"].as_str().unwrap_or(FMT_SHORT),
        "prompt": "",
        "options": [],
        "answer": "",
        "bloom": it["bloom"].as_str().unwrap_or("understand"),
    })
}

// ---------------------------------------------------------------- 差異化學習單 (per Bloom level)

/// The support forms per question, cycling: (基礎版, 一般版, 進階版), strictly less each time.
const FAKE_FORMS: [(&str, &str, &str); 3] = [
    ("sentence_frame", "sentence_starter", "none"),
    ("cloze", "word_bank", "sentence_starter"),
    ("word_bank", "sentence_frame", "none"),
];

/// The two longest words of a sentence (the words a frame takes out), in sentence order.
fn key_words(sentence: &str) -> Vec<String> {
    let words: Vec<String> = sentence
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
        .filter(|w| w.len() > 2)
        .collect();
    let mut longest = words.clone();
    longest.sort_by_key(|w| std::cmp::Reverse(w.len()));
    longest.truncate(2);
    words
        .into_iter()
        .filter(|w| longest.contains(w))
        .take(2)
        .collect()
}

/// `sentence` with `words` replaced by blanks.
fn blanked(sentence: &str, words: &[String]) -> String {
    sentence
        .split_whitespace()
        .map(|w| {
            let bare = w.trim_matches(|c: char| !c.is_alphanumeric());
            if words.iter().any(|k| k == bare) {
                w.replace(bare, "___")
            } else {
                w.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// One version of a fake question.
fn fake_version(kind: &str, sentence: &str, level: &str) -> Value {
    let keys = key_words(sentence);
    let full = format!("The text tells us this: {sentence}");
    let (model, support, bank): (String, Vec<String>, Vec<String>) = match kind {
        "cloze" => (
            sentence.to_string(),
            vec![blanked(sentence, &keys[..keys.len().min(1)])],
            keys.iter().take(1).cloned().collect(),
        ),
        "sentence_frame" => (
            sentence.to_string(),
            vec![blanked(sentence, &keys)],
            keys.clone(),
        ),
        "word_bank" => (
            String::new(),
            vec!["Use two of these words in your answer.".to_string()],
            keys.iter()
                .cloned()
                .chain(["because".to_string(), "so".to_string()])
                .collect(),
        ),
        "sentence_starter" => (
            String::new(),
            vec!["The text tells us that …".to_string()],
            Vec::new(),
        ),
        _ => (String::new(), Vec::new(), Vec::new()),
    };
    let (answer, scoring) = match level {
        "basic" => (
            keys.join(" "),
            "（測試）寫出關鍵片語即可給分，不要求完整句。",
        ),
        "standard" => (full, "（測試）一個完整句，內容正確即給分。"),
        _ => (
            format!("{full} I think this matters for the whole text."),
            "（測試）1–2 個完整句，用自己的話說明。",
        ),
    };
    json!({
        "support_kind": kind, "model_sentence": model, "support": support,
        "word_bank": bank, "answer": answer, "scoring_zh": scoring,
    })
}

/// `count` questions of one level for a 差異化學習單, like [`bloom_level`] (formats and
/// paragraphs in turn, remember / understand questions quote the paragraph's first sentence),
/// each with its three versions in the forms of `FAKE_FORMS`; then three steps, an example and
/// linking words (none for remember). Markers:
///
/// | marker                | effect                                                              |
/// |-----------------------|---------------------------------------------------------------------|
/// | `FAKE_TIER_LOCATION`  | every call: the first question's 基礎版 support also says 「Look at paragraph 1 again.」 (the line is removed) |
/// | `FAKE_TIER_FLAT`      | first attempt: the first question's 一般版 has as much support as its 基礎版 (repaired) |
/// | `FAKE_BLOCK`          | only formats outside the level's (the level fails)                  |
pub fn tiered_level(ctx: &Value, attempt: i32, has: &dyn Fn(&str) -> bool) -> Value {
    let bloom = ctx["bloom"].as_str().unwrap_or("remember");
    let count = ctx["count"].as_u64().unwrap_or(1).max(1) as usize;
    let formats = strings(&ctx["formats"]);
    let run_no = ctx["run_no"].as_i64().unwrap_or(1);
    let redo = ctx["redo"] == true;
    let paragraphs: Vec<(String, String)> = ctx["paragraphs"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|p| {
                    (
                        p["code"].as_str().unwrap_or("P1").to_string(),
                        p["text"].as_str().unwrap_or_default().to_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let steps = json!([
        "Read the question first.",
        "Read the text again and underline the words that help you.",
        "Write your answer. Use the support under the question if you need it."
    ]);
    if has("FAKE_BLOCK") || formats.is_empty() || paragraphs.is_empty() {
        return json!({ "items": [{
            "answer_format": "探究", "prompt": "請討論。", "evidence": "", "paragraph_ids": [],
            "advanced": fake_version("none", "", "advanced"),
            "standard": fake_version("none", "", "standard"),
            "basic": fake_version("none", "", "basic"),
        }], "steps": steps, "example": "",
        "linking_words": { "function": "", "basic": [], "standard_more": [], "advanced_more": [] } });
    }
    let quotes = matches!(bloom, "remember" | "understand");
    let tag = if redo {
        format!("[{bloom} v{run_no} redo]")
    } else {
        format!("[{bloom} v{run_no}]")
    };
    let items: Vec<Value> = (0..count)
        .map(|k| {
            let (code, text) = &paragraphs[k % paragraphs.len()];
            let sentence = first_sentence(text);
            let label = format!("paragraph {}", k % paragraphs.len() + 1);
            let format = formats[k % formats.len()].as_str();
            let raw = item(format, &label, &sentence, k + 1, "core");
            let (basic, standard, advanced) = FAKE_FORMS[k % FAKE_FORMS.len()];
            let standard = if k == 0 && has("FAKE_TIER_FLAT") && attempt == 1 {
                basic
            } else {
                standard
            };
            let mut basic_v = fake_version(basic, &sentence, "basic");
            if k == 0 && has("FAKE_TIER_LOCATION") {
                if let Some(lines) = basic_v["support"].as_array_mut() {
                    lines.push(json!("Look at paragraph 1 again."));
                }
            }
            let mut out = json!({
                "answer_format": format,
                "prompt": format!("{} {tag}", raw["prompt_en"].as_str().unwrap_or_default()),
                "evidence": if quotes { sentence.clone() } else { String::new() },
                "paragraph_ids": [code],
                "advanced": fake_version(advanced, &sentence, "advanced"),
                "standard": fake_version(standard, &sentence, "standard"),
                "basic": basic_v,
            });
            if format == FMT_ORGANIZER {
                out["organizer"] = fake_organizer(k + 1, &label, &sentence, true);
            }
            out
        })
        .collect();
    let linking = match bloom {
        "remember" => {
            json!({ "function": "", "basic": [], "standard_more": [], "advanced_more": [] })
        }
        "analyze" | "evaluate" => json!({
            "function": "giving reasons", "basic": ["because", "so"],
            "standard_more": ["since", "as a result"], "advanced_more": ["consequently", "therefore"],
        }),
        _ => json!({
            "function": "putting ideas in order", "basic": ["first", "next", "then"],
            "standard_more": ["after that", "finally"], "advanced_more": ["subsequently"],
        }),
    };
    json!({
        "items": items,
        "steps": steps,
        "example": "Q: Who is the story about? A: It is about a shop owner and her neighbors.",
        "linking_words": linking,
    })
}
