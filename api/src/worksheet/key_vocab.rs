//! Key Vocabulary (the worksheet's word table): part of speech and the two meaning languages.
//!
//! Every row keeps an English meaning (`Vocab::definition`) and a Chinese one
//! (`Vocab::meaning_zh`); the worksheet's `vocabulary_language` only chooses which one is
//! printed, so switching back and forth loses neither. New worksheets get both from the warm-up
//! step (content words) or from [`super::task_words`] (academic words). A row without a Chinese
//! meaning (an older worksheet, a word the teacher added) is filled by ONE call of the existing
//! gloss prompt when the teacher switches to Chinese ([`fill_chinese`]); when that cannot be
//! done the row keeps its English meaning and the teacher is told so.

use std::collections::{BTreeMap, HashMap};

use serde_json::json;

use super::vocab_actions::{gloss_schema, RawGlosses, KEY_GLOSS, SCAFFOLD, SCHEMA_GLOSS};
use super::{task_words, BankWord, Content, GLOSS_DEFINITION_CHARS, GLOSS_MEANING_CHARS};
use crate::ai::pipeline::InteractionMeta;
use crate::ai::LlmRequest;
use crate::application::ai::{attempt, finish_failure, finish_output, Failure};
use crate::audit::scaffold_id;
use crate::checks::{self as base_checks, CheckResult};
use crate::error::AppResult;
use crate::prompts::{self, latest_required as prompt};
use crate::text::clip_chars;
use crate::vocabulary::{self, Lexicon};
use crate::AppState;

pub fn has_han(s: &str) -> bool {
    s.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
}

// ---------------------------------------------------------------- part of speech

/// The parts of speech a row may carry.
pub const POS_SET: [&str; 11] = [
    "n.", "v.", "adj.", "adv.", "prep.", "conj.", "pron.", "det.", "aux.", "interj.", "phr.",
];

/// One part of speech in any usual writing (`noun`, `n`, `n.`, `Adj.` …) as one of [`POS_SET`];
/// `None` for anything else. Several written together (`n./v.`) count as the first.
pub fn normalise_pos(raw: &str) -> Option<&'static str> {
    let first = raw
        .split(['/', ',', ';', '、'])
        .next()
        .unwrap_or_default()
        .trim()
        .trim_matches(|c| c == '(' || c == ')')
        .trim()
        .trim_end_matches('.')
        .trim()
        .to_lowercase();
    Some(match first.as_str() {
        "n" | "noun" => "n.",
        "v" | "verb" | "phrasal verb" | "vi" | "vt" => "v.",
        "adj" | "a" | "adjective" => "adj.",
        "adv" | "adverb" => "adv.",
        "prep" | "preposition" => "prep.",
        "conj" | "conjunction" => "conj.",
        "pron" | "pronoun" => "pron.",
        "det" | "determiner" | "art" | "article" => "det.",
        "aux" | "auxiliary" | "modal" | "modal verb" => "aux.",
        "interj" | "int" | "interjection" => "interj.",
        "phr" | "phrase" | "idiom" => "phr.",
        _ => return None,
    })
}

/// The parts of speech the word list prints for a word (`n./v.` → n., v.), as [`POS_SET`].
fn listed_atoms(listed: &str) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for atom in listed.split('/') {
        if let Some(p) = normalise_pos(atom) {
            if !out.contains(&p) {
                out.push(p);
            }
        }
    }
    out
}

/// What to print as the part of speech of a content word: the model's (normalised) when the
/// word list gives it for the word (or gives none); otherwise the list's when it gives exactly
/// one; otherwise the model's normalised value (empty when it has none).
pub fn resolve_pos(model: &str, listed: &str) -> String {
    let m = normalise_pos(model);
    let atoms = listed_atoms(listed);
    match m {
        Some(m) if atoms.is_empty() || atoms.contains(&m) => m.to_string(),
        _ if atoms.len() == 1 => atoms[0].to_string(),
        m => m.unwrap_or_default().to_string(),
    }
}

/// The parts of speech the word list prints for `word` (`n./v.`), also for an inflected form;
/// "" when it is not listed or the list has none.
pub fn listed_pos_of(lex: &Lexicon, word: &str) -> String {
    let key = word.trim().replace('’', "'").to_lowercase();
    let direct = lex.listed_pos(&key);
    if !direct.is_empty() {
        return direct;
    }
    vocabulary::lookup(lex, &key)
        .map(|h| lex.listed_pos(&h.base))
        .unwrap_or_default()
}

// ---------------------------------------------------------------- Chinese meanings

/// The distinct words (lower-cased) of the tables that still lack a Chinese meaning, in order
/// of appearance. Task words are not among them while the fixed table has theirs.
pub fn missing_zh(content: &Content) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for b in &content.blocks {
        for v in &b.vocabulary {
            let word = v.word.trim().to_lowercase();
            let fixed = v.task_word && task_words::lookup(&word).is_some();
            if !word.is_empty() && v.lacks_zh() && !fixed && !out.contains(&word) {
                out.push(word);
            }
        }
    }
    out
}

/// A word of a word bank still lacks what the sheet prints beside it: its part of speech, or
/// any meaning at all.
fn bank_word_missing(w: &BankWord) -> bool {
    w.pos.trim().is_empty() || (w.definition.trim().is_empty() && w.meaning_zh.trim().is_empty())
}

/// The distinct words (lower-cased) of the word banks that still lack a part of speech or a
/// meaning, in order of appearance.
pub fn missing_bank(content: &Content) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for b in &content.blocks {
        for s in &b.scaffolds {
            for w in s.bank.iter().flat_map(|b| &b.words) {
                let word = w.word.trim().to_lowercase();
                if !word.is_empty() && bank_word_missing(w) && !out.contains(&word) {
                    out.push(word);
                }
            }
        }
    }
    out
}

/// Gives every word of a word bank without a part of speech the one the word list prints for it
/// (when it prints exactly one). No model involved.
pub fn bank_pos_from_list(content: &mut Content, lexicon: Option<&Lexicon>) -> bool {
    let Some(lex) = lexicon else {
        return false;
    };
    let mut changed = false;
    for s in content
        .blocks
        .iter_mut()
        .flat_map(|b| b.scaffolds.iter_mut())
    {
        for w in s.bank.iter_mut().flat_map(|b| b.words.iter_mut()) {
            if w.pos.trim().is_empty() {
                let pos = resolve_pos("", &listed_pos_of(lex, &w.word));
                if !pos.is_empty() {
                    w.pos = pos;
                    changed = true;
                }
            }
        }
    }
    changed
}

/// Puts the fixed Chinese hint on every task word that lacks one (no model involved).
fn fill_task_words(content: &mut Content) -> bool {
    let mut changed = false;
    for v in content
        .blocks
        .iter_mut()
        .flat_map(|b| b.vocabulary.iter_mut())
    {
        if v.task_word && v.lacks_zh() {
            if let Some(t) = task_words::lookup(&v.word) {
                v.meaning_zh = t.meaning_zh.to_string();
                changed = true;
            }
        }
    }
    changed
}

/// What to fill: the Key Vocabulary tables' Chinese meanings (the teacher switched to Chinese)
/// and/or the part of speech and meanings of the word banks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(default)]
pub struct Scope {
    pub tables: bool,
    pub banks: bool,
}

/// What came of asking for the missing meanings.
#[derive(Debug, Clone, PartialEq)]
pub struct Filled {
    /// The content was changed (save it as a new version).
    pub changed: bool,
    /// Words that got something from the model.
    pub filled: usize,
    /// Said to the teacher when some words keep what they had.
    pub notice: Option<String>,
}

const MSG_STILL_ENGLISH: &str = "沒有中文字義的字會先印英文字義；你也可以直接在字詞表上自己填。";
const MSG_NO_POS: &str = "沒有詞性的字會先只印字；你也可以自己在字詞表上填。";

fn notices(tables: usize, banks: usize, why: &str) -> Option<String> {
    let mut out = Vec::new();
    if tables > 0 {
        out.push(format!(
            "{why}{tables} 個字沒有中文字義。{MSG_STILL_ENGLISH}"
        ));
    }
    if banks > 0 {
        out.push(format!(
            "{why}字詞表鷹架有 {banks} 個字沒有詞性或字義。{MSG_NO_POS}"
        ));
    }
    (!out.is_empty()).then(|| out.join("\n"))
}

/// Fills what the worksheet's tables and word banks still lack with ONE model call (the
/// `worksheet_vocab_gloss` prompt, logged like every other call) and stores nothing itself: the
/// caller saves `content` when `changed`. Nothing missing: no call. It never fails the caller:
/// with AI off, a failed or partial answer, the words concerned keep what they had and `notice`
/// says so.
pub async fn fill(
    state: &AppState,
    teacher_id: i64,
    worksheet_id: i64,
    lesson_id: Option<i64>,
    content: &mut Content,
    lexicon: Option<&Lexicon>,
    scope: Scope,
) -> AppResult<Filled> {
    let mut changed = false;
    if scope.tables {
        changed |= fill_task_words(content);
    }
    if scope.banks {
        changed |= bank_pos_from_list(content, lexicon);
    }
    let table_words = if scope.tables {
        missing_zh(content)
    } else {
        Vec::new()
    };
    let bank_words = if scope.banks {
        missing_bank(content)
    } else {
        Vec::new()
    };
    let mut words = table_words.clone();
    for w in &bank_words {
        if !words.contains(w) {
            words.push(w.clone());
        }
    }
    if words.is_empty() {
        return Ok(Filled {
            changed,
            filled: 0,
            notice: None,
        });
    }
    if !state.ai.enabled() {
        return Ok(Filled {
            changed,
            filled: 0,
            notice: notices(table_words.len(), bank_words.len(), "目前沒有開啟 AI，"),
        });
    }
    words.truncate(super::vocab_actions::MAX_WORDS);
    let detail = words
        .iter()
        .map(|w| line_of(content, w, lexicon))
        .collect::<Vec<_>>()
        .join("\n");
    let target = vocabulary::DEFAULT_TARGET_LEVEL;
    let vars: HashMap<&'static str, String> = HashMap::from([
        ("target_level", target.to_string()),
        ("word_count", words.len().to_string()),
        ("words", detail.clone()),
        ("version_label", "字詞表".to_string()),
        ("round_note", String::new()),
    ]);
    let key = KEY_GLOSS;
    let prompt = prompt(&state.db, key).await?;
    let required = vec![("words", "要處理的字"), ("target_level", "目標級別")];
    let system = prompts::render(&prompt.system_text, &vars);
    let user = prompts::render_user_with(&prompt, &vars, &required);
    // The contexts come from the teacher's material: the logged copy keeps length and a hash.
    let mut logged = vars.clone();
    logged.insert("words", prompts::redacted("學習單上下文", &detail));
    let logged_user = prompts::render_user_with(&prompt, &logged, &required);
    let schema = gloss_schema(&words);
    let max_tokens = 300 + 120 * words.len() as u32;
    let temperature = 0.3_f32;
    let snapshot = json!({
        "inputs": logged,
        "worksheet": { "id": worksheet_id, "purpose": "key_vocabulary", "words": words },
        "prompt": { "key": key, "version_no": prompt.version_no,
                    "system": system, "user": logged_user },
        "schema": SCHEMA_GLOSS,
        "request": { "temperature": temperature, "max_tokens": max_tokens },
    });
    let meta = InteractionMeta {
        teacher_id,
        module: key,
        unit_id: None,
        lesson_id,
        scaffold_id: scaffold_id(&state.db, SCAFFOLD).await?,
        prompt_version_id: Some(prompt.id),
    };
    let fake_context = json!({
        "task": key,
        "target_level": target,
        "words": words.iter().map(|w| json!({ "base": w, "forms": [w], "pos": "" }))
            .collect::<Vec<_>>(),
        "places": [],
    });
    let req = LlmRequest {
        system,
        user,
        schema_name: SCHEMA_GLOSS,
        schema,
        fake_context,
        attempt: 1,
        max_tokens: Some(max_tokens),
        temperature: Some(temperature),
        images: Vec::new(),
    };
    let a = attempt(state, &meta, &snapshot, &req, None).await?;
    // word → (Chinese meaning, part of speech, English meaning)
    let mut got: BTreeMap<String, (String, String, String)> = BTreeMap::new();
    match &a.response {
        Ok(resp) => match serde_json::from_value::<RawGlosses>(resp.json.clone()) {
            Ok(raw) => {
                let mut problems = Vec::new();
                for g in &raw.glosses {
                    let word = g.word.trim().to_lowercase();
                    let one = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
                    let (meaning, definition) = (one(&g.meaning_zh), one(&g.definition_en));
                    if !words.contains(&word) || got.contains_key(&word) {
                        continue;
                    }
                    if !has_han(&meaning) {
                        problems.push(format!("「{word}」的 meaning_zh 要寫中文意思。"));
                    }
                    let definition = if has_han(&definition) {
                        String::new()
                    } else {
                        clip_chars(&definition, GLOSS_DEFINITION_CHARS)
                    };
                    let meaning = if has_han(&meaning) {
                        clip_chars(&meaning, GLOSS_MEANING_CHARS)
                    } else {
                        String::new()
                    };
                    got.insert(word, (meaning, g.pos.clone(), definition));
                }
                let results = if problems.is_empty() {
                    vec![CheckResult::pass("schema"), CheckResult::pass("rules")]
                } else {
                    vec![
                        CheckResult::pass("schema"),
                        CheckResult::fail("rules", problems.join("；")),
                    ]
                };
                let status = if got.is_empty() { "blocked" } else { "shown" };
                finish_output(
                    &state.db,
                    &a,
                    resp,
                    status,
                    base_checks::report(&results),
                    Vec::new(),
                )
                .await?;
            }
            Err(e) => {
                let report = base_checks::report(&[CheckResult::fail("schema", e.to_string())]);
                finish_output(&state.db, &a, resp, "blocked", report, Vec::new()).await?;
            }
        },
        Err(e) => {
            let _: Failure = finish_failure(&state.db, &a, e).await?;
        }
    }
    let mut filled = 0;
    let listed_of = |word: &str| lexicon.map(|l| listed_pos_of(l, word)).unwrap_or_default();
    for v in content
        .blocks
        .iter_mut()
        .flat_map(|b| b.vocabulary.iter_mut())
    {
        let Some((zh, pos, _)) = got.get(&v.word.trim().to_lowercase()) else {
            continue;
        };
        if v.lacks_zh() && !zh.is_empty() && scope.tables {
            v.meaning_zh = zh.clone();
            filled += 1;
            changed = true;
        }
        if v.pos.trim().is_empty() {
            v.pos = resolve_pos(pos, &listed_of(&v.word));
        }
    }
    for w in content
        .blocks
        .iter_mut()
        .flat_map(|b| b.scaffolds.iter_mut())
        .flat_map(|s| s.bank.iter_mut())
        .flat_map(|b| b.words.iter_mut())
    {
        let Some((zh, pos, en)) = got.get(&w.word.trim().to_lowercase()) else {
            continue;
        };
        if !scope.banks {
            continue;
        }
        if w.pos.trim().is_empty() {
            w.pos = resolve_pos(pos, &listed_of(&w.word));
        }
        if w.meaning_zh.trim().is_empty() && !zh.is_empty() {
            w.meaning_zh = zh.clone();
        }
        if w.definition.trim().is_empty() && !en.is_empty() {
            w.definition = en.clone();
        }
        filled += 1;
        changed = true;
    }
    let (still_tables, still_banks) = (
        if scope.tables {
            missing_zh(content).len()
        } else {
            0
        },
        if scope.banks {
            missing_bank(content).len()
        } else {
            0
        },
    );
    let why = if got.is_empty() {
        "Manna 這次沒有寫出字義，"
    } else {
        "這次還有："
    };
    Ok(Filled {
        changed,
        filled,
        notice: notices(still_tables, still_banks, why),
    })
}

/// One word as the gloss prompt is shown it.
fn line_of(content: &Content, word: &str, lexicon: Option<&Lexicon>) -> String {
    let v = content
        .blocks
        .iter()
        .flat_map(|b| b.vocabulary.iter())
        .find(|v| v.word.trim().to_lowercase() == word);
    let level = v.and_then(|v| v.level);
    let listed = lexicon.map(|l| listed_pos_of(l, word)).unwrap_or_default();
    let mut notes: Vec<String> = Vec::new();
    if let Some(l) = level {
        notes.push(format!("第 {l} 級"));
    }
    if !listed.is_empty() {
        notes.push(format!("詞彙表詞性：{listed}"));
    }
    let notes = if notes.is_empty() {
        String::new()
    } else {
        format!("（{}）", notes.join("；"))
    };
    let context = v.map(|v| v.context.trim()).filter(|c| !c.is_empty());
    let english = v.map(|v| v.definition.trim()).filter(|c| !c.is_empty());
    let mut out = format!("- {word}{notes}\n  學習單裡的字形：{word}");
    if let Some(c) = context {
        out.push_str(&format!("\n  上下文：「{}」", clip_chars(c, 100)));
    }
    if let Some(e) = english {
        out.push_str(&format!("\n  英文解釋：{}", clip_chars(e, 100)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parts_of_speech_are_normalised() {
        assert_eq!(normalise_pos("noun"), Some("n."));
        assert_eq!(normalise_pos(" Adj. "), Some("adj."));
        assert_eq!(normalise_pos("v"), Some("v."));
        assert_eq!(normalise_pos("n./v."), Some("n."));
        assert_eq!(normalise_pos("(art.)"), Some("det."));
        assert_eq!(normalise_pos("phrasal verb"), Some("v."));
        assert_eq!(normalise_pos("whatever"), None);
        assert_eq!(normalise_pos(""), None);
        for p in POS_SET {
            assert_eq!(normalise_pos(p), Some(p));
        }
    }

    #[test]
    fn the_word_list_decides_between_parts_of_speech() {
        // The model's, when the list gives it (or gives none).
        assert_eq!(resolve_pos("noun", "n./v."), "n.");
        assert_eq!(resolve_pos("v.", "n./v."), "v.");
        assert_eq!(resolve_pos("adj.", ""), "adj.");
        // Not one the list gives: the list's when it gives exactly one …
        assert_eq!(resolve_pos("adj.", "n."), "n.");
        assert_eq!(resolve_pos("", "v."), "v.");
        assert_eq!(resolve_pos("nonsense", "adv."), "adv.");
        // … otherwise the model's normalised value.
        assert_eq!(resolve_pos("adj.", "n./v."), "adj.");
        assert_eq!(resolve_pos("nonsense", "n./v."), "");
        assert_eq!(resolve_pos("", ""), "");
    }
}
