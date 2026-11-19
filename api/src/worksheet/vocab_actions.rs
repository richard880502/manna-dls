//! 詞彙級別檢查：處理超出級別的字 (docs/API.md「詞彙級別檢查：處理超出級別的字」).
//!
//! For each word above the target level the teacher chooses 提供字義 (keep the word, add its
//! meaning to the block's Glossary), 換字 or 不處理. Meanings come from one bounded model call
//! for all the chosen words (the 參考詞彙表 has levels and parts of speech but no meanings).
//!
//! 換字 works on sentences, not on word forms: swapping a form in place gave ungrammatical text
//! (「according to the text」→「as said to the text」). Every sentence 換字 may change is sent
//! whole, and the model writes it again with as small a change as it can; the server keeps a
//! rewrite only when every word it adds is within the target level, the word is gone, and the
//! change stays near the word ([`check_rewrite`]). The teacher sees each sentence before → after,
//! can untick any, and applying writes exactly what was shown, as one new worksheet version
//! (`cause = "vocabulary"`) through the same version-and-history path as the other edits, so
//! 「復原上一版」 works per block.
//!
//! What 換字 never changes: the reading text itself (it is not part of the worksheet), the key
//! words the lesson teaches (字詞表), word banks (they are the answers), a simplified text
//! (調整文本 is the reading text rewritten), the title and the objectives, picture captions,
//! and a word in quotation marks (it names the word or quotes the text).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::assemble::{block_name, project, text_units, Sheet};
use super::store::{self, Worksheet};
use super::vocab::{self, OverLevelWord, VocabCheck, ACTION_GLOSS, ACTION_LEAVE, ACTION_REPLACE};
use super::{
    Audience, Block, Content, Gloss, Level, Origin, ScaffoldType, GLOSS_DEFINITION_CHARS,
    GLOSS_MEANING_CHARS, GLOSS_POS_CHARS,
};
use crate::ai::pipeline::InteractionMeta;
use crate::ai::LlmRequest;
use crate::checks::{self as base_checks, CheckResult};
use crate::error::{AppError, AppResult};
use crate::prompts;
use crate::routes::{clip_chars, log_usage, scaffold_id};
use crate::sop::ai::{attempt, finish_failure, finish_output, prompt, Failure};
use crate::vocabulary::{self, Lexicon};
use crate::worksheet::template::{enum_of, obj, s_max};
use crate::AppState;

pub const KEY_GLOSS: &str = "worksheet_vocab_gloss";
pub const KEY_REPLACE: &str = "worksheet_vocab_replace";
pub const SCAFFOLD: &str = "SC-WS-VOCAB";
pub const SCHEMA_GLOSS: &str = "worksheet_vocab_gloss";
pub const SCHEMA_REPLACE: &str = "worksheet_vocab_replace";

/// Most words in one suggestion request (one model call).
pub const MAX_WORDS: usize = 20;
/// Most sentences one 換字 request rewrites (one model call).
pub const MAX_PLACES: usize = 40;
/// Words a replacement hint may have (`give up`).
pub const MAX_REPLACEMENT_WORDS: usize = 3;
/// A rewrite may change at most this many words of the original per occurrence of a word to
/// avoid (「according to」 is two)…
pub const CHANGED_PER_OCCURRENCE: usize = 2;
/// …and add at most this many per occurrence (plus one).
pub const ADDED_PER_OCCURRENCE: usize = 3;
/// A change must start or end within this many tokens of a word to avoid.
pub const CHANGE_WINDOW: usize = 3;
const WORD_CHARS: usize = 40;
const MODEL_MEANING_CHARS: usize = 30;
const MODEL_DEFINITION_CHARS: usize = 120;
const MODEL_POS_CHARS: usize = 12;
const BASE_TEMPERATURE: f32 = 0.3;
const REPAIR_TEMPERATURE_STEP: f32 = 0.4;

pub const MSG_STALE: &str = "學習單在這次分析之後改過了，請重新分析，再處理超出級別的字。";
pub const MSG_RUNNING: &str = "Manna 正在產生這份學習單，好了之後再處理。";

/// The Glossary entries go to this version only, or to every version that prints the word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GlossScope {
    #[default]
    Level,
    All,
}

// ---------------------------------------------------------------- what the model writes

#[derive(Debug, Clone, Deserialize)]
pub struct RawGlosses {
    pub glosses: Vec<RawGloss>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawGloss {
    pub word: String,
    #[serde(default)]
    pub pos: String,
    #[serde(default)]
    pub meaning_zh: String,
    #[serde(default)]
    pub definition_en: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawReplace {
    #[serde(default)]
    pub words: Vec<RawWordHint>,
    pub sentences: Vec<RawSentence>,
}

/// The word the model mostly uses instead (shown to the teacher as a hint only).
#[derive(Debug, Clone, Deserialize)]
pub struct RawWordHint {
    pub word: String,
    #[serde(default)]
    pub replacement: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawSentence {
    pub id: String,
    #[serde(default)]
    pub rewritten: String,
}

/// `worksheet_vocab_gloss`: one entry per word, `word` from the words asked about, then `pos`,
/// `meaning_zh` (one line) and `definition_en`; every string bounded.
pub fn gloss_schema(bases: &[String]) -> Value {
    let words: Vec<&str> = bases.iter().map(String::as_str).collect();
    obj(vec![(
        "glosses",
        json!({
            "type": "array",
            "items": obj(vec![
                ("word", enum_of(&words)),
                ("pos", s_max(MODEL_POS_CHARS)),
                ("meaning_zh", s_max(MODEL_MEANING_CHARS)),
                ("definition_en", s_max(MODEL_DEFINITION_CHARS)),
            ]),
            "minItems": words.len(),
            "maxItems": words.len(),
        }),
    )])
}

/// The longest rewrite of a sentence the schema allows.
fn rewrite_chars(original: &str) -> usize {
    (original.chars().count() + 60).min(800)
}

/// `worksheet_vocab_replace`: first one entry per word (`word` from the words, the
/// `replacement` it mostly uses), then one entry per sentence (`id` from the sentences, the
/// sentence written again); every string bounded.
pub fn replace_schema(bases: &[String], places: &[Place]) -> Value {
    let words: Vec<&str> = bases.iter().map(String::as_str).collect();
    let ids: Vec<&str> = places.iter().map(|p| p.id.as_str()).collect();
    let longest = places
        .iter()
        .map(|p| rewrite_chars(&p.original))
        .max()
        .unwrap_or(200);
    obj(vec![
        (
            "words",
            json!({
                "type": "array",
                "items": obj(vec![
                    ("word", enum_of(&words)),
                    ("replacement", s_max(WORD_CHARS)),
                ]),
                "minItems": words.len(),
                "maxItems": words.len(),
            }),
        ),
        (
            "sentences",
            json!({
                "type": "array",
                "items": obj(vec![
                    ("id", enum_of(&ids)),
                    ("rewritten", s_max(longest)),
                ]),
                "minItems": ids.len(),
                "maxItems": ids.len(),
            }),
        ),
    ])
}

// ---------------------------------------------------------------- what the teacher gets

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GlossSuggestion {
    pub pos: String,
    pub meaning_zh: String,
    pub definition_en: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WordSuggestion {
    pub base: String,
    /// The call that wrote it (for `teacher_decisions`); `null` when none did.
    pub interaction_id: Option<i64>,
    /// 提供字義
    pub gloss: Option<GlossSuggestion>,
    /// 換字: the word the rewrites mostly use (a hint; checked to be within the level).
    pub replacement: Option<String>,
    /// 換字: the sentences ([`Suggestions::places`]) that have this word.
    pub places: Vec<String>,
    /// What the code set aside, in the teacher's words.
    pub notes: Vec<String>,
}

/// One sentence 換字 rewrites, before → after.
#[derive(Debug, Clone, Serialize)]
pub struct PlaceSuggestion {
    #[serde(flatten)]
    pub place: Place,
    /// The sentence written again; `null` when no rewrite passed the checks.
    pub rewritten: Option<String>,
    /// The call that wrote it.
    pub interaction_id: Option<i64>,
    /// Why there is none.
    pub problem: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Suggestions {
    pub action: String,
    /// `shown` (at least one word has a suggestion) | `blocked` | `timeout` | `error` | `disabled`
    pub status: &'static str,
    pub message: Option<String>,
    /// The last call made.
    pub interaction_id: Option<i64>,
    /// In the order asked.
    pub words: Vec<WordSuggestion>,
    /// 換字 only: every sentence, in worksheet order.
    pub places: Vec<PlaceSuggestion>,
}

// ---------------------------------------------------------------- checks (pure)

fn has_han(s: &str) -> bool {
    s.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

const POS_ATOMS: [&str; 12] = [
    "n.", "v.", "adj.", "adv.", "prep.", "conj.", "pron.", "art.", "aux.", "int.", "det.", "phr.",
];

/// The part of speech to print: the model's when it is one of the usual ones and the list does
/// not say otherwise, else the list's (when it lists only one).
fn clean_pos(model: &str, listed: &str) -> String {
    let m = model.trim().to_lowercase();
    let m = if m.is_empty() || m.ends_with('.') {
        m
    } else {
        format!("{m}.")
    };
    let listed_atoms: Vec<String> = listed
        .split('/')
        .map(|p| {
            p.trim_matches(|c| c == '(' || c == ')' || c == ' ')
                .to_string()
        })
        .filter(|p| !p.is_empty())
        .collect();
    let usual = POS_ATOMS.contains(&m.as_str());
    if usual && (listed_atoms.is_empty() || listed_atoms.contains(&m)) {
        m
    } else if listed_atoms.len() == 1 {
        listed_atoms[0].clone()
    } else {
        String::new()
    }
}

/// The words of `text` above `target`, as the analysis finds them.
fn words_above(text: &str, lex: &Lexicon, target: u8) -> Vec<String> {
    vocabulary::analyze(text, lex, target, &[])
        .above_target
        .into_iter()
        .map(|a| a.base)
        .collect()
}

/// What a second call should fix.
type Problems = Vec<String>;

/// Meanings that can be shown, by word, and what a second call should fix. A meaning must be
/// Chinese; an English definition that uses a word above the target level (often the word
/// itself) is dropped, not sent back.
pub fn evaluate_glosses(
    raw: &RawGlosses,
    words: &[OverLevelWord],
    lex: &Lexicon,
    target: u8,
) -> (BTreeMap<String, (GlossSuggestion, Problems)>, Problems) {
    let mut out: BTreeMap<String, (GlossSuggestion, Vec<String>)> = BTreeMap::new();
    let mut problems = Vec::new();
    for g in &raw.glosses {
        let base = g.word.trim().to_lowercase();
        let Some(word) = words.iter().find(|w| w.base == base) else {
            problems.push(format!("「{}」不是要解釋的字。", clip_chars(&base, 30)));
            continue;
        };
        if out.contains_key(&base) {
            continue;
        }
        let meaning = one_line(&g.meaning_zh);
        if !has_han(&meaning) {
            problems.push(format!("「{base}」的 meaning_zh 要寫中文意思。"));
            continue;
        }
        let meaning = clip_chars(&meaning, GLOSS_MEANING_CHARS);
        let mut notes = Vec::new();
        let mut definition = one_line(&g.definition_en);
        if has_han(&definition) {
            definition.clear();
        } else if !definition.is_empty() {
            let above = words_above(&definition, lex, target);
            if !above.is_empty() {
                notes.push(format!(
                    "英文解釋用了超出級別的字（{}），所以沒有附上。",
                    above.join("、")
                ));
                definition.clear();
            }
        }
        out.insert(
            base,
            (
                GlossSuggestion {
                    pos: clip_chars(&clean_pos(&g.pos, &word.pos), GLOSS_POS_CHARS),
                    meaning_zh: meaning,
                    definition_en: clip_chars(&definition, GLOSS_DEFINITION_CHARS),
                },
                notes,
            ),
        );
    }
    for w in words {
        if !out.contains_key(&w.base) {
            problems.push(format!("沒有給「{}」的意思。", w.base));
        }
    }
    (out, problems)
}

/// The highest word-list level of a replacement's words, or why it cannot be used: every word
/// must be a listed word (or a form of one) within `target`.
pub fn replacement_level(lex: &Lexicon, text: &str, target: u8) -> Result<u8, String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return Err("沒有寫要換成什麼字。".to_string());
    }
    if words.len() > MAX_REPLACEMENT_WORDS {
        return Err(format!("「{}」太長了。", clip_chars(text, 40)));
    }
    let mut highest = 1;
    for w in words {
        if !w
            .chars()
            .all(|c| c.is_ascii_alphabetic() || c == '\'' || c == '-')
        {
            return Err(format!("「{}」不是英文單字。", clip_chars(w, 40)));
        }
        let parts: Vec<&str> = w.split('-').filter(|p| !p.is_empty()).collect();
        for p in parts {
            match vocabulary::word_level(lex, p).level {
                Some(l) if l <= target => highest = highest.max(l),
                Some(l) => {
                    return Err(format!("「{p}」是第 {l} 級，超出第 {target} 級。"));
                }
                None => return Err(format!("「{p}」不在參考詞彙表裡，無法確認級別。")),
            }
        }
    }
    Ok(highest)
}

// ---------------------------------------------------------------- the texts 換字 may change

/// Which text of a block a sentence is in.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FieldPart {
    Title,
    Instructions,
    /// A step of a procedural scaffold (`id` = scaffold, `index` = step).
    Step,
    /// A verbal or instructional scaffold's text (`id` = scaffold).
    Support,
    /// A question (`id` = item).
    Prompt,
    /// A choice (`id` = item, `index`).
    Option,
    /// A 表格整理 column heading (`id` = item, `index`).
    TableHeading,
    /// A 圖像組織 question's organizer title (`id` = item).
    OrganizerTitle,
    /// A 圖像組織 section label: a column heading, row label or box name (`id` = item, `index` =
    /// section). The centre word and the cells (pre-filled text, answers) are never changed.
    OrganizerLabel,
    /// 基礎版 hint (`id` = item).
    Hint,
    /// A line of this version's support under a question (`id` = item, `index`).
    TierSupport,
}

/// Where a text 換字 may change is in the worksheet content.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FieldKey {
    pub block: String,
    pub part: FieldPart,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,
}

fn changeable_support(kind_type: ScaffoldType, kind: &str) -> bool {
    kind_type != ScaffoldType::Procedural
        && kind != super::assemble::KIND_SIMPLIFIED
        && kind != super::KIND_WORD_LIST
}

/// The text at `key` when `level` prints it and 換字 may change it.
pub fn field_mut<'a>(
    content: &'a mut Content,
    level: Level,
    key: &FieldKey,
) -> Option<&'a mut String> {
    let b = content
        .blocks
        .iter_mut()
        .find(|b| b.enabled && b.id == key.block)?;
    let id = key.id.as_deref();
    match key.part {
        FieldPart::Title => Some(&mut b.title),
        FieldPart::Instructions => Some(&mut b.instructions),
        FieldPart::Step | FieldPart::Support => {
            let s = b
                .scaffolds
                .iter_mut()
                .find(|s| Some(s.id.as_str()) == id && s.levels.contains(&level))?;
            if key.part == FieldPart::Step {
                if s.kind_type != ScaffoldType::Procedural {
                    return None;
                }
                s.steps.get_mut(key.index?)
            } else if changeable_support(s.kind_type, &s.kind) {
                Some(&mut s.content)
            } else {
                None
            }
        }
        _ => {
            let it = b
                .items
                .iter_mut()
                .find(|i| Some(i.id.as_str()) == id && i.levels.contains(&level))?;
            match key.part {
                FieldPart::Prompt => Some(&mut it.prompt),
                FieldPart::Option => it.options.get_mut(key.index?),
                FieldPart::TableHeading => it.table.as_mut()?.columns.get_mut(key.index?),
                // Only while the question prints its organizer (its format is 圖像組織).
                FieldPart::OrganizerTitle | FieldPart::OrganizerLabel => {
                    it.printed_organizer()?;
                    let org = it.organizer.as_mut()?;
                    if key.part == FieldPart::OrganizerTitle {
                        Some(&mut org.title)
                    } else {
                        org.sections.get_mut(key.index?).map(|s| &mut s.label)
                    }
                }
                FieldPart::Hint if level == Level::Basic => Some(&mut it.hint),
                FieldPart::TierSupport => it
                    .tiers
                    .iter_mut()
                    .find(|t| t.level == level)?
                    .support
                    .get_mut(key.index?),
                _ => None,
            }
        }
    }
}

/// Every text `level` prints that 換字 may change, with where it is: block titles and
/// instructions, steps, supports (not a simplified text or a word bank), and this version's
/// questions, options, table headings, hints and support lines (not word banks). Shared texts
/// change in every version that prints them.
pub fn changeable_fields(content: &Content, level: Level) -> Vec<(FieldKey, String)> {
    let mut out = Vec::new();
    for b in content.blocks.iter().filter(|b| b.enabled) {
        let key = |part, id: Option<&str>, index| FieldKey {
            block: b.id.clone(),
            part,
            id: id.map(str::to_string),
            index,
        };
        out.push((key(FieldPart::Title, None, None), b.title.clone()));
        out.push((
            key(FieldPart::Instructions, None, None),
            b.instructions.clone(),
        ));
        for s in b.scaffolds.iter().filter(|s| s.levels.contains(&level)) {
            if s.kind_type == ScaffoldType::Procedural {
                for (i, step) in s.steps.iter().enumerate() {
                    out.push((key(FieldPart::Step, Some(&s.id), Some(i)), step.clone()));
                }
            } else if changeable_support(s.kind_type, &s.kind) {
                out.push((
                    key(FieldPart::Support, Some(&s.id), None),
                    s.content.clone(),
                ));
            }
        }
        for it in b.items.iter().filter(|i| i.levels.contains(&level)) {
            let id = Some(it.id.as_str());
            out.push((key(FieldPart::Prompt, id, None), it.prompt.clone()));
            for (i, o) in it.options.iter().enumerate() {
                out.push((key(FieldPart::Option, id, Some(i)), o.clone()));
            }
            for (i, c) in it.table.iter().flat_map(|t| t.columns.iter()).enumerate() {
                out.push((key(FieldPart::TableHeading, id, Some(i)), c.clone()));
            }
            if let Some(org) = it.printed_organizer() {
                out.push((key(FieldPart::OrganizerTitle, id, None), org.title.clone()));
                for (i, s) in org.sections.iter().enumerate() {
                    out.push((key(FieldPart::OrganizerLabel, id, Some(i)), s.label.clone()));
                }
            }
            if level == Level::Basic {
                out.push((key(FieldPart::Hint, id, None), it.hint.clone()));
            }
            for t in it.tiers.iter().filter(|t| t.level == level) {
                for (i, line) in t.support.iter().enumerate() {
                    out.push((key(FieldPart::TierSupport, id, Some(i)), line.clone()));
                }
            }
        }
    }
    out
}

/// Where a text is, in the teacher's words: 「A 活動 · 第 4 題」.
fn label_of(sheet: &Sheet, content: &Content, key: &FieldKey) -> String {
    let Some(sb) = sheet.blocks.iter().find(|b| b.id == key.block) else {
        return String::new();
    };
    let name = block_name(sb);
    let no = || {
        sb.items
            .iter()
            .find(|i| Some(i.id.as_str()) == key.id.as_deref())
            .map(|i| i.no)
            .unwrap_or_default()
    };
    let n = key.index.unwrap_or_default() + 1;
    let what = match key.part {
        FieldPart::Title => "標題".to_string(),
        FieldPart::Instructions => "作答指示".to_string(),
        FieldPart::Step => format!("步驟 {n}"),
        FieldPart::Support => content
            .block(&key.block)
            .and_then(|b| {
                b.scaffolds
                    .iter()
                    .find(|s| Some(s.id.as_str()) == key.id.as_deref())
            })
            .map(|s| s.kind.clone())
            .unwrap_or_else(|| "鷹架".to_string()),
        FieldPart::Prompt => format!("第 {} 題", no()),
        FieldPart::Option => format!("第 {} 題選項 {n}", no()),
        FieldPart::TableHeading => format!("第 {} 題表格欄位 {n}", no()),
        FieldPart::OrganizerTitle => format!("第 {} 題圖像組織標題", no()),
        FieldPart::OrganizerLabel => format!("第 {} 題圖像組織標籤 {n}", no()),
        FieldPart::Hint => format!("第 {} 題提示", no()),
        FieldPart::TierSupport => format!("第 {} 題支持", no()),
    };
    format!("{name} · {what}")
}

/// One sentence 換字 rewrites: where it is, the sentence (`start..end`, in characters of the
/// text), and the words to avoid in it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Place {
    /// `p1`, `p2` … in worksheet order (the model's ids).
    pub id: String,
    pub field: FieldKey,
    /// 「A 活動 · 第 1 題」
    pub place: String,
    pub start: usize,
    pub end: usize,
    pub original: String,
    /// The chosen words it has (base forms).
    pub bases: Vec<String>,
}

fn is_sentence_end(chars: &[char], i: usize) -> bool {
    matches!(chars[i], '.' | '!' | '?')
        && chars
            .get(i + 1)
            .is_none_or(|c| c.is_whitespace() || matches!(c, '"' | '”' | '’' | ')' | '\''))
}

/// The sentence around `start..end` (character offsets): from after the previous sentence end
/// or line break to its own end mark (with a closing quote or bracket), or the line's end.
pub fn sentence_span(chars: &[char], start: usize, end: usize) -> (usize, usize) {
    let mut s = start;
    while s > 0 && chars[s - 1] != '\n' && !is_sentence_end(chars, s - 1) {
        s -= 1;
    }
    while s < start && chars[s].is_whitespace() {
        s += 1;
    }
    let mut e = end;
    while e < chars.len() && chars[e] != '\n' {
        if is_sentence_end(chars, e) {
            e += 1;
            while e < chars.len() && matches!(chars[e], '"' | '”' | '’' | ')' | '\'') {
                e += 1;
            }
            break;
        }
        e += 1;
    }
    while e > end && chars[e - 1].is_whitespace() {
        e -= 1;
    }
    (s, e)
}

/// Every sentence `level` prints that has one of `bases` where 換字 may change it (not quoted).
/// A sentence with two of the words is one place.
pub fn collect_places(
    title: &str,
    content: &Content,
    level: Level,
    lex: &Lexicon,
    target: u8,
    bases: &[String],
) -> Vec<Place> {
    let sheet = project(title, content, level, Audience::Student);
    let mut out: Vec<Place> = Vec::new();
    for (key, text) in changeable_fields(content, level) {
        if text.trim().is_empty() {
            continue;
        }
        let analysis = vocabulary::analyze(&text, lex, target, &[]);
        let chars: Vec<char> = text.chars().collect();
        let mut line_starts = vec![0usize];
        line_starts.extend(
            chars
                .iter()
                .enumerate()
                .filter(|(_, c)| **c == '\n')
                .map(|(i, _)| i + 1),
        );
        let mut spans: Vec<(usize, usize, Vec<String>)> = Vec::new();
        for base in bases {
            let Some(entry) = analysis.above_target.iter().find(|a| &a.base == base) else {
                continue;
            };
            for p in &entry.positions {
                let Some(line_start) = line_starts.get(p.line.saturating_sub(1)) else {
                    continue;
                };
                let start = line_start + p.col.saturating_sub(1);
                let end = start + p.form.chars().count();
                if end > chars.len() || vocab::quoted(&chars, start, end) {
                    continue;
                }
                let (s, e) = sentence_span(&chars, start, end);
                match spans.iter_mut().find(|x| x.0 == s && x.1 == e) {
                    Some(x) if !x.2.contains(base) => x.2.push(base.clone()),
                    Some(_) => {}
                    None => spans.push((s, e, vec![base.clone()])),
                }
            }
        }
        spans.sort_by_key(|x| x.0);
        for (start, end, found) in spans {
            out.push(Place {
                id: format!("p{}", out.len() + 1),
                field: key.clone(),
                place: label_of(&sheet, content, &key),
                start,
                end,
                original: chars[start..end].iter().collect(),
                bases: found,
            });
        }
    }
    out
}

// ---------------------------------------------------------------- checking a rewrite (pure)

/// Lower-cased words (letters, digits, inner apostrophes and hyphens) and single punctuation
/// marks: what the change of a rewrite is measured in.
fn diff_tokens(s: &str) -> Vec<String> {
    let chars: Vec<char> = s.chars().map(|c| if c == '’' { '\'' } else { c }).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_alphanumeric() {
            let start = i;
            while i < chars.len()
                && (chars[i].is_alphanumeric()
                    || (matches!(chars[i], '\'' | '-')
                        && chars.get(i + 1).is_some_and(|n| n.is_alphanumeric())))
            {
                i += 1;
            }
            out.push(chars[start..i].iter().collect::<String>().to_lowercase());
        } else {
            out.push(c.to_string());
            i += 1;
        }
    }
    out
}

fn is_word(t: &str) -> bool {
    t.chars().next().is_some_and(char::is_alphanumeric)
}

/// One run of change between two token lists: original tokens `from..to` became `added` new
/// ones (words only are counted).
struct Hunk {
    from: usize,
    to: usize,
    removed_words: usize,
    added_words: usize,
}

fn hunks(a: &[String], b: &[String]) -> Vec<Hunk> {
    let (n, m) = (a.len(), b.len());
    let mut dp = vec![vec![0u16; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if a[i] == b[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    let mut out: Vec<Hunk> = Vec::new();
    let mut open: Option<Hunk> = None;
    let (mut i, mut j) = (0, 0);
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            if let Some(h) = open.take() {
                out.push(h);
            }
            i += 1;
            j += 1;
            continue;
        }
        let h = open.get_or_insert(Hunk {
            from: i,
            to: i,
            removed_words: 0,
            added_words: 0,
        });
        if j >= m || (i < n && dp[i + 1][j] >= dp[i][j + 1]) {
            h.removed_words += usize::from(is_word(&a[i]));
            i += 1;
            h.to = i;
        } else {
            h.added_words += usize::from(is_word(&b[j]));
            j += 1;
        }
    }
    out.extend(open);
    out
}

/// A rewritten sentence the worksheet may print, or why not: one line of English, every word it
/// adds a listed word within `target` (words the sentence already had are fine), none of `bases`
/// left (except quoted), and the change local — at most [`CHANGED_PER_OCCURRENCE`] words
/// changed and [`ADDED_PER_OCCURRENCE`] (+1) added per occurrence, every change within
/// [`CHANGE_WINDOW`] tokens of an occurrence.
pub fn check_rewrite(
    lex: &Lexicon,
    target: u8,
    original: &str,
    rewritten: &str,
    bases: &[String],
) -> Result<String, String> {
    let new = one_line(rewritten);
    if new.is_empty() {
        return Err("沒有寫出改寫的句子。".to_string());
    }
    if has_han(&new) {
        return Err("改寫的句子有中文。".to_string());
    }
    if new == one_line(original) {
        return Err("句子沒有改。".to_string());
    }
    let before = vocabulary::analyze(original, lex, target, &[]);
    let after = vocabulary::analyze(&new, lex, target, &[]);
    let chars: Vec<char> = new.chars().collect();
    for base in bases {
        if let Some(e) = after.above_target.iter().find(|a| &a.base == base) {
            let unquoted = e.positions.iter().any(|p| {
                let s = p.col.saturating_sub(1);
                !vocab::quoted(&chars, s, s + p.form.chars().count())
            });
            if unquoted {
                return Err(format!("「{base}」還在句子裡。"));
            }
        }
    }
    let had: HashSet<&str> = before
        .above_target
        .iter()
        .map(|a| a.base.as_str())
        .collect();
    if let Some(a) = after
        .above_target
        .iter()
        .find(|a| !had.contains(a.base.as_str()))
    {
        let form = a.positions.first().map_or(a.base.as_str(), |p| &p.form);
        return Err(format!(
            "用了「{form}」（第 {} 級），超出第 {target} 級。",
            a.level
        ));
    }
    let old_tokens = diff_tokens(original);
    let old_words: HashSet<&str> = old_tokens.iter().map(String::as_str).collect();
    let is_new = |w: &str| !old_words.contains(w.to_lowercase().replace('’', "'").as_str());
    if let Some(l) = after
        .unknown
        .iter()
        .chain(&after.proper_nouns)
        .find(|l| is_new(&l.word))
    {
        return Err(format!("「{}」不在參考詞彙表裡，無法確認級別。", l.word));
    }
    for l in after.multiword.iter().filter(|l| is_new(&l.word)) {
        let ok = l.occurrences.iter().all(|o| {
            o.component_levels
                .as_ref()
                .is_some_and(|parts| parts.iter().all(|c| c.level.is_some_and(|v| v <= target)))
        });
        if !ok {
            return Err(format!("「{}」無法確認級別。", l.word));
        }
    }
    let new_tokens = diff_tokens(&new);
    if old_tokens.len() > 300 || new_tokens.len() > 300 {
        return Err("句子太長了。".to_string());
    }
    let occurrences: Vec<usize> = old_tokens
        .iter()
        .enumerate()
        .filter(|(_, t)| {
            is_word(t)
                && bases.iter().any(|b| {
                    *t == b || vocabulary::word_level(lex, t).base.as_deref() == Some(b.as_str())
                })
        })
        .map(|(i, _)| i)
        .collect();
    if occurrences.is_empty() {
        return Err("原句裡找不到要換的字。".to_string());
    }
    let k = occurrences.len();
    let changes = hunks(&old_tokens, &new_tokens);
    let removed: usize = changes.iter().map(|h| h.removed_words).sum();
    let added: usize = changes.iter().map(|h| h.added_words).sum();
    if removed > CHANGED_PER_OCCURRENCE * k || added > ADDED_PER_OCCURRENCE * k + 1 {
        return Err("改動太多：只能改這個字和它旁邊的幾個字，句子其他部分要保持原樣。".to_string());
    }
    let near = |h: &Hunk| {
        occurrences
            .iter()
            .any(|&o| o + CHANGE_WINDOW >= h.from && o < h.to + CHANGE_WINDOW)
    };
    if !changes.iter().all(near) {
        return Err("改到了離這個字較遠的地方。".to_string());
    }
    Ok(new)
}

/// What one replace call gave: rewrites that passed, by place id, why the others did not, the
/// hint word of each word, and what a second call should fix.
#[derive(Debug, Default)]
pub struct Rewrites {
    pub sentences: BTreeMap<String, String>,
    pub failed: BTreeMap<String, String>,
    pub hints: BTreeMap<String, String>,
    pub notes: BTreeMap<String, Vec<String>>,
    pub problems: Vec<String>,
}

pub fn evaluate_rewrites(
    raw: &RawReplace,
    bases: &[String],
    places: &[Place],
    lex: &Lexicon,
    target: u8,
) -> Rewrites {
    let mut out = Rewrites::default();
    for w in &raw.words {
        let base = w.word.trim().to_lowercase();
        if !bases.contains(&base) || out.hints.contains_key(&base) {
            continue;
        }
        let hint = one_line(&w.replacement);
        if hint.is_empty() {
            continue;
        }
        match replacement_level(lex, &hint, target) {
            Ok(_) if !hint.eq_ignore_ascii_case(&base) => {
                out.hints.insert(base, hint);
            }
            Ok(_) => {}
            Err(why) => out.notes.entry(base).or_default().push(why),
        }
    }
    for s in &raw.sentences {
        let id = s.id.trim();
        let Some(place) = places.iter().find(|p| p.id == id) else {
            out.problems
                .push(format!("「{}」不是要改寫的句子。", clip_chars(id, 20)));
            continue;
        };
        if out.sentences.contains_key(id) {
            continue;
        }
        match check_rewrite(lex, target, &place.original, &s.rewritten, &place.bases) {
            Ok(new) => {
                out.failed.remove(id);
                out.sentences.insert(id.to_string(), new);
            }
            Err(why) => {
                out.problems.push(format!(
                    "{id}（{}）：{why}",
                    clip_chars(&one_line(&s.rewritten), 120)
                ));
                out.failed.insert(id.to_string(), why);
            }
        }
    }
    for p in places {
        if !out.sentences.contains_key(&p.id) && !out.failed.contains_key(&p.id) {
            out.problems.push(format!("沒有改寫 {}。", p.id));
            out.failed
                .insert(p.id.clone(), "這次沒有寫出改寫的句子。".to_string());
        }
    }
    out
}

// ---------------------------------------------------------------- glosses in content (pure)

/// Whether the sheet prints `base` anywhere (any level of the list counts).
fn prints(sheet: &Sheet, lex: &Lexicon, base: &str) -> bool {
    let text = text_units(sheet)
        .into_iter()
        .map(|u| u.text)
        .collect::<Vec<_>>()
        .join("\n");
    !text.trim().is_empty() && words_above(&text, lex, 0).iter().any(|b| b == base)
}

/// The block whose Glossary explains `base` in `level`: the first block that prints it, or the
/// first printed block when only the title or the objectives have it; `None` when the version
/// does not print it.
pub fn gloss_block(
    title: &str,
    content: &Content,
    level: Level,
    lex: &Lexicon,
    base: &str,
) -> Option<String> {
    let sheet = project(title, content, level, Audience::Student);
    if !prints(&sheet, lex, base) {
        return None;
    }
    let one = |b: &super::assemble::SheetBlock| Sheet {
        title: String::new(),
        objectives: Vec::new(),
        blocks: vec![b.clone()],
        ..sheet.clone()
    };
    sheet
        .blocks
        .iter()
        .find(|b| prints(&one(b), lex, base))
        .or(sheet.blocks.first())
        .map(|b| b.id.clone())
}

/// Puts `gloss` in the Glossary of the block that should explain `base` in each of `levels`
/// (and takes those levels off any other entry for the word). Returns the blocks changed.
pub fn add_gloss(
    title: &str,
    content: &mut Content,
    levels: &[Level],
    lex: &Lexicon,
    base: &str,
    gloss: &Gloss,
) -> BTreeSet<String> {
    let mut changed = BTreeSet::new();
    for &level in levels {
        let Some(target) = gloss_block(title, content, level, lex, base) else {
            continue;
        };
        for b in &mut content.blocks {
            let before = b.glosses.clone();
            if b.id == target {
                match b
                    .glosses
                    .iter_mut()
                    .find(|g| g.word.eq_ignore_ascii_case(base))
                {
                    Some(g) => {
                        g.pos = gloss.pos.clone();
                        g.meaning_zh = gloss.meaning_zh.clone();
                        g.definition_en = gloss.definition_en.clone();
                        g.origin = gloss.origin;
                        if !g.levels.contains(&level) {
                            g.levels.push(level);
                        }
                    }
                    None => b.glosses.push(Gloss {
                        levels: vec![level],
                        ..gloss.clone()
                    }),
                }
            } else {
                for g in b
                    .glosses
                    .iter_mut()
                    .filter(|g| g.word.eq_ignore_ascii_case(base))
                {
                    g.levels.retain(|l| *l != level);
                }
                b.glosses.retain(|g| !g.levels.is_empty());
            }
            if b.glosses != before {
                changed.insert(b.id.clone());
            }
        }
    }
    changed
}

/// After 換字: a Glossary entry for `base` stays only in the versions that still print it.
pub fn prune_glosses(
    title: &str,
    content: &mut Content,
    lex: &Lexicon,
    base: &str,
) -> BTreeSet<String> {
    let still: Vec<Level> = content
        .levels
        .clone()
        .into_iter()
        .filter(|l| prints(&project(title, content, *l, Audience::Student), lex, base))
        .collect();
    let mut changed = BTreeSet::new();
    for b in &mut content.blocks {
        let before = b.glosses.clone();
        for g in b
            .glosses
            .iter_mut()
            .filter(|g| g.word.eq_ignore_ascii_case(base))
        {
            g.levels.retain(|l| still.contains(l));
        }
        b.glosses.retain(|g| !g.levels.is_empty());
        if b.glosses != before {
            changed.insert(b.id.clone());
        }
    }
    changed
}

// ---------------------------------------------------------------- the model call

fn words_detail(words: &[&OverLevelWord]) -> String {
    words
        .iter()
        .map(|w| {
            let pos = if w.pos.is_empty() {
                String::new()
            } else {
                format!("；詞彙表詞性：{}", w.pos)
            };
            let contexts = w
                .contexts
                .iter()
                .map(|c| format!("「{}」", clip_chars(c, 100)))
                .collect::<Vec<_>>()
                .join("、");
            format!(
                "- {}（第 {} 級{pos}）\n  學習單裡的字形：{}\n  上下文：{contexts}",
                w.base,
                w.level,
                w.forms.join("、"),
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// How many times this worksheet has asked for this kind of suggestion (the gateway caches
/// identical requests, so a request asked again must differ).
async fn round(state: &AppState, teacher_id: i64, key: &str, worksheet_id: i64) -> AppResult<i64> {
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM ai_interactions
          WHERE teacher_id = $1 AND module = $2 AND attempt = 1
            AND input_snapshot->'worksheet'->>'id' = $3",
    )
    .bind(teacher_id)
    .bind(key)
    .bind(worksheet_id.to_string())
    .fetch_one(&state.db)
    .await?;
    Ok(n + 1)
}

fn places_detail(places: &[Place]) -> String {
    places
        .iter()
        .map(|p| {
            format!(
                "- {}｜{}｜要避開：{}\n  原句：{}",
                p.id,
                p.place,
                p.bases.join("、"),
                one_line(&p.original)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Meanings (`action = gloss`) or sentence rewrites (`replace`) for the chosen words of a
/// check: one call for all of them and, for what it left without a usable answer, one changed
/// retry.
pub async fn suggest(
    state: &AppState,
    teacher_id: i64,
    ws: &Worksheet,
    check: &VocabCheck,
    action: &str,
    bases: &[String],
) -> AppResult<Suggestions> {
    let key = match action {
        ACTION_GLOSS => KEY_GLOSS,
        ACTION_REPLACE => KEY_REPLACE,
        _ => return Err(AppError::bad_request("action 需為 gloss 或 replace。")),
    };
    if check.stale {
        return Err(AppError::Conflict(MSG_STALE.to_string()));
    }
    let mut chosen: Vec<&OverLevelWord> = Vec::new();
    for b in bases {
        let b = b.trim().to_lowercase();
        let Some(w) = check.words.iter().find(|w| w.base == b) else {
            return Err(AppError::bad_request(format!(
                "「{}」不是這次分析中超出級別的字。",
                clip_chars(&b, 40)
            )));
        };
        if !chosen.iter().any(|c| c.base == w.base) {
            chosen.push(w);
        }
    }
    if chosen.is_empty() {
        return Err(AppError::bad_request("請先選要處理的字。"));
    }
    if chosen.len() > MAX_WORDS {
        return Err(AppError::bad_request(format!(
            "一次最多 {MAX_WORDS} 個字。"
        )));
    }
    if action == ACTION_REPLACE {
        if let Some(w) = chosen.iter().find(|w| w.replaceable == 0) {
            return Err(AppError::bad_request(format!(
                "「{}」出現的地方都不能換字（{}），請改用提供字義。",
                w.base,
                w.fixed.join("、")
            )));
        }
    }
    let bases: Vec<String> = chosen.iter().map(|w| w.base.clone()).collect();
    let target = check.target_level.clamp(1, 6) as u8;
    let lex = vocab::lexicon(&state.db).await?.ok_or_else(|| {
        AppError::Conflict("系統尚未匯入大考中心參考詞彙表，無法分析詞彙級別。".to_string())
    })?;
    let places = if action == ACTION_REPLACE {
        let latest = store::latest(&state.db, ws.id).await?;
        let places = collect_places(
            &ws.title,
            &latest.content,
            check.level,
            &lex,
            target,
            &bases,
        );
        if places.len() > MAX_PLACES {
            return Err(AppError::bad_request(format!(
                "這些字一共出現在 {} 句裡，一次最多改寫 {MAX_PLACES} 句；請少選幾個字。",
                places.len()
            )));
        }
        if places.is_empty() {
            return Err(AppError::bad_request(
                "這些字出現的地方都不能換字，請改用提供字義。",
            ));
        }
        places
    } else {
        Vec::new()
    };
    let empty = |status: &'static str, message: &str, id: Option<i64>| Suggestions {
        action: action.to_string(),
        status,
        message: Some(message.to_string()),
        interaction_id: id,
        words: chosen
            .iter()
            .map(|w| WordSuggestion {
                base: w.base.clone(),
                interaction_id: None,
                gloss: None,
                replacement: None,
                places: places
                    .iter()
                    .filter(|p| p.bases.contains(&w.base))
                    .map(|p| p.id.clone())
                    .collect(),
                notes: Vec::new(),
            })
            .collect(),
        places: places
            .iter()
            .map(|p| PlaceSuggestion {
                place: p.clone(),
                rewritten: None,
                interaction_id: None,
                problem: None,
            })
            .collect(),
    };
    if !state.ai.enabled() {
        return Ok(empty("disabled", crate::ai::pipeline::MSG_DISABLED, None));
    }
    let round = round(state, teacher_id, key, ws.id).await?;
    let detail = words_detail(&chosen);
    let sentences = places_detail(&places);
    let vars: HashMap<&'static str, String> = HashMap::from([
        ("target_level", target.to_string()),
        ("word_count", chosen.len().to_string()),
        ("words", detail.clone()),
        ("sentence_count", places.len().to_string()),
        ("sentences", sentences.clone()),
        ("version_label", check.level.label().to_string()),
        (
            "round_note",
            if round > 1 {
                format!("（這是這份學習單第 {round} 次請你建議。）")
            } else {
                String::new()
            },
        ),
    ]);
    let prompt = prompt(&state.db, key).await?;
    let mut required = vec![("words", "要處理的字"), ("target_level", "目標級別")];
    if action == ACTION_REPLACE {
        required.push(("sentences", "要改寫的句子"));
    }
    let system = prompts::render(&prompt.system_text, &vars);
    let user = prompts::render_user_with(&prompt, &vars, &required);
    // The contexts and sentences come from the worksheet (made from the teacher's material): the
    // logged copy keeps their length and a hash.
    let mut logged = vars.clone();
    logged.insert("words", prompts::redacted("學習單上下文", &detail));
    logged.insert("sentences", prompts::redacted("學習單句子", &sentences));
    let logged_user = prompts::render_user_with(&prompt, &logged, &required);
    let (schema_name, schema, max_tokens) = match action {
        ACTION_GLOSS => (
            SCHEMA_GLOSS,
            gloss_schema(&bases),
            300 + 120 * chosen.len() as u32,
        ),
        _ => {
            let sentence_tokens: u32 = places
                .iter()
                .map(|p| 40 + p.original.chars().count() as u32 / 2)
                .sum();
            (
                SCHEMA_REPLACE,
                replace_schema(&bases, &places),
                (300 + 40 * chosen.len() as u32 + sentence_tokens).min(8000),
            )
        }
    };
    let temperature = BASE_TEMPERATURE + 0.1 * (round - 1).clamp(0, 4) as f32;
    let snapshot = json!({
        "inputs": logged,
        "worksheet": { "id": ws.id, "check_id": check.id, "level": check.level,
                       "version_no": check.version_no, "words": bases,
                       "places": places.iter().map(|p| json!({ "id": p.id, "field": p.field,
                           "start": p.start, "end": p.end, "bases": p.bases })).collect::<Vec<_>>() },
        "prompt": { "key": key, "version_no": prompt.version_no,
                    "system": system, "user": logged_user },
        "schema": schema_name,
        "request": { "temperature": temperature, "max_tokens": max_tokens },
    });
    let meta = InteractionMeta {
        teacher_id,
        module: key,
        unit_id: None,
        lesson_id: ws.lesson_id,
        scaffold_id: scaffold_id(&state.db, SCAFFOLD).await?,
        prompt_version_id: Some(prompt.id),
    };
    let fake_context = json!({
        "task": key,
        "target_level": target,
        "words": chosen.iter().map(|w| json!({ "base": w.base, "forms": w.forms, "pos": w.pos }))
            .collect::<Vec<_>>(),
        "places": places.iter().map(|p| json!({ "id": p.id, "sentence": p.original, "bases": p.bases }))
            .collect::<Vec<_>>(),
    });
    let mut req = LlmRequest {
        system,
        user: user.clone(),
        schema_name,
        schema,
        fake_context,
        attempt: 1,
        max_tokens: Some(max_tokens),
        temperature: Some(temperature),
        images: Vec::new(),
    };
    let owned: Vec<OverLevelWord> = chosen.iter().map(|w| (*w).clone()).collect();
    // gloss: base → (suggestion, notes, interaction)
    let mut glosses: BTreeMap<String, (GlossSuggestion, Vec<String>, i64)> = BTreeMap::new();
    // replace: place id → (sentence, interaction); why the others failed; hints; notes
    let mut rewritten: BTreeMap<String, (String, i64)> = BTreeMap::new();
    let mut failed: BTreeMap<String, String> = BTreeMap::new();
    let mut hints: BTreeMap<String, String> = BTreeMap::new();
    let mut notes: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut last_id: Option<i64> = None;
    let mut failure: Option<&'static str> = None;
    let mut retry_of: Option<i64> = None;
    let complete = |glosses: &BTreeMap<String, (GlossSuggestion, Vec<String>, i64)>,
                    rewritten: &BTreeMap<String, (String, i64)>| {
        if action == ACTION_GLOSS {
            glosses.len() == owned.len()
        } else {
            rewritten.len() == places.len()
        }
    };
    for attempt_no in 1..=2 {
        let mut snap = snapshot.clone();
        if attempt_no > 1 {
            snap["retry_of"] = json!(retry_of);
            snap["repair_note"] = json!(req.user.strip_prefix(&user).unwrap_or_default());
            snap["request"]["temperature"] = json!(req.temperature);
        }
        let a = attempt(state, &meta, &snap, &req, retry_of).await?;
        last_id = Some(a.id);
        retry_of = Some(a.id);
        let problems: Vec<String> = match &a.response {
            Ok(resp) => {
                let parsed: Result<(usize, Vec<String>), String> = if action == ACTION_GLOSS {
                    serde_json::from_value::<RawGlosses>(resp.json.clone())
                        .map_err(|e| e.to_string())
                        .map(|raw| {
                            let (ok, problems) = evaluate_glosses(&raw, &owned, &lex, target);
                            let mut new = 0;
                            for (base, (g, n)) in ok {
                                glosses.entry(base).or_insert_with(|| {
                                    new += 1;
                                    (g, n, a.id)
                                });
                            }
                            (new, problems)
                        })
                } else {
                    serde_json::from_value::<RawReplace>(resp.json.clone())
                        .map_err(|e| e.to_string())
                        .map(|raw| {
                            let r = evaluate_rewrites(&raw, &bases, &places, &lex, target);
                            let mut new = 0;
                            for (id, s) in r.sentences {
                                failed.remove(&id);
                                rewritten.entry(id).or_insert_with(|| {
                                    new += 1;
                                    (s, a.id)
                                });
                            }
                            for (id, why) in r.failed {
                                if !rewritten.contains_key(&id) {
                                    failed.insert(id, why);
                                }
                            }
                            for (base, hint) in r.hints {
                                hints.entry(base).or_insert(hint);
                            }
                            for (base, n) in r.notes {
                                notes.entry(base).or_default().extend(n);
                            }
                            (new, r.problems)
                        })
                };
                match parsed {
                    Ok((new, problems)) => {
                        let results = if problems.is_empty() {
                            vec![CheckResult::pass("schema"), CheckResult::pass("rules")]
                        } else {
                            vec![
                                CheckResult::pass("schema"),
                                CheckResult::fail("rules", problems.join("；")),
                            ]
                        };
                        let status = if new > 0 { "shown" } else { "blocked" };
                        finish_output(
                            &state.db,
                            &a,
                            resp,
                            status,
                            base_checks::report(&results),
                            Vec::new(),
                        )
                        .await?;
                        problems
                    }
                    Err(e) => {
                        let report = base_checks::report(&[CheckResult::fail("schema", e)]);
                        finish_output(&state.db, &a, resp, "blocked", report, Vec::new()).await?;
                        vec!["上一次的輸出不符合指定的 JSON 格式。".to_string()]
                    }
                }
            }
            Err(e) => match finish_failure(&state.db, &a, e).await? {
                Failure::Timeout => {
                    failure = Some("timeout");
                    break;
                }
                Failure::Error => {
                    failure = Some(if matches!(e, crate::ai::ProviderError::Disabled) {
                        "disabled"
                    } else {
                        "error"
                    });
                    break;
                }
                Failure::Retryable => vec!["上一次沒有交出完整的輸出。".to_string()],
            },
        };
        if complete(&glosses, &rewritten) || attempt_no == 2 {
            break;
        }
        failure = None;
        let note = format!(
            "\n\n【重要：這是第二次請求】上一次的輸出沒有通過平台的檢查。請修正下列問題後，重新輸出完整的 JSON（每一項都要有）：\n{}",
            problems
                .iter()
                .take(12)
                .map(|p| format!("- {}", clip_chars(p, 300)))
                .collect::<Vec<_>>()
                .join("\n")
        );
        req.user = format!("{user}{note}");
        req.attempt = 2;
        req.temperature = Some(temperature + REPAIR_TEMPERATURE_STEP);
    }
    let found = if action == ACTION_GLOSS {
        glosses.len()
    } else {
        rewritten.len()
    };
    if found == 0 {
        let (status, message) = match failure {
            Some("timeout") => ("timeout", crate::ai::pipeline::MSG_TIMEOUT),
            Some("disabled") => ("disabled", crate::ai::pipeline::MSG_DISABLED),
            Some(_) => ("error", crate::ai::pipeline::MSG_ERROR),
            None => (
                "blocked",
                "這次沒有可用的建議，你可以再試一次，或直接在學習單上修改。",
            ),
        };
        let mut out = empty(status, message, last_id);
        for p in &mut out.places {
            p.problem = failed.get(&p.place.id).cloned();
        }
        if action == ACTION_REPLACE && status == "blocked" {
            for w in &mut out.words {
                w.notes = notes.remove(&w.base).unwrap_or_default();
                w.notes
                    .push("這個字的句子都沒有可用的改寫，可以改用提供字義。".to_string());
            }
        }
        return Ok(out);
    }
    let message = if action == ACTION_GLOSS {
        let missing = owned.len() - glosses.len();
        (missing > 0)
            .then(|| format!("有 {missing} 個字這次沒有可用的字義，可以再試一次，或自己寫。"))
    } else {
        let missing = places.len() - rewritten.len();
        (missing > 0).then(|| {
            format!("有 {missing} 句這次沒有可用的改寫（用了超出級別的字、改動太大或沒有寫出），這些地方不會改。")
        })
    };
    let words = owned
        .iter()
        .map(|w| {
            if action == ACTION_GLOSS {
                return match glosses.remove(&w.base) {
                    Some((g, n, id)) => WordSuggestion {
                        base: w.base.clone(),
                        interaction_id: Some(id),
                        gloss: Some(g),
                        replacement: None,
                        places: Vec::new(),
                        notes: n,
                    },
                    None => WordSuggestion {
                        base: w.base.clone(),
                        interaction_id: None,
                        gloss: None,
                        replacement: None,
                        places: Vec::new(),
                        notes: vec!["這個字這次沒有可用的字義。".to_string()],
                    },
                };
            }
            let mine: Vec<&Place> = places
                .iter()
                .filter(|p| p.bases.contains(&w.base))
                .collect();
            let mut word_notes = notes.remove(&w.base).unwrap_or_default();
            if !mine.iter().any(|p| rewritten.contains_key(&p.id)) {
                word_notes.push("這個字的句子都沒有可用的改寫，可以改用提供字義。".to_string());
            }
            WordSuggestion {
                base: w.base.clone(),
                interaction_id: mine
                    .iter()
                    .find_map(|p| rewritten.get(&p.id).map(|(_, id)| *id)),
                gloss: None,
                replacement: hints.get(&w.base).cloned(),
                places: mine.iter().map(|p| p.id.clone()).collect(),
                notes: word_notes,
            }
        })
        .collect();
    Ok(Suggestions {
        action: action.to_string(),
        status: "shown",
        message,
        interaction_id: last_id,
        words,
        places: places
            .iter()
            .map(|p| PlaceSuggestion {
                place: p.clone(),
                rewritten: rewritten.get(&p.id).map(|(s, _)| s.clone()),
                interaction_id: rewritten.get(&p.id).map(|(_, id)| *id),
                problem: failed.get(&p.id).cloned(),
            })
            .collect(),
    })
}

// ---------------------------------------------------------------- apply

#[derive(Debug, Clone, Deserialize)]
pub struct GlossInput {
    #[serde(default)]
    pub pos: String,
    #[serde(default)]
    pub meaning_zh: String,
    #[serde(default)]
    pub definition_en: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WordChoice {
    pub base: String,
    /// `gloss` | `replace` | `leave`
    pub action: String,
    #[serde(default)]
    pub gloss: Option<GlossInput>,
    /// The suggestion call this word's suggestion came from, if any.
    #[serde(default)]
    pub interaction_id: Option<i64>,
    /// 換字: how many rewritten sentences of this word the teacher was shown (for the record:
    /// all applied → adopt, some unticked → revise).
    #[serde(default)]
    pub offered: Option<usize>,
}

/// One sentence as the teacher was shown it (before → after) and kept ticked.
#[derive(Debug, Clone, Deserialize)]
pub struct RewriteInput {
    pub field: FieldKey,
    pub start: usize,
    pub end: usize,
    pub original: String,
    pub rewritten: String,
    pub bases: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApplyRequest {
    /// The worksheet version the check is about (`check.version_no`).
    pub base_version_no: i32,
    #[serde(default)]
    pub gloss_scope: GlossScope,
    pub words: Vec<WordChoice>,
    /// 換字: the sentences to write, exactly as shown.
    #[serde(default)]
    pub rewrites: Vec<RewriteInput>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReplacedWord {
    pub base: String,
    /// Sentences rewritten.
    pub sentences: usize,
    /// Places 換字 could have changed that still have it (unticked, or no rewrite).
    pub left: usize,
}

#[derive(Debug, Serialize)]
pub struct Applied {
    /// The new worksheet version; `null` when nothing in the worksheet changed (only 不處理).
    pub version_no: Option<i32>,
    pub glossed: Vec<String>,
    pub replaced: Vec<ReplacedWord>,
    pub left: Vec<String>,
    /// The version analysed again.
    pub check: VocabCheck,
}

/// One sentence to write: `start..end` of a text becomes the rewrite.
type Edit = (usize, usize, String);

struct Decision {
    interaction_id: i64,
    item_index: i32,
    decision: &'static str,
    revised_content: Option<String>,
    note: String,
}

/// The suggestion call `id` of this teacher: its module and output.
async fn interaction(
    state: &AppState,
    teacher_id: i64,
    id: i64,
) -> AppResult<Option<(String, Value)>> {
    let row: Option<(String, Option<Value>)> = sqlx::query_as(
        "SELECT module, output FROM ai_interactions
          WHERE id = $1 AND teacher_id = $2 AND module IN ($3, $4)",
    )
    .bind(id)
    .bind(teacher_id)
    .bind(KEY_GLOSS)
    .bind(KEY_REPLACE)
    .fetch_optional(&state.db)
    .await?;
    Ok(row.map(|(m, o)| (m, o.unwrap_or(Value::Null))))
}

/// What the teacher did with a word a call suggested something for (`teacher_decisions`):
/// taken as it was → adopt; changed (a meaning edited, some sentences unticked) → revise;
/// another action → reject.
fn decision_for(
    choice: &WordChoice,
    module: &str,
    output: &Value,
    id: i64,
    final_text: &str,
    applied: usize,
) -> Decision {
    let list = if module == KEY_GLOSS {
        &output["glosses"]
    } else {
        &output["words"]
    };
    let (index, entry) = list
        .as_array()
        .and_then(|a| {
            a.iter().enumerate().find(|(_, e)| {
                e["word"].as_str().map(str::to_lowercase).as_deref() == Some(&choice.base)
            })
        })
        .map(|(i, e)| (i as i32, e.clone()))
        .unwrap_or((0, Value::Null));
    let matching_action = (module == KEY_GLOSS && choice.action == ACTION_GLOSS)
        || (module == KEY_REPLACE && choice.action == ACTION_REPLACE);
    if !matching_action {
        let what = match choice.action.as_str() {
            ACTION_GLOSS => "改用提供字義",
            ACTION_REPLACE => "改用換字",
            _ => "不處理",
        };
        return Decision {
            interaction_id: id,
            item_index: index,
            decision: "reject",
            revised_content: None,
            note: format!("詞彙級別：{}→{what}", choice.base),
        };
    }
    let (adopted, note) = if module == KEY_GLOSS {
        let same = choice.gloss.as_ref().is_some_and(|g| {
            entry["meaning_zh"].as_str().map(one_line).as_deref() == Some(g.meaning_zh.trim())
        });
        (same, format!("詞彙級別：提供字義 {}", choice.base))
    } else {
        let offered = choice.offered.unwrap_or(applied).max(applied);
        (
            applied == offered,
            format!(
                "詞彙級別：換字 {}（套用 {applied}／{offered} 句）",
                choice.base
            ),
        )
    };
    Decision {
        interaction_id: id,
        item_index: index,
        decision: if adopted { "adopt" } else { "revise" },
        revised_content: (!adopted).then(|| final_text.to_string()),
        note,
    }
}

/// Applies the teacher's choices for the words of `check` to the latest version: one new version
/// (`cause = "vocabulary"`), the changed blocks kept in `worksheet_block_history`, the decisions
/// recorded, and the version analysed again.
pub async fn apply(
    state: &AppState,
    teacher_id: i64,
    ws: &Worksheet,
    check: &VocabCheck,
    req: ApplyRequest,
) -> AppResult<Applied> {
    let db = &state.db;
    if check.stale || req.base_version_no != check.version_no {
        return Err(AppError::Conflict(MSG_STALE.to_string()));
    }
    if crate::worksheet::ai::jobs(state, ws.id).await?.1 {
        return Err(AppError::Conflict(MSG_RUNNING.to_string()));
    }
    if req.words.is_empty() {
        return Err(AppError::bad_request("請先選要怎麼處理這些字。"));
    }
    if req.words.len() > check.words.len() || req.rewrites.len() > MAX_PLACES * 5 {
        return Err(AppError::bad_request("要處理的字太多了。"));
    }
    let lex = vocab::lexicon(db).await?.ok_or_else(|| {
        AppError::Conflict("系統尚未匯入大考中心參考詞彙表，無法分析詞彙級別。".to_string())
    })?;
    let target = check.target_level.clamp(1, 6) as u8;
    let level = check.level;
    let latest = store::latest(db, ws.id).await?;
    if latest.version_no != req.base_version_no {
        return Err(AppError::Conflict(MSG_STALE.to_string()));
    }
    let mut content = latest.content.clone();
    let gloss_levels: Vec<Level> = match req.gloss_scope {
        GlossScope::Level => vec![level],
        GlossScope::All => content.levels.clone(),
    };

    // Every word once, each known to the check.
    let mut choices: Vec<(WordChoice, &OverLevelWord)> = Vec::new();
    for choice in &req.words {
        let base = choice.base.trim().to_lowercase();
        let Some(word) = check.words.iter().find(|w| w.base == base) else {
            return Err(AppError::bad_request(format!(
                "「{}」不是這次分析中超出級別的字。",
                clip_chars(&base, 40)
            )));
        };
        if choices.iter().any(|(c, _)| c.base == base) {
            return Err(AppError::bad_request(format!("「{base}」重複了。")));
        }
        if !matches!(
            choice.action.as_str(),
            ACTION_GLOSS | ACTION_REPLACE | ACTION_LEAVE
        ) {
            return Err(AppError::bad_request(
                "action 需為 gloss、replace 或 leave。",
            ));
        }
        choices.push((
            WordChoice {
                base,
                ..choice.clone()
            },
            word,
        ));
    }
    let replacing: Vec<&str> = choices
        .iter()
        .filter(|(c, _)| c.action == ACTION_REPLACE)
        .map(|(c, _)| c.base.as_str())
        .collect();

    // 換字: the sentences exactly as shown, checked again against the text and the word list.
    let mut changed_blocks: BTreeMap<String, Option<i64>> = BTreeMap::new();
    let interaction_of = |base: &str| {
        choices
            .iter()
            .find(|(c, _)| c.base == base)
            .and_then(|(c, _)| c.interaction_id)
    };
    let mut by_field: Vec<(FieldKey, Vec<Edit>)> = Vec::new();
    let mut applied_of: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for r in &req.rewrites {
        let bases: Vec<String> = r.bases.iter().map(|b| b.trim().to_lowercase()).collect();
        if bases.is_empty() {
            return Err(AppError::bad_request("每一句都要寫出要換的字。"));
        }
        if let Some(b) = bases.iter().find(|b| !replacing.contains(&b.as_str())) {
            return Err(AppError::bad_request(format!(
                "「{}」這一句也換掉了「{b}」：「{b}」要選換字，或取消這一句。",
                clip_chars(&r.original, 60)
            )));
        }
        let Some(text) = field_mut(&mut content, level, &r.field).map(|t| t.clone()) else {
            return Err(AppError::Conflict(MSG_STALE.to_string()));
        };
        let chars: Vec<char> = text.chars().collect();
        if r.start > r.end
            || r.end > chars.len()
            || chars[r.start..r.end].iter().collect::<String>() != r.original
        {
            return Err(AppError::Conflict(MSG_STALE.to_string()));
        }
        let new =
            check_rewrite(&lex, target, &r.original, &r.rewritten, &bases).map_err(|why| {
                AppError::bad_request(format!(
                    "「{}」不能這樣改：{why}",
                    clip_chars(&r.original, 60)
                ))
            })?;
        match by_field.iter_mut().find(|(k, _)| *k == r.field) {
            Some((_, edits)) => edits.push((r.start, r.end, new.clone())),
            None => by_field.push((r.field.clone(), vec![(r.start, r.end, new.clone())])),
        }
        for b in &bases {
            applied_of.entry(b.clone()).or_default().push(new.clone());
        }
        changed_blocks
            .entry(r.field.block.clone())
            .or_insert_with(|| interaction_of(&bases[0]));
    }
    for (key, mut edits) in by_field {
        edits.sort_by_key(|e| std::cmp::Reverse(e.0));
        if edits.windows(2).any(|w| w[1].1 > w[0].0) {
            return Err(AppError::bad_request("有兩句改寫重疊了。"));
        }
        let Some(text) = field_mut(&mut content, level, &key) else {
            return Err(AppError::Conflict(MSG_STALE.to_string()));
        };
        let mut chars: Vec<char> = text.chars().collect();
        for (start, end, new) in edits {
            chars.splice(start..end, new.chars());
        }
        *text = chars.into_iter().collect();
    }

    let mut glossed = Vec::new();
    let mut replaced = Vec::new();
    let mut left = Vec::new();
    let mut handled = Vec::new();
    let mut decisions: Vec<Decision> = Vec::new();
    let mut research: Vec<Value> = Vec::new();
    for (choice, word) in &choices {
        let base = choice.base.clone();
        let suggestion = match choice.interaction_id {
            Some(id) => Some((
                id,
                interaction(state, teacher_id, id)
                    .await?
                    .ok_or_else(|| AppError::not_found("找不到這筆 AI 建議。"))?,
            )),
            None => None,
        };
        let mut final_text = String::new();
        let mut applied = 0;
        match choice.action.as_str() {
            ACTION_GLOSS => {
                let g = choice
                    .gloss
                    .as_ref()
                    .ok_or_else(|| AppError::bad_request(format!("「{base}」要附上中文意思。")))?;
                let meaning = one_line(&g.meaning_zh);
                if meaning.is_empty() {
                    return Err(AppError::bad_request(format!("「{base}」要附上中文意思。")));
                }
                let edited = suggestion.as_ref().is_some_and(|(_, (_, out))| {
                    out["glosses"].as_array().is_some_and(|a| {
                        !a.iter().any(|e| {
                            e["word"].as_str().map(str::to_lowercase).as_deref()
                                == Some(base.as_str())
                                && e["meaning_zh"].as_str().map(one_line).as_deref()
                                    == Some(meaning.as_str())
                        })
                    })
                });
                let gloss = Gloss {
                    word: base.clone(),
                    pos: one_line(&g.pos),
                    meaning_zh: meaning.clone(),
                    definition_en: one_line(&g.definition_en),
                    levels: Vec::new(),
                    origin: match (&suggestion, edited) {
                        (None, _) => Origin::Teacher,
                        (Some(_), true) => Origin::AiEdited,
                        (Some(_), false) => Origin::Ai,
                    },
                };
                for id in add_gloss(&ws.title, &mut content, &gloss_levels, &lex, &base, &gloss) {
                    changed_blocks.entry(id).or_insert(choice.interaction_id);
                }
                final_text = format!("{} {}", gloss.meaning_zh, gloss.definition_en)
                    .trim()
                    .to_string();
                glossed.push(base.clone());
                handled.push(base.clone());
                research.push(
                    json!({ "base": base, "level": word.level, "action": ACTION_GLOSS,
                                      "edited": edited }),
                );
            }
            ACTION_REPLACE => {
                let sentences = applied_of.get(&base).cloned().unwrap_or_default();
                if sentences.is_empty() {
                    return Err(AppError::bad_request(format!(
                        "「{base}」選了換字，但沒有勾選任何一句。"
                    )));
                }
                applied = sentences.len();
                for id in prune_glosses(&ws.title, &mut content, &lex, &base) {
                    changed_blocks.entry(id).or_insert(choice.interaction_id);
                }
                final_text = sentences.join(" / ");
                handled.push(base.clone());
                replaced.push((base.clone(), applied));
                research.push(
                    json!({ "base": base, "level": word.level, "action": ACTION_REPLACE,
                                      "sentences": applied, "offered": choice.offered }),
                );
            }
            _ => {
                left.push(base.clone());
                research.push(
                    json!({ "base": base, "level": word.level, "action": ACTION_LEAVE,
                                      "key_word": word.key_word }),
                );
            }
        }
        if let Some((id, (module, output))) = &suggestion {
            decisions.push(decision_for(
                choice,
                module,
                output,
                *id,
                &final_text,
                applied,
            ));
        }
    }
    // What 換字 left where it could have changed the word (unticked, or no rewrite).
    let still: BTreeMap<String, usize> = {
        let places = collect_places(
            &ws.title,
            &content,
            level,
            &lex,
            target,
            &replaced.iter().map(|(b, _)| b.clone()).collect::<Vec<_>>(),
        );
        let mut n = BTreeMap::new();
        for p in places {
            for b in p.bases {
                *n.entry(b).or_insert(0) += 1;
            }
        }
        n
    };
    let replaced: Vec<ReplacedWord> = replaced
        .into_iter()
        .map(|(base, sentences)| ReplacedWord {
            left: still.get(&base).copied().unwrap_or_default(),
            base,
            sentences,
        })
        .collect();

    content.normalise().map_err(AppError::bad_request)?;
    vocab::relookup_levels(&mut content, Some(&lex));
    let mut version_no = None;
    {
        let mut tx = db.begin().await?;
        sqlx::query("SELECT id FROM worksheets WHERE id = $1 FOR UPDATE")
            .bind(ws.id)
            .execute(&mut *tx)
            .await?;
        let current: i32 = sqlx::query_scalar(
            "SELECT MAX(version_no) FROM worksheet_versions WHERE worksheet_id = $1",
        )
        .bind(ws.id)
        .fetch_one(&mut *tx)
        .await?;
        if current != req.base_version_no {
            return Err(AppError::Conflict(MSG_STALE.to_string()));
        }
        if content != latest.content {
            for (block_id, interaction_id) in &changed_blocks {
                let Some(old) = latest.content.blocks.iter().find(|b| &b.id == block_id) else {
                    continue;
                };
                let new: Option<&Block> = content.blocks.iter().find(|b| &b.id == block_id);
                if new == Some(old) {
                    continue;
                }
                sqlx::query(
                    "INSERT INTO worksheet_block_history (worksheet_id, block_id, block, interaction_id)
                     VALUES ($1, $2, $3, $4)",
                )
                .bind(ws.id)
                .bind(block_id)
                .bind(sqlx::types::Json(old))
                .bind(interaction_id)
                .execute(&mut *tx)
                .await?;
            }
            let block_ids: Vec<&String> = changed_blocks.keys().collect();
            let saved = store::insert_version_from(
                &mut tx,
                ws.id,
                &content,
                "vocabulary",
                Some(serde_json::json!({ "cause": "vocabulary", "block_ids": block_ids })),
            )
            .await?;
            version_no = Some(saved.version_no);
        }
        for d in &decisions {
            sqlx::query(
                "INSERT INTO teacher_decisions
                   (ai_interaction_id, teacher_id, item_index, decision, revised_content, reason_note)
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(d.interaction_id)
            .bind(teacher_id)
            .bind(d.item_index)
            .bind(d.decision)
            .bind(&d.revised_content)
            .bind(&d.note)
            .execute(&mut *tx)
            .await?;
        }
        log_usage(
            &mut *tx,
            teacher_id,
            "worksheet_vocabulary_applied",
            json!({
                "worksheet_id": ws.id, "check_id": check.id, "level": level,
                "target_level": target, "base_version_no": req.base_version_no,
                "version_no": version_no, "gloss_scope": req.gloss_scope, "words": research,
            }),
        )
        .await?;
        tx.commit().await?;
    }
    let check = vocab::run_with(db, ws, level, target, &left, &handled).await?;
    Ok(Applied {
        version_no,
        glossed,
        replaced,
        left,
        check,
    })
}

// ---------------------------------------------------------------- fake provider

/// The fake provider's answers (`AI_PROVIDER=fake`). Markers in the request:
///
/// | marker | effect |
/// |---|---|
/// | `FAKE_VOCAB_OVER` | every word's hint is unlisted (dropped), and the first sentence's rewrite uses a level-6 word: rejected |
/// | `FAKE_VOCAB_WHOLE` | the first sentence is written as a different sentence: rejected (the change is not local) |
/// | `FAKE_VOCAB_HARD_DEF` | every English definition uses the word itself: the code drops the definition |
/// | `FAKE_VOCAB_MISSING` | the first call leaves out the last word (gloss) or sentence (replace); the retry has it |
pub mod fake {
    use serde_json::{json, Value};

    /// (word, Chinese meaning, part of speech, definition)
    const MEANINGS: [(&str, &str, &str, &str); 5] = [
        ("abandon", "拋棄；放棄", "v.", "to leave and not come back"),
        ("enormous", "巨大的", "adj.", "very big"),
        ("obstacle", "障礙", "n.", "something that stops you"),
        (
            "resilient",
            "有韌性的",
            "adj.",
            "able to get better after a hard time",
        ),
        ("accord", "依照", "n.", ""),
    ];

    /// (word, the word it mostly becomes)
    const HINTS: [(&str, &str); 4] = [
        ("abandon", "leave"),
        ("enormous", "huge"),
        ("obstacle", "problem"),
        ("accord", "based on"),
    ];

    /// Phrases rewritten as a whole (before the single forms).
    const PHRASES: [(&str, &str); 2] = [("according to", "based on"), ("an enormous", "a huge")];

    /// (form, what it becomes)
    const FORMS: [(&str, &str); 8] = [
        ("abandoned", "left"),
        ("abandoning", "leaving"),
        ("abandons", "leaves"),
        ("abandon", "leave"),
        ("enormous", "huge"),
        ("obstacles", "problems"),
        ("obstacle", "problem"),
        ("according", "based"),
    ];

    fn list(ctx: &Value, key: &str) -> Vec<Value> {
        ctx[key].as_array().cloned().unwrap_or_default()
    }

    fn cased(rep: &str, original: &str) -> String {
        if original.starts_with(|c: char| c.is_uppercase()) {
            let mut c = rep.chars();
            c.next()
                .map(|f| format!("{}{}", f.to_uppercase(), c.as_str()))
                .unwrap_or_default()
        } else {
            rep.to_string()
        }
    }

    fn replace_phrase(s: &str, from: &str, to: &str) -> String {
        let low = s.to_lowercase();
        match low.find(from) {
            Some(i) if s.is_ascii() => {
                format!("{}{}{}", &s[..i], cased(to, &s[i..]), &s[i + from.len()..])
            }
            _ => s.to_string(),
        }
    }

    /// Every whole word `from` (any case) becomes `to` (in the same case).
    fn replace_word(s: &str, from: &str, to: &str) -> String {
        let chars: Vec<char> = s.chars().collect();
        let mut out = String::new();
        let mut i = 0;
        while i < chars.len() {
            if chars[i].is_ascii_alphabetic() {
                let start = i;
                while i < chars.len() && chars[i].is_ascii_alphabetic() {
                    i += 1;
                }
                let w: String = chars[start..i].iter().collect();
                if w.eq_ignore_ascii_case(from) {
                    out.push_str(&cased(to, &w));
                } else {
                    out.push_str(&w);
                }
            } else {
                out.push(chars[i]);
                i += 1;
            }
        }
        out
    }

    fn rewrite(sentence: &str) -> String {
        let mut s = sentence.to_string();
        for (from, to) in PHRASES {
            s = replace_phrase(&s, from, to);
        }
        for (from, to) in FORMS {
            s = replace_word(&s, from, to);
        }
        s
    }

    fn missing(mut list: Vec<Value>, attempt: i32, has: &dyn Fn(&str) -> bool) -> Vec<Value> {
        if attempt == 1 && has("FAKE_VOCAB_MISSING") && list.len() > 1 {
            list.pop();
        }
        list
    }

    pub fn gloss(ctx: &Value, attempt: i32, has: &dyn Fn(&str) -> bool) -> Value {
        let glosses: Vec<Value> = missing(list(ctx, "words"), attempt, has)
            .into_iter()
            .map(|w| {
                let base = w["base"].as_str().unwrap_or_default().to_string();
                let (meaning, pos, def) = MEANINGS
                    .iter()
                    .find(|(x, ..)| *x == base)
                    .map(|(_, m, p, d)| (m.to_string(), p.to_string(), d.to_string()))
                    .unwrap_or_else(|| ("（意思）".to_string(), "n.".to_string(), String::new()));
                let def = if has("FAKE_VOCAB_HARD_DEF") {
                    format!("to {base}")
                } else {
                    def
                };
                json!({ "word": base, "pos": pos, "meaning_zh": meaning, "definition_en": def })
            })
            .collect();
        json!({ "glosses": glosses })
    }

    pub fn replace(ctx: &Value, attempt: i32, has: &dyn Fn(&str) -> bool) -> Value {
        let words: Vec<Value> = list(ctx, "words")
            .into_iter()
            .map(|w| {
                let base = w["base"].as_str().unwrap_or_default();
                let hint = if has("FAKE_VOCAB_OVER") {
                    "colossal"
                } else {
                    HINTS
                        .iter()
                        .find(|(x, _)| *x == base)
                        .map_or("thing", |(_, h)| *h)
                };
                json!({ "word": base, "replacement": hint })
            })
            .collect();
        let sentences: Vec<Value> = missing(list(ctx, "places"), attempt, has)
            .into_iter()
            .enumerate()
            .map(|(i, p)| {
                let sentence = p["sentence"].as_str().unwrap_or_default();
                let rewritten = if i == 0 && has("FAKE_VOCAB_WHOLE") {
                    "The family left the house.".to_string()
                } else if i == 0 && has("FAKE_VOCAB_OVER") {
                    let mut s = sentence.to_string();
                    for (from, _) in FORMS {
                        s = replace_word(&s, from, "gigantic");
                    }
                    s
                } else {
                    rewrite(sentence)
                };
                json!({ "id": p["id"], "rewritten": rewritten })
            })
            .collect();
        json!({ "words": words, "sentences": sentences })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vocabulary::LexiconRow;
    use crate::worksheet::{Block, Item};

    fn lex() -> Lexicon {
        let rows = [
            ("a", 1, "art."),
            ("an", 1, "art."),
            ("the", 1, "art."),
            ("they", 1, "pron."),
            ("it", 1, "pron."),
            ("why", 1, "adv."),
            ("did", 1, "aux."),
            ("do", 1, "aux./v."),
            ("what", 1, "pron."),
            ("does", 1, "aux."),
            ("would", 1, "aux."),
            ("happen", 1, "v."),
            ("if", 1, "conj."),
            ("mean", 1, "v."),
            ("family", 1, "n."),
            ("house", 1, "n."),
            ("old", 1, "adj."),
            ("their", 1, "pron."),
            ("tree", 1, "n."),
            ("saw", 1, "v."),
            ("we", 1, "pron."),
            ("leave", 1, "v."),
            ("give", 1, "v."),
            ("up", 1, "adv."),
            ("big", 1, "adj."),
            ("huge", 2, "adj."),
            ("problem", 1, "n."),
            ("text", 3, "n."),
            ("base", 2, "n./v."),
            ("on", 1, "prep."),
            ("to", 1, "prep."),
            ("and", 1, "conj."),
            ("not", 1, "adv."),
            ("come", 1, "v."),
            ("back", 1, "adv."),
            ("abandon", 5, "v."),
            ("enormous", 5, "adj."),
            ("obstacle", 5, "n."),
            ("accord", 5, "n./v."),
            ("gigantic", 6, "adj."),
        ];
        Lexicon::new(
            "T",
            rows.into_iter().map(|(w, l, p)| LexiconRow {
                word: w.into(),
                level: Some(l),
                pos: p.into(),
                kind: "base".into(),
                appendix_category: String::new(),
            }),
        )
    }

    fn word(base: &str, forms: &[&str]) -> OverLevelWord {
        OverLevelWord {
            base: base.into(),
            level: 5,
            pos: "v.".into(),
            count: forms.len(),
            forms: forms.iter().map(|f| f.to_string()).collect(),
            replaceable: 1,
            fixed: vec![],
            in_material: false,
            key_word: false,
            glossed: None,
            kept: false,
            suggested: ACTION_REPLACE.into(),
            reason: String::new(),
            contexts: vec![],
        }
    }

    fn b(s: &str) -> Vec<String> {
        vec![s.to_string()]
    }

    #[test]
    fn a_phrase_is_rewritten_as_a_whole() {
        let lex = lex();
        let original = "What would happen if bees disappeared, according to the text?";
        assert_eq!(
            check_rewrite(
                &lex,
                4,
                original,
                "What would happen if bees disappeared, based on the text?",
                &b("accord")
            )
            .unwrap(),
            "What would happen if bees disappeared, based on the text?"
        );
        // Swapping the form alone leaves the word… and keeping 「according」 is not allowed.
        assert!(check_rewrite(&lex, 4, original, original, &b("accord")).is_err());
        // Words the sentence already had (bees, disappeared: not listed) are fine; new ones not.
        assert!(check_rewrite(
            &lex,
            4,
            original,
            "What would happen if bees disappeared, as stated in the text?",
            &b("accord")
        )
        .unwrap_err()
        .contains("不在參考詞彙表"));
    }

    #[test]
    fn a_rewrite_with_a_harder_word_or_a_new_sentence_is_rejected() {
        let lex = lex();
        let original = "Why did the family abandon their old house?";
        assert!(check_rewrite(
            &lex,
            4,
            original,
            "Why did the family leave their old house?",
            &b("abandon")
        )
        .is_ok());
        assert!(check_rewrite(
            &lex,
            4,
            original,
            "Why did the family gigantic their old house?",
            &b("abandon")
        )
        .unwrap_err()
        .contains("第 6 級"));
        let whole = check_rewrite(
            &lex,
            4,
            original,
            "The family left the house.",
            &b("abandon"),
        );
        assert!(whole.unwrap_err().contains("改動太多"));
        // A change far from the word.
        assert!(check_rewrite(
            &lex,
            4,
            "Why did the family abandon their old house and come back to the big tree?",
            "Why did the family leave their old house and come back to the huge tree?",
            &b("abandon")
        )
        .unwrap_err()
        .contains("較遠"));
        assert!(check_rewrite(&lex, 4, original, "為什麼", &b("abandon")).is_err());
        // A quoted word may stay.
        assert!(check_rewrite(
            &lex,
            4,
            "They abandon “abandon”.",
            "They leave “abandon”.",
            &b("abandon")
        )
        .is_ok());
    }

    #[test]
    fn sentences_are_found_and_shared_by_the_words_in_them() {
        let lex = lex();
        let content = Content {
            levels: vec![Level::Standard],
            blocks: vec![Block {
                id: "b1".into(),
                title: "Part A".into(),
                instructions: "Read it. Do not abandon a hard one. Then come back.".into(),
                items: vec![Item {
                    id: "i1".into(),
                    prompt:
                        "Why did they abandon their enormous house? What does \"abandon\" mean?"
                            .into(),
                    options: vec!["an enormous tree".into()],
                    levels: vec![Level::Standard],
                    ..Item::default()
                }],
                ..Block::default()
            }],
            ..Content::default()
        };
        let bases = vec!["abandon".to_string(), "enormous".to_string()];
        let places = collect_places("T", &content, Level::Standard, &lex, 4, &bases);
        let got: Vec<(&str, &str, Vec<&str>)> = places
            .iter()
            .map(|p| {
                (
                    p.place.as_str(),
                    p.original.as_str(),
                    p.bases.iter().map(String::as_str).collect(),
                )
            })
            .collect();
        assert_eq!(
            got,
            vec![
                (
                    "A 活動 · 作答指示",
                    "Do not abandon a hard one.",
                    vec!["abandon"]
                ),
                (
                    "A 活動 · 第 1 題",
                    "Why did they abandon their enormous house?",
                    vec!["abandon", "enormous"]
                ),
                (
                    "A 活動 · 第 1 題選項 1",
                    "an enormous tree",
                    vec!["enormous"]
                ),
            ]
        );
        assert_eq!(places[1].start, 0);
        assert_eq!(places[0].start, 9);
        let mut c = content.clone();
        assert_eq!(
            field_mut(&mut c, Level::Standard, &places[2].field).unwrap(),
            "an enormous tree"
        );
        assert!(field_mut(&mut c, Level::Basic, &places[2].field).is_none());
    }

    #[test]
    fn an_organizer_is_analysed_and_only_its_title_and_labels_are_rewritten() {
        use crate::worksheet::organizer::{OrgCell, OrgSection, Organizer, OrganizerKind};
        use crate::worksheet::FMT_ORGANIZER;
        let lex = lex();
        let cell = |answer: &str, given: &str| OrgCell {
            answer: answer.into(),
            given: given.into(),
            prefilled: if given.is_empty() {
                vec![]
            } else {
                vec![Level::Standard]
            },
        };
        let mut content = Content {
            levels: vec![Level::Standard],
            blocks: vec![Block {
                id: "b1".into(),
                title: "Part A".into(),
                items: vec![Item {
                    id: "i1".into(),
                    prompt: "Fill in the map.".into(),
                    answer_format: Some(FMT_ORGANIZER.into()),
                    organizer: Some(Organizer {
                        kind: OrganizerKind::MindMap,
                        title: "An enormous problem".into(),
                        center: "obstacle".into(),
                        sections: vec![
                            OrgSection {
                                label: "Why they abandon it".into(),
                                cells: vec![cell("", "They abandon the old house.")],
                            },
                            OrgSection {
                                label: "Big".into(),
                                cells: vec![cell("an enormous tree", "")],
                            },
                            OrgSection {
                                label: "Old".into(),
                                cells: vec![cell("", "")],
                            },
                        ],
                    }),
                    levels: vec![Level::Standard],
                    ..Item::default()
                }],
                ..Block::default()
            }],
            ..Content::default()
        };

        // The analysis reads what the student sheet prints: title, centre, labels and the
        // pre-filled cell, never the answer (teacher version only).
        let sheet = project("T", &content, Level::Standard, Audience::Student);
        let units = text_units(&sheet);
        let text = units
            .iter()
            .map(|u| u.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("obstacle") && text.contains("They abandon the old house."));
        assert!(!text.contains("enormous tree"));
        let analysis = vocabulary::analyze(&text, &lex, 4, &[]);
        let words = vocab::over_level_words(
            &analysis,
            &units,
            &content,
            Level::Standard,
            "",
            &lex,
            &BTreeSet::new(),
        );
        let get = |base: &str| words.iter().find(|w| w.base == base).unwrap();
        // abandon: the label may change, the pre-filled cell may not.
        assert_eq!(get("abandon").replaceable, 1);
        assert_eq!(
            get("abandon").fixed,
            vec!["A 活動 · 第 1 題圖像組織預填：圖像組織的中心詞或預填答案"]
        );
        assert_eq!(get("enormous").replaceable, 1);
        assert_eq!(get("obstacle").replaceable, 0);
        assert_eq!(
            get("obstacle").fixed,
            vec!["A 活動 · 第 1 題圖像組織中心詞：圖像組織的中心詞或預填答案"]
        );

        // 換字: the title and the label, not the centre, the pre-filled text or the answers.
        let bases: Vec<String> = ["abandon", "enormous", "obstacle"]
            .map(String::from)
            .to_vec();
        let places = collect_places("T", &content, Level::Standard, &lex, 4, &bases);
        let got: Vec<(&str, &str)> = places
            .iter()
            .map(|p| (p.place.as_str(), p.original.as_str()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("A 活動 · 第 1 題圖像組織標題", "An enormous problem"),
                ("A 活動 · 第 1 題圖像組織標籤 1", "Why they abandon it"),
            ]
        );
        let mut c = content.clone();
        *field_mut(&mut c, Level::Standard, &places[1].field).unwrap() = "Why they leave it".into();
        let org = c.blocks[0].items[0].organizer.as_ref().unwrap();
        assert_eq!(org.sections[0].label, "Why they leave it");
        assert_eq!(
            org.sections[0].cells[0].given,
            "They abandon the old house."
        );

        // A question that no longer prints its organizer has nothing there to change.
        content.blocks[0].items[0].answer_format = Some("簡答".into());
        assert!(collect_places("T", &content, Level::Standard, &lex, 4, &bases).is_empty());
        assert!(field_mut(&mut content, Level::Standard, &places[0].field).is_none());
    }

    #[test]
    fn the_fake_rewrites_a_phrase_and_the_hints_are_checked() {
        let lex = lex();
        let places = vec![Place {
            id: "p1".into(),
            field: FieldKey {
                block: "b1".into(),
                part: FieldPart::Prompt,
                id: Some("i1".into()),
                index: None,
            },
            place: String::new(),
            start: 0,
            end: 10,
            original: "What would happen if bees disappeared, according to the text?".into(),
            bases: b("accord"),
        }];
        let ctx = json!({ "words": [{ "base": "accord" }],
                          "places": [{ "id": "p1", "sentence": places[0].original, "bases": ["accord"] }] });
        let raw: RawReplace = serde_json::from_value(fake::replace(&ctx, 1, &|_| false)).unwrap();
        let r = evaluate_rewrites(&raw, &b("accord"), &places, &lex, 4);
        assert_eq!(
            r.sentences["p1"],
            "What would happen if bees disappeared, based on the text?"
        );
        assert_eq!(r.hints["accord"], "based on");
        let raw: RawReplace =
            serde_json::from_value(fake::replace(&ctx, 1, &|m| m == "FAKE_VOCAB_OVER")).unwrap();
        let r = evaluate_rewrites(&raw, &b("accord"), &places, &lex, 4);
        assert!(r.sentences.is_empty(), "{:?}", r.failed);
        assert!(r.notes["accord"][0].contains("colossal"));
    }

    #[test]
    fn glosses_need_chinese_and_lose_a_hard_definition() {
        let lex = lex();
        let words = vec![
            word("abandon", &["abandoned"]),
            word("enormous", &["enormous"]),
        ];
        let raw = RawGlosses {
            glosses: vec![
                RawGloss {
                    word: "abandon".into(),
                    pos: "v".into(),
                    meaning_zh: "拋棄\n放棄".into(),
                    definition_en: "to abandon something".into(),
                },
                RawGloss {
                    word: "enormous".into(),
                    pos: "adj.".into(),
                    meaning_zh: "very big".into(),
                    definition_en: String::new(),
                },
            ],
        };
        let (ok, problems) = evaluate_glosses(&raw, &words, &lex, 4);
        let (g, notes) = &ok["abandon"];
        assert_eq!(g.meaning_zh, "拋棄 放棄");
        assert_eq!(g.pos, "v.");
        assert_eq!(g.definition_en, "", "{notes:?}");
        assert!(notes[0].contains("abandon"));
        assert!(!ok.contains_key("enormous"));
        assert!(problems.iter().any(|p| p.contains("enormous")));
    }

    #[test]
    fn a_hint_must_be_within_the_level() {
        let lex = lex();
        assert_eq!(replacement_level(&lex, "give up", 4), Ok(1));
        assert!(replacement_level(&lex, "gigantic", 4)
            .unwrap_err()
            .contains("第 6 級"));
        assert!(replacement_level(&lex, "colossal", 4)
            .unwrap_err()
            .contains("不在參考詞彙表"));
    }

    #[test]
    fn schemas_bound_every_string_and_array() {
        let bases = vec!["abandon".to_string(), "enormous".to_string()];
        let g = gloss_schema(&bases);
        assert_eq!(g["properties"]["glosses"]["maxItems"], 2);
        assert_eq!(
            g["properties"]["glosses"]["items"]["properties"]["word"]["enum"],
            json!(["abandon", "enormous"])
        );
        let places = vec![Place {
            id: "p1".into(),
            field: FieldKey {
                block: "b1".into(),
                part: FieldPart::Title,
                id: None,
                index: None,
            },
            place: String::new(),
            start: 0,
            end: 4,
            original: "Abandon it.".into(),
            bases: b("abandon"),
        }];
        let r = replace_schema(&bases, &places);
        assert_eq!(r["properties"]["sentences"]["maxItems"], 1);
        assert_eq!(
            r["properties"]["sentences"]["items"]["properties"]["id"]["enum"],
            json!(["p1"])
        );
        let text = r.to_string();
        assert!(text.find("\"words\"").unwrap() < text.find("\"sentences\"").unwrap());
        assert!(text.find("\"id\"").unwrap() < text.find("\"rewritten\"").unwrap());
        fn bounded(v: &Value, path: &str) {
            match v {
                Value::Object(o) => {
                    match o.get("type").and_then(Value::as_str) {
                        Some("string") if !o.contains_key("enum") => {
                            assert!(o.contains_key("maxLength"), "{path}")
                        }
                        Some("array") => assert!(o.contains_key("maxItems"), "{path}"),
                        _ => {}
                    }
                    for (k, x) in o {
                        bounded(x, &format!("{path}.{k}"));
                    }
                }
                Value::Array(a) => a.iter().for_each(|x| bounded(x, path)),
                _ => {}
            }
        }
        bounded(&g, "gloss");
        bounded(&r, "replace");
    }
}
