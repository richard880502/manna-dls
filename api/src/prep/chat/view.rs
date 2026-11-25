//! What the model reads of the worksheet (the compact view in the fixed message prefix) and what
//! the teacher reads of a turn's changes (built from the difference between the worksheet as it
//! was and as it was left, never from the model's words).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::tools::Work;
use crate::text::clip_chars;
use crate::worksheet::images::{actions, SlotState};
use crate::worksheet::{Block, Content, ScaffoldType, VocabLang, FMT_TABLE};

/// One line of 「這一輪改了」 (`PrepChatResult.changes`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Change {
    /// `block` | `picture` | `vocabulary`
    pub target: String,
    pub block_id: Option<String>,
    /// 「C 區」「C 區的圖」「Key Vocabulary」
    pub label: String,
    pub summary: String,
}

/// 「沒有改成：B 區（原因）」.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NotApplied {
    pub label: String,
    pub reason: String,
}

fn kind_name(b: &Block) -> &'static str {
    match b.kind {
        crate::worksheet::BlockKind::Warmup => "暖身",
        crate::worksheet::BlockKind::Activity => "活動",
        crate::worksheet::BlockKind::ExitTicket => "Exit ticket",
    }
}

/// The worksheet in a few lines per block (no answers: `read_block` gives the whole block).
pub fn compact(
    content: &Content,
    labels: &HashMap<String, String>,
    printed: &HashMap<String, usize>,
    states: &[SlotState],
) -> String {
    let mut out: Vec<String> = Vec::new();
    for b in content.blocks.iter().filter(|b| b.enabled) {
        let label = labels.get(&b.id).filter(|l| !l.is_empty());
        out.push(format!(
            "【{}】block_id: {}｜{}｜標題：{}",
            label.map_or("沒有字母".to_string(), |l| format!("{l} 區")),
            b.id,
            kind_name(b),
            clip_chars(b.title.trim(), 80)
        ));
        if !b.instructions.trim().is_empty() {
            out.push(format!(
                "  作答指示：{}",
                clip_chars(b.instructions.trim(), 200)
            ));
        }
        for (k, i) in b.items.iter().enumerate() {
            let no = printed
                .get(&i.id)
                .map(|n| format!("，學生版第 {n} 題"))
                .unwrap_or_default();
            let mut line = format!(
                "  第 {} 題（{}{no}）：{}",
                k + 1,
                i.answer_format.as_deref().unwrap_or("未指定"),
                clip_chars(i.prompt.trim(), 160)
            );
            if let Some(t) = i
                .table
                .as_ref()
                .filter(|_| i.answer_format.as_deref() == Some(FMT_TABLE))
            {
                line.push_str(&format!(
                    "［表格 欄：{}｜列：{}］",
                    t.columns.join("、"),
                    t.row_labels.join("、")
                ));
            }
            if !i.frame.trim().is_empty() {
                line.push_str("［有句型框架］");
            }
            if !i.hint.trim().is_empty() {
                line.push_str("［有提示］");
            }
            out.push(line);
        }
        let scaffolds: Vec<String> = b
            .scaffolds
            .iter()
            .map(|s| match s.kind_type {
                ScaffoldType::Procedural => format!("{}（{} 步）", s.kind, s.steps.len()),
                _ => match &s.bank {
                    Some(bank) => format!(
                        "{}：{}",
                        s.kind,
                        bank.words
                            .iter()
                            .map(|w| w.word.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    None => s.kind.clone(),
                },
            })
            .collect();
        if !scaffolds.is_empty() {
            out.push(format!("  鷹架：{}", scaffolds.join("；")));
        }
        if !b.vocabulary.is_empty() {
            out.push(format!(
                "  Key Vocabulary（字義語言：{}）：{}",
                if content.vocabulary_language == VocabLang::Zh {
                    "中文"
                } else {
                    "英文"
                },
                b.vocabulary
                    .iter()
                    .map(|v| v.word.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        for d in actions::describe(b, states) {
            out.push(format!(
                "  圖 {}（slot_id: {}）：{}｜{}",
                d["number"],
                d["slot_id"].as_str().unwrap_or_default(),
                clip_chars(d["purpose"].as_str().unwrap_or_default(), 60),
                d["state"].as_str().unwrap_or_default()
            ));
        }
    }
    if out.is_empty() {
        "（學習單還沒有區塊）".to_string()
    } else {
        out.join("\n")
    }
}

/// The blocks for the fake provider (what its scripts edit).
pub fn fake_blocks(content: &Content, labels: &HashMap<String, String>) -> Value {
    json!(content
        .blocks
        .iter()
        .filter(|b| b.enabled)
        .map(|b| json!({
            "block_id": b.id,
            "label": labels.get(&b.id).cloned().unwrap_or_default(),
            "title": b.title,
            "instructions": b.instructions,
            "has_vocabulary": !b.vocabulary.is_empty(),
            "slots": b.image_slots.iter().map(|s| s.id.clone()).collect::<Vec<_>>(),
            "items": b.items.iter().map(|i| json!({
                "answer_format": i.answer_format, "prompt": i.prompt,
                "options": i.options, "answer": i.answer,
                "table": i.table.as_ref().map(crate::worksheet::table::RawTable::from),
            })).collect::<Vec<_>>(),
        }))
        .collect::<Vec<_>>())
}

/// What changed inside a block (everything but its Key Vocabulary), in words; empty when
/// nothing did.
fn block_parts(before: &Block, after: &Block) -> Vec<String> {
    let mut parts = Vec::new();
    if before.title != after.title {
        parts.push("改了標題".to_string());
    }
    if before.instructions != after.instructions {
        parts.push("改了作答指示".to_string());
    }
    let n = before.items.len().min(after.items.len());
    for k in 0..n {
        let (b, a) = (&before.items[k], &after.items[k]);
        let no = k + 1;
        if b.answer_format != a.answer_format && a.answer_format.as_deref() == Some(FMT_TABLE) {
            let cols = a
                .table
                .as_ref()
                .map(|t| format!("（欄：{}）", t.columns.join("、")))
                .unwrap_or_default();
            parts.push(format!("第 {no} 題改成表格整理{cols}"));
        } else if b.answer_format != a.answer_format {
            parts.push(format!(
                "第 {no} 題改成{}",
                a.answer_format.as_deref().unwrap_or("未指定")
            ));
        } else if b.prompt != a.prompt {
            parts.push(format!("第 {no} 題改了題目"));
        } else if b.table != a.table {
            parts.push(format!("第 {no} 題改了表格"));
        } else if b.options != a.options || b.answer != a.answer {
            parts.push(format!("第 {no} 題改了選項或答案"));
        }
        if b.frame != a.frame {
            parts.push(format!("第 {no} 題的句型框架"));
        }
        if b.hint != a.hint {
            parts.push(format!("第 {no} 題的提示"));
        }
    }
    if after.items.len() > n {
        parts.push(format!("新增 {} 題", after.items.len() - n));
    }
    if before.items.len() > n {
        parts.push(format!("刪除 {} 題", before.items.len() - n));
    }
    if before.scaffolds != after.scaffolds {
        let steps = |b: &Block| -> Vec<Vec<String>> {
            b.scaffolds
                .iter()
                .filter(|s| s.kind_type == ScaffoldType::Procedural)
                .map(|s| s.steps.clone())
                .collect()
        };
        let banks = |b: &Block| -> Vec<_> {
            b.scaffolds
                .iter()
                .filter(|s| s.is_word_bank())
                .map(|s| s.bank.clone())
                .collect()
        };
        if steps(before) != steps(after) {
            parts.push("改了步驟鷹架".to_string());
        }
        if banks(before) != banks(after) {
            parts.push("改了 Word bank".to_string());
        }
        if before.scaffolds.len() != after.scaffolds.len() {
            parts.push("增減了鷹架".to_string());
        }
    }
    parts
}

fn count_words(before: &Block, after: &Block) -> String {
    let has = |b: &Block, w: &str| {
        b.vocabulary
            .iter()
            .any(|v| v.word.trim().eq_ignore_ascii_case(w.trim()))
    };
    let added = after
        .vocabulary
        .iter()
        .filter(|v| !has(before, &v.word))
        .count();
    let removed = before
        .vocabulary
        .iter()
        .filter(|v| !has(after, &v.word))
        .count();
    let mut parts = Vec::new();
    if added > 0 {
        parts.push(format!("新增 {added} 個字"));
    }
    if removed > 0 {
        parts.push(format!("移除 {removed} 個字"));
    }
    if parts.is_empty() {
        parts.push("改了字義或詞性".to_string());
    }
    parts.join("、")
}

/// The changes between the worksheet as the turn found it and as it left it.
pub fn changes(work: &Work<'_>) -> Vec<Change> {
    let (before, after) = (&work.original, &work.content);
    let mut out = Vec::new();
    // Blocks as they were, in the order of the worksheet as it is now, then the ones that left.
    for b in &after.blocks {
        let name = work.name(&b.id);
        match before.block(&b.id) {
            None => out.push(Change {
                target: "block".into(),
                block_id: Some(b.id.clone()),
                label: name,
                summary: format!(
                    "新增了這一區「{}」（{} 題）",
                    clip_chars(b.title.trim(), 30),
                    b.items.len()
                ),
            }),
            Some(old) => {
                if old.enabled != b.enabled {
                    out.push(Change {
                        target: "block".into(),
                        block_id: Some(b.id.clone()),
                        label: name.clone(),
                        summary: if b.enabled {
                            "重新開啟了這一區".into()
                        } else {
                            "關閉了這一區（沒有刪除）".into()
                        },
                    });
                    continue;
                }
                let parts = block_parts(old, b);
                if !parts.is_empty() {
                    out.push(Change {
                        target: "block".into(),
                        block_id: Some(b.id.clone()),
                        label: name.clone(),
                        summary: parts.join("、"),
                    });
                }
                if old.vocabulary != b.vocabulary {
                    out.push(Change {
                        target: "vocabulary".into(),
                        block_id: Some(b.id.clone()),
                        label: "Key Vocabulary".into(),
                        summary: count_words(old, b),
                    });
                }
                if work.moved.contains(&b.id) && moved(before, after, &b.id) {
                    out.push(Change {
                        target: "block".into(),
                        block_id: Some(b.id.clone()),
                        label: name,
                        summary: "調整了順序".into(),
                    });
                }
            }
        }
    }
    if before.vocabulary_language != after.vocabulary_language {
        out.push(Change {
            target: "vocabulary".into(),
            block_id: None,
            label: "Key Vocabulary".into(),
            summary: if after.vocabulary_language == VocabLang::Zh {
                "字義改成中文".into()
            } else {
                "字義改成英文".into()
            },
        });
    }
    out
}

/// Whether `id` is somewhere else among the blocks both versions have.
fn moved(before: &Content, after: &Content, id: &str) -> bool {
    let common = |c: &Content, other: &Content| -> Vec<String> {
        c.blocks
            .iter()
            .filter(|b| other.block(&b.id).is_some())
            .map(|b| b.id.clone())
            .collect()
    };
    let (a, b) = (common(before, after), common(after, before));
    a.iter().position(|x| x == id) != b.iter().position(|x| x == id)
}
