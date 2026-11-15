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
//! | `FAKE_BLOCK`             | every step: output with nothing usable                              |
//! | `FAKE_RUNAWAY`           | every step: call 1 runs to `max_tokens` in whitespace (see `ai::fake`) |
//! | `FAKE_SLOW`              | each call waits three seconds first (handled in `sop::ai`)          |
//!
//! `FAKE_WS_SPLICE_ALWAYS` evidence has no verbatim piece of five words, so it is cleared.

use serde_json::{json, Value};

use super::schemas::TARGET_WARMUP;
use super::{
    task_for_function, FMT_BLANK, FMT_CHOICE, FMT_MATCHING, FMT_ORGANIZER, FMT_PARAGRAPH,
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
            format!("Draw a cause-and-effect chain for {label} with at least three boxes. (Question {k})"),
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
    json!({
        "item_id": format!("X-{k}"), "answer_format": format, "tier": tier,
        "prompt_en": prompt, "options": options, "answer_en": answer, "evidence": evidence,
    })
}

pub fn tasks(ctx: &Value, attempt: i32, has: &dyn Fn(&str) -> bool) -> Value {
    let invalid = has("FAKE_BLOCK");
    let cjk = has("FAKE_WS_CJK_ALWAYS") || (has("FAKE_WS_CJK") && attempt == 1);
    let bad_label = has("FAKE_WS_BAD_LABEL") && attempt == 1;
    let splice = has("FAKE_WS_SPLICE_ALWAYS");
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
            k += 1;
            let mut challenge = item(FMT_SHORT, &label, &sentence, k, "challenge");
            challenge["prompt_en"] = json!(format!(
                "Do you agree with the idea in {label}? Give two reasons, one of them from the text. (Question {k})"
            ));
            challenge["answer_en"] = json!("Answers will vary. Both reasons should be explained.");
            challenge["evidence"] = Value::Null;
            items.push(challenge);
            if run_no > 1 {
                for it in &mut items {
                    let p = it["prompt_en"].as_str().unwrap_or_default().to_string();
                    it["prompt_en"] = json!(format!("{p} [draft {run_no}]"));
                }
            }
            if invalid {
                for it in &mut items {
                    it["prompt_en"] = json!("請用中文回答這一題。");
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
            json!({
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
                "student_tasks": student_tasks,
                "objective_refs": objectives.iter().take(1).collect::<Vec<_>>(),
                "instructions_en": format!("Read {label} again, then answer."),
                "items": items,
                "design_reason_zh": format!("先確認{}的關鍵訊息，再往「{}」推進。", if nos.len() > 1 { "這幾段" } else { "這一段" }, function.1),
            })
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
    let selected = strings(&ctx["selected_kinds"]);
    let content_of = |kind: &str| -> &'static str {
        match kind {
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

pub fn exit_ticket(ctx: &Value, _attempt: i32, has: &dyn Fn(&str) -> bool) -> Value {
    if has("FAKE_BLOCK") {
        return json!({ "estimated_minutes": 3, "items": [{
            "item_id": "E1", "purpose": "形成性評量", "prompt_en": "今天學到什麼？",
            "answer_format": FMT_SHORT, "options": [], "student_tasks": ["學習反思"],
            "objective_refs": [], "sample_answer_en": null,
        }] });
    }
    let count = ctx["exit_count"].as_u64().unwrap_or(3) as usize;
    let objectives = strings(&ctx["objective_ids"]);
    let mut items = vec![
        json!({
            "item_id": "E1", "purpose": "形成性評量",
            "prompt_en": "In one or two sentences, explain the most important idea of today's text in your own words.",
            "answer_format": FMT_SHORT, "options": [], "student_tasks": ["理解內容"],
            "objective_refs": objectives.iter().take(1).collect::<Vec<_>>(),
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
