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

/// One academic task word: the word as listed, its part of speech, a fixed English definition
/// (student-friendly, at most [`MAX_DEFINITION_WORDS`] words, simpler than the word and without
/// the word itself) and its Chinese hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskWord {
    pub word: &'static str,
    pub pos: &'static str,
    pub definition_en: &'static str,
    pub meaning_zh: &'static str,
}

/// Longest English definition of a task word, in words.
pub const MAX_DEFINITION_WORDS: usize = 15;

/// (the word, the forms that count as it).
const TASK_WORDS: [(TaskWord, &[&str]); 30] = [
    (
        TaskWord {
            word: "support your answer",
            pos: "phr.",
            definition_en: "use facts or details from the text to show your idea is right",
            meaning_zh: "用文中的證據支持你的答案",
        },
        &[
            "support your answers?",
            "support your ideas?",
            "support your opinions?",
        ],
    ),
    (
        TaskWord {
            word: "in your own words",
            pos: "phr.",
            definition_en: "say it in a way that is not copied from the text",
            meaning_zh: "用你自己的話說",
        },
        &["in your own words"],
    ),
    (
        TaskWord {
            word: "according to",
            pos: "prep.",
            definition_en: "as someone or something says or shows",
            meaning_zh: "根據……（文中怎麼說）",
        },
        &["according to"],
    ),
    (
        TaskWord {
            word: "main idea",
            pos: "n.",
            definition_en: "the most important point of a text",
            meaning_zh: "主旨",
        },
        &["main ideas?"],
    ),
    (
        TaskWord {
            word: "topic sentence",
            pos: "n.",
            definition_en: "the sentence that tells what a paragraph is about",
            meaning_zh: "主題句",
        },
        &["topic sentences?"],
    ),
    (
        TaskWord {
            word: "evidence",
            pos: "n.",
            definition_en: "facts or details that show something is true",
            meaning_zh: "證據",
        },
        &["evidence"],
    ),
    (
        TaskWord {
            word: "give reasons",
            pos: "phr.",
            definition_en: "say why something is true or why you think so",
            meaning_zh: "提出理由",
        },
        &["give (?:a|one|two|three|your) reasons?"],
    ),
    (
        TaskWord {
            word: "compare",
            pos: "v.",
            definition_en: "to look at how things are the same and different",
            meaning_zh: "比較（找出相同與不同）",
        },
        &["compar(?:e|es|ed|ing|ison)"],
    ),
    (
        TaskWord {
            word: "contrast",
            pos: "v.",
            definition_en: "to look at how things are different",
            meaning_zh: "對比（找出不同）",
        },
        &["contrast(?:s|ed|ing)?"],
    ),
    (
        TaskWord {
            word: "summarize",
            pos: "v.",
            definition_en: "to tell the main points in a few short sentences",
            meaning_zh: "摘要（用幾句話說出重點）",
        },
        &["summari[sz](?:e|es|ed|ing)", "summary"],
    ),
    (
        TaskWord {
            word: "paraphrase",
            pos: "v.",
            definition_en: "to say something again in different words",
            meaning_zh: "換句話說",
        },
        &["paraphras(?:e|es|ed|ing)"],
    ),
    (
        TaskWord {
            word: "retell",
            pos: "v.",
            definition_en: "to tell what happened again",
            meaning_zh: "重述",
        },
        &["retell(?:s|ing)?"],
    ),
    (
        TaskWord {
            word: "explain",
            pos: "v.",
            definition_en: "to make something clear with details or reasons",
            meaning_zh: "解釋、說明",
        },
        &["explain(?:s|ed|ing)?", "explanation"],
    ),
    (
        TaskWord {
            word: "describe",
            pos: "v.",
            definition_en: "to tell what someone or something is like",
            meaning_zh: "描述",
        },
        &["describ(?:e|es|ed|ing)", "description"],
    ),
    (
        TaskWord {
            word: "identify",
            pos: "v.",
            definition_en: "to find and name something",
            meaning_zh: "找出、指出",
        },
        &["identif(?:y|ies|ied|ying)"],
    ),
    (
        TaskWord {
            word: "infer",
            pos: "v.",
            definition_en: "to guess what is true by using clues",
            meaning_zh: "推論（從線索推想）",
        },
        &["infer(?:s|red|ring)?", "inferences?"],
    ),
    (
        TaskWord {
            word: "predict",
            pos: "v.",
            definition_en: "to say what will happen next",
            meaning_zh: "預測",
        },
        &["predict(?:s|ed|ing)?", "predictions?"],
    ),
    (
        TaskWord {
            word: "analyze",
            pos: "v.",
            definition_en: "to look closely at the parts to understand something",
            meaning_zh: "分析",
        },
        &["analy[sz](?:e|es|ed|ing)"],
    ),
    (
        TaskWord {
            word: "evaluate",
            pos: "v.",
            definition_en: "to decide how good or useful something is",
            meaning_zh: "評估、判斷好壞",
        },
        &["evaluat(?:e|es|ed|ing)"],
    ),
    (
        TaskWord {
            word: "justify",
            pos: "v.",
            definition_en: "to give good reasons for what you think or do",
            meaning_zh: "說明理由",
        },
        &["justif(?:y|ies|ied|ying)"],
    ),
    (
        TaskWord {
            word: "classify",
            pos: "v.",
            definition_en: "to put things into groups",
            meaning_zh: "分類",
        },
        &["classif(?:y|ies|ied|ying)", "categori[sz]e"],
    ),
    (
        TaskWord {
            word: "organize",
            pos: "v.",
            definition_en: "to put things in a clear order or plan",
            meaning_zh: "整理",
        },
        &["organi[sz](?:e|es|ed|ing)"],
    ),
    (
        TaskWord {
            word: "sequence",
            pos: "v.",
            definition_en: "to put events in the order they happened",
            meaning_zh: "依先後順序排列",
        },
        &["sequence"],
    ),
    (
        TaskWord {
            word: "cause and effect",
            pos: "phr.",
            definition_en: "why something happens and what happens because of it",
            meaning_zh: "原因與結果",
        },
        &["cause and effect", "causes? and effects?"],
    ),
    (
        TaskWord {
            word: "conclusion",
            pos: "n.",
            definition_en: "the final idea or decision after thinking about the facts",
            meaning_zh: "結論",
        },
        &["conclusions?", "conclud(?:e|es|ed|ing)"],
    ),
    (
        TaskWord {
            word: "opinion",
            pos: "n.",
            definition_en: "what a person thinks or feels about something",
            meaning_zh: "意見、看法",
        },
        &["opinions?"],
    ),
    (
        TaskWord {
            word: "fact",
            pos: "n.",
            definition_en: "something that is true and can be checked",
            meaning_zh: "事實",
        },
        &["facts?"],
    ),
    (
        TaskWord {
            word: "point of view",
            pos: "phr.",
            definition_en: "the way a person sees or thinks about something",
            meaning_zh: "觀點",
        },
        &["points? of view"],
    ),
    (
        TaskWord {
            word: "purpose",
            pos: "n.",
            definition_en: "the reason why something is done or made",
            meaning_zh: "目的",
        },
        &["purposes?"],
    ),
    (
        TaskWord {
            word: "signal words",
            pos: "phr.",
            definition_en: "special clues in a sentence that show how ideas connect",
            meaning_zh: "提示詞（例如 because、however）",
        },
        &["signal words?"],
    ),
];

static PATTERNS: LazyLock<Vec<(TaskWord, Regex)>> = LazyLock::new(|| {
    TASK_WORDS
        .iter()
        .map(|(word, forms)| {
            let re = Regex::new(&format!(r"(?i)\b(?:{})\b", forms.join("|")))
                .expect("valid task-word pattern");
            (*word, re)
        })
        .collect()
});

/// The task word listed as `word` (any case).
pub fn lookup(word: &str) -> Option<TaskWord> {
    let key = word.trim().to_lowercase();
    TASK_WORDS
        .iter()
        .find(|(t, _)| t.word == key)
        .map(|(t, _)| *t)
}

/// The task words of `texts`, in the order they first appear, at most [`MAX_TASK_WORDS`].
pub fn find(texts: &[String]) -> Vec<TaskWord> {
    let mut found: Vec<((usize, usize), TaskWord)> = Vec::new();
    for (word, re) in PATTERNS.iter() {
        let first = texts
            .iter()
            .enumerate()
            .find_map(|(i, t)| re.find(t).map(|m| (i, m.start())));
        if let Some(at) = first {
            found.push((at, *word));
        }
    }
    found.sort_by_key(|(at, _)| *at);
    found
        .into_iter()
        .map(|(_, w)| w)
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
    let words: Vec<TaskWord> = find(&texts)
        .into_iter()
        .filter(|t| !listed.contains(&t.word.to_lowercase()))
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
    for t in words {
        block.vocabulary.push(Vocab {
            word: t.word.to_string(),
            level: lexicon.and_then(|lex| vocabulary::word_level(lex, t.word).level),
            // English by default, the Chinese hint beside it (both fixed here, no model).
            definition: t.definition_en.to_string(),
            meaning_zh: t.meaning_zh.to_string(),
            pos: t.pos.to_string(),
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
        let words: Vec<&str> = found.iter().map(|t| t.word).collect();
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
        assert_eq!(found[0].meaning_zh, "比較（找出相同與不同）");
        assert_eq!(found[0].pos, "v.");
        // Inflected forms count; a word inside another does not ("scanner", "factory").
        let found = find(&texts(&[
            "Explaining is hard. The factory used a scanner. Give two reasons.",
        ]));
        let words: Vec<&str> = found.iter().map(|t| t.word).collect();
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
        assert_eq!(
            warmup[2].definition,
            "to make something clear with details or reasons"
        );
        assert_eq!(warmup[2].meaning_zh, "解釋、說明");
        assert_eq!(warmup[2].pos, "v.");
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

    #[test]
    fn every_task_word_has_a_plain_english_definition_and_a_pos() {
        for (t, _) in TASK_WORDS.iter() {
            let def = t.definition_en.to_lowercase();
            assert!(
                !def.trim().is_empty() && def.split_whitespace().count() <= MAX_DEFINITION_WORDS,
                "{}: {}",
                t.word,
                t.definition_en
            );
            assert!(!def.contains(t.word), "{} uses itself", t.word);
            if !t.word.contains(' ') {
                let stem: String = t.word.chars().take(5).collect();
                assert!(!def.contains(&stem), "{} uses its stem", t.word);
            }
            assert!(
                crate::worksheet::key_vocab::normalise_pos(t.pos).is_some(),
                "{}",
                t.word
            );
            assert!(t
                .meaning_zh
                .chars()
                .any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)));
        }
    }
}
