//! SIOP V2: the general academic words of the instructions (compare, summarize, support your
//! answer …) are the words that most often stop students, and the most often left out of a
//! word list. After a 完整學習單 is generated, the code finds them in what the sheet tells the
//! students to do (block instructions, questions, step-by-step directions) and lists them in the
//! vocabulary section, each with a short Chinese hint. No model is involved: the words and
//! their hints are fixed here, so they cannot be made up.
//!
//! They go into the warm-up's word list (the sheet's vocabulary section), or into the first
//! activity block when there is no warm-up, marked `task_word` so that the next generation or
//! block regeneration can find them again.

use std::sync::LazyLock;

use regex::Regex;

use super::bloom::Output;
use super::{BlockKind, Content, ScaffoldType, Vocab};
use crate::vocabulary::{self, Lexicon};

/// At most this many task words (V1: few and key).
pub const MAX_TASK_WORDS: usize = 8;

/// (the word as listed, its Chinese hint, the forms that count as it).
const TASK_WORDS: [(&str, &str, &[&str]); 30] = [
    (
        "support your answer",
        "用文中的證據支持你的答案",
        &[
            "support your answers?",
            "support your ideas?",
            "support your opinions?",
        ],
    ),
    (
        "in your own words",
        "用你自己的話說",
        &["in your own words"],
    ),
    ("according to", "根據……（文中怎麼說）", &["according to"]),
    ("main idea", "主旨", &["main ideas?"]),
    ("topic sentence", "主題句", &["topic sentences?"]),
    ("evidence", "證據", &["evidence"]),
    (
        "give reasons",
        "提出理由",
        &["give (?:a|one|two|three|your) reasons?"],
    ),
    (
        "compare",
        "比較（找出相同與不同）",
        &["compar(?:e|es|ed|ing|ison)"],
    ),
    ("contrast", "對比（找出不同）", &["contrast(?:s|ed|ing)?"]),
    (
        "summarize",
        "摘要（用幾句話說出重點）",
        &["summari[sz](?:e|es|ed|ing)", "summary"],
    ),
    ("paraphrase", "換句話說", &["paraphras(?:e|es|ed|ing)"]),
    ("retell", "重述", &["retell(?:s|ing)?"]),
    (
        "explain",
        "解釋、說明",
        &["explain(?:s|ed|ing)?", "explanation"],
    ),
    (
        "describe",
        "描述",
        &["describ(?:e|es|ed|ing)", "description"],
    ),
    ("identify", "找出、指出", &["identif(?:y|ies|ied|ying)"]),
    (
        "infer",
        "推論（從線索推想）",
        &["infer(?:s|red|ring)?", "inferences?"],
    ),
    ("predict", "預測", &["predict(?:s|ed|ing)?", "predictions?"]),
    ("analyze", "分析", &["analy[sz](?:e|es|ed|ing)"]),
    ("evaluate", "評估、判斷好壞", &["evaluat(?:e|es|ed|ing)"]),
    ("justify", "說明理由", &["justif(?:y|ies|ied|ying)"]),
    (
        "classify",
        "分類",
        &["classif(?:y|ies|ied|ying)", "categori[sz]e"],
    ),
    ("organize", "整理", &["organi[sz](?:e|es|ed|ing)"]),
    ("sequence", "依先後順序排列", &["sequence"]),
    (
        "cause and effect",
        "原因與結果",
        &["cause and effect", "causes? and effects?"],
    ),
    (
        "conclusion",
        "結論",
        &["conclusions?", "conclud(?:e|es|ed|ing)"],
    ),
    ("opinion", "意見、看法", &["opinions?"]),
    ("fact", "事實", &["facts?"]),
    ("point of view", "觀點", &["points? of view"]),
    ("purpose", "目的", &["purposes?"]),
    (
        "signal words",
        "提示詞（例如 because、however）",
        &["signal words?"],
    ),
];

static PATTERNS: LazyLock<Vec<(&'static str, &'static str, Regex)>> = LazyLock::new(|| {
    TASK_WORDS
        .iter()
        .map(|(word, hint, forms)| {
            let re = Regex::new(&format!(r"(?i)\b(?:{})\b", forms.join("|")))
                .expect("valid task-word pattern");
            (*word, *hint, re)
        })
        .collect()
});

/// The task words of `texts`, in the order they first appear, at most [`MAX_TASK_WORDS`].
pub fn find(texts: &[String]) -> Vec<(&'static str, &'static str)> {
    let mut found: Vec<((usize, usize), &'static str, &'static str)> = Vec::new();
    for (word, hint, re) in PATTERNS.iter() {
        let first = texts
            .iter()
            .enumerate()
            .find_map(|(i, t)| re.find(t).map(|m| (i, m.start())));
        if let Some(at) = first {
            found.push((at, word, hint));
        }
    }
    found.sort_by_key(|(at, _, _)| *at);
    found
        .into_iter()
        .map(|(_, w, h)| (w, h))
        .take(MAX_TASK_WORDS)
        .collect()
}

/// Whether a worksheet gets task words: the 完整學習單 made by the four steps (not one made
/// from a template, which keeps its own sections, nor the outputs made per Bloom level).
pub fn applies(content: &Content) -> bool {
    content
        .chosen
        .as_ref()
        .is_some_and(|c| c.settings.output == Output::Worksheet)
        && !content
            .blocks
            .iter()
            .any(|b| b.template_section_id.is_some())
}

/// What the sheet tells the students to do: instructions, questions and step-by-step
/// directions of the enabled blocks, in order.
fn instruction_texts(content: &Content) -> Vec<String> {
    let mut out = Vec::new();
    for b in content.blocks.iter().filter(|b| b.enabled) {
        out.push(b.instructions.clone());
        out.extend(b.items.iter().map(|i| i.prompt.clone()));
        out.extend(
            b.scaffolds
                .iter()
                .filter(|s| s.kind_type == ScaffoldType::Procedural)
                .flat_map(|s| s.steps.iter().cloned()),
        );
    }
    out.retain(|t| !t.trim().is_empty());
    out
}

/// Puts the task words of `content` in its vocabulary section again: the ones listed before are
/// taken out, the words of the instructions as they are now go into the warm-up's list (or the
/// first activity block's, without a warm-up), after its key words; a word that already is a
/// key word is not listed twice. Levels come from the word list, as for every listed word.
pub fn refresh(content: &mut Content, lexicon: Option<&Lexicon>) {
    for b in &mut content.blocks {
        b.vocabulary.retain(|v| !v.task_word);
    }
    if !applies(content) {
        return;
    }
    let texts = instruction_texts(content);
    let listed: Vec<String> = content
        .blocks
        .iter()
        .flat_map(|b| b.vocabulary.iter().map(|v| v.word.trim().to_lowercase()))
        .collect();
    let words: Vec<(&str, &str)> = find(&texts)
        .into_iter()
        .filter(|(w, _)| !listed.contains(&w.to_lowercase()))
        .collect();
    if words.is_empty() {
        return;
    }
    let host = content
        .blocks
        .iter()
        .position(|b| b.enabled && b.kind == BlockKind::Warmup && !b.items.is_empty())
        .or_else(|| {
            content
                .blocks
                .iter()
                .position(|b| b.enabled && b.kind == BlockKind::Activity && !b.items.is_empty())
        });
    let Some(host) = host else {
        return;
    };
    let block = &mut content.blocks[host];
    for (word, hint) in words {
        block.vocabulary.push(Vocab {
            word: word.to_string(),
            level: lexicon.and_then(|lex| vocabulary::word_level(lex, word).level),
            definition: hint.to_string(),
            context: String::new(),
            paragraph_id: None,
            task_word: true,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worksheet::{Block, Chosen, Item, Scaffold, Settings};

    fn texts(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn task_words_are_found_in_the_order_they_appear() {
        let found = find(&texts(&[
            "Read paragraph 2. Compare the two shops and support your answer with evidence.",
            "Summarize the text in your own words.",
            "What happened in order to save the bees?",
        ]));
        let words: Vec<&str> = found.iter().map(|(w, _)| *w).collect();
        assert_eq!(
            words,
            [
                "compare",
                "support your answer",
                "evidence",
                "summarize",
                "in your own words"
            ]
        );
        assert_eq!(found[0].1, "比較（找出相同與不同）");
        // Inflected forms count; a word inside another does not ("scanner", "factory").
        let found = find(&texts(&[
            "Explaining is hard. The factory used a scanner. Give two reasons.",
        ]));
        let words: Vec<&str> = found.iter().map(|(w, _)| *w).collect();
        assert_eq!(words, ["explain", "give reasons"]);
        let many = find(&texts(&[
            "compare contrast summarize paraphrase retell explain describe identify infer predict",
        ]));
        assert_eq!(many.len(), MAX_TASK_WORDS);
    }

    fn content(with_warmup: bool) -> Content {
        let mut blocks = Vec::new();
        if with_warmup {
            blocks.push(Block {
                id: "warmup".into(),
                kind: BlockKind::Warmup,
                items: vec![Item {
                    prompt: "Predict what the text is about.".into(),
                    ..Item::default()
                }],
                vocabulary: vec![Vocab {
                    word: "predict".into(),
                    definition: "say what will happen".into(),
                    ..Vocab::default()
                }],
                ..Block::default()
            });
        }
        blocks.push(Block {
            id: "b1".into(),
            instructions: "Work with a partner. Compare the two ideas.".into(),
            items: vec![Item {
                prompt: "Explain why the bees matter. Support your answer.".into(),
                ..Item::default()
            }],
            scaffolds: vec![Scaffold {
                kind_type: ScaffoldType::Procedural,
                steps: vec!["Underline the evidence first.".into()],
                ..Scaffold::default()
            }],
            ..Block::default()
        });
        Content {
            blocks,
            chosen: Some(Chosen {
                settings: Settings::default(),
                ..Chosen::default()
            }),
            ..Content::default()
        }
    }

    #[test]
    fn task_words_go_into_the_vocabulary_section_once() {
        let mut c = content(true);
        refresh(&mut c, None);
        let warmup = &c.blocks[0].vocabulary;
        let words: Vec<&str> = warmup.iter().map(|v| v.word.as_str()).collect();
        // "predict" is a key word already; the rest follow the key words.
        assert_eq!(
            words,
            [
                "predict",
                "compare",
                "explain",
                "support your answer",
                "evidence"
            ]
        );
        assert!(warmup[1].task_word && !warmup[0].task_word);
        assert_eq!(warmup[2].definition, "解釋、說明");
        assert!(c.blocks[1].vocabulary.is_empty());
        // Again: the same list, not twice.
        refresh(&mut c, None);
        assert_eq!(c.blocks[0].vocabulary.len(), 5);
        // Without a warm-up, the first activity block lists them.
        let mut c = content(false);
        refresh(&mut c, None);
        assert_eq!(c.blocks[0].vocabulary.len(), 4);
        // Other outputs and template worksheets get none (and lose what they had).
        let mut c = content(true);
        refresh(&mut c, None);
        c.chosen.as_mut().unwrap().settings.output = Output::Comprehension;
        refresh(&mut c, None);
        assert_eq!(c.blocks[0].vocabulary.len(), 1);
        let mut c = content(true);
        c.blocks[1].template_section_id = Some("s1".into());
        refresh(&mut c, None);
        assert_eq!(c.blocks[0].vocabulary.len(), 1);
    }
}
