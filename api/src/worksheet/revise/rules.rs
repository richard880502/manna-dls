use super::{NEW, REPLY_CHARS};
use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::super::bloom::Bloom;
use super::super::checks::{evaluate_revision, CleanSectionItem, RevisionDraft, SectionCtx};
use super::super::comments::{Comment, Field};
use super::super::images::actions::{self, RawPictureAction};
use super::super::material::MaterialParagraph as Paragraph;
use super::super::template::{
    enum_of, obj, s_max, strings_max, RawOutItem, ITEM_ANSWER_CHARS, ITEM_OPTIONS,
    ITEM_OPTION_CHARS, ITEM_PROMPT_CHARS, MSG_EXTERNAL, SECTION_INSTRUCTIONS_CHARS,
    SECTION_TITLE_CHARS,
};
use super::super::{
    Block, Gloss, Item, Level, Origin, Scaffold, ScaffoldType, Vocab, ANSWER_FORMATS, FMT_TABLE,
    KIND_EXAMPLE, KIND_FRAMES, KIND_GUIDING, KIND_STEPS, KIND_WORD_LIST, MAX_ID_CHARS, MAX_ITEMS,
    MAX_LIST,
};
use crate::prep::clean_text;
use crate::vocabulary::{self, Lexicon};

// ---------------------------------------------------------------- what the model writes

#[derive(Debug, Clone, Deserialize)]
pub struct RawRevisedItem {
    /// `Q1`, `Q2` … (a question of the block) or `new`.
    pub source: String,
    pub keep: bool,
    pub answer_format: String,
    pub prompt: String,
    pub options: Vec<String>,
    pub answer: String,
    pub bloom: String,
}

/// One scaffold of the block as the model writes it: `source` is `S1`… or `new`; what it does is
/// `action`. A scaffold the model does not list stays as it is.
#[derive(Debug, Clone, Deserialize)]
pub struct RawScaffoldEdit {
    pub source: String,
    /// `keep` | `change` | `remove`
    pub action: String,
    /// Only a new scaffold takes its kind from here; a stored one keeps its own.
    #[serde(default)]
    pub kind: String,
    /// The text of a language / instructional scaffold.
    #[serde(default)]
    pub content: String,
    /// The steps of a procedural scaffold.
    #[serde(default)]
    pub steps: Vec<String>,
}

/// One row of the word table (`W1`… or `new`).
#[derive(Debug, Clone, Deserialize)]
pub struct RawWordEdit {
    pub source: String,
    /// `keep` | `change` | `remove`
    pub action: String,
    /// Only a new row takes its word from here.
    #[serde(default)]
    pub word: String,
    #[serde(default)]
    pub meaning: String,
}

/// One entry of the Glossary (`G1`…): kept or removed, nothing else.
#[derive(Debug, Clone, Deserialize)]
pub struct RawGlossEdit {
    pub source: String,
    /// `keep` | `remove`
    pub action: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RawBlockRevision {
    /// "" when unchanged.
    pub title: String,
    /// "" when unchanged.
    pub instructions: String,
    pub items: Vec<RawRevisedItem>,
    /// Scaffolds to change, remove or add (only the ones that change need to be listed).
    #[serde(default)]
    pub scaffolds: Vec<RawScaffoldEdit>,
    /// Rows of the word table to change, remove or add.
    #[serde(default)]
    pub words: Vec<RawWordEdit>,
    /// Entries of the Glossary to remove.
    #[serde(default)]
    pub glossary: Vec<RawGlossEdit>,
    /// Pictures to add, redraw or remove (proposals; run when the teacher accepts).
    #[serde(default)]
    pub picture_actions: Vec<RawPictureAction>,
    pub reply: String,
}

/// The code of the `k`-th question (0-based) of the block, as the model sees it.
pub fn code(k: usize) -> String {
    format!("Q{}", k + 1)
}

/// The code of the `k`-th scaffold (0-based): `S1`…
pub fn scaffold_code(k: usize) -> String {
    format!("S{}", k + 1)
}

/// The code of the `k`-th row of the word table (0-based): `W1`…
pub fn word_code(k: usize) -> String {
    format!("W{}", k + 1)
}

/// The code of the `k`-th Glossary entry (0-based): `G1`…
pub fn gloss_code(k: usize) -> String {
    format!("G{}", k + 1)
}

fn index_of(code: &str, n: usize) -> Option<usize> {
    index_with('Q', code, n)
}

fn index_with(letter: char, code: &str, n: usize) -> Option<usize> {
    let k: usize = code.trim().strip_prefix(letter)?.parse().ok()?;
    (1..=n).contains(&k).then(|| k - 1)
}

/// Most questions a revised block may have: room to add a few, never past the worksheet limit.
pub fn max_items(current: usize) -> usize {
    (current + 6).clamp(12, MAX_ITEMS)
}

/// The output budget: about 220 tokens per question it may write, plus the rest.
pub fn max_tokens(max_items: usize) -> u32 {
    (1000 + 220 * max_items.min(30) as u32).clamp(2500, 6000)
}

/// What the block has besides its questions, for the schema and the budget.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Extras {
    pub scaffolds: usize,
    /// The kinds the block's scaffolds have, in order (a schema enum needs them).
    pub kinds: Vec<String>,
    pub words: usize,
    pub glosses: usize,
}

impl Extras {
    pub fn of(block: &Block) -> Self {
        Self {
            scaffolds: block.scaffolds.len(),
            kinds: block.scaffolds.iter().map(|s| s.kind.clone()).collect(),
            words: block.vocabulary.len(),
            glosses: block.glosses.len(),
        }
    }

    /// Room for the scaffolds, words and glossary entries the model may write.
    pub fn tokens(&self) -> u32 {
        (400 + 90 * (self.scaffolds + self.words + self.glosses).min(40) as u32).min(4000)
    }
}

/// Kinds of scaffold the model may add (the ones a teacher asks for by name).
pub const ADDABLE_KINDS: [&str; 4] = [KIND_FRAMES, KIND_GUIDING, KIND_EXAMPLE, KIND_STEPS];
pub const ACTIONS: [&str; 3] = ["keep", "change", "remove"];
pub const SCAFFOLD_CONTENT_CHARS: usize = 1200;
pub const SCAFFOLD_STEP_CHARS: usize = 300;
pub const MAX_STEPS: usize = 12;
pub const WORD_CHARS: usize = 60;
pub const MEANING_CHARS: usize = 200;
/// At most this many scaffolds or words are added by one revision.
const MAX_ADDED: usize = 5;

/// `worksheet_block_revise`, properties in the order they are written: the title and
/// instructions ("" = unchanged), every question (`source` and `keep` first, so the model
/// decides what to keep before it writes), the scaffolds, the word table and the Glossary
/// (`source` and `action` first), then the reply.
pub fn schema(
    question_count: usize,
    max_items: usize,
    block_id: &str,
    slot_ids: &[String],
    picture_max: usize,
    extras: &Extras,
) -> Value {
    let mut sources: Vec<String> = (0..question_count).map(code).collect();
    sources.push(NEW.to_string());
    let sources: Vec<&str> = sources.iter().map(String::as_str).collect();
    let blooms: Vec<&str> = Bloom::ALL.iter().map(|b| b.as_str()).collect();
    let item = obj(vec![
        ("source", enum_of(&sources)),
        ("keep", json!({ "type": "boolean" })),
        ("answer_format", enum_of(&ANSWER_FORMATS)),
        ("prompt", s_max(ITEM_PROMPT_CHARS)),
        ("options", strings_max(ITEM_OPTIONS, ITEM_OPTION_CHARS)),
        ("answer", s_max(ITEM_ANSWER_CHARS)),
        ("bloom", enum_of(&blooms)),
    ]);
    let coded = |f: fn(usize) -> String, n: usize, with_new: bool| -> Vec<String> {
        let mut v: Vec<String> = (0..n).map(f).collect();
        if with_new {
            v.push(NEW.to_string());
        }
        v
    };
    let enum_strings = |v: &[String]| enum_of(&v.iter().map(String::as_str).collect::<Vec<_>>());
    let mut kinds: Vec<String> = ADDABLE_KINDS.iter().map(|k| k.to_string()).collect();
    for k in &extras.kinds {
        if !kinds.contains(k) {
            kinds.push(k.clone());
        }
    }
    let scaffold = obj(vec![
        (
            "source",
            enum_strings(&coded(scaffold_code, extras.scaffolds, true)),
        ),
        ("action", enum_of(&ACTIONS)),
        ("kind", enum_strings(&kinds)),
        ("content", s_max(SCAFFOLD_CONTENT_CHARS)),
        ("steps", strings_max(MAX_STEPS, SCAFFOLD_STEP_CHARS)),
    ]);
    let word = obj(vec![
        (
            "source",
            enum_strings(&coded(word_code, extras.words, true)),
        ),
        ("action", enum_of(&ACTIONS)),
        ("word", s_max(WORD_CHARS)),
        ("meaning", s_max(MEANING_CHARS)),
    ]);
    // A schema enum cannot be empty: without a Glossary the list may have no entries at all.
    let gloss = obj(vec![
        (
            "source",
            enum_strings(&coded(gloss_code, extras.glosses.max(1), false)),
        ),
        ("action", enum_of(&["keep", "remove"])),
    ]);
    obj(vec![
        ("title", s_max(SECTION_TITLE_CHARS)),
        ("instructions", s_max(SECTION_INSTRUCTIONS_CHARS)),
        (
            "items",
            json!({ "type": "array", "items": item, "maxItems": max_items }),
        ),
        (
            "scaffolds",
            json!({ "type": "array", "items": scaffold,
                    "maxItems": extras.scaffolds + MAX_ADDED }),
        ),
        (
            "words",
            json!({ "type": "array", "items": word, "maxItems": extras.words + MAX_ADDED }),
        ),
        (
            "glossary",
            json!({ "type": "array", "items": gloss, "maxItems": extras.glosses }),
        ),
        (
            "picture_actions",
            actions::schema(&[block_id.to_string()], slot_ids, picture_max),
        ),
        ("reply", s_max(REPLY_CHARS)),
    ])
}

// ---------------------------------------------------------------- what may change

/// What the code lets a revision change.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scope {
    /// Anything in the block: the title, the instructions, every question, adding and removing.
    pub open: bool,
    pub title: bool,
    pub instructions: bool,
    /// Questions that may change or be removed (by id).
    pub items: HashSet<String>,
}

impl Scope {
    /// From what the teacher sent: a sentence, or a note on the whole block, or 「可以動到這一區的
    /// 其他題」 opens the whole block; notes on the title, the instructions or questions open only
    /// those (a new question may always be added).
    pub fn from(instruction: &str, notes: &[Comment], whole_block: bool) -> Self {
        let mut s = Scope {
            open: whole_block || !instruction.trim().is_empty(),
            ..Scope::default()
        };
        for c in notes {
            match c.field {
                Field::Block => s.open = true,
                Field::Title => s.title = true,
                Field::Instructions => s.instructions = true,
                Field::Item => match c.anchor.item_id.as_deref() {
                    Some(id) => {
                        s.items.insert(id.to_string());
                    }
                    // The question is gone: the note is about the block now.
                    _ => s.open = true,
                },
            }
        }
        s
    }

    fn item(&self, id: &str) -> bool {
        self.open || self.items.contains(id)
    }

    fn title(&self) -> bool {
        self.open || self.title
    }

    fn instructions(&self) -> bool {
        self.open || self.instructions
    }

    /// The scaffolds, the word table and the Glossary change only when the whole block is open.
    fn extras(&self) -> bool {
        self.open
    }

    /// What the model is told it may change (`codes`: item id → code).
    pub fn describe(&self, block: &Block) -> String {
        if self.open {
            return "整個區塊（標題、作答指示、每一題、程序鷹架與語言鷹架、字詞表和 Glossary 都可以改或刪除，也可以新增題目、鷹架或字詞）".to_string();
        }
        let mut parts: Vec<String> = Vec::new();
        if self.title {
            parts.push("標題".to_string());
        }
        if self.instructions {
            parts.push("作答指示".to_string());
        }
        for (k, item) in block.items.iter().enumerate() {
            if self.items.contains(&item.id) {
                parts.push(code(k));
            }
        }
        format!(
            "只能改 {}（可以新增題目）；其他題一律 keep: true{}；鷹架、字詞表和 Glossary 不能動（scaffolds、words、glossary 都給空陣列）",
            parts.join("、"),
            if self.title || self.instructions {
                ""
            } else {
                "，標題與作答指示寫空字串"
            }
        )
    }
}

// ---------------------------------------------------------------- merge

pub struct MergeCtx<'a> {
    pub old: &'a Block,
    pub scope: &'a Scope,
    pub section: &'a SectionCtx<'a>,
    /// The worksheet's versions (a new question appears in all of them; a 差異化學習單 has every
    /// question in all three).
    pub levels: &'a [Level],
    /// A block made for one Bloom level keeps every question at that level.
    pub block_bloom: Option<Bloom>,
    /// Question ids used anywhere in the worksheet.
    pub taken: &'a HashSet<String>,
    /// Unique per proposal, e.g. `b3-p17`; new question ids are `<prefix>n<k>`.
    pub new_prefix: &'a str,
    /// The question's number on the student sheet (id → no), for what the teacher is told.
    pub printed: &'a HashMap<String, usize>,
    pub max_items: usize,
    /// The word list, for the level of a word added to the word table (the model never writes it).
    pub lexicon: Option<&'a Lexicon>,
    /// The material, for the words that may be added to the word table.
    pub paragraphs: &'a [Paragraph],
}

#[derive(Debug, Clone, PartialEq)]
pub struct Merged {
    pub block: Block,
    pub reply: String,
    pub changed: bool,
    /// Changes left out because they failed the checks (「第 5 題的修改」), as the teacher is told.
    pub failed: Vec<String>,
    /// The reply said the block has no X while it has (the reply is dropped, and it is a problem
    /// for the retry).
    pub false_claim: bool,
}

enum Entry {
    Keep(usize),
    /// (question, draft index)
    Modify(usize, usize),
    New(usize),
}

fn raw_out(it: &RawRevisedItem) -> RawOutItem {
    RawOutItem {
        answer_format: it.answer_format.clone(),
        prompt: it.prompt.clone(),
        options: it.options.clone(),
        answer: it.answer.clone(),
        bloom: Some(it.bloom.clone()),
        organizer: None,
        table: None,
    }
}

fn same_question(a: &Item, b: &Item) -> bool {
    a.answer_format == b.answer_format
        && a.prompt.trim() == b.prompt.trim()
        && a.options == b.options
        && a.answer.trim() == b.answer.trim()
        && a.bloom == b.bloom
}

/// The stored question with what the model changed: format, prompt, options and Bloom level;
/// the answer (with its evidence) only when it changed. Id, versions, hint and purpose stay. A
/// 差異化學習單 question keeps its versions' support and answers only while its prompt is
/// unchanged: a cloze cut from the old answer would not fit a new question.
fn modified(old: &Item, c: &CleanSectionItem, block_bloom: Option<Bloom>) -> Item {
    let mut item = old.clone();
    item.answer_format = Some(c.answer_format.clone());
    item.prompt = c.prompt.clone();
    item.options = c.options.clone();
    if c.answer.trim() != old.answer.trim() {
        item.answer = c.answer.clone();
        item.evidence = c.evidence.clone().unwrap_or_default();
    }
    item.bloom = block_bloom.or(c.bloom).or(old.bloom);
    if same_question(&item, old) {
        return old.clone();
    }
    if item.prompt.trim() != old.prompt.trim() {
        // A new prompt: the old versions' support and the old table (a new prompt names its own
        // columns) no longer fit.
        item.tiers.clear();
        item.table = None;
    }
    // The revision's schema has no organizer: the stored one stays.
    if c.organizer.is_some() {
        item.organizer = c.organizer.clone();
    }
    item.origin = Origin::Ai;
    item
}

/// A question id that is not used anywhere in the worksheet.
fn fresh_id(prefix: &str, k: usize, taken: &HashSet<String>, made: &HashSet<String>) -> String {
    let base = format!("{prefix}n{k}");
    let mut id = base.clone();
    let mut extra = 1;
    while taken.contains(&id) || made.contains(&id) {
        id = format!("{base}x{extra}");
        extra += 1;
    }
    id.chars().take(MAX_ID_CHARS).collect()
}

// ---------------------------------------------------------------- scaffolds, word table, Glossary

/// What the scaffolds, the word table and the Glossary became.
struct ExtrasOut {
    scaffolds: Vec<Scaffold>,
    vocabulary: Vec<Vocab>,
    glosses: Vec<Gloss>,
    /// Changes left out because they failed the checks, as the teacher is told.
    failed: Vec<String>,
    /// What to tell the model when it tries again.
    problems: Vec<String>,
}

/// The first entry for each stored code (a code used twice counts once), and the entries for
/// new ones.
fn by_source<T>(
    entries: &[T],
    source: impl Fn(&T) -> &str,
    letter: char,
    n: usize,
) -> (Vec<Option<&T>>, Vec<&T>) {
    let mut stored: Vec<Option<&T>> = vec![None; n];
    let mut new = Vec::new();
    for e in entries {
        match index_with(letter, source(e), n) {
            Some(i) if stored[i].is_none() => stored[i] = Some(e),
            Some(_) => {}
            None if source(e).trim() == NEW => new.push(e),
            None => {}
        }
    }
    (stored, new)
}

fn plain(s: &str, max: usize) -> String {
    clean_text(s, max).trim().to_string()
}

/// The stored scaffold with what the model wrote: a procedural one takes its steps, any other
/// its text; kind, versions and heading stay.
fn modified_scaffold(old: &Scaffold, e: &RawScaffoldEdit) -> Result<Scaffold, String> {
    let mut s = old.clone();
    if old.kind_type == ScaffoldType::Procedural {
        let steps: Vec<String> = e
            .steps
            .iter()
            .map(|t| plain(t, SCAFFOLD_STEP_CHARS))
            .filter(|t| !t.is_empty())
            .collect();
        if steps.is_empty() || steps.len() > MAX_STEPS {
            return Err(format!("步驟要有 1 到 {MAX_STEPS} 步"));
        }
        s.steps = steps;
    } else {
        let content = plain(&e.content, SCAFFOLD_CONTENT_CHARS);
        if content.is_empty() {
            return Err("鷹架的內容不可以是空的（要拿掉就用 remove）".to_string());
        }
        s.content = content;
    }
    if s == *old {
        return Ok(old.clone());
    }
    s.origin = Origin::Ai;
    Ok(s)
}

fn new_scaffold(e: &RawScaffoldEdit, id: String, levels: &[Level]) -> Result<Scaffold, String> {
    let kind = e.kind.trim();
    if !ADDABLE_KINDS.contains(&kind) {
        return Err(format!(
            "新增鷹架的 kind 只能是 {}",
            ADDABLE_KINDS.join("、")
        ));
    }
    let procedural = kind == KIND_STEPS;
    let mut s = Scaffold {
        id,
        kind_type: if procedural {
            ScaffoldType::Procedural
        } else {
            ScaffoldType::Verbal
        },
        kind: kind.to_string(),
        levels: levels.to_vec(),
        origin: Origin::Ai,
        ..Scaffold::default()
    };
    if procedural {
        s.steps = e
            .steps
            .iter()
            .map(|t| plain(t, SCAFFOLD_STEP_CHARS))
            .filter(|t| !t.is_empty())
            .collect();
        if s.steps.is_empty() || s.steps.len() > MAX_STEPS {
            return Err(format!("步驟要有 1 到 {MAX_STEPS} 步"));
        }
    } else {
        s.content = plain(&e.content, SCAFFOLD_CONTENT_CHARS);
        if s.content.is_empty() {
            return Err("新增鷹架的內容是空的".to_string());
        }
    }
    Ok(s)
}

fn merge_scaffolds(raw: &[RawScaffoldEdit], m: &MergeCtx<'_>, out: &mut ExtrasOut) {
    let old = &m.old.scaffolds;
    let (stored, new) = by_source(raw, |e| &e.source, 'S', old.len());
    for (i, s) in old.iter().enumerate() {
        let name = || format!("鷹架 {}", scaffold_code(i));
        match stored[i].map(|e| (e, e.action.trim())) {
            Some((_, "remove")) => {}
            Some((e, "change")) => match modified_scaffold(s, e) {
                Ok(next) => out.scaffolds.push(next),
                Err(why) => {
                    out.failed.push(format!("{}的修改", name()));
                    out.problems.push(format!("{}：{why}", name()));
                    out.scaffolds.push(s.clone());
                }
            },
            _ => out.scaffolds.push(s.clone()),
        }
    }
    let mut made: HashSet<String> = old.iter().map(|s| s.id.clone()).collect();
    let mut added = 0;
    for e in new.into_iter().filter(|e| e.action.trim() == "change") {
        let name = format!("新增的第 {} 個鷹架", added + 1);
        if added >= MAX_ADDED || out.scaffolds.len() >= MAX_LIST {
            out.failed.push(name);
            continue;
        }
        let mut id = format!("{}s{}", m.new_prefix, added + 1);
        let mut extra = 1;
        while made.contains(&id) {
            id = format!("{}s{}x{extra}", m.new_prefix, added + 1);
            extra += 1;
        }
        let id: String = id.chars().take(MAX_ID_CHARS).collect();
        match new_scaffold(e, id.clone(), m.levels) {
            Ok(s) => {
                made.insert(id);
                added += 1;
                out.scaffolds.push(s);
            }
            Err(why) => {
                out.failed.push(name.clone());
                out.problems.push(format!("{name}：{why}"));
            }
        }
    }
}

/// Everything in the block the students read, for telling whether a word is really in it.
fn block_text(b: &Block) -> String {
    let mut t = vec![b.title.clone(), b.instructions.clone()];
    for i in &b.items {
        t.push(i.prompt.clone());
        t.extend(i.options.iter().cloned());
    }
    for s in &b.scaffolds {
        t.push(s.content.clone());
        t.extend(s.steps.iter().cloned());
    }
    t.join("\n")
}

/// Whether `word` (a word or a short phrase) is in `text`: as written, or with a short ending
/// (signals, identified).
fn occurs_in(word: &str, text: &str) -> bool {
    let word = word.trim().to_lowercase();
    if word.is_empty() {
        return false;
    }
    let text = text.to_lowercase();
    if word.contains(' ') || word.contains('-') {
        return text.contains(&word);
    }
    let stem: String = word
        .chars()
        .take(word.chars().count().saturating_sub(2).max(3))
        .collect();
    text.split(|c: char| !(c.is_alphabetic() || c == '\'' || c == '’'))
        .any(|t| t == word || (word.chars().count() >= 5 && t.starts_with(&stem)))
}

/// The sentence of the paragraph that has the word, for the row's example.
fn sentence_with(word: &str, text: &str) -> String {
    text.split_inclusive(['.', '?', '!', '。'])
        .find(|s| occurs_in(word, s))
        .map(|s| crate::text::clip_chars(s.trim(), 300))
        .unwrap_or_default()
}

fn valid_word(w: &str) -> bool {
    !w.is_empty()
        && w.chars().count() <= WORD_CHARS
        && w.chars()
            .all(|c| c.is_ascii_alphabetic() || matches!(c, ' ' | '-' | '\'' | '’'))
}

fn merge_words(raw: &[RawWordEdit], m: &MergeCtx<'_>, out: &mut ExtrasOut) {
    let old = &m.old.vocabulary;
    let (stored, new) = by_source(raw, |e| &e.source, 'W', old.len());
    for (i, v) in old.iter().enumerate() {
        match stored[i].map(|e| (e, e.action.trim())) {
            Some((_, "remove")) => {}
            // The hint of an 指示用語 is fixed by the code: it stays whatever the model wrote.
            Some((e, "change")) if !v.task_word => {
                let meaning = plain(&e.meaning, MEANING_CHARS);
                if meaning.is_empty() {
                    out.failed.push(format!("字詞表 {} 的修改", word_code(i)));
                    out.problems.push(format!(
                        "字詞表 {}（{}）的解釋不可以是空的（要拿掉就用 remove）",
                        word_code(i),
                        v.word
                    ));
                    out.vocabulary.push(v.clone());
                } else if meaning == v.definition.trim() {
                    out.vocabulary.push(v.clone());
                } else {
                    let mut next = v.clone();
                    next.definition = meaning;
                    out.vocabulary.push(next);
                }
            }
            _ => out.vocabulary.push(v.clone()),
        }
    }
    let passage: String = m
        .paragraphs
        .iter()
        .map(|p| p.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let corpus = format!("{passage}\n{}", block_text(m.old));
    let mut added = 0;
    for e in new.into_iter().filter(|e| e.action.trim() == "change") {
        let word = e.word.trim().to_string();
        let name = format!("新增的字詞「{}」", crate::text::clip_chars(&word, 30));
        let meaning = plain(&e.meaning, MEANING_CHARS);
        let listed = out
            .vocabulary
            .iter()
            .any(|v| v.word.trim().eq_ignore_ascii_case(&word));
        let problem = if !valid_word(&word) {
            Some("要是英文字或片語")
        } else if meaning.is_empty() {
            Some("缺少解釋")
        } else if !occurs_in(&word, &corpus) {
            Some("這個字沒有出現在教材或這一區裡，字詞表只能列出教材裡真的有的字")
        } else if listed {
            Some("字詞表已經有這個字了")
        } else {
            None
        };
        if added >= MAX_ADDED || out.vocabulary.len() >= MAX_LIST {
            out.failed.push(name);
            continue;
        }
        if let Some(why) = problem {
            out.failed.push(name.clone());
            out.problems.push(format!("{name}：{why}"));
            continue;
        }
        added += 1;
        let at = m.paragraphs.iter().find(|p| occurs_in(&word, &p.text));
        out.vocabulary.push(Vocab {
            // The level is the word list's, never the model's.
            level: m
                .lexicon
                .and_then(|lex| vocabulary::word_level(lex, &word.to_lowercase()).level),
            context: at
                .map(|p| sentence_with(&word, &p.text))
                .unwrap_or_default(),
            paragraph_id: at.map(|p| p.id.clone()),
            word,
            definition: meaning,
            task_word: false,
        });
    }
}

fn merge_glosses(raw: &[RawGlossEdit], old: &[Gloss], out: &mut ExtrasOut) {
    let (stored, _) = by_source(raw, |e| &e.source, 'G', old.len());
    for (i, g) in old.iter().enumerate() {
        if !matches!(stored[i], Some(e) if e.action.trim() == "remove") {
            out.glosses.push(g.clone());
        }
    }
}

fn merge_extras(raw: &RawBlockRevision, m: &MergeCtx<'_>) -> ExtrasOut {
    let old = m.old;
    let mut out = ExtrasOut {
        scaffolds: Vec::new(),
        vocabulary: Vec::new(),
        glosses: Vec::new(),
        failed: Vec::new(),
        problems: Vec::new(),
    };
    if !m.scope.extras() {
        out.scaffolds = old.scaffolds.clone();
        out.vocabulary = old.vocabulary.clone();
        out.glosses = old.glosses.clone();
        return out;
    }
    merge_scaffolds(&raw.scaffolds, m, &mut out);
    merge_words(&raw.words, m, &mut out);
    merge_glosses(&raw.glossary, &old.glosses, &mut out);
    out
}

// ---------------------------------------------------------------- 「這一區沒有 X」

/// A part of the block the reply may talk about: what it is called to the teacher, whether the
/// block has it, and the words a reply uses for it.
struct Part {
    name: &'static str,
    present: bool,
    nouns: Vec<&'static str>,
}

fn parts_of(b: &Block) -> Vec<Part> {
    let kind = |k: &str| b.scaffolds.iter().any(|s| s.kind == k);
    let table_question = b
        .items
        .iter()
        .any(|i| i.answer_format.as_deref() == Some(FMT_TABLE));
    let mut words = vec![
        "單字表",
        "字詞表",
        "字彙表",
        "詞彙表",
        "生字表",
        "單字列表",
        "單字清單",
        "字詞列表",
        "word table",
        "word list",
        "vocabulary table",
        "vocabulary list",
    ];
    if !table_question {
        words.push("表格");
    }
    vec![
        Part {
            name: "字詞表",
            present: !b.vocabulary.is_empty() || kind(KIND_WORD_LIST),
            nouns: words,
        },
        Part {
            name: "Glossary",
            present: !b.glosses.is_empty(),
            nouns: vec!["glossary", "字義註解", "字義表", "字彙補充"],
        },
        Part {
            name: "程序鷹架（步驟）",
            present: b
                .scaffolds
                .iter()
                .any(|s| s.kind_type == ScaffoldType::Procedural),
            nouns: vec!["程序鷹架", "分步", "步驟"],
        },
        Part {
            name: "句型框架",
            present: kind(KIND_FRAMES),
            nouns: vec!["句型", "sentence frame", "句首"],
        },
        Part {
            name: "引導問題",
            present: kind(KIND_GUIDING),
            nouns: vec!["引導問題"],
        },
        Part {
            name: "作答示例",
            present: kind(KIND_EXAMPLE),
            nouns: vec!["作答示例", "示例", "範例"],
        },
        Part {
            name: "鷹架",
            present: !b.scaffolds.is_empty(),
            nouns: vec!["鷹架", "scaffold"],
        },
    ]
}

const NEGATORS: [&str; 9] = [
    "沒有",
    "没有",
    "找不到",
    "看不到",
    "沒看到",
    "不存在",
    "並無",
    "目前無",
    "未見",
];
/// Characters that make 「沒有改動鷹架」 a statement about what was done, not about what is there.
const ACTION_CHARS: &str = "改動修刪更換調變新增加換";
const STOPS: [char; 9] = ['。', '；', ';', '！', '!', '？', '?', '，', '\n'];

/// The parts of the block (`b`, as it will be) that the reply says are not there: a negation
/// right before the name of one the block has (「目前這一區沒有單字表格」). The teacher reads
/// the reply next to the block, so such a reply is wrong, not careful.
pub fn false_absences(reply: &str, b: &Block) -> Vec<&'static str> {
    let text = reply.to_lowercase();
    let mut said: Vec<&'static str> = Vec::new();
    for part in parts_of(b).into_iter().filter(|p| p.present) {
        let hit = NEGATORS.iter().any(|neg| {
            text.match_indices(neg).any(|(at, _)| {
                let rest = &text[at + neg.len()..];
                let rest: String = rest.chars().take_while(|c| !STOPS.contains(c)).collect();
                part.nouns.iter().any(|noun| {
                    rest.find(noun).is_some_and(|k| {
                        let gap = &rest[..k];
                        gap.chars().count() <= 6 && !gap.chars().any(|c| ACTION_CHARS.contains(c))
                    })
                })
            })
        }) || english_no(&text, &part.nouns);
        if hit {
            said.push(part.name);
        }
    }
    said
}

/// 「there is no word table」, 「no scaffolds」.
fn english_no(text: &str, nouns: &[&str]) -> bool {
    text.match_indices("no ").any(|(at, _)| {
        let word_start = !text[..at]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphabetic());
        let rest: String = text[at + 3..]
            .chars()
            .take_while(|c| !STOPS.contains(c) && *c != '.')
            .collect();
        word_start
            && nouns
                .iter()
                .any(|n| n.is_ascii() && rest.find(n).is_some_and(|k| k <= 12))
    })
}

/// Builds the revised block from the stored one and what the model wrote, and returns the
/// problems for the retry (the checks the changed questions failed). A change that fails the
/// checks is not taken: the question stays as it was (a new one is left out) and the reply
/// says so.
pub fn merge(raw: &RawBlockRevision, m: &MergeCtx<'_>) -> (Merged, Vec<String>) {
    let old = m.old;
    let n = old.items.len();
    let mut used = vec![false; n];
    let mut drafts: Vec<(String, RawOutItem)> = Vec::new();
    let mut entries: Vec<Entry> = Vec::new();
    let mut added = 0;
    for it in &raw.items {
        match index_of(&it.source, n).filter(|i| !used[*i]) {
            Some(i) => {
                used[i] = true;
                if it.keep || !m.scope.item(&old.items[i].id) {
                    entries.push(Entry::Keep(i));
                } else {
                    drafts.push((code(i), raw_out(it)));
                    entries.push(Entry::Modify(i, drafts.len() - 1));
                }
            }
            // A kept "new" question has nothing in it.
            None if it.keep => {}
            None => {
                added += 1;
                drafts.push((format!("新增的第 {added} 題"), raw_out(it)));
                entries.push(Entry::New(drafts.len() - 1));
            }
        }
    }
    if !m.scope.open {
        // The questions stay in their order; one the model left out comes back unless the
        // teacher's note was on it; a new one follows the question it came after.
        let mut after: Vec<Vec<usize>> = vec![Vec::new(); n + 1];
        let mut slot: Vec<Option<Entry>> = (0..n).map(|_| None).collect();
        let mut last = 0;
        for e in entries {
            match e {
                Entry::Keep(i) | Entry::Modify(i, _) => {
                    last = i + 1;
                    slot[i] = Some(e);
                }
                Entry::New(d) => after[last].push(d),
            }
        }
        let mut ordered: Vec<Entry> = after[0].iter().map(|d| Entry::New(*d)).collect();
        for (i, s) in slot.into_iter().enumerate() {
            match s {
                Some(e) => ordered.push(e),
                None if !m.scope.item(&old.items[i].id) => ordered.push(Entry::Keep(i)),
                None => {}
            }
            ordered.extend(after[i + 1].iter().map(|d| Entry::New(*d)));
        }
        entries = ordered;
    }

    let changed_text = |allowed: bool, new: &str, old: &str| -> Option<String> {
        let new = new.trim();
        (allowed && !new.is_empty() && new != old.trim()).then(|| new.to_string())
    };
    let title = changed_text(m.scope.title(), &raw.title, &old.title);
    let instructions = changed_text(m.scope.instructions(), &raw.instructions, &old.instructions);
    let draft = RevisionDraft {
        title: title.as_deref(),
        instructions: instructions.as_deref(),
        effective_title: title.as_deref().unwrap_or(&old.title),
        effective_instructions: instructions.as_deref().unwrap_or(&old.instructions),
        items: drafts.iter().map(|(name, it)| (name.clone(), it)).collect(),
    };
    let e = evaluate_revision(&draft, m.section);
    let clean = e
        .output
        .unwrap_or_else(|| super::super::checks::CleanRevision {
            title: None,
            instructions: None,
            items: vec![None; drafts.len()],
        });

    let name = |i: usize| match m.printed.get(&old.items[i].id) {
        Some(no) => format!("第 {no} 題"),
        None => format!("這一區的第 {} 題", i + 1),
    };
    let mut notes: Vec<String> = Vec::new();
    let mut block = old.clone();
    if let Some(t) = clean.title.filter(|t| !t.trim().is_empty()) {
        block.title = t;
    }
    if let Some(i) = clean.instructions.filter(|i| !i.trim().is_empty()) {
        block.instructions = i;
    }
    let mut made: HashSet<String> = HashSet::new();
    let mut items: Vec<Item> = Vec::new();
    let mut dropped_new = 0;
    for e in entries {
        match e {
            Entry::Keep(i) => items.push(old.items[i].clone()),
            Entry::Modify(i, d) => match &clean.items[d] {
                Some(c) => items.push(modified(&old.items[i], c, m.block_bloom)),
                None => {
                    notes.push(format!("{}的修改", name(i)));
                    items.push(old.items[i].clone());
                }
            },
            Entry::New(d) => match &clean.items[d] {
                Some(c) => {
                    let bloom = m.block_bloom.or(c.bloom);
                    let id = fresh_id(m.new_prefix, made.len() + 1, m.taken, &made);
                    made.insert(id.clone());
                    items.push(Item {
                        id,
                        answer_format: Some(c.answer_format.clone()),
                        prompt: c.prompt.clone(),
                        options: c.options.clone(),
                        answer: c.answer.clone(),
                        evidence: c.evidence.clone().unwrap_or_default(),
                        purpose: None,
                        levels: m.levels.to_vec(),
                        origin: Origin::Ai,
                        bloom,
                        hint: String::new(),
                        tiers: Vec::new(),
                        table: c.table.as_ref().map(|t| t.to_item_table()),
                        kept_options: Vec::new(),
                        organizer: c.organizer.clone(),
                    });
                }
                None => dropped_new += 1,
            },
        }
    }
    if dropped_new > 0 {
        notes.push(format!("{dropped_new} 題新增的題目"));
    }
    if items.len() > m.max_items {
        items.truncate(m.max_items);
    }
    block.items = items;
    if old.items.is_empty()
        && !block.items.is_empty()
        && old.message.as_deref() != Some(MSG_EXTERNAL)
    {
        block.message = None;
    }
    let extras = merge_extras(raw, m);
    block.scaffolds = extras.scaffolds;
    block.vocabulary = extras.vocabulary;
    // Taking every row out keeps them, so 「恢復字詞表」 can bring the table back.
    if block.vocabulary.is_empty() && !old.vocabulary.is_empty() {
        block.vocabulary_removed = old.vocabulary.clone();
    }
    block.glosses = extras.glosses;
    notes.extend(extras.failed);
    let mut problems = e.problems;
    problems.extend(extras.problems);
    let changed = block.title != old.title
        || block.instructions != old.instructions
        || block.items != old.items
        || block.scaffolds != old.scaffolds
        || block.vocabulary != old.vocabulary
        || block.glosses != old.glosses;
    let mut reply = clean_text(&raw.reply, REPLY_CHARS);
    // A reply that says the block has no X while it has X (the block as it will be) is wrong:
    // the model is told once, and the teacher never reads it.
    let absent = false_absences(&reply, &block);
    let false_claim = !absent.is_empty();
    if false_claim {
        problems.push(format!(
            "回覆說這一區沒有{}，但這一區有（內容裡有列出代號）。請對照「這一區目前的內容」，依教師的要求處理它（要拿掉就在 scaffolds／words／glossary 寫 action: remove），不要說它不存在。",
            absent.join("、")
        ));
        reply.clear();
    }
    if !notes.is_empty() && changed {
        reply = format!(
            "{}（{}沒有通過學習單的檢查，維持原樣。）",
            reply.trim(),
            notes.join("、")
        );
    }
    (
        Merged {
            block,
            reply: reply.trim().to_string(),
            changed,
            failed: notes,
            false_claim,
        },
        problems,
    )
}

// ---------------------------------------------------------------- what changed (for the preview)

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ItemChange {
    pub id: String,
    /// `kept` | `modified` | `new` | `removed` (a removed one is in `before`)
    pub change: &'static str,
}

/// A scaffold, a row of the word table or a Glossary entry the revision changes (the ones it
/// keeps are not listed).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExtraChange {
    /// `scaffold` | `word` | `gloss`
    pub group: &'static str,
    /// The scaffold's id; the word of a row or an entry (a word is listed once).
    pub id: String,
    /// What the teacher knows it by: the scaffold's kind, or the word.
    pub label: String,
    /// `modified` | `new` | `removed`
    pub change: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Changes {
    pub title: bool,
    pub instructions: bool,
    /// In the order of the revised block, removed questions where they were.
    pub items: Vec<ItemChange>,
    /// The scaffolds, the word table and the Glossary: what is changed, added or removed.
    #[serde(default)]
    pub extras: Vec<ExtraChange>,
}

/// What differs between two lists of things with a key: changed and removed ones (in the old
/// order), then new ones (in the new order).
fn diff_by<T: PartialEq>(
    before: &[T],
    after: &[T],
    key: impl Fn(&T) -> String,
    group: &'static str,
    label: impl Fn(&T) -> String,
) -> Vec<ExtraChange> {
    let mut out = Vec::new();
    for b in before {
        let k = key(b);
        let change = match after.iter().find(|a| key(a) == k) {
            None => "removed",
            Some(a) if a != b => "modified",
            Some(_) => continue,
        };
        out.push(ExtraChange {
            group,
            id: k,
            label: label(b),
            change,
        });
    }
    for a in after {
        let k = key(a);
        if !before.iter().any(|b| key(b) == k) {
            out.push(ExtraChange {
                group,
                id: k,
                label: label(a),
                change: "new",
            });
        }
    }
    out
}

fn extra_changes(before: &Block, after: &Block) -> Vec<ExtraChange> {
    let mut out = diff_by(
        &before.scaffolds,
        &after.scaffolds,
        |s| s.id.clone(),
        "scaffold",
        |s| s.kind.clone(),
    );
    out.extend(diff_by(
        &before.vocabulary,
        &after.vocabulary,
        |v| v.word.trim().to_lowercase(),
        "word",
        |v| v.word.clone(),
    ));
    out.extend(diff_by(
        &before.glosses,
        &after.glosses,
        |g| g.word.trim().to_lowercase(),
        "gloss",
        |g| g.word.clone(),
    ));
    out
}

pub fn changes(before: &Block, after: &Block) -> Changes {
    let pos = |id: &str| before.items.iter().position(|i| i.id == id);
    let mut items: Vec<ItemChange> = after
        .items
        .iter()
        .map(|i| ItemChange {
            id: i.id.clone(),
            change: match before.items.iter().find(|b| b.id == i.id) {
                None => "new",
                Some(b) if b == i => "kept",
                Some(_) => "modified",
            },
        })
        .collect();
    for (r, b) in before.items.iter().enumerate() {
        if after.items.iter().any(|i| i.id == b.id) {
            continue;
        }
        // After the last question that came before it, and the new ones that follow that one.
        let mut at = items
            .iter()
            .rposition(|c| pos(&c.id).is_some_and(|p| p < r))
            .map_or(0, |p| p + 1);
        while items.get(at).is_some_and(|c| c.change == "new") {
            at += 1;
        }
        items.insert(
            at,
            ItemChange {
                id: b.id.clone(),
                change: "removed",
            },
        );
    }
    Changes {
        title: before.title != after.title,
        instructions: before.instructions != after.instructions,
        items,
        extras: extra_changes(before, after),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::materials::annotations::AnchorStatus;
    use crate::worksheet::comments::STATUS_OPEN;
    use crate::worksheet::comments::{CommentAnchor, Field};
    use chrono::Utc;

    fn item(id: &str, prompt: &str, answer: &str) -> Item {
        Item {
            id: id.into(),
            answer_format: Some("簡答題".into()),
            prompt: prompt.into(),
            answer: answer.into(),
            levels: vec![Level::Standard],
            origin: Origin::Teacher,
            hint: format!("hint {id}"),
            ..Item::default()
        }
    }

    fn block() -> Block {
        Block {
            id: "b2".into(),
            title: "Find the facts".into(),
            instructions: "Answer in English.".into(),
            items: vec![
                item(
                    "b2-i1",
                    "Why did Mrs. Lin put out the stand?",
                    "To help people.",
                ),
                item("b2-i2", "What did the neighbors think?", "It would fail."),
                item(
                    "b2-i3",
                    "What happened in the first month?",
                    "Umbrellas disappeared.",
                ),
            ],
            teacher_notes: "keep me".into(),
            ..Block::default()
        }
    }

    fn keep(k: usize) -> RawRevisedItem {
        RawRevisedItem {
            source: code(k),
            keep: true,
            answer_format: "簡答題".into(),
            prompt: String::new(),
            options: Vec::new(),
            answer: String::new(),
            bloom: "remember".into(),
        }
    }

    fn change(source: &str, prompt: &str) -> RawRevisedItem {
        RawRevisedItem {
            source: source.into(),
            keep: false,
            answer_format: "簡答題".into(),
            prompt: prompt.into(),
            options: Vec::new(),
            answer: "Answers will vary.".into(),
            bloom: "understand".into(),
        }
    }

    fn run(raw: &RawBlockRevision, scope: &Scope) -> (Merged, Vec<String>) {
        run_on(&block(), raw, scope)
    }

    fn run_on(old: &Block, raw: &RawBlockRevision, scope: &Scope) -> (Merged, Vec<String>) {
        let passage = "Mrs. Lin put an umbrella stand outside her shop.";
        let section = SectionCtx {
            passage,
            item_count: 12,
            needs_external: false,
            tables_asked: false,
        };
        let taken: HashSet<String> = old.items.iter().map(|i| i.id.clone()).collect();
        let printed = HashMap::from([("b2-i2".to_string(), 5)]);
        merge(
            raw,
            &MergeCtx {
                old,
                scope,
                section: &section,
                levels: &[Level::Standard],
                block_bloom: None,
                taken: &taken,
                new_prefix: "b2-p7",
                printed: &printed,
                max_items: 12,
                lexicon: None,
                paragraphs: &[],
            },
        )
    }

    fn note_on(id: &str) -> Comment {
        Comment {
            id: 1,
            worksheet_id: 1,
            block_id: "b2".into(),
            field: Field::Item,
            item_id: Some(id.into()),
            quote: String::new(),
            note: "太難".into(),
            share_with_ai: true,
            status: STATUS_OPEN.into(),
            proposal_id: None,
            anchor: CommentAnchor {
                status: AnchorStatus::Ok,
                block_id: Some("b2".into()),
                item_id: Some(id.into()),
                start: None,
                end: None,
            },
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn kept_questions_are_the_stored_ones_and_a_change_keeps_what_the_model_does_not_write() {
        let raw = RawBlockRevision {
            title: "".into(),
            instructions: "".into(),
            items: vec![
                keep(0),
                change("Q2", "Did the neighbors like the idea?"),
                keep(2),
            ],
            picture_actions: vec![],
            scaffolds: vec![],
            words: vec![],
            glossary: vec![],
            reply: "已把第 5 題改簡單。".into(),
        };
        let (m, problems) = run(&raw, &Scope::from("改簡單", &[], false));
        assert!(problems.is_empty(), "{problems:?}");
        assert!(m.changed);
        let old = block();
        assert_eq!(m.block.items[0], old.items[0], "byte for byte");
        assert_eq!(m.block.items[2], old.items[2]);
        let q2 = &m.block.items[1];
        assert_eq!(q2.id, "b2-i2", "same id");
        assert_eq!(q2.prompt, "Did the neighbors like the idea?");
        assert_eq!(q2.hint, "hint b2-i2", "the hint stays");
        assert_eq!(q2.origin, Origin::Ai);
        assert_eq!(
            m.block.teacher_notes, "keep me",
            "fields it never sees stay"
        );
        assert_eq!(m.block.title, old.title);
        let c = changes(&old, &m.block);
        assert_eq!(
            c.items.iter().map(|x| x.change).collect::<Vec<_>>(),
            ["kept", "modified", "kept"]
        );
    }

    #[test]
    fn a_note_on_one_question_keeps_the_others_whatever_the_model_wrote() {
        let raw = RawBlockRevision {
            title: "New title".into(),
            instructions: "".into(),
            // Q3 first, Q1 changed, Q2 changed, Q3 left out entirely, one new question.
            items: vec![
                change("Q1", "Touched one?"),
                change("Q2", "Did the neighbors like the idea?"),
                change("new", "What would you do?"),
            ],
            picture_actions: vec![],
            scaffolds: vec![],
            words: vec![],
            glossary: vec![],
            reply: "ok".into(),
        };
        let scope = Scope::from("", &[note_on("b2-i2")], false);
        let (m, _) = run(&raw, &scope);
        let old = block();
        assert_eq!(m.block.title, old.title, "only Q2 may change");
        assert_eq!(m.block.items[0], old.items[0]);
        assert_eq!(m.block.items[1].prompt, "Did the neighbors like the idea?");
        assert_eq!(
            m.block.items[2].prompt, "What would you do?",
            "a new one follows Q2"
        );
        assert_eq!(m.block.items[2].id, "b2-p7n1");
        assert_eq!(
            m.block.items[3], old.items[2],
            "left out but not in scope: kept"
        );
        // Opened up, a question left out is removed.
        let (m, _) = run(&raw, &Scope::from("", &[note_on("b2-i2")], true));
        assert_eq!(m.block.items.len(), 3);
        assert_eq!(m.block.title, "New title");
        let c = changes(&old, &m.block);
        assert_eq!(
            c.items
                .iter()
                .map(|x| (x.id.as_str(), x.change))
                .collect::<Vec<_>>(),
            [
                ("b2-i1", "modified"),
                ("b2-i2", "modified"),
                ("b2-p7n1", "new"),
                ("b2-i3", "removed")
            ]
        );
    }

    #[test]
    fn a_change_that_fails_the_checks_keeps_the_question_and_is_retried() {
        let mut bad = change("Q2", "Did the neighbors like it?");
        bad.answer_format = "連連看".into();
        let raw = RawBlockRevision {
            title: "".into(),
            instructions: "".into(),
            items: vec![keep(0), bad, keep(2)],
            picture_actions: vec![],
            scaffolds: vec![],
            words: vec![],
            glossary: vec![],
            reply: "已修改。".into(),
        };
        let (m, problems) = run(&raw, &Scope::from("x", &[], false));
        assert!(!problems.is_empty());
        assert!(!m.changed);
        assert_eq!(m.block, block());
        assert_eq!(m.failed, ["第 5 題的修改"]);
        // A copy that changes nothing is not a change either.
        let same = RawRevisedItem {
            source: "Q1".into(),
            keep: false,
            answer_format: "簡答題".into(),
            prompt: " Why did Mrs. Lin put out the stand? ".into(),
            options: Vec::new(),
            answer: "To help people.".into(),
            bloom: "remember".into(),
        };
        let mut b = block();
        b.items[0].bloom = Some(Bloom::Remember);
        let raw = RawBlockRevision {
            title: "".into(),
            instructions: "".into(),
            items: vec![same, keep(1), keep(2)],
            picture_actions: vec![],
            scaffolds: vec![],
            words: vec![],
            glossary: vec![],
            reply: "".into(),
        };
        let (m, _) = run_on(&b, &raw, &Scope::from("x", &[], false));
        assert_eq!(m.block.items[0], b.items[0]);
        assert!(!m.changed);
    }

    #[test]
    fn the_schema_writes_keep_before_the_question_and_the_reply_last() {
        let s = schema(3, 12, "b2", &[], 0, &Extras::default());
        let keys: Vec<&String> = s["properties"].as_object().unwrap().keys().collect();
        assert_eq!(
            keys,
            [
                "title",
                "instructions",
                "items",
                "scaffolds",
                "words",
                "glossary",
                "picture_actions",
                "reply"
            ]
        );
        let item = &s["properties"]["items"]["items"];
        let keys: Vec<&String> = item["properties"].as_object().unwrap().keys().collect();
        assert_eq!(&keys[..2], ["source", "keep"]);
        assert_eq!(
            item["properties"]["source"]["enum"],
            json!(["Q1", "Q2", "Q3", "new"])
        );
        assert_eq!(s["properties"]["items"]["maxItems"], 12);
        assert_eq!(s["properties"]["reply"]["maxLength"], REPLY_CHARS);
        assert_eq!(max_items(20), 26);
        assert!(max_tokens(12) >= 2500);
    }

    // ------------------------------------------------ scaffolds, word table, Glossary

    fn scaffold(id: &str, kind: &str, procedural: bool) -> Scaffold {
        Scaffold {
            id: id.into(),
            kind_type: if procedural {
                ScaffoldType::Procedural
            } else {
                ScaffoldType::Verbal
            },
            kind: kind.into(),
            content: if procedural {
                String::new()
            } else {
                "The first event is ___.".into()
            },
            steps: if procedural {
                vec!["Read.".into(), "Underline.".into()]
            } else {
                Vec::new()
            },
            levels: vec![Level::Standard],
            origin: Origin::Ai,
            ..Scaffold::default()
        }
    }

    fn rich_block() -> Block {
        Block {
            scaffolds: vec![
                scaffold("s1", KIND_STEPS, true),
                scaffold("s2", KIND_FRAMES, false),
            ],
            vocabulary: vec![
                Vocab {
                    word: "signal words".into(),
                    definition: "提示詞".into(),
                    task_word: true,
                    ..Vocab::default()
                },
                Vocab {
                    word: "umbrella".into(),
                    level: Some(2),
                    definition: "雨傘".into(),
                    ..Vocab::default()
                },
            ],
            glosses: vec![Gloss {
                word: "abandon".into(),
                meaning_zh: "放棄".into(),
                levels: vec![Level::Standard],
                ..Gloss::default()
            }],
            ..block()
        }
    }

    fn all_kept() -> Vec<RawRevisedItem> {
        vec![keep(0), keep(1), keep(2)]
    }

    fn scaffold_edit(source: &str, action: &str, content: &str, steps: &[&str]) -> RawScaffoldEdit {
        RawScaffoldEdit {
            source: source.into(),
            action: action.into(),
            kind: KIND_FRAMES.into(),
            content: content.into(),
            steps: steps.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn word_edit(source: &str, action: &str, word: &str, meaning: &str) -> RawWordEdit {
        RawWordEdit {
            source: source.into(),
            action: action.into(),
            word: word.into(),
            meaning: meaning.into(),
        }
    }

    fn lex() -> Lexicon {
        use crate::vocabulary::LexiconRow;
        Lexicon::new(
            "test",
            [LexiconRow {
                word: "umbrella".into(),
                level: Some(3),
                pos: "n.".into(),
                kind: "word".into(),
                appendix_category: String::new(),
            }],
        )
    }

    fn run_extras(raw: &RawBlockRevision, scope: &Scope) -> (Merged, Vec<String>) {
        let old = rich_block();
        let paragraphs = [Paragraph {
            id: "p1".into(),
            text: "Mrs. Lin put an umbrella stand outside her shop. People used the umbrellas."
                .into(),
        }];
        let lexicon = lex();
        let section = SectionCtx {
            passage: &paragraphs[0].text,
            item_count: 12,
            needs_external: false,
            tables_asked: false,
        };
        let taken: HashSet<String> = old.items.iter().map(|i| i.id.clone()).collect();
        merge(
            raw,
            &MergeCtx {
                old: &old,
                scope,
                section: &section,
                levels: &[Level::Standard],
                block_bloom: None,
                taken: &taken,
                new_prefix: "b2-p7",
                printed: &HashMap::new(),
                max_items: 12,
                lexicon: Some(&lexicon),
                paragraphs: &paragraphs,
            },
        )
    }

    fn open() -> Scope {
        Scope::from("改", &[], false)
    }

    fn raw_with(
        scaffolds: Vec<RawScaffoldEdit>,
        words: Vec<RawWordEdit>,
        glossary: Vec<RawGlossEdit>,
        reply: &str,
    ) -> RawBlockRevision {
        RawBlockRevision {
            title: String::new(),
            instructions: String::new(),
            items: all_kept(),
            scaffolds,
            words,
            glossary,
            picture_actions: vec![],
            reply: reply.into(),
        }
    }

    #[test]
    fn removing_every_row_removes_the_word_table_and_nothing_else() {
        let raw = raw_with(
            vec![],
            vec![
                word_edit("W1", "remove", "", ""),
                word_edit("W2", "remove", "", ""),
            ],
            vec![],
            "已刪除字詞表。",
        );
        let (m, problems) = run_extras(&raw, &open());
        assert!(problems.is_empty(), "{problems:?}");
        assert!(m.changed);
        assert!(m.block.vocabulary.is_empty());
        let old = rich_block();
        assert_eq!(
            m.block.vocabulary_removed, old.vocabulary,
            "the rows are kept so 恢復字詞表 can bring them back"
        );
        assert_eq!(m.block.scaffolds, old.scaffolds);
        assert_eq!(m.block.glosses, old.glosses);
        assert_eq!(m.block.items, old.items);
        let c = changes(&old, &m.block);
        assert_eq!(
            c.extras
                .iter()
                .map(|e| (e.group, e.label.as_str(), e.change))
                .collect::<Vec<_>>(),
            [
                ("word", "signal words", "removed"),
                ("word", "umbrella", "removed")
            ]
        );
    }

    #[test]
    fn what_the_model_does_not_list_stays_and_a_closed_scope_keeps_everything() {
        let raw = raw_with(vec![], vec![], vec![], "沒有要改的。");
        let (m, _) = run_extras(&raw, &open());
        assert!(!m.changed);
        assert_eq!(m.block, rich_block());
        // Notes on one question: the model's removals are not taken.
        let raw = raw_with(
            vec![scaffold_edit("S1", "remove", "", &[])],
            vec![word_edit("W2", "remove", "", "")],
            vec![RawGlossEdit {
                source: "G1".into(),
                action: "remove".into(),
            }],
            "ok",
        );
        let (m, _) = run_extras(&raw, &Scope::from("", &[note_on("b2-i2")], false));
        assert_eq!(m.block, rich_block());
        assert!(!m.changed);
    }

    #[test]
    fn a_sentence_frame_and_steps_are_edited_in_place() {
        let raw = raw_with(
            vec![
                scaffold_edit("S1", "change", "", &["Read twice.", " ", "Underline."]),
                scaffold_edit("S2", "change", "The opening event is ___.", &[]),
            ],
            vec![],
            vec![],
            "已改。",
        );
        let (m, problems) = run_extras(&raw, &open());
        assert!(problems.is_empty(), "{problems:?}");
        let s = &m.block.scaffolds;
        assert_eq!(s[0].steps, ["Read twice.", "Underline."], "blank steps go");
        assert_eq!(s[0].id, "s1");
        assert_eq!(s[1].content, "The opening event is ___.");
        assert_eq!((s[1].kind.as_str(), s[1].origin), (KIND_FRAMES, Origin::Ai));
        assert_eq!(s[1].levels, [Level::Standard]);
        // A copy that changes nothing is no change; an empty text is refused and retried.
        let same = raw_with(
            vec![scaffold_edit(
                "S2",
                "change",
                "The first event is ___.",
                &[],
            )],
            vec![],
            vec![],
            "",
        );
        assert!(!run_extras(&same, &open()).0.changed);
        let empty = raw_with(
            vec![
                scaffold_edit("S2", "change", "  ", &[]),
                scaffold_edit("S1", "change", "", &[]),
            ],
            vec![],
            vec![],
            "",
        );
        let (m, problems) = run_extras(&empty, &open());
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert_eq!(m.block, rich_block());
        assert_eq!(m.failed, ["鷹架 S1的修改", "鷹架 S2的修改"]);
    }

    #[test]
    fn a_new_scaffold_has_a_known_kind_and_the_worksheets_versions() {
        let raw = raw_with(
            vec![
                scaffold_edit("new", "change", "I think ___ because ___.", &[]),
                RawScaffoldEdit {
                    kind: "不存在的種類".into(),
                    ..scaffold_edit("new", "change", "x", &[])
                },
            ],
            vec![],
            vec![],
            "加了。",
        );
        let (m, problems) = run_extras(&raw, &open());
        assert_eq!(m.block.scaffolds.len(), 3);
        let added = &m.block.scaffolds[2];
        assert_eq!(added.id, "b2-p7s1");
        assert_eq!(added.kind_type, ScaffoldType::Verbal);
        assert_eq!(added.levels, [Level::Standard]);
        assert_eq!(added.origin, Origin::Ai);
        assert!(!added.auto);
        assert_eq!(
            problems.len(),
            1,
            "the unknown kind is sent back: {problems:?}"
        );
    }

    #[test]
    fn a_word_is_added_only_when_it_is_in_the_text_and_its_level_is_the_lists() {
        let raw = raw_with(
            vec![],
            vec![
                word_edit("new", "change", "stand", "攤位"),
                word_edit("new", "change", "zebra", "斑馬"),
                word_edit("new", "change", "Umbrella", "傘"),
                word_edit("new", "change", "shop", " "),
                word_edit("new", "change", "sh0p", "店"),
            ],
            vec![],
            "加了。",
        );
        let (m, problems) = run_extras(&raw, &open());
        let words: Vec<&str> = m.block.vocabulary.iter().map(|v| v.word.as_str()).collect();
        assert_eq!(words, ["signal words", "umbrella", "stand"]);
        let stand = &m.block.vocabulary[2];
        assert_eq!(stand.definition, "攤位");
        assert_eq!(stand.paragraph_id.as_deref(), Some("p1"));
        assert!(stand.context.contains("umbrella stand"));
        assert_eq!(
            stand.level, None,
            "not in the word list: unknown, not the model's guess"
        );
        assert!(!stand.task_word);
        assert_eq!(problems.len(), 4, "{problems:?}");
        // A word of the list gets the list's level.
        let raw = raw_with(
            vec![],
            vec![
                word_edit("W2", "remove", "", ""),
                word_edit("new", "change", "umbrellas", "雨傘（多個）"),
            ],
            vec![],
            "",
        );
        let (m, problems) = run_extras(&raw, &open());
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(m.block.vocabulary[1].word, "umbrellas");
        assert_eq!(m.block.vocabulary[1].level, Some(3));
    }

    #[test]
    fn the_hint_of_a_task_word_is_the_codes_and_a_meaning_may_be_changed_but_not_emptied() {
        let raw = raw_with(
            vec![],
            vec![
                word_edit("W1", "change", "", "我亂寫的"),
                word_edit("W2", "change", "", "有遮雨功能的傘"),
            ],
            vec![],
            "",
        );
        let (m, problems) = run_extras(&raw, &open());
        assert!(problems.is_empty());
        assert_eq!(m.block.vocabulary[0].definition, "提示詞");
        assert_eq!(m.block.vocabulary[1].definition, "有遮雨功能的傘");
        assert_eq!(m.block.vocabulary[1].level, Some(2), "level untouched");
        let raw = raw_with(vec![], vec![word_edit("W2", "change", "", "")], vec![], "");
        let (m, problems) = run_extras(&raw, &open());
        assert_eq!(problems.len(), 1);
        assert_eq!(m.block.vocabulary, rich_block().vocabulary);
    }

    #[test]
    fn the_glossary_can_only_lose_entries() {
        let raw = raw_with(
            vec![],
            vec![],
            vec![RawGlossEdit {
                source: "G1".into(),
                action: "remove".into(),
            }],
            "已刪除。",
        );
        let (m, _) = run_extras(&raw, &open());
        assert!(m.block.glosses.is_empty());
        let c = changes(&rich_block(), &m.block);
        assert_eq!(c.extras[0].group, "gloss");
        assert_eq!(c.extras[0].change, "removed");
    }

    #[test]
    fn a_reply_that_says_a_part_is_missing_while_it_is_there_is_wrong() {
        let b = rich_block();
        let says = |r: &str| false_absences(r, &b);
        assert_eq!(
            says("目前這一區沒有單字表格；請問您希望刪除或改成其他題型的是哪一題？"),
            ["字詞表"]
        );
        assert_eq!(says("這一區沒有字詞表。"), ["字詞表"]);
        assert_eq!(says("找不到任何鷹架。"), ["鷹架"]);
        assert_eq!(says("找不到句型框架，所以沒改。"), ["句型框架"]);
        assert_eq!(says("There is no word table in this block."), ["字詞表"]);
        assert_eq!(says("這一區沒有 Glossary。"), ["Glossary"]);
        // True statements, and statements about what was done, are not claims.
        assert!(says("已刪除單字表格。").is_empty());
        assert!(says("沒有改動鷹架，只刪除了字詞表。").is_empty());
        assert!(
            says("這一區沒有引導問題。").is_empty(),
            "it really has none"
        );
        assert!(says("沒有題目需要修改；請問要改哪一題？").is_empty());
        // A block that has none may say so.
        let bare = block();
        assert!(false_absences("這一區沒有單字表格。", &bare).is_empty());
    }

    #[test]
    fn a_false_claim_is_a_problem_and_the_reply_is_not_shown() {
        let raw = raw_with(
            vec![],
            vec![],
            vec![],
            "目前這一區沒有單字表格；請問您希望刪除哪一題？",
        );
        let (m, problems) = run_extras(&raw, &open());
        assert!(m.false_claim);
        assert!(m.reply.is_empty());
        assert!(!m.changed);
        assert!(
            problems
                .iter()
                .any(|p| p.contains("回覆說這一區沒有字詞表")),
            "{problems:?}"
        );
        // Once the table is gone the same sentence is no longer a claim about what is there.
        let raw = raw_with(
            vec![],
            vec![
                word_edit("W1", "remove", "", ""),
                word_edit("W2", "remove", "", ""),
            ],
            vec![],
            "已刪除，現在這一區沒有單字表格了。",
        );
        let (m, problems) = run_extras(&raw, &open());
        assert!(!m.false_claim, "{problems:?}");
        assert!(problems.is_empty());
        assert!(m.reply.contains("沒有單字表格"));
    }

    #[test]
    fn the_schema_has_the_three_arrays_with_codes_and_actions() {
        let extras = Extras::of(&rich_block());
        let s = schema(3, 12, "b2", &[], 0, &extras);
        let props = s["properties"].as_object().unwrap();
        let keys: Vec<&String> = props.keys().collect();
        assert_eq!(
            keys,
            [
                "title",
                "instructions",
                "items",
                "scaffolds",
                "words",
                "glossary",
                "picture_actions",
                "reply"
            ]
        );
        let sc = &props["scaffolds"]["items"]["properties"];
        assert_eq!(sc["source"]["enum"], json!(["S1", "S2", "new"]));
        assert_eq!(sc["action"]["enum"], json!(["keep", "change", "remove"]));
        assert!(sc["kind"]["enum"]
            .as_array()
            .unwrap()
            .contains(&json!(KIND_FRAMES)));
        assert_eq!(
            props["words"]["items"]["properties"]["source"]["enum"],
            json!(["W1", "W2", "new"])
        );
        let g = &props["glossary"];
        assert_eq!(g["items"]["properties"]["source"]["enum"], json!(["G1"]));
        assert_eq!(g["maxItems"], 1);
        // No Glossary: still a valid enum, and nothing may be listed.
        let none = schema(3, 12, "b2", &[], 0, &Extras::of(&block()));
        assert_eq!(none["properties"]["glossary"]["maxItems"], 0);
        assert!(extras.tokens() > Extras::default().tokens());
    }
}
