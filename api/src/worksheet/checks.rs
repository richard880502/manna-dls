//! Automatic checks of the four worksheet steps, ported from
//! research/sop-validation/chain/checks.py (docs/LLM_VALIDATION.md section 5).
//!
//! Every `evaluate_*` does two things at once: it reports which checks failed (a failure sends
//! the output back to the model once, with the problems listed), and it returns the output with
//! the offending parts taken out. After the repair call the cleaned output is what the teacher
//! gets, so one bad item never costs the whole step; only an output with nothing usable left is
//! blocked.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashSet};

use super::bloom::{Bloom, RawBloomCheck, RawBloomLevel, HINT_CHARS, REASON_CHARS};
use super::organizer::{self, Organizer, RawOrganizer};
use super::schemas::{
    RawBlock, RawExit, RawExitItem, RawItem, RawScaffoldSet, RawScaffolds, RawTasks, RawWarmup,
    TARGET_WARMUP, TIER_CHALLENGE, TIER_CORE, TIER_SUPPORT,
};
use super::table::{self, RawTable};
use super::template::{SupportRole, WsTemplateSupport};
use super::tiered::{RawLinkingWords, RawTieredLevel, RawVersion};
use super::{
    info_function_from_label, student_tasks, ItemTier, Level, Practice, ScaffoldType,
    ANSWER_FORMATS, EXIT_PURPOSES, FMT_BLANK, FMT_CHOICE, FMT_MATCHING, FMT_SHORT, FMT_TABLE,
    FMT_TRUE_FALSE, INSTRUCTIONAL_KINDS, KIND_FRAMES, KIND_STEPS, PROCEDURAL_KINDS, VERBAL_KINDS,
};
use crate::checks::CheckResult;
use crate::memory::OptionHabits;
use crate::sop::checks::{clean_markup, english_only, is_verbatim, locate_verbatim, norm};
use crate::syllables;
use crate::text::clip_chars;

pub struct Eval<T> {
    /// The output with everything that failed taken out; `None` when nothing usable is left.
    pub output: Option<T>,
    pub results: Vec<CheckResult>,
    /// What to tell the model in the repair call.
    pub problems: Vec<String>,
    /// What the code changed in `output` (logged in `check_results.repairs`).
    pub repairs: Vec<String>,
}

impl<T> Eval<T> {
    pub fn needs_repair(&self) -> bool {
        self.results.iter().any(|r| !r.passed)
    }
}

/// Collects failures per check, in the order the checks are declared.
struct Tally {
    names: Vec<&'static str>,
    failures: BTreeMap<&'static str, Vec<String>>,
    /// Problems the code fixed completely: reported, but no reason for a repair call.
    fixed: BTreeMap<&'static str, Vec<String>>,
    problems: Vec<String>,
    repairs: Vec<String>,
}

fn joined(list: &[String]) -> String {
    list.iter()
        .take(5)
        .map(|s| clip_chars(s, 160))
        .collect::<Vec<_>>()
        .join("；")
}

impl Tally {
    fn new(names: &[&'static str]) -> Self {
        Self {
            names: names.to_vec(),
            failures: BTreeMap::new(),
            fixed: BTreeMap::new(),
            problems: Vec::new(),
            repairs: Vec::new(),
        }
    }

    /// Records a failed check, what to tell the model and what the code does about it.
    fn fail(&mut self, name: &'static str, problem: String, repair: &str) {
        debug_assert!(self.names.contains(&name), "undeclared check {name}");
        self.failures.entry(name).or_default().push(problem.clone());
        if !repair.is_empty() {
            self.repairs
                .push(format!("{repair}：{}", clip_chars(&problem, 160)));
        }
        self.problems.push(problem);
    }

    /// Records a problem the code has fully repaired (`repair` says how). The check still
    /// passes: on its own it never costs a repair call, but if one is made for another reason
    /// the model is told about this too.
    fn fixed(&mut self, name: &'static str, problem: String, repair: &str) {
        debug_assert!(self.names.contains(&name), "undeclared check {name}");
        let note = format!("{repair}：{}", clip_chars(&problem, 160));
        self.fixed.entry(name).or_default().push(note.clone());
        self.repairs.push(note);
        self.problems.push(problem);
    }

    fn finish<T>(self, output: Option<T>) -> Eval<T> {
        let results = self
            .names
            .iter()
            .map(
                |name| match (self.failures.get(name), self.fixed.get(name)) {
                    (Some(list), _) => CheckResult::fail(name, joined(list)),
                    (None, Some(list)) => CheckResult::repaired(name, joined(list)),
                    (None, None) => CheckResult::pass(name),
                },
            )
            .collect();
        Eval {
            output,
            results,
            problems: self.problems,
            repairs: self.repairs,
        }
    }
}

fn word_count(s: &str) -> usize {
    s.split_whitespace().count()
}

fn clean(s: &str) -> String {
    clean_markup(s)
}

/// A question's prompt, cleaned, without a number of its own at the start: the sheet numbers
/// its questions ([`crate::worksheet::assemble::without_own_number`]).
fn clean_prompt(s: &str) -> String {
    crate::worksheet::assemble::without_own_number(&clean(s)).to_string()
}

fn clean_all(list: &[String]) -> Vec<String> {
    list.iter()
        .map(|s| clean(s))
        .filter(|s| !s.is_empty())
        .collect()
}

/// Shorter pieces of a spliced quote are fragments, not evidence.
pub const MIN_EVIDENCE_WORDS: usize = 5;

/// The longest piece of a quote that is word for word in `passage` and at least five words
/// long ([`MIN_EVIDENCE_WORDS`]). The quote is cut where the model joined pieces: at "..." / "…"
/// and at sentence ends.
pub fn verbatim_piece(quote: &str, passage: &str) -> Option<String> {
    let mut pieces: Vec<String> = Vec::new();
    for part in quote.replace('…', "...").split("...") {
        // Where sentences may end; every run of consecutive sentences is a candidate (so an
        // abbreviation such as "Mrs." does not cut a sentence in two).
        let mut cuts = vec![0];
        let chars: Vec<(usize, char)> = part.char_indices().collect();
        for (k, (i, c)) in chars.iter().enumerate() {
            let next_is_space = chars.get(k + 1).is_none_or(|(_, n)| n.is_whitespace());
            if matches!(c, '.' | '!' | '?' | ';') && next_is_space {
                cuts.push(i + c.len_utf8());
            }
        }
        if cuts.last() != Some(&part.len()) {
            cuts.push(part.len());
        }
        for a in 0..cuts.len() {
            for b in a + 1..cuts.len() {
                pieces.push(part[cuts[a]..cuts[b]].to_string());
            }
        }
    }
    pieces
        .into_iter()
        .map(|p| {
            p.trim()
                .trim_matches(|c: char| c == '"' || c == '\'' || c == '“' || c == '”')
                .trim()
                .to_string()
        })
        .filter(|p| word_count(p) >= MIN_EVIDENCE_WORDS)
        .filter_map(|p| locate_verbatim(&p, passage))
        .max_by_key(|p| p.chars().count())
}

/// An answer of four words or more that appears word for word in `text`.
pub(crate) fn contains_answer(text: &str, answer: &str) -> bool {
    let a = norm(answer).to_lowercase();
    word_count(&a) >= 4 && norm(text).to_lowercase().contains(&a)
}

// ---------------------------------------------------------------- (c) tasks

pub struct TasksCtx<'a> {
    pub paragraphs: &'a [String],
    pub objective_ids: &'a [String],
    pub item_budget: usize,
    /// A single-block regeneration: the paragraph numbers the one block has to cover.
    pub only: Option<&'a [i64]>,
    /// Option counts the teacher's confirmed habits allow besides the defaults (empty while
    /// the teacher-memory switch is off, which keeps the default rules exactly).
    pub habits: OptionHabits,
    /// The sheet's learning objectives ([`objective_lines`]): no question may copy one so that it
    /// gives the answer away ([`copied_objective`], [`CHECK_NOT_AN_OBJECTIVE`]).
    pub objectives: &'a [String],
}

const TASK_CHECKS: [&str; 11] = [
    "labels_from_spec_sets",
    "english_only",
    "evidence_verbatim",
    "items_wellformed",
    "organizer_wellformed",
    "answer_not_in_prompt",
    CHECK_NOT_AN_OBJECTIVE,
    "objective_refs_valid",
    "all_paragraphs_covered",
    "blocks_in_paragraph_order",
    "at_least_3_info_functions",
];
const TASK_CHECKS_WITH_BUDGET: [&str; 13] = [
    "labels_from_spec_sets",
    "english_only",
    "evidence_verbatim",
    "items_wellformed",
    "organizer_wellformed",
    "answer_not_in_prompt",
    CHECK_NOT_AN_OBJECTIVE,
    "objective_refs_valid",
    "all_paragraphs_covered",
    "blocks_in_paragraph_order",
    "at_least_3_info_functions",
    "item_count_within_budget",
    "practice_in_order",
];

/// SIOP Q4 on a whole design (blocks in their final order): when the stages do not go
/// 示範 → 共同練習 → 同儕練習 → 獨立應用 ([`Practice::in_order`]), a failure for the repair
/// call, and the blocks relabelled by position so that the output is usable as it is.
fn check_practice(blocks: &mut [RawBlock], t: &mut Tally) {
    let stages: Vec<Option<Practice>> = blocks
        .iter()
        .map(|b| b.practice.as_deref().and_then(Practice::from_label))
        .collect();
    if Practice::in_order(&stages) {
        return;
    }
    let written = blocks
        .iter()
        .map(|b| {
            format!(
                "{}={}",
                b.block_id.trim(),
                b.practice.as_deref().unwrap_or("（沒有寫）")
            )
        })
        .collect::<Vec<_>>()
        .join("、");
    t.fail(
        "practice_in_order",
        format!(
            "逐段任務的 practice 要依「示範 → 共同練習 → 同儕練習 → 獨立應用」排列：第一個區塊是示範，最後一個是獨立應用，中間不可倒退；四個以上區塊時四個階段都要有。這次是：{written}。"
        ),
        "依區塊順序重新標示 practice",
    );
    let n = blocks.len();
    for (i, b) in blocks.iter_mut().enumerate() {
        b.practice = Some(Practice::for_position(i, n).label().to_string());
    }
}

static OPTION_LETTER: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"^\s*(?:\(([A-Ha-h])\)|([A-Ha-h])[.)．、:])\s*(.*)$").expect("valid regex")
});

/// Puts a choice item in the form the checks expect, before checking it. The model often writes
/// options as 「A. …」 and the answer as "B" (real-model run: an exit ticket failed this way):
/// letters are stripped when every option carries them in order, and an answer that is a letter,
/// or an option with its letter, or an option in another case, becomes that option exactly.
pub(crate) fn normalize_choice(options: &mut [String], answer: &mut String) {
    let lettered: Vec<Option<(char, String)>> = options
        .iter()
        .map(|o| {
            OPTION_LETTER.captures(o).map(|c| {
                let l = c
                    .get(1)
                    .or(c.get(2))
                    .map_or('?', |m| m.as_str().chars().next().unwrap_or('?'));
                (l.to_ascii_uppercase(), c[3].trim().to_string())
            })
        })
        .collect();
    let in_order = !options.is_empty()
        && lettered.iter().enumerate().all(|(i, l)| {
            l.as_ref()
                .is_some_and(|(c, rest)| *c == (b'A' + i as u8) as char && !rest.is_empty())
        });
    if in_order {
        for (o, l) in options.iter_mut().zip(lettered) {
            if let Some((_, rest)) = l {
                *o = rest;
            }
        }
    }
    let a = answer.trim();
    if options.iter().any(|o| o.trim() == a) {
        return;
    }
    let bare = a.trim_matches(|c: char| c == '(' || c == ')' || c == '.' || c == ' ');
    if bare.len() == 1 {
        if let Some(c) = bare.chars().next().filter(char::is_ascii_alphabetic) {
            let idx = (c.to_ascii_uppercase() as u8).wrapping_sub(b'A') as usize;
            if let Some(o) = options.get(idx) {
                *answer = o.clone();
                return;
            }
        }
    }
    let text = OPTION_LETTER
        .captures(a)
        .map_or(a.to_string(), |c| c[3].trim().to_string());
    let key = |s: &str| norm(s).to_lowercase().trim_end_matches('.').to_string();
    if let Some(o) = options.iter().find(|o| key(o) == key(&text)) {
        *answer = o.clone();
    }
}

// ---------------------------------------------------------------- 填空題: a blank to write in

/// The blank the prompts ask a fill-in item to show where the answer goes.
pub const BLANK: &str = "______";

/// A place to write in a prompt: three underscores or more (「______」, what every prompt asks
/// for; full-width 「＿＿＿」 too) or empty brackets 「( )」「（　）」「（）」.
static BLANK_MARKER: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"_{3,}|＿{3,}|\([ \t\x{3000}]+\)|（[ \t\x{3000}]*）").expect("valid regex")
});

/// Whether a prompt has a blank for the student to write in.
pub(crate) fn has_blank_marker(prompt: &str) -> bool {
    BLANK_MARKER.is_match(prompt)
}

/// A number the model puts before an answer's parts: 「(1) 」「1. 」「2) 」.
static ANSWER_NUMBER: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"^(?:\(\d+\)|\d+[.)])\s*").expect("valid regex")
});

/// The phrases of a reference answer that could be blanked out: the parts between commas,
/// semicolons, slashes or line breaks, without numbering, quotes or end punctuation, three
/// characters or more, longest first (so "vivid image of the scene" goes before "vivid").
fn answer_phrases(answer: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for part in answer.split([',', ';', '/', '|', '\n', '，', '；', '、']) {
        let part = ANSWER_NUMBER.replace(part.trim(), "");
        let mut phrase = part
            .trim_matches(|c: char| c.is_whitespace() || "\"'“”‘’.!?。:：()（）".contains(c))
            .to_string();
        for lead in ["and ", "or "] {
            if phrase.to_lowercase().starts_with(lead) {
                phrase = phrase[lead.len()..].trim_start().to_string();
            }
        }
        if phrase.chars().count() >= 3 && !out.iter().any(|o| o.eq_ignore_ascii_case(&phrase)) {
            out.push(phrase);
        }
    }
    out.sort_by_key(|p| std::cmp::Reverse(p.chars().count()));
    out
}

/// What the code does with a 填空題 that still has no blank after the repair call.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum MissingBlank {
    /// The prompt with every phrase of the reference answer it contains (any case, whole words)
    /// replaced by 「______」.
    Blanked(String),
    /// No phrase of the answer is in the prompt (or blanking would leave nothing to read): the
    /// item becomes a 簡答題, which at least prints answer lines.
    ShortAnswer,
}

/// The deterministic fallback for a 填空題 without a blank (see [`MissingBlank`]).
pub(crate) fn missing_blank(prompt: &str, answer: &str) -> MissingBlank {
    let mut text = prompt.to_string();
    let mut blanked = false;
    for phrase in answer_phrases(answer) {
        let word = |c: Option<char>| c.is_some_and(char::is_alphanumeric);
        let body = phrase
            .split_whitespace()
            .map(regex::escape)
            .collect::<Vec<_>>()
            .join(r"\s+");
        let pattern = format!(
            "(?i){}{body}{}",
            if word(phrase.chars().next()) {
                r"\b"
            } else {
                ""
            },
            if word(phrase.chars().last()) {
                r"\b"
            } else {
                ""
            },
        );
        let Ok(re) = regex::Regex::new(&pattern) else {
            continue;
        };
        if re.is_match(&text) {
            text = re.replace_all(&text, BLANK).into_owned();
            blanked = true;
        }
    }
    let left = BLANK_MARKER
        .replace_all(&text, "")
        .chars()
        .filter(|c| c.is_alphabetic())
        .count();
    if blanked && left >= 3 {
        MissingBlank::Blanked(text)
    } else {
        MissingBlank::ShortAnswer
    }
}

/// A 填空題 must show a blank to write in. One without fails `items_wellformed` (the repair call
/// is told why) and is fixed in code: the reference answer's phrases are blanked out of the
/// prompt, or, with none to blank, the item becomes a 簡答題. `field` is the prompt's name in the
/// schema (prompt_en / prompt).
fn blank_check(
    format: &mut String,
    prompt: &mut String,
    answer: &str,
    iname: &str,
    field: &str,
    t: &mut Tally,
) {
    if format != FMT_BLANK || has_blank_marker(prompt) {
        return;
    }
    let problem = format!(
        "{iname} 是填空題，但 {field} 裡沒有空格，學生沒有地方寫答案：要把答案原本的位置換成 ______，答案的字詞不可以留在 {field} 裡（不要把完整的句子抄進題目、再另外列出答案）。"
    );
    match missing_blank(prompt, answer) {
        MissingBlank::Blanked(text) => {
            t.fail(
                "items_wellformed",
                problem,
                "把題目裡參考答案的字詞換成 ______",
            );
            *prompt = text;
        }
        MissingBlank::ShortAnswer => {
            t.fail(
                "items_wellformed",
                problem,
                "題目裡沒有參考答案的字詞可以挖空，改成簡答題（印作答線）",
            );
            *format = FMT_SHORT.to_string();
        }
    }
}

// ---------------------------------------------------------------- 句型框架 written as a question

/// Words a sentence starts with when it asks the student to do something (an instruction or a
/// question word): a prompt with such a sentence is a question, even when it carries a frame.
const INSTRUCTION_WORDS: [&str; 64] = [
    "add",
    "answer",
    "ask",
    "change",
    "check",
    "choose",
    "circle",
    "compare",
    "complete",
    "copy",
    "correct",
    "create",
    "decide",
    "describe",
    "design",
    "discuss",
    "draw",
    "explain",
    "fill",
    "find",
    "finish",
    "give",
    "highlight",
    "how",
    "identify",
    "imagine",
    "introduce",
    "label",
    "list",
    "look",
    "make",
    "mark",
    "match",
    "name",
    "note",
    "number",
    "order",
    "pick",
    "practice",
    "predict",
    "present",
    "put",
    "read",
    "report",
    "respond",
    "retell",
    "rewrite",
    "say",
    "select",
    "share",
    "show",
    "sort",
    "state",
    "summarize",
    "talk",
    "tell",
    "think",
    "underline",
    "use",
    "what",
    "when",
    "where",
    "why",
    "write",
];

/// Whether `prompt` is written in the student's voice ("I …", "my …", "we …").
fn first_person(prompt: &str) -> bool {
    prompt
        .split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '’'))
        .any(|w| {
            let w = w.replace('’', "'");
            matches!(w.as_str(), "I" | "I'm" | "I've" | "I'd" | "I'll")
                || matches!(
                    w.to_lowercase().as_str(),
                    "my" | "me" | "myself" | "we" | "our"
                )
        })
}

/// Whether a sentence of `prompt` starts with an instruction or a question word.
fn gives_instruction(prompt: &str) -> bool {
    prompt
        .split(['.', '!', '?', ':', ';', '\n', '：', '。'])
        .filter_map(|sentence| {
            sentence
                .split(|c: char| !c.is_alphabetic())
                .find(|w| !w.is_empty())
                .map(str::to_lowercase)
        })
        .any(|w| INSTRUCTION_WORDS.contains(&w.as_str()))
}

/// Whether the text between the blanks of `prompt` is quoted from the material (three words or
/// more word for word): a cloze of the text, not a frame written for the student.
fn quotes_material(prompt: &str, passage: &str) -> bool {
    let passage = norm(passage).to_lowercase();
    BLANK_MARKER.split(prompt).any(|piece| {
        let piece = piece.trim_matches(|c: char| !c.is_alphanumeric());
        word_count(piece) >= 3 && passage.contains(&norm(piece).to_lowercase())
    })
}

/// Whether a question is nothing but a sentence frame: a language scaffold in the student's voice
/// with blanks to complete (「I think ___ because ___.」, 「Hi! In the 3rd paragraph, the author
/// describes ______. … I identified ______ as …」), with no question or instruction of its own,
/// and not quoted from the material (a cloze of a first-person text is a real question). On the
/// sheet a frame supports a task; numbered as a question of its own it reads as one (SIOP Q2, T4).
pub(crate) fn sentence_frame_only(prompt: &str, passage: &str) -> bool {
    has_blank_marker(prompt)
        && !prompt.contains(['?', '？'])
        && first_person(prompt)
        && !gives_instruction(prompt)
        && !quotes_material(prompt, passage)
}

/// A greeting that opens a frame for sharing aloud: 「Hi!」「Hello,」「Hey」「Good morning」.
static GREETING: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(
        r"(?i)^(?:hi|hello|hey|greetings|good\s+(?:morning|afternoon|evening)|dear\s+(?:classmates|friends|everyone|all|class|teacher))\b",
    )
    .expect("valid regex")
});

/// The greeting-led fill-in frame in `prompt`, if it is one: a frame for sharing aloud
/// (「Hi! In Paragraph 2, the author describes ______. … I identified ______ as …」), at the start
/// of the prompt or wrapped in a task after a colon (「Write a short analysis paragraph using the
/// frame: Hi! …」, 「Complete the analysis paragraph: Hi! …」). The frame must have a blank and
/// not be quoted from the material (a cloze of a letter in the text is a real question). Returns
/// the frame without the wrapper. A frame like this is a support, never a numbered question.
pub(crate) fn greeting_frame(prompt: &str, passage: &str) -> Option<String> {
    let prompt = prompt.trim();
    if !has_blank_marker(prompt) {
        return None;
    }
    let starts = std::iter::once(0).chain(
        prompt
            .char_indices()
            .filter(|(_, c)| matches!(c, ':' | '：' | '\n'))
            .map(|(i, c)| i + c.len_utf8()),
    );
    for start in starts {
        let rest = prompt[start..]
            .trim_start()
            .trim_start_matches(['"', '“', '\'', '‘'])
            .trim_start();
        if GREETING.is_match(rest) && has_blank_marker(rest) && !quotes_material(rest, passage) {
            let frame = rest.trim_end().trim_end_matches(['"', '”']).trim_end();
            return Some(frame.to_string());
        }
    }
    None
}

/// Verbs a direction (an imperative step written for the student) starts with.
const DIRECTION_VERBS: [&str; 70] = [
    "add",
    "answer",
    "ask",
    "begin",
    "check",
    "choose",
    "circle",
    "cite",
    "compare",
    "complete",
    "connect",
    "consider",
    "copy",
    "count",
    "cross",
    "decide",
    "describe",
    "discuss",
    "draw",
    "explain",
    "fill",
    "find",
    "focus",
    "follow",
    "give",
    "go",
    "highlight",
    "identify",
    "include",
    "jot",
    "label",
    "list",
    "listen",
    "look",
    "make",
    "mark",
    "match",
    "mention",
    "name",
    "note",
    "notice",
    "number",
    "pay",
    "pick",
    "point",
    "put",
    "quote",
    "read",
    "refer",
    "remember",
    "reread",
    "retell",
    "review",
    "rewrite",
    "say",
    "scan",
    "search",
    "select",
    "share",
    "show",
    "skim",
    "sort",
    "start",
    "state",
    "summarize",
    "support",
    "take",
    "talk",
    "underline",
    "use",
];

/// A leading bullet, number or 「Step 1:」 before a line of a support box.
static LINE_BULLET: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"(?i)^\s*(?:[-•*·▪]\s*|\(?\d{1,2}[.)]\s+|step\s*\d{1,2}\s*[:.)\-–]?\s*)")
        .expect("valid regex")
});

/// A lead that only points at the frame after it (「Use this frame:」「Complete the sentence
/// frame:」): in a box of sentence frames it says nothing the heading does not.
static FRAME_LEAD: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(
        r"(?i)^\s*(?:please\s+)?(?:use|try|complete|fill\s+in|follow)\s+(?:this|these|the|the\s+following|a)\s+(?:sentence\s+)?(?:frames?|starters?|sentence\s+starters?|patterns?)(?:\s+below)?\s*[:：]\s*",
    )
    .expect("valid regex")
});

/// A line of a support box without its bullet or step number.
fn without_bullet(line: &str) -> &str {
    let line = line.trim();
    match LINE_BULLET.find(line) {
        Some(m) if m.end() < line.len() => line[m.end()..].trim(),
        _ => line,
    }
}

/// Whether `text` is a direction: an imperative step with no blank to write in and no question
/// (「Identify the common idea about bees in the first sentence.」).
pub(crate) fn direction_line(text: &str) -> bool {
    let text = without_bullet(text);
    if has_blank_marker(text) || text.contains(['?', '？']) {
        return false;
    }
    text.split(|c: char| !c.is_alphabetic())
        .find(|w| !w.is_empty())
        .is_some_and(|w| DIRECTION_VERBS.contains(&w.to_lowercase().as_str()))
}

/// The sentences of one line: cut after 「.」「!」「?」 followed by a space and a capital letter or a
/// quote. 「Hi! In Paragraph 2, …」 is two sentences that stay together in a frame.
fn sentences(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    for (k, &(i, c)) in chars.iter().enumerate() {
        if !matches!(c, '.' | '!' | '?') {
            continue;
        }
        let next = chars[k + 1..].iter().find(|(_, c)| !c.is_whitespace());
        let spaced = chars.get(k + 1).is_some_and(|(_, c)| c.is_whitespace());
        if spaced && next.is_some_and(|(_, c)| c.is_uppercase() || "\"“'‘".contains(*c)) {
            let end = i + c.len_utf8();
            out.push(line[start..end].trim());
            start = end;
        }
    }
    out.push(line[start..].trim());
    out.retain(|s| !s.is_empty());
    out
}

/// A box of sentence frames, sorted: the frames (without a 「Use this frame:」 lead) and the
/// directions written among them (imperative steps without a blank, whole lines or sentences of
/// a line), which belong in a Directions box. Lines that are neither stay with the frames.
pub(crate) fn split_frames(lines: &[String]) -> (Vec<String>, Vec<String>) {
    let mut frames = Vec::new();
    let mut directions = Vec::new();
    for line in lines.iter().flat_map(|l| l.lines()) {
        let body = without_bullet(line);
        if body.is_empty() {
            continue;
        }
        if direction_line(body) {
            // One step per sentence.
            directions.extend(sentences(body).into_iter().map(str::to_string));
            continue;
        }
        let mut kept: Vec<&str> = Vec::new();
        for s in sentences(body) {
            if direction_line(s) {
                directions.push(s.to_string());
            } else {
                kept.push(s);
            }
        }
        let frame = kept.join(" ");
        let frame = FRAME_LEAD.replace(&frame, "").trim().to_string();
        if !frame.is_empty() {
            frames.push(frame);
        }
    }
    (frames, directions)
}

/// A box of directions, sorted: the steps and the frames written among them (lines with a
/// blank, without a 「Use this frame:」 lead).
pub(crate) fn split_directions(lines: &[String]) -> (Vec<String>, Vec<String>) {
    let mut directions = Vec::new();
    let mut frames = Vec::new();
    for line in lines.iter().flat_map(|l| l.lines()) {
        let body = without_bullet(line);
        if body.is_empty() {
            continue;
        }
        if has_blank_marker(body) {
            frames.push(FRAME_LEAD.replace(body, "").trim().to_string());
        } else {
            directions.push(body.to_string());
        }
    }
    (directions, frames)
}

/// A question that is only a sentence frame fails `items_wellformed`: the repair call is told to
/// write what the student does. The question is kept here; when it is still only a frame, the
/// block is assembled with the frame in its 「Sentence frames」 box, not numbered
/// ([`crate::worksheet::assemble::lift_frames`]). A greeting-led frame wrapped in a task
/// (「Write … using the frame: Hi! …」) counts as a frame too ([`greeting_frame`]).
fn frame_check(prompt: &str, passage: &str, iname: &str, field: &str, t: &mut Tally) {
    if !sentence_frame_only(prompt, passage) && greeting_frame(prompt, passage).is_none() {
        return;
    }
    t.fail(
        "items_wellformed",
        format!(
            "{iname} 只有句型框架（「{}」），沒有題目：句型框架是語言鷹架，不是一題。{field} 要換成一題針對課文內容的題目，寫出要學生做什麼（完整的題目或指示，例如 \"Explain one feature of narrative writing in paragraph 3.\"）；{FRAME_PREFIX_NOTE}；也不要用 Hi!、Hello! 這類招呼語開頭。",
            clip_chars(prompt, 80)
        ),
        "句型框架移出題號，印在這一區的 Sentence frames 方框（不編號）",
    );
}

/// What the repair call of a question that is only a sentence frame is told about a lead such as
/// 「Complete the statement:」: a frame with a lead is still a frame (a copied objective with one
/// is refused by [`objective_check`]).
const FRAME_PREFIX_NOTE: &str = "只在句型框架前面加上「Complete the statement:」「Complete the self-assessment statement:」這類開頭，它還是句型框架，不是題目";

// ---------------------------------------------------------------- a learning objective is not a question

/// The check that refuses a question copied from a learning objective ([`objective_check`]).
pub const CHECK_NOT_AN_OBJECTIVE: &str = "not_a_copy_of_objective";

/// A question that shares this many words in a row with a learning objective copies it (in
/// Chinese, every two characters count as one word).
pub const OBJECTIVE_COPY_WORDS: usize = 6;

/// The objectives a sheet has, as [`objective_check`] reads them: the teacher's text and the
/// students' line of each objective (`(text, student_facing_en)`), one line each, trimmed,
/// without duplicates.
pub fn objective_lines<'a>(
    objectives: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (text, en) in objectives {
        for line in text.lines().chain(en.lines()).map(str::trim) {
            if !line.is_empty() && !out.iter().any(|o| o == line) {
                out.push(line.to_string());
            }
        }
    }
    out
}

/// The words the copy check compares: lowercase letters and digits (an apostrophe inside a word
/// kept), each CJK character a word of its own, punctuation dropped. `None` is a blank to write
/// in (「______」「( )」).
fn copy_words(text: &str) -> Vec<Option<String>> {
    let marked = BLANK_MARKER.replace_all(text, " \u{1} ");
    let chars: Vec<char> = marked.chars().collect();
    let mut out: Vec<Option<String>> = Vec::new();
    let mut word = String::new();
    fn flush(word: &mut String, out: &mut Vec<Option<String>>) {
        if !word.is_empty() {
            out.push(Some(std::mem::take(word)));
        }
    }
    for (i, &c) in chars.iter().enumerate() {
        if c == '\u{1}' {
            flush(&mut word, &mut out);
            out.push(None);
        } else if crate::retrieval::is_cjk(c) {
            flush(&mut word, &mut out);
            out.push(Some(c.to_string()));
        } else if c.is_alphanumeric() {
            word.extend(c.to_lowercase());
        } else if matches!(c, '\'' | '’')
            && !word.is_empty()
            && chars.get(i + 1).is_some_and(|n| n.is_alphanumeric())
        {
            word.push('\'');
        } else {
            flush(&mut word, &mut out);
        }
    }
    flush(&mut word, &mut out);
    out
}

/// An English word weighs 2, a CJK character 1: a run of [`OBJECTIVE_COPY_WORDS`] words is
/// twice that weight.
fn copy_weight(word: &str) -> usize {
    if word.chars().all(crate::retrieval::is_cjk) {
        1
    } else {
        2
    }
}

/// A word of a question as the copy check reads it: `None` is a blank; `true` marks the place of
/// the answer (the blank, or the answer's words put back into it).
type CopyWord = (Option<String>, bool);

/// A run of words a question shares with an objective.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SharedRun {
    weight: usize,
    /// The run takes in the place of the answer: the blank stands for a word of the objective.
    answer: bool,
}

/// The runs of `objective`'s words that `prompt` has in a row (a blank stands for any one word)
/// that are at least [`OBJECTIVE_COPY_WORDS`] long and not word for word in the material
/// (`passage`, see [`spaced_words`]: a cloze of a line the objective quotes is a question about
/// the text). Returns the heaviest, one that takes in the answer's place first.
fn shared_run(prompt: &[CopyWord], objective: &[String], passage: &str) -> Option<SharedRun> {
    let threshold = OBJECTIVE_COPY_WORDS * 2;
    let mut best: Option<SharedRun> = None;
    // (weight, length, takes in the answer's place) of the common run ending at the previous
    // prompt word and objective word j.
    let mut prev = vec![(0usize, 0usize, false); objective.len() + 1];
    for (word, answer) in prompt {
        let mut cur = vec![(0usize, 0usize, false); objective.len() + 1];
        for (j, o) in objective.iter().enumerate() {
            let weight = match word {
                None => 0,
                Some(w) if w == o => copy_weight(w),
                Some(_) => continue,
            };
            let (w, len, a) = prev[j];
            cur[j + 1] = (w + weight, len + 1, a || *answer);
            let (weight, len, answer) = cur[j + 1];
            let run = SharedRun { weight, answer };
            let better = best.is_none_or(|b| (run.answer, run.weight) > (b.answer, b.weight));
            if weight >= threshold && better {
                let words = objective[j + 1 - len..=j].join(" ");
                if !passage.contains(&format!(" {words} ")) {
                    best = Some(run);
                }
            }
        }
        prev = cur;
    }
    best
}

/// The words of `text` joined with single spaces, with a space at each end (blanks left out).
fn spaced_words(text: &str) -> String {
    let words: Vec<String> = copy_words(text).into_iter().flatten().collect();
    format!(" {} ", words.join(" "))
}

/// `prompt`'s words with its blanks filled in from `answer`: one blank takes the whole answer;
/// several take the answer's parts (between commas, semicolons, slashes), in order, when there
/// are as many. `None` when there is no blank or nothing to fill it with.
fn filled_words(prompt: &[Option<String>], answer: &str) -> Option<Vec<CopyWord>> {
    let blanks = prompt.iter().filter(|w| w.is_none()).count();
    if blanks == 0 {
        return None;
    }
    let parts: Vec<Vec<Option<String>>> = if blanks == 1 {
        vec![copy_words(answer)]
    } else {
        answer
            .split([',', ';', '/', '|', '\n', '，', '；', '、'])
            .map(copy_words)
            .filter(|w| !w.is_empty())
            .collect()
    };
    if parts.len() != blanks || parts.iter().any(Vec::is_empty) {
        return None;
    }
    let mut parts = parts.into_iter();
    let mut out = Vec::new();
    for w in prompt {
        match w {
            Some(w) => out.push((Some(w.clone()), false)),
            None => out.extend(
                parts
                    .next()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|w| (w, true)),
            ),
        }
    }
    Some(out)
}

/// An answer that is no answer: a self-rating or a reflection has none (「Answers will vary.」).
fn open_answer(answer: &str) -> bool {
    let a = norm(answer).to_lowercase();
    let a = a.trim_matches(|c: char| !c.is_alphanumeric());
    a.is_empty()
        || [
            "answers will vary",
            "answers may vary",
            "answer will vary",
            "answer may vary",
            "open answer",
            "open-ended",
            "students' own",
            "student's own",
            "依學生",
            "學生自行",
            "自由作答",
            "答案不一",
            "無標準答案",
            "沒有標準答案",
        ]
        .iter()
        .any(|p| a.starts_with(p))
}

/// The objective a question copies so that the objective gives its answer away, if any. The
/// prompt shares a run of [`OBJECTIVE_COPY_WORDS`] words or more with the objective (any case and
/// punctuation; a blank stands for any one word, or is filled with the answer; not a run that is
/// word for word in the material), the question has an answer ([`open_answer`]: a self-rating or
/// a reflection with none may quote an objective), and either
/// * the run takes in the blank (the blank is a word of the objective: 「I can explain how the
///   two roads symbolize life ______.」, a lead in front or not),
/// * the answer is only words of that objective (at most four), or
/// * it is a 是非題 whose statement is mostly the objective (two thirds of its words).
///
/// A question about the text that shares words with an objective but whose answer the
/// objective does not give (「Mrs. Lin kept the umbrella stand because ______.」 under 「I can
/// explain why Mrs. Lin kept the umbrella stand.」) is not a copy.
pub(crate) fn copied_objective<'o>(
    format: &str,
    prompt: &str,
    answer: &str,
    objectives: &'o [String],
    passage: &str,
) -> Option<&'o str> {
    if objectives.is_empty() || open_answer(answer) {
        return None;
    }
    let words = copy_words(prompt);
    let plain: Vec<CopyWord> = words.iter().map(|w| (w.clone(), w.is_none())).collect();
    let filled = filled_words(&words, answer);
    let prompt_weight: usize = words.iter().flatten().map(|w| copy_weight(w)).sum();
    let answer_words: Vec<String> = copy_words(answer).into_iter().flatten().collect();
    let passage = spaced_words(passage);
    objectives.iter().map(String::as_str).find(|objective| {
        let target: Vec<String> = copy_words(objective).into_iter().flatten().collect();
        let runs: Vec<SharedRun> = std::iter::once(plain.as_slice())
            .chain(filled.as_deref())
            .filter_map(|p| shared_run(p, &target, &passage))
            .collect();
        let Some(heaviest) = runs.iter().map(|r| r.weight).max() else {
            return false;
        };
        let blank_in_run = runs.iter().any(|r| r.answer);
        let from_objective = !answer_words.is_empty()
            && answer_words.iter().map(|w| copy_weight(w)).sum::<usize>() <= 8
            && answer_words.iter().all(|w| target.contains(w));
        let statement = format.trim() == FMT_TRUE_FALSE && heaviest * 3 >= prompt_weight * 2;
        blank_in_run || from_objective || statement
    })
}

/// The repair note of a question copied from a learning objective.
pub fn objective_copy_note(iname: &str, objective: &str) -> String {
    format!(
        "{iname} 照抄了學習目標（{}）：學習目標只供理解教學重點，學生在學習單上方已看得到，請換成一題針對課文內容的題目。",
        clip_chars(objective, 120)
    )
}

/// A question copied from a learning objective ([`copied_objective`]) fails
/// [`CHECK_NOT_AN_OBJECTIVE`] and is dropped (the objectives print at the top of the sheet: its
/// answer is right there). Returns whether the question is kept.
fn objective_check(
    (format, prompt, answer): (&str, &str, &str),
    iname: &str,
    (objectives, passage): (&[String], &str),
    t: &mut Tally,
) -> bool {
    let Some(objective) = copied_objective(format, prompt, answer, objectives, passage) else {
        return true;
    };
    t.fail(
        CHECK_NOT_AN_OBJECTIVE,
        objective_copy_note(iname, objective),
        "拿掉這一題",
    );
    false
}

// ---------------------------------------------------------------- 是非題: a statement to judge

/// A question for the student's view: 「Do you think …」「In your opinion …」「Do you agree …」.
static OPINION: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(
        r"(?i)\b(?:do|would|did|does)\s+you\s+(?:think|believe|agree|feel)\b|\bin\s+your\s+(?:opinion|view)\b|\bwhat\s+do\s+you\s+think\b",
    )
    .expect("valid regex")
});

/// 「Do you think X?」「Do you agree that X?」「Is it true that X?」: X is the statement to judge.
static TF_STATEMENT: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(
        r"(?i)^\s*(?:(?:do|would)\s+you\s+(?:think|believe|feel)\s+(?:that\s+)?|(?:do|would)\s+you\s+agree\s+that\s+|is\s+it\s+true\s+that\s+)([^?？]+?)\s*[?？]\s*$",
    )
    .expect("valid regex")
});

/// Whether a 是非題 is not a statement to judge: it asks a question (「Is the second sentence the
/// topic sentence?」) or the student's opinion (「Do you think this sentence serves as a topic
/// sentence?」), which True / False cannot answer.
pub(crate) fn true_false_question(prompt: &str) -> bool {
    prompt.contains(['?', '？']) || OPINION.is_match(prompt)
}

/// What the code makes of a 是非題 that asks a question, when the repair call wrote one again.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TrueFalseFix {
    /// 「Do you think X?」 → the statement 「X.」 (still a 是非題, the same answer).
    Statement(String),
    /// Anything else: a 簡答題 asking Yes or No with a reason ([`YES_NO_REASON`]).
    YesNo(String),
}

/// What a 是非題 that asked a question asks once it is a 簡答題.
pub const YES_NO_REASON: &str = "Answer Yes or No, and give one reason from the text.";

pub(crate) fn true_false_fix(prompt: &str) -> TrueFalseFix {
    if let Some(c) = TF_STATEMENT.captures(prompt.trim()) {
        let clause = c[1].trim();
        if clause.split_whitespace().count() >= 3 && !OPINION.is_match(clause) {
            let mut chars = clause.chars();
            let first: String = chars
                .next()
                .map(|f| f.to_uppercase().collect())
                .unwrap_or_default();
            return TrueFalseFix::Statement(format!(
                "{first}{}.",
                chars.as_str().trim_end_matches(['.', '。'])
            ));
        }
    }
    TrueFalseFix::YesNo(format!("{} {YES_NO_REASON}", prompt.trim()))
}

/// A 是非題 must be a statement students judge from the text (「The second sentence of Paragraph 1
/// is the topic sentence.」), not a question or a question for their opinion. One that is fails
/// `items_wellformed` (the repair call is told how to write it) and is fixed in code
/// ([`true_false_fix`]): the statement in 「Do you think X?」 is judged instead, otherwise the item
/// becomes a 簡答題 asking Yes or No with a reason (options go; True / False answers become Yes /
/// No). `field` is the prompt's name in the schema.
fn true_false_check(
    format: &mut String,
    prompt: &mut String,
    options: &mut Vec<String>,
    answer: Option<&mut String>,
    (iname, field): (&str, &str),
    t: &mut Tally,
) {
    if format != FMT_TRUE_FALSE || !true_false_question(prompt) {
        return;
    }
    let problem = format!(
        "{iname} 是是非題，但 {field} 是問句或問學生的意見（「{}」），學生無法用 True／False 回答：是非題要寫成一句可以依課文判斷對錯的直述句（例如 \"The second sentence of Paragraph 1 is the topic sentence.\"）；要問學生的看法時改成簡答題，請學生回答 Yes 或 No 並說明理由。",
        clip_chars(prompt, 80)
    );
    match true_false_fix(prompt) {
        TrueFalseFix::Statement(s) => {
            t.fail("items_wellformed", problem, "把問句改成要判斷對錯的直述句");
            *prompt = s;
        }
        TrueFalseFix::YesNo(s) => {
            t.fail(
                "items_wellformed",
                problem,
                "改成簡答題：回答 Yes 或 No 並說明理由",
            );
            *prompt = s;
            *format = FMT_SHORT.to_string();
            options.clear();
            if let Some(answer) = answer {
                match answer.trim() {
                    "True" => *answer = "Yes.".to_string(),
                    "False" => *answer = "No.".to_string(),
                    _ => {}
                }
            }
        }
    }
}

/// The default rules: 選擇題 exactly 4 distinct options, 配對題 at least 3. A teacher's confirmed
/// option-count habit (only while the PI switch is on, [`crate::memory::option_habits`])
/// additionally allows that count; nothing else changes.
fn item_wellformed(it: &RawItem, habits: OptionHabits) -> Result<(), Cow<'static, str>> {
    match it.answer_format.as_str() {
        f if f == FMT_CHOICE => {
            let distinct: HashSet<&str> = it.options.iter().map(|o| o.trim()).collect();
            let allowed = |n: usize| n == 4 || habits.choice_options == Some(n);
            if !allowed(it.options.len()) || distinct.len() != it.options.len() {
                Err(match habits.choice_options.filter(|n| *n != 4) {
                    None => Cow::Borrowed("選擇題要有 4 個不重複的選項"),
                    Some(n) => Cow::Owned(format!(
                        "選擇題要有 4 個或 {n} 個（老師確認的習慣）不重複的選項"
                    )),
                })
            } else if !it.options.iter().any(|o| o.trim() == it.answer_en.trim()) {
                Err(Cow::Borrowed(
                    "選擇題的 answer_en 必須與其中一個選項完全相同",
                ))
            } else {
                Ok(())
            }
        }
        f if f == FMT_TRUE_FALSE => {
            if matches!(it.answer_en.trim(), "True" | "False") {
                Ok(())
            } else {
                Err(Cow::Borrowed("是非題的 answer_en 必須是 True 或 False"))
            }
        }
        f if f == FMT_MATCHING => {
            let min = habits.matching_pairs.map_or(3, |n| n.min(3));
            if it.options.len() >= min {
                Ok(())
            } else if min == 3 {
                Err(Cow::Borrowed("配對題的 options 至少要 3 個"))
            } else {
                Err(Cow::Owned(format!("配對題的 options 至少要 {min} 個")))
            }
        }
        _ => Ok(()),
    }
}

/// 表格整理: a question's table, cleaned ([`table::check`]). A question of another format loses
/// the table it carries. A placeholder heading is repaired from the question's own words without
/// a repair call; headings or row labels the code cannot supply fail `items_wellformed` (one
/// repair call, then the question keeps what it has and the teacher fills in the rest).
/// `asked`: the schema asked for a table (a revision's does not: the stored one stays).
fn table_of(
    format: &str,
    raw: Option<&RawTable>,
    prompt: &str,
    iname: &str,
    (english, asked): (bool, bool),
    t: &mut Tally,
) -> Option<RawTable> {
    if format != super::FMT_TABLE {
        if let Some(p) = table::stray(raw, format, iname) {
            t.fixed("items_wellformed", p.text, p.repair);
        }
        return None;
    }
    if !asked {
        return None;
    }
    let checked = table::check(raw, prompt, english, iname);
    for p in checked.problems {
        if p.needs_model {
            t.fail("items_wellformed", p.text, p.repair);
        } else {
            t.fixed("items_wellformed", p.text, p.repair);
        }
    }
    checked.table
}

/// 圖像組織: a question's organizer, cleaned ([`organizer::from_raw`]); `None` for a question of
/// another format (an organizer it carries is dropped) and for an organizer question without
/// one, which prints an empty box as before. Nothing here costs a repair call except Chinese in
/// an organizer where students may only read English (`english`): the organizer is dropped.
/// `asked`: the schema asked for an organizer (a missing one is reported).
fn organizer_of(
    format: &str,
    raw: Option<&RawOrganizer>,
    iname: &str,
    (tiered, english, asked): (bool, bool, bool),
    t: &mut Tally,
) -> Option<Organizer> {
    if format != super::FMT_ORGANIZER {
        if raw.is_some_and(|r| !r.sections.is_empty()) {
            t.fixed(
                "organizer_wellformed",
                format!("{iname}（{format}）不是圖像組織，organizer 要填 null。"),
                "拿掉 organizer",
            );
        }
        return None;
    }
    let Some(raw) = raw.filter(|r| !r.sections.is_empty()) else {
        if asked {
            t.fixed(
                "organizer_wellformed",
                format!(
                    "{iname}（圖像組織）沒有 organizer；要選一種類型，寫出每一格的標題與參考答案。"
                ),
                "印成空白方框",
            );
        }
        return None;
    };
    let (org, fixes) = organizer::from_raw(raw, tiered);
    for (problem, repair) in fixes {
        t.fixed(
            "organizer_wellformed",
            format!("{iname}：{problem}"),
            repair,
        );
    }
    if english && !organizer::english(&org) {
        t.fail(
            "english_only",
            format!("{iname} 的圖像組織含有中文；學生看得到的欄位只能用英文。"),
            "拿掉圖像組織，印成空白方框",
        );
        return None;
    }
    Some(org)
}

fn paragraph_label(nos: &[i64]) -> String {
    match nos {
        [] => "Whole text".to_string(),
        [one] => format!("Paragraph {one}"),
        [first, .., last] if (*last - *first) as usize + 1 == nos.len() => {
            format!("Paragraphs {first}–{last}")
        }
        many => format!(
            "Paragraphs {}",
            many.iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

pub fn default_block_title(nos: &[i64], function_label: &str) -> String {
    let verb = match function_label {
        "發現資訊" => "Find",
        "整理資訊" => "Organize",
        "分析資訊" => "Analyze",
        "比較資訊" => "Compare",
        "創造資訊" => "Create",
        _ => "Read and answer",
    };
    format!("{}: {verb}", paragraph_label(nos))
}

pub fn evaluate_tasks(raw: &RawTasks, ctx: &TasksCtx<'_>) -> Eval<Vec<RawBlock>> {
    let n = ctx.paragraphs.len() as i64;
    let passage = ctx.paragraphs.join("\n");
    let all_tasks = student_tasks();
    let mut t = Tally::new(if ctx.only.is_some() {
        &TASK_CHECKS
    } else {
        &TASK_CHECKS_WITH_BUDGET
    });
    let mut blocks: Vec<RawBlock> = Vec::new();

    for (bi, b) in raw.blocks.iter().enumerate() {
        let name = if b.block_id.trim().is_empty() {
            format!("B{}", bi + 1)
        } else {
            b.block_id.trim().to_string()
        };
        if info_function_from_label(&b.info_function).is_none() {
            t.fail(
                "labels_from_spec_sets",
                format!(
                    "{name} 的 info_function「{}」不在允許的清單內。",
                    b.info_function
                ),
                "整個區塊拿掉",
            );
            continue;
        }
        let mut block = b.clone();
        block.title_en = clean(&b.title_en);
        block.instructions_en = clean(&b.instructions_en);
        block.design_reason_zh = clean(&b.design_reason_zh);
        block.student_tasks = b
            .student_tasks
            .iter()
            .filter(|label| {
                let ok = all_tasks.contains(&label.as_str());
                if !ok {
                    t.fail(
                        "labels_from_spec_sets",
                        format!("{name} 的 student_tasks「{label}」不在允許的清單內。"),
                        "拿掉這個標籤",
                    );
                }
                ok
            })
            .cloned()
            .collect();
        block.student_tasks.dedup();
        block.paragraph_nos = match ctx.only {
            Some(only) => only.to_vec(),
            None => {
                let mut nos: Vec<i64> = b
                    .paragraph_nos
                    .iter()
                    .copied()
                    .filter(|p| (1..=n).contains(p))
                    .collect();
                nos.sort_unstable();
                nos.dedup();
                nos
            }
        };
        if block.paragraph_nos.is_empty() {
            t.fail(
                "all_paragraphs_covered",
                format!("{name} 的 paragraph_nos 沒有對應到教材的段落（1 到 {n}）。"),
                "整個區塊拿掉",
            );
            continue;
        }
        if !english_only(&block.title_en) || block.title_en.is_empty() {
            if !block.title_en.is_empty() {
                t.fail(
                    "english_only",
                    format!("{name} 的 title_en 含有中文，必須用英文。"),
                    "改用預設標題",
                );
            }
            block.title_en = default_block_title(&block.paragraph_nos, &block.info_function);
        }
        if !english_only(&block.instructions_en) {
            t.fail(
                "english_only",
                format!("{name} 的 instructions_en 含有中文，必須用英文。"),
                "清空作答指示",
            );
            block.instructions_en = String::new();
        }
        let before = block.objective_refs.len();
        block
            .objective_refs
            .retain(|r| ctx.objective_ids.contains(r));
        if block.objective_refs.len() != before {
            t.fail(
                "objective_refs_valid",
                format!(
                    "{name} 的 objective_refs 只能使用這些代號：{}。",
                    if ctx.objective_ids.is_empty() {
                        "（沒有提供目標，請給空陣列）".to_string()
                    } else {
                        ctx.objective_ids.join("、")
                    }
                ),
                "拿掉不存在的目標代號",
            );
        }

        let mut items: Vec<RawItem> = Vec::new();
        for (ii, it) in b.items.iter().enumerate() {
            let iname = format!("{name} 第 {} 題", ii + 1);
            let mut item = it.clone();
            item.prompt_en = clean_prompt(&it.prompt_en);
            item.answer_en = clean(&it.answer_en);
            item.options = clean_all(&it.options);
            // A label outside the fixed set (a provider that ignores the schema) is dropped.
            item.bloom = it
                .bloom
                .as_deref()
                .and_then(Bloom::parse)
                .map(|b| b.as_str().to_string());
            if !ANSWER_FORMATS.contains(&item.answer_format.as_str()) {
                t.fail(
                    "labels_from_spec_sets",
                    format!(
                        "{iname} 的 answer_format「{}」不在允許的清單內。",
                        it.answer_format
                    ),
                    "拿掉這一題",
                );
                continue;
            }
            let texts = [&item.prompt_en, &item.answer_en];
            if item.prompt_en.is_empty()
                || !texts.iter().all(|s| english_only(s))
                || !item.options.iter().all(|s| english_only(s))
            {
                t.fail(
                    "english_only",
                    format!(
                        "{iname} 的題目、選項或答案含有中文（或題目是空的）；學生看得到的欄位只能用英文。"
                    ),
                    "拿掉這一題",
                );
                continue;
            }
            if item.answer_format == FMT_CHOICE {
                normalize_choice(&mut item.options, &mut item.answer_en);
            }
            let answer = item.answer_en.clone();
            blank_check(
                &mut item.answer_format,
                &mut item.prompt_en,
                &answer,
                &iname,
                "prompt_en",
                &mut t,
            );
            true_false_check(
                &mut item.answer_format,
                &mut item.prompt_en,
                &mut item.options,
                Some(&mut item.answer_en),
                (&iname, "prompt_en"),
                &mut t,
            );
            if !objective_check(
                (&item.answer_format, &item.prompt_en, &item.answer_en),
                &iname,
                (ctx.objectives, &passage),
                &mut t,
            ) {
                continue;
            }
            frame_check(&item.prompt_en, &passage, &iname, "prompt_en", &mut t);
            if let Err(why) = item_wellformed(&item, ctx.habits) {
                t.fail(
                    "items_wellformed",
                    format!("{iname}（{}）：{why}。", item.answer_format),
                    "拿掉這一題",
                );
                continue;
            }
            if contains_answer(&item.prompt_en, &item.answer_en) {
                t.fail(
                    "answer_not_in_prompt",
                    format!("{iname} 的題幹直接寫出了答案。"),
                    "拿掉這一題",
                );
                continue;
            }
            item.organizer = organizer_of(
                &item.answer_format,
                it.organizer.as_ref(),
                &iname,
                (false, true, true),
                &mut t,
            )
            .map(|o| RawOrganizer::from(&o));
            item.table = table_of(
                &item.answer_format,
                it.table.as_ref(),
                &item.prompt_en,
                &iname,
                (true, true),
                &mut t,
            );
            // Evidence is only a note for the teacher: one that is not verbatim is cut down to
            // its longest verbatim piece, or dropped, in code (as the SOP analysis drops it).
            // It never costs the step a repair call on its own.
            item.evidence = match it.evidence.as_deref().map(clean) {
                Some(e) if e.is_empty() => None,
                Some(e) if is_verbatim(&e, &passage) => locate_verbatim(&e, &passage),
                Some(e) => {
                    let piece = verbatim_piece(&e, &passage);
                    t.fixed(
                        "evidence_verbatim",
                        format!(
                            "{iname} 的 evidence「{}」不是教材的連續片段。請逐字複製一段連續文字，不可用「...」省略或拼接；找不到就填 null。",
                            clip_chars(&e, 120)
                        ),
                        if piece.is_some() {
                            "只留下其中逐字的一段"
                        } else {
                            "清空這一題的依據"
                        },
                    );
                    piece
                }
                None => None,
            };
            if ![TIER_CORE, TIER_SUPPORT, TIER_CHALLENGE].contains(&item.tier.as_str()) {
                item.tier = TIER_CORE.to_string();
            }
            items.push(item);
        }
        if items.is_empty() {
            t.repairs
                .push(format!("{name} 沒有剩下可用的題目，整個區塊拿掉"));
            continue;
        }
        // Every version shows the core items: a block needs at least one.
        if !items.iter().any(|i| i.tier == TIER_CORE) {
            items[0].tier = TIER_CORE.to_string();
        }
        for tier in [TIER_SUPPORT, TIER_CHALLENGE] {
            let mut seen = false;
            items.retain(|i| {
                if i.tier != tier {
                    return true;
                }
                let keep = !seen;
                seen = true;
                keep
            });
        }
        block.items = items;
        blocks.push(block);
    }

    if ctx.only.is_some() {
        blocks.truncate(1);
    } else {
        let covered: HashSet<i64> = blocks
            .iter()
            .flat_map(|b| b.paragraph_nos.clone())
            .collect();
        let missing: Vec<String> = (1..=n)
            .filter(|p| !covered.contains(p))
            .map(|p| format!("[P{p}]"))
            .collect();
        if !missing.is_empty() && !blocks.is_empty() {
            t.fail(
                "all_paragraphs_covered",
                format!(
                    "這些段落沒有被任何區塊涵蓋：{}。每一段都要有區塊。",
                    missing.join("、")
                ),
                "",
            );
        }
        // A closing whole-text block (first and last paragraph) may follow the others.
        let closing = blocks
            .last()
            .is_some_and(|b| n > 1 && b.paragraph_nos.contains(&1) && b.paragraph_nos.contains(&n));
        let ordered = if closing {
            blocks.len() - 1
        } else {
            blocks.len()
        };
        let firsts: Vec<i64> = blocks[..ordered]
            .iter()
            .map(|b| b.paragraph_nos[0])
            .collect();
        if firsts.windows(2).any(|w| w[0] > w[1]) {
            t.fail(
                "blocks_in_paragraph_order",
                "區塊沒有依段落順序排列。".to_string(),
                "依段落順序重新排列區塊",
            );
            blocks[..ordered].sort_by_key(|b| b.paragraph_nos[0]);
        }
        let functions: HashSet<&str> = blocks.iter().map(|b| b.info_function.as_str()).collect();
        if blocks.len() >= 3 && functions.len() < 3 {
            t.fail(
                "at_least_3_info_functions",
                format!(
                    "整份設計只用了 {} 種資訊處理功能；三個以上區塊時至少要 3 種。",
                    functions.len()
                ),
                "",
            );
        }
        let limit = (ctx.item_budget * 3).div_ceil(2).max(blocks.len());
        let core = |blocks: &[RawBlock]| -> usize {
            blocks
                .iter()
                .flat_map(|b| &b.items)
                .filter(|i| i.tier == TIER_CORE)
                .count()
        };
        let total = core(&blocks);
        if total > limit {
            t.fail(
                "item_count_within_budget",
                format!(
                    "核心題（tier = core）共 {total} 題，超過這份學習單的時間能做完的量；請減少到大約 {} 題。",
                    ctx.item_budget
                ),
                "從題數最多的區塊末端刪去多的核心題",
            );
            while core(&blocks) > limit {
                let Some(fullest) = blocks
                    .iter_mut()
                    .filter(|b| b.items.iter().filter(|i| i.tier == TIER_CORE).count() > 1)
                    .max_by_key(|b| b.items.iter().filter(|i| i.tier == TIER_CORE).count())
                else {
                    break;
                };
                if let Some(pos) = fullest.items.iter().rposition(|i| i.tier == TIER_CORE) {
                    fullest.items.remove(pos);
                }
            }
        }
        // Q4, on the blocks as they will be printed (after any reordering above).
        check_practice(&mut blocks, &mut t);
    }

    for (bi, b) in blocks.iter_mut().enumerate() {
        b.block_id = format!("B{}", bi + 1);
        for (ii, it) in b.items.iter_mut().enumerate() {
            it.item_id = format!("B{}-{}", bi + 1, ii + 1);
        }
    }
    let output = if blocks.is_empty() {
        None
    } else {
        Some(blocks)
    };
    t.finish(output)
}

/// Paragraph numbers (from 1) that no block covers.
pub fn uncovered_paragraphs(blocks: &[RawBlock], paragraphs: usize) -> Vec<i64> {
    let covered: HashSet<i64> = blocks
        .iter()
        .flat_map(|b| b.paragraph_nos.clone())
        .collect();
    (1..=paragraphs as i64)
        .filter(|p| !covered.contains(p))
        .collect()
}

// ---------------------------------------------------------------- (d) warm-up

/// A word the model may pick as a key word: found in the material by the code, with its level
/// from the word list (never from the model).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub word: String,
    pub level: Option<u8>,
    pub paragraph_no: i64,
    /// The sentence of the material the word appears in.
    pub sentence: String,
    /// The parts of speech the word list prints for it (`n./v.`); "" when unlisted.
    pub pos: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CleanActivity {
    pub title: String,
    pub instructions: String,
    pub prompts: Vec<String>,
    pub student_tasks: Vec<String>,
    pub answer_format: Option<String>,
    pub purpose_zh: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CleanWord {
    pub word: String,
    pub level: Option<u8>,
    pub definition: String,
    /// Part of speech in the context sentence, one of `key_vocab::POS_SET` or "".
    pub pos: String,
    /// Traditional Chinese meaning; "" when the model gave none that could be used.
    pub meaning_zh: String,
    pub context: String,
    pub paragraph_no: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CleanWarmup {
    pub activities: Vec<CleanActivity>,
    pub words: Vec<CleanWord>,
}

pub struct WarmupCtx<'a> {
    pub paragraphs: &'a [String],
    pub candidates: &'a [Candidate],
    pub vocab_count: usize,
}

const WARMUP_CHECKS: [&str; 5] = [
    "background_1_2_activities",
    "english_only",
    "labels_from_spec_sets",
    "vocab_only_from_candidates",
    "context_sentence_verbatim",
];

pub fn evaluate_warmup(raw: &RawWarmup, ctx: &WarmupCtx<'_>) -> Eval<CleanWarmup> {
    let mut t = Tally::new(&WARMUP_CHECKS);
    let all_tasks = student_tasks();
    let mut activities = Vec::new();
    for (i, a) in raw.background_activities.iter().enumerate() {
        let name = format!("background_activities 第 {} 個", i + 1);
        let title = clean(&a.title_en);
        let instructions = clean(&a.instructions_en);
        let prompts = clean_all(&a.prompts_en);
        let english = english_only(&title)
            && english_only(&instructions)
            && prompts.iter().all(|p| english_only(p));
        if !english || (prompts.is_empty() && instructions.is_empty()) {
            t.fail(
                "english_only",
                format!("{name} 含有中文或沒有內容；題目與指示只能用英文。"),
                "拿掉這個活動",
            );
            continue;
        }
        let student_tasks: Vec<String> = a
            .student_tasks
            .iter()
            .filter(|l| all_tasks.contains(&l.as_str()))
            .cloned()
            .collect();
        let format_ok = ANSWER_FORMATS.contains(&a.answer_format.as_str());
        if student_tasks.len() != a.student_tasks.len() || !format_ok {
            t.fail(
                "labels_from_spec_sets",
                format!("{name} 的 student_tasks 或 answer_format 用了清單以外的標籤。"),
                "拿掉清單以外的標籤",
            );
        }
        // A warm-up activity has prompts but no options: as 選擇題 or 配對題 it would print a
        // question with nothing to choose from (real-model run), so it becomes 簡答題.
        let answer_format = format_ok.then(|| {
            if a.answer_format == FMT_CHOICE || a.answer_format == FMT_MATCHING {
                t.fixed(
                    "labels_from_spec_sets",
                    format!(
                        "{name} 的 answer_format 是「{}」，但暖身活動沒有選項。",
                        a.answer_format
                    ),
                    "改成簡答題",
                );
                FMT_SHORT.to_string()
            } else {
                a.answer_format.clone()
            }
        });
        activities.push(CleanActivity {
            title,
            instructions,
            prompts,
            student_tasks,
            answer_format,
            purpose_zh: clean(&a.purpose_zh),
        });
    }
    if activities.is_empty() {
        t.fail(
            "background_1_2_activities",
            "background_activities 需要 1–2 個可用的活動。".to_string(),
            "",
        );
    }
    activities.truncate(2);

    let mut words: Vec<CleanWord> = Vec::new();
    for v in &raw.key_vocabulary {
        let word = v.word.trim();
        let Some(candidate) = ctx
            .candidates
            .iter()
            .find(|c| c.word.eq_ignore_ascii_case(word))
        else {
            t.fail(
                "vocab_only_from_candidates",
                format!("關鍵字「{word}」不在候選字彙清單裡；只能從清單挑，寫法要完全相同。"),
                "拿掉這個字",
            );
            continue;
        };
        if words.iter().any(|w| w.word == candidate.word) {
            continue;
        }
        let definition = clean(&v.student_friendly_definition_en);
        if !english_only(&definition) {
            t.fail(
                "english_only",
                format!("關鍵字「{word}」的解釋含有中文，必須用簡單的英文。"),
                "清空這個字的解釋",
            );
        }
        let sentence = clean(&v.context_sentence);
        let paragraph = usize::try_from(v.paragraph_no)
            .ok()
            .and_then(|p| p.checked_sub(1))
            .and_then(|p| ctx.paragraphs.get(p));
        let verbatim = paragraph.is_some_and(|p| is_verbatim(&sentence, p));
        let (context, paragraph_no) = if verbatim {
            (sentence, Some(v.paragraph_no))
        } else {
            // The code found the word's sentence in the material itself, so this is fully
            // repaired here (gemma drops the quotation marks of dialogue, for one).
            t.fixed(
                "context_sentence_verbatim",
                format!(
                    "關鍵字「{word}」的 context_sentence 不是 [P{}] 的原句；請逐字複製含有這個字的句子。",
                    v.paragraph_no
                ),
                "改用程式在教材中找到的句子",
            );
            (candidate.sentence.clone(), Some(candidate.paragraph_no))
        };
        let meaning_zh = clean(&v.meaning_zh);
        words.push(CleanWord {
            word: candidate.word.clone(),
            level: candidate.level,
            pos: super::key_vocab::resolve_pos(&v.pos, &candidate.pos),
            meaning_zh: if super::key_vocab::has_han(&meaning_zh) {
                meaning_zh
            } else {
                String::new()
            },
            definition: if english_only(&definition) {
                definition
            } else {
                String::new()
            },
            context,
            paragraph_no,
        });
    }
    words.truncate(ctx.vocab_count.max(1));
    let output = if activities.is_empty() && words.is_empty() {
        None
    } else {
        Some(CleanWarmup { activities, words })
    };
    t.finish(output)
}

// ---------------------------------------------------------------- (e) scaffolds

pub struct ScaffoldsCtx<'a> {
    /// `warm_up` and the block ids scaffolds were asked for.
    pub targets: &'a [String],
    /// Reference answers of the main tasks: none of them may appear in a scaffold.
    pub answers: &'a [String],
    /// The questions of each target, in the order the prompt numbers them: where a sentence
    /// frame of its own may go (an empty list: none is accepted, as in the SOP's step 5).
    pub items: &'a [TargetItems],
}

/// The questions of one target as the scaffolds step numbers them (1, 2 …).
#[derive(Debug, Clone, PartialEq)]
pub struct TargetItems {
    pub target: String,
    /// (answer format, reference answer) of each question.
    pub items: Vec<(String, String)>,
}

/// Most characters of a question's own sentence frame.
pub const ITEM_FRAME_CHARS: usize = 600;

/// Whether a question of this format wants a spoken or written answer of its own words, so that
/// a sentence frame under it helps: 簡答題 and 段落寫作 (a choice, a table or a cloze has
/// nothing to say aloud).
pub fn frame_format(format: Option<&str>) -> bool {
    matches!(format, Some(f) if f == super::FMT_SHORT || f == super::FMT_PARAGRAPH)
}

/// Most characters of a hint under a question.
pub const ITEM_HINT_CHARS: usize = 200;

/// A hint for one question as the model wrote it (a guiding prompt that points at where to
/// look or what to think about, phrased as a hint): plain English, short, no answer in it.
pub fn item_hint(raw: &str, answer: &str) -> Result<String, String> {
    let text = clean(raw);
    if text.is_empty() {
        return Err("提示是空的".to_string());
    }
    if !english_only(&text) {
        return Err("提示含有中文；只能用英文".to_string());
    }
    if text.chars().count() > ITEM_HINT_CHARS {
        return Err(format!("提示太長（最多 {ITEM_HINT_CHARS} 字）"));
    }
    if contains_answer(&text, answer) {
        return Err("提示直接寫出了參考答案".to_string());
    }
    Ok(text)
}

/// A question's own sentence frame as the model wrote it: plain English with at least one
/// `___` blank, not a direction, not a copy of the answer. `Err` says what is wrong (in the
/// teacher's words, for the repair call).
pub fn item_frame(raw: &str, answer: &str) -> Result<String, String> {
    let text = clean(raw);
    if text.is_empty() {
        return Err("句型框架是空的".to_string());
    }
    if !english_only(&text) {
        return Err("句型框架含有中文；只能用英文".to_string());
    }
    if !has_blank_marker(&text) {
        return Err("句型框架沒有空格 ___，學生沒有東西可以填".to_string());
    }
    if text.chars().count() > ITEM_FRAME_CHARS {
        return Err(format!("句型框架太長（最多 {ITEM_FRAME_CHARS} 字）"));
    }
    if contains_answer(&text, answer) {
        return Err("句型框架直接寫出了參考答案".to_string());
    }
    let (_, directions) = split_frames(std::slice::from_ref(&text));
    if !directions.is_empty() && directions.len() > 1 {
        return Err("句型框架裡寫了作答步驟；只寫有空格的句子".to_string());
    }
    Ok(text)
}

const SCAFFOLD_CHECKS: [&str; 6] = [
    "targets_match_blocks_and_warmup",
    "each_target_has_procedural",
    "kinds_from_spec_sets",
    "english_only",
    "no_answer_leak",
    "supports_wellformed",
];

/// Most steps a procedural scaffold of the scaffolds step keeps once directions written in a
/// sentence-frame box have joined it.
const MAX_STEPS_WITH_DIRECTIONS: usize = 8;

/// `text` without its parenthesised Chinese glosses ("routine (常規)", "routine（常規）"), when
/// that leaves English only; `None` when there is Chinese elsewhere too.
pub fn without_cjk_glosses(text: &str) -> Option<String> {
    let mut out = String::new();
    let mut rest = text;
    while let Some(open) = rest.find(['(', '（']) {
        let after = &rest[open..];
        let close = after.find([')', '）'])?;
        let inner = &after[after.chars().next()?.len_utf8()..close];
        out.push_str(&rest[..open]);
        if english_only(inner) {
            out.push_str(&after[..close + after[close..].chars().next()?.len_utf8()]);
        } else {
            // Drop the space before the gloss too.
            while out.ends_with(' ') {
                out.pop();
            }
        }
        rest = &after[close + after[close..].chars().next()?.len_utf8()..];
    }
    out.push_str(rest);
    let out = out.trim().to_string();
    (!out.is_empty() && english_only(&out)).then_some(out)
}

/// A target id the way the model may spell it: "warm_up", "Warm-up" and "warmup" are one,
/// as are "B1", "b1" and "[B1]".
fn target_key(s: &str) -> String {
    s.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// The requested target the model meant, if any.
pub fn canonical_target(written: &str, targets: &[String]) -> Option<String> {
    let key = target_key(written);
    targets
        .iter()
        .find(|t| !key.is_empty() && target_key(t) == key)
        .cloned()
}

pub fn evaluate_scaffolds(raw: &RawScaffolds, ctx: &ScaffoldsCtx<'_>) -> Eval<Vec<RawScaffoldSet>> {
    let mut t = Tally::new(&SCAFFOLD_CHECKS);
    let mut sets: Vec<RawScaffoldSet> = Vec::new();
    for s in &raw.scaffolds {
        let written = s.target.trim().to_string();
        let target = match canonical_target(&written, ctx.targets) {
            Some(target) => target,
            // The prompt names the warm-up's target even when the warm-up failed and was not
            // sent: a set for it is simply not needed.
            None if target_key(&written) == target_key(TARGET_WARMUP) => {
                t.fixed(
                    "targets_match_blocks_and_warmup",
                    format!("這次沒有暖身，不需要 target「{written}」。"),
                    "拿掉這一筆",
                );
                continue;
            }
            None => {
                t.fail(
                    "targets_match_blocks_and_warmup",
                    format!(
                        "target「{written}」不在要設計的區塊裡；只能用：{}。",
                        ctx.targets.join("、")
                    ),
                    "拿掉這一筆",
                );
                continue;
            }
        };
        if sets.iter().any(|x| x.target == target) {
            t.fixed(
                "targets_match_blocks_and_warmup",
                format!("target「{target}」重複了。"),
                "只留第一筆",
            );
            continue;
        }
        let mut support = |list: &[super::schemas::RawSupport], kinds: &[&str], what: &str| {
            let mut out = Vec::new();
            for sup in list {
                let mut content = clean(&sup.content_en);
                // A word bank with Chinese glosses, "behavior (行為), routine (常規)": the
                // glosses go, the English stays.
                if !english_only(&content) {
                    if let Some(english) = without_cjk_glosses(&content) {
                        t.fixed(
                            "english_only",
                            format!(
                                "{target} 的{what}「{}」在括號裡加了中文；鷹架只能用英文。",
                                sup.kind
                            ),
                            "拿掉括號裡的中文",
                        );
                        content = english;
                    }
                }
                if !kinds.contains(&sup.kind.as_str()) {
                    t.fail(
                        "kinds_from_spec_sets",
                        format!("{target} 的{what}種類「{}」不在允許的清單內。", sup.kind),
                        "拿掉這個鷹架",
                    );
                } else if content.is_empty() || !english_only(&content) {
                    t.fail(
                        "english_only",
                        format!(
                            "{target} 的{what}「{}」含有中文或沒有內容；鷹架只能用英文。",
                            sup.kind
                        ),
                        "拿掉這個鷹架",
                    );
                } else if ctx.answers.iter().any(|a| contains_answer(&content, a)) {
                    t.fail(
                        "no_answer_leak",
                        format!(
                            "{target} 的{what}「{}」直接寫出了參考答案；鷹架不可以給答案。",
                            sup.kind
                        ),
                        "拿掉這個鷹架",
                    );
                } else {
                    out.push(super::schemas::RawSupport {
                        kind: sup.kind.clone(),
                        content_en: content,
                    });
                }
            }
            out.truncate(2);
            out
        };
        let mut verbal = support(&s.verbal, &VERBAL_KINDS, "語言鷹架");
        let instructional = support(&s.instructional, &INSTRUCTIONAL_KINDS, "教學鷹架");
        let directions = frames_without_directions(&target, &mut verbal, &mut t);
        let item_frames = item_frames_of(&target, &s.item_frames, ctx, &mut t);
        let item_hints = item_hints_of(&target, &s.item_hints, ctx, &mut t);
        let mut procedural = Vec::new();
        for p in &s.procedural {
            let mut steps = clean_all(&p.steps_en);
            steps.truncate(5);
            if !PROCEDURAL_KINDS.contains(&p.kind.as_str()) {
                t.fail(
                    "kinds_from_spec_sets",
                    format!("{target} 的程序鷹架種類「{}」不在允許的清單內。", p.kind),
                    "拿掉這個鷹架",
                );
            } else if steps.is_empty() || !steps.iter().all(|s| english_only(s)) {
                t.fail(
                    "english_only",
                    format!("{target} 的程序鷹架含有中文或沒有步驟；steps_en 只能用英文。"),
                    "拿掉這個鷹架",
                );
            } else if steps
                .iter()
                .any(|s| ctx.answers.iter().any(|a| contains_answer(s, a)))
            {
                t.fail(
                    "no_answer_leak",
                    format!("{target} 的程序鷹架步驟裡寫出了參考答案。"),
                    "拿掉這個鷹架",
                );
            } else {
                procedural.push(super::schemas::RawProcedure {
                    kind: p.kind.clone(),
                    steps_en: steps,
                });
            }
        }
        procedural.truncate(2);
        if !directions.is_empty() {
            let at = procedural
                .iter()
                .position(|p| p.kind == KIND_STEPS)
                .unwrap_or(0);
            match procedural.get_mut(at) {
                Some(p) => {
                    for d in directions {
                        if !p.steps_en.iter().any(|x| norm(x) == norm(&d)) {
                            p.steps_en.push(d);
                        }
                    }
                    p.steps_en.truncate(MAX_STEPS_WITH_DIRECTIONS);
                }
                None => procedural.push(super::schemas::RawProcedure {
                    kind: KIND_STEPS.to_string(),
                    steps_en: directions
                        .into_iter()
                        .take(MAX_STEPS_WITH_DIRECTIONS)
                        .collect(),
                }),
            }
        }
        if procedural.is_empty() {
            t.fail(
                "each_target_has_procedural",
                format!(
                    "{target} 沒有可用的程序鷹架；每個區塊至少要 1 個，steps_en 列出 2–5 個步驟。"
                ),
                "由程式產生分步指示",
            );
        }
        sets.push(RawScaffoldSet {
            target,
            verbal,
            instructional,
            procedural,
            item_frames,
            item_hints,
        });
    }
    let missing: Vec<&str> = ctx
        .targets
        .iter()
        .filter(|target| !sets.iter().any(|s| &s.target == *target))
        .map(String::as_str)
        .collect();
    if !missing.is_empty() {
        t.fail(
            "targets_match_blocks_and_warmup",
            format!(
                "這些區塊沒有鷹架：{}。每個 target 都要有一筆。",
                missing.join("、")
            ),
            "缺的區塊由程式產生分步指示",
        );
    }
    let output = if sets.is_empty() { None } else { Some(sets) };
    t.finish(output)
}

/// The sentence frames of one target's questions that may stay: a frame goes only to a question
/// that wants a spoken or written answer ([`frame_format`]); one for another question, or for a
/// question that is not there, is dropped; one that is not a frame ([`item_frame`]) fails
/// `supports_wellformed`, and so asks for the repair call. One frame per question (the first).
fn item_frames_of(
    target: &str,
    raw: &[super::schemas::RawItemFrame],
    ctx: &ScaffoldsCtx<'_>,
    t: &mut Tally,
) -> Vec<super::schemas::RawItemFrame> {
    let items: &[(String, String)] = ctx
        .items
        .iter()
        .find(|x| x.target == target)
        .map_or(&[], |x| x.items.as_slice());
    let mut out: Vec<super::schemas::RawItemFrame> = Vec::new();
    for f in raw {
        let at = usize::try_from(f.item_no)
            .ok()
            .filter(|n| (1..=items.len()).contains(n));
        let Some(n) = at else {
            if !items.is_empty() {
                t.fixed(
                    "supports_wellformed",
                    format!("{target} 沒有第 {} 題，句型框架不能放在那裡。", f.item_no),
                    "拿掉這個句型框架",
                );
            }
            continue;
        };
        let (format, answer) = &items[n - 1];
        if !frame_format(Some(format)) {
            t.fixed(
                "supports_wellformed",
                format!("{target} 的第 {n} 題是{format}，不需要口說或書寫的句型框架。"),
                "拿掉這個句型框架",
            );
            continue;
        }
        if out.iter().any(|o| o.item_no == f.item_no) {
            t.fixed(
                "supports_wellformed",
                format!("{target} 的第 {n} 題有兩個句型框架。"),
                "只留第一個",
            );
            continue;
        }
        match item_frame(&f.frame_en, answer) {
            Ok(frame) => out.push(super::schemas::RawItemFrame {
                item_no: f.item_no,
                frame_en: frame,
            }),
            Err(why) => t.fail(
                "supports_wellformed",
                format!(
                    "{target} 第 {n} 題的句型框架（「{}」）有問題：{why}。",
                    clip_chars(&f.frame_en, 80)
                ),
                "拿掉這個句型框架",
            ),
        }
    }
    out
}

/// The hints of one target's questions that may stay: for a question that is there, one per
/// question (the first); one that is not a hint ([`item_hint`]) fails `supports_wellformed`.
fn item_hints_of(
    target: &str,
    raw: &[super::schemas::RawItemHint],
    ctx: &ScaffoldsCtx<'_>,
    t: &mut Tally,
) -> Vec<super::schemas::RawItemHint> {
    let items: &[(String, String)] = ctx
        .items
        .iter()
        .find(|x| x.target == target)
        .map_or(&[], |x| x.items.as_slice());
    let mut out: Vec<super::schemas::RawItemHint> = Vec::new();
    for h in raw {
        let at = usize::try_from(h.item_no)
            .ok()
            .filter(|n| (1..=items.len()).contains(n));
        let Some(n) = at else {
            if !items.is_empty() {
                t.fixed(
                    "supports_wellformed",
                    format!("{target} 沒有第 {} 題，提示不能放在那裡。", h.item_no),
                    "拿掉這個提示",
                );
            }
            continue;
        };
        if out.iter().any(|o| o.item_no == h.item_no) {
            t.fixed(
                "supports_wellformed",
                format!("{target} 的第 {n} 題有兩個提示。"),
                "只留第一個",
            );
            continue;
        }
        match item_hint(&h.hint_en, &items[n - 1].1) {
            Ok(hint) => out.push(super::schemas::RawItemHint {
                item_no: h.item_no,
                hint_en: hint,
            }),
            Err(why) => t.fail(
                "supports_wellformed",
                format!(
                    "{target} 第 {n} 題的提示（「{}」）有問題：{why}。",
                    clip_chars(&h.hint_en, 80)
                ),
                "拿掉這個提示",
            ),
        }
    }
    out
}

/// A 句型框架 support of the scaffolds step holds frames only. Directions written in it
/// (imperative steps without a blank, 「Identify the topic sentence.」) fail
/// `supports_wellformed` and are returned, to join the target's procedural steps (the block's
/// directions); a 「Use this frame:」 lead goes. A support left without frames is dropped.
fn frames_without_directions(
    target: &str,
    verbal: &mut Vec<super::schemas::RawSupport>,
    t: &mut Tally,
) -> Vec<String> {
    let mut moved = Vec::new();
    for sup in verbal.iter_mut().filter(|s| s.kind == KIND_FRAMES) {
        let (frames, directions) = split_frames(std::slice::from_ref(&sup.content_en));
        if !directions.is_empty() {
            t.fail(
                "supports_wellformed",
                format!(
                    "{target} 的句型框架裡寫了作答步驟（「{}」）：句型框架只寫有空格 ___ 讓學生完成的句子，不要寫步驟或指示；步驟寫在程序鷹架的 steps_en。",
                    clip_chars(&directions.join(" "), 100)
                ),
                "步驟移到這一區的分步指示",
            );
            moved.extend(directions);
        }
        let content = frames.join("\n");
        if content != sup.content_en.trim() {
            sup.content_en = content;
        }
    }
    verbal.retain(|s| !s.content_en.trim().is_empty());
    moved
}

// ---------------------------------------------------------------- (f) exit ticket

pub struct ExitCtx<'a> {
    pub objective_ids: &'a [String],
    /// Prompts of the main tasks: the exit ticket must not repeat them.
    pub task_prompts: &'a [String],
    /// The material, paragraphs joined (a cloze of it is not a sentence frame).
    pub passage: &'a str,
    /// The sheet's learning objectives ([`objective_lines`]): no question may copy one so that it
    /// gives the answer away ([`copied_objective`], [`CHECK_NOT_AN_OBJECTIVE`]).
    pub objectives: &'a [String],
}

const EXIT_CHECKS: [&str; 10] = [
    "labels_from_spec_sets",
    "english_only",
    "items_wellformed",
    CHECK_NOT_AN_OBJECTIVE,
    "not_a_repeat_of_main_task",
    "item_count_2_4",
    "has_formative_item",
    "has_reflection_or_question_item",
    "formative_has_sample_answer",
    "covers_every_objective",
];

/// SIOP E1: the codes of the objectives no exit-ticket item checks (in the order given).
pub fn uncovered_objectives(items: &[RawExitItem], objective_ids: &[String]) -> Vec<String> {
    objective_ids
        .iter()
        .filter(|code| !items.iter().any(|i| i.objective_refs.contains(code)))
        .cloned()
        .collect()
}

pub fn evaluate_exit(raw: &RawExit, ctx: &ExitCtx<'_>) -> Eval<Vec<RawExitItem>> {
    evaluate_exit_items(raw, ctx, true)
}

/// [`evaluate_exit`]; with `whole_set` false (one item again, 換一個 in the lesson SOP) the
/// checks that count a whole exit ticket (2–4 items, one of each kind, every objective) are
/// left out: one usable item is enough (the caller picks the one it asked for).
pub fn evaluate_exit_items(
    raw: &RawExit,
    ctx: &ExitCtx<'_>,
    whole_set: bool,
) -> Eval<Vec<RawExitItem>> {
    let mut t = Tally::new(&EXIT_CHECKS);
    let all_tasks = student_tasks();
    let prompts: HashSet<String> = ctx
        .task_prompts
        .iter()
        .map(|p| norm(p).to_lowercase())
        .collect();
    let formative = EXIT_PURPOSES[0].1;
    let mut items: Vec<RawExitItem> = Vec::new();
    for (i, it) in raw.items.iter().enumerate() {
        let name = format!("第 {} 題", i + 1);
        if !EXIT_PURPOSES.iter().any(|(_, l)| *l == it.purpose)
            || !ANSWER_FORMATS.contains(&it.answer_format.as_str())
        {
            t.fail(
                "labels_from_spec_sets",
                format!("{name} 的 purpose 或 answer_format 不在允許的清單內。"),
                "拿掉這一題",
            );
            continue;
        }
        let mut item = it.clone();
        item.prompt_en = clean_prompt(&it.prompt_en);
        item.options = clean_all(&it.options);
        item.sample_answer_en = it
            .sample_answer_en
            .as_deref()
            .map(clean)
            .filter(|s| !s.is_empty());
        if item.answer_format == FMT_CHOICE {
            let mut answer = item.sample_answer_en.clone().unwrap_or_default();
            normalize_choice(&mut item.options, &mut answer);
            if item.sample_answer_en.is_some() {
                item.sample_answer_en = Some(answer);
            }
        }
        let english = english_only(&item.prompt_en)
            && item.options.iter().all(|o| english_only(o))
            && item.sample_answer_en.as_deref().is_none_or(english_only);
        if item.prompt_en.is_empty() || !english {
            t.fail(
                "english_only",
                format!("{name} 含有中文或題目是空的；題目只能用英文。"),
                "拿掉這一題",
            );
            continue;
        }
        let answer = item.sample_answer_en.clone().unwrap_or_default();
        blank_check(
            &mut item.answer_format,
            &mut item.prompt_en,
            &answer,
            &name,
            "prompt_en",
            &mut t,
        );
        true_false_check(
            &mut item.answer_format,
            &mut item.prompt_en,
            &mut item.options,
            item.sample_answer_en.as_mut(),
            (&name, "prompt_en"),
            &mut t,
        );
        // A reflection or a question for the teacher has no right answer: it may quote one.
        if item.purpose == formative
            && !objective_check(
                (
                    &item.answer_format,
                    &item.prompt_en,
                    item.sample_answer_en.as_deref().unwrap_or_default(),
                ),
                &name,
                (ctx.objectives, ctx.passage),
                &mut t,
            )
        {
            continue;
        }
        frame_check(&item.prompt_en, ctx.passage, &name, "prompt_en", &mut t);
        if item.answer_format == FMT_CHOICE && item.options.len() < 2 {
            t.fail(
                "items_wellformed",
                format!("{name} 是選擇題，options 要列出選項。"),
                "拿掉這一題",
            );
            continue;
        }
        if prompts.contains(&norm(&item.prompt_en).to_lowercase()) {
            t.fail(
                "not_a_repeat_of_main_task",
                format!("{name} 與主要任務的題目重複。"),
                "拿掉這一題",
            );
            continue;
        }
        let before = item.student_tasks.len();
        item.student_tasks
            .retain(|l| all_tasks.contains(&l.as_str()));
        if item.student_tasks.len() != before {
            t.fail(
                "labels_from_spec_sets",
                format!("{name} 的 student_tasks 用了清單以外的標籤。"),
                "拿掉清單以外的標籤",
            );
        }
        item.objective_refs
            .retain(|r| ctx.objective_ids.contains(r));
        if item.purpose == formative && item.sample_answer_en.is_none() {
            t.fail(
                "formative_has_sample_answer",
                format!("{name} 是形成性評量，需要 sample_answer_en。"),
                "",
            );
        }
        items.push(item);
    }
    if !whole_set {
        if items.is_empty() {
            t.fail(
                "item_count_2_4",
                "items 需要 1 題可用的題目。".to_string(),
                "",
            );
        }
        let output = if items.is_empty() { None } else { Some(items) };
        return t.finish(output);
    }
    if !(2..=4).contains(&items.len()) {
        t.fail(
            "item_count_2_4",
            format!(
                "Exit ticket 需要 2–4 題可用的題目，現在有 {} 題。",
                items.len()
            ),
            if items.len() > 4 {
                "只留前 4 題"
            } else {
                ""
            },
        );
    }
    items.truncate(4);
    if !items.iter().any(|i| i.purpose == formative) {
        t.fail(
            "has_formative_item",
            "至少要有 1 題形成性評量。".to_string(),
            "",
        );
    }
    if !items.iter().any(|i| i.purpose != formative) {
        t.fail(
            "has_reflection_or_question_item",
            "至少要有 1 題學生反思或蒐集問題。".to_string(),
            "",
        );
    }
    // E1: every 內容目標 and 語言目標 is checked by at least one item (`objective_refs`).
    let uncovered = uncovered_objectives(&items, ctx.objective_ids);
    if !items.is_empty() && !uncovered.is_empty() {
        t.fail(
            "covers_every_objective",
            format!(
                "Exit ticket 要檢核這一課的每一個目標，這些目標沒有任何一題檢核（objective_refs 沒有它）：{}。請讓每個目標至少有一題，一題可以同時檢核兩個目標。",
                uncovered.join("、")
            ),
            "",
        );
    }
    for (i, it) in items.iter_mut().enumerate() {
        it.item_id = format!("E{}", i + 1);
    }
    let output = if items.is_empty() { None } else { Some(items) };
    t.finish(output)
}

/// Whether `target` is the warm-up's scaffold target.
pub fn is_warmup_target(target: &str) -> bool {
    target == TARGET_WARMUP
}

// ---------------------------------------------------------------- a block made from a template

/// One block generated from a template section (instructions may be Chinese: the template's
/// instruction language decides, so there is no English-only check here).
pub struct SectionCtx<'a> {
    /// The new material, paragraphs joined.
    pub passage: &'a str,
    pub item_count: usize,
    pub needs_external: bool,
    /// The section's schema asked for a table (表格整理 may be among its formats).
    pub tables_asked: bool,
    /// Headings and row labels of those tables are student-facing English (a revision of a
    /// worksheet), so a Chinese one is refused.
    pub english_tables: bool,
    /// The sheet's learning objectives ([`objective_lines`]): no question may copy one so that it
    /// gives the answer away ([`copied_objective`], [`CHECK_NOT_AN_OBJECTIVE`]).
    pub objectives: &'a [String],
}

#[derive(Debug, Clone, PartialEq)]
pub struct CleanSectionItem {
    pub answer_format: String,
    pub prompt: String,
    pub options: Vec<String>,
    pub answer: String,
    /// The answer's verbatim support in the material, when there is one (teacher version).
    pub evidence: Option<String>,
    /// The Bloom level the model gave the item (template blocks, prompt version 2); `None`
    /// when it gave none or a value outside the six.
    pub bloom: Option<Bloom>,
    /// 圖像組織: the cleaned organizer (`None` for other formats and when none was written).
    pub organizer: Option<Organizer>,
    /// 表格整理: the cleaned table (`None` for other formats and when the schema had none).
    pub table: Option<RawTable>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CleanSection {
    pub title: String,
    pub instructions: String,
    pub scaffold: String,
    pub items: Vec<CleanSectionItem>,
}

const SECTION_CHECKS: [&str; 9] = [
    "labels_from_spec_sets",
    "items_wellformed",
    "organizer_wellformed",
    "answer_not_in_prompt",
    CHECK_NOT_AN_OBJECTIVE,
    "item_count_within_template",
    "has_items",
    "no_invented_material",
    "syllables_match_dictionary",
];

/// Text that names a resource from outside the material: a web address or a video site.
fn names_a_resource(text: &str) -> bool {
    let t = text.to_lowercase();
    [
        "http://", "https://", "www.", "youtube", "youtu.be", "ted.com", ".com/", ".org/",
    ]
    .iter()
    .any(|k| t.contains(k))
}

fn described(corrections: &[syllables::Correction]) -> String {
    corrections
        .iter()
        .map(|c| format!("{} → {}", c.written, c.dictionary))
        .collect::<Vec<_>>()
        .join("、")
}

/// Puts the dictionary's syllable division in an item's answer (the model gets these wrong:
/// "dra-ma-tic" for dra-mat-ic). A choice item's answer has to stay one of its options, so it
/// moves to the option the dictionary agrees with; when no option does, the item is dropped.
/// Returns whether the item is kept.
fn syllable_answer_from_dictionary(
    item: &mut CleanSectionItem,
    iname: &str,
    t: &mut Tally,
) -> bool {
    if item.answer_format == FMT_CHOICE {
        if matches!(syllables::check(item.answer.trim()), Some(Err(_))) {
            let right = item
                .options
                .iter()
                .find(|o| matches!(syllables::check(o.trim()), Some(Ok(()))))
                .cloned();
            match right {
                Some(option) => {
                    t.fixed(
                        "syllables_match_dictionary",
                        format!("{iname} 的答案 {} 與字典不同，改選 {option}", item.answer),
                        "依字典改正",
                    );
                    item.answer = option;
                }
                None => {
                    t.fail(
                        "syllables_match_dictionary",
                        format!("{iname} 的選項裡沒有字典的音節劃分（{}）。", item.answer),
                        "拿掉這一題",
                    );
                    return false;
                }
            }
        }
        return true;
    }
    let (fixed, corrections) = syllables::correct(&item.answer);
    if !corrections.is_empty() {
        t.fixed(
            "syllables_match_dictionary",
            format!("{iname} 的音節劃分與字典不同：{}", described(&corrections)),
            "依字典改正",
        );
        item.answer = fixed;
    }
    true
}

/// The checks every generated item shares: a prompt, a well-formed choice item (a bare letter
/// answer "B" names the option in that position; at least 2 distinct options; the answer is one
/// of them), a matching item with options, and no answer in the stem. Records what failed;
/// returns whether the item is kept.
fn item_checks(item: &mut CleanSectionItem, iname: &str, t: &mut Tally) -> bool {
    if item.prompt.is_empty() {
        t.fail(
            "items_wellformed",
            format!("{iname} 的題目是空的。"),
            "拿掉這一題",
        );
        return false;
    }
    let answer = item.answer.clone();
    blank_check(
        &mut item.answer_format,
        &mut item.prompt,
        &answer,
        iname,
        "prompt",
        t,
    );
    true_false_check(
        &mut item.answer_format,
        &mut item.prompt,
        &mut item.options,
        Some(&mut item.answer),
        (iname, "prompt"),
        t,
    );
    if item.answer_format == FMT_CHOICE {
        normalize_choice(&mut item.options, &mut item.answer);
        let distinct: HashSet<&str> = item.options.iter().map(|o| o.trim()).collect();
        let letter = item
            .answer
            .trim()
            .trim_matches(|c| c == '(' || c == ')' || c == '.');
        let matches = item.options.iter().any(|o| o.trim() == item.answer.trim());
        if !matches && letter.len() == 1 {
            if let Some(c) = letter.chars().next().filter(char::is_ascii_alphabetic) {
                let idx = (c.to_ascii_uppercase() as u8 - b'A') as usize;
                if let Some(option) = item.options.get(idx) {
                    item.answer = option.clone();
                }
            }
        }
        if item.options.len() < 2 || distinct.len() != item.options.len() {
            t.fail(
                "items_wellformed",
                format!("{iname}（選擇題）要有至少 2 個不重複的選項。"),
                "拿掉這一題",
            );
            return false;
        }
        if !item.options.iter().any(|o| o.trim() == item.answer.trim()) {
            t.fail(
                "items_wellformed",
                format!("{iname}（選擇題）的 answer 必須與其中一個選項完全相同。"),
                "拿掉這一題",
            );
            return false;
        }
    }
    if item.answer_format == FMT_MATCHING && item.options.len() < 2 {
        t.fail(
            "items_wellformed",
            format!("{iname}（配對題）的 options 至少要 2 個。"),
            "拿掉這一題",
        );
        return false;
    }
    if contains_answer(&item.prompt, &item.answer) {
        t.fail(
            "answer_not_in_prompt",
            format!("{iname} 的題幹直接寫出了答案。"),
            "拿掉這一題",
        );
        return false;
    }
    true
}

/// Checks one generated template block: fixed answer-format labels, well-formed closed items
/// (a choice item's answer is one of its options), no answer in the stem, at most the
/// section's item count, at least one item, and, for a block that needs the teacher's own
/// material, no named or invented resource. An answer quoted from the material gets that quote
/// as its evidence (material grounding for the teacher version).
pub fn evaluate_section(
    raw: &crate::worksheet::template::RawSectionOut,
    ctx: &SectionCtx<'_>,
) -> Eval<CleanSection> {
    let mut t = Tally::new(&SECTION_CHECKS);
    let (out, _) = section_core(raw, ctx, &mut t);
    finish_section(out, ctx, t, |out| out)
}

/// The title, instructions, scaffold text and items of a generated block, cleaned and checked
/// (see [`evaluate_section`]); also whether the block is about syllables.
fn section_core(
    raw: &crate::worksheet::template::RawSectionOut,
    ctx: &SectionCtx<'_>,
    t: &mut Tally,
) -> (CleanSection, bool) {
    let mut out = CleanSection {
        title: clean(&raw.title),
        instructions: clean(&raw.instructions),
        scaffold: clean(&raw.scaffold),
        items: Vec::new(),
    };
    let syllable_section = syllables::mentions_syllables(&out.title)
        || syllables::mentions_syllables(&out.instructions);
    section_text_checks(
        [
            ("title", &mut out.title),
            ("instructions", &mut out.instructions),
            ("scaffold", &mut out.scaffold),
        ],
        syllable_section,
        ctx,
        t,
    );
    for (ii, it) in raw.items.iter().enumerate() {
        let iname = format!("第 {} 題", ii + 1);
        if let Some(item) = section_item(it, &iname, syllable_section, ctx, t) {
            out.items.push(item);
        }
    }
    (out, syllable_section)
}

/// At most the section's item count (the first ones), at least one item.
fn finish_section<T>(
    mut out: CleanSection,
    ctx: &SectionCtx<'_>,
    mut t: Tally,
    wrap: impl FnOnce(CleanSection) -> T,
) -> Eval<T> {
    if out.items.len() > ctx.item_count {
        t.fixed(
            "item_count_within_template",
            format!(
                "交出 {} 題，版型這一區是 {} 題。",
                out.items.len(),
                ctx.item_count
            ),
            "只留前面幾題",
        );
        out.items.truncate(ctx.item_count);
    }
    if out.items.is_empty() {
        t.fail(
            "has_items",
            "沒有交出可以用的題目；這一區至少要有 1 題。".to_string(),
            "",
        );
        return t.finish(None);
    }
    t.finish(Some(wrap(out)))
}

// ---------------------------------------------------------------- a template block's support boxes

const TEMPLATE_SECTION_CHECKS: [&str; 10] = [
    "labels_from_spec_sets",
    "items_wellformed",
    "organizer_wellformed",
    "answer_not_in_prompt",
    CHECK_NOT_AN_OBJECTIVE,
    "item_count_within_template",
    "has_items",
    "no_invented_material",
    "syllables_match_dictionary",
    "supports_wellformed",
];

/// One support box of a template block, checked: what it prints as, under which heading.
#[derive(Debug, Clone, PartialEq)]
pub struct CleanSupport {
    /// The sample's heading; `None` prints the kind's own heading (a template read before
    /// supports, or a box of frames the code had to add).
    pub heading: Option<String>,
    /// What the box is for (`None`: a kind no role prints as).
    pub role: Option<SupportRole>,
    pub kind_type: ScaffoldType,
    pub kind: String,
    pub lines: Vec<String>,
}

impl CleanSupport {
    fn of_role(heading: Option<String>, role: SupportRole) -> Self {
        Self {
            heading,
            role: Some(role),
            kind_type: role.kind_type(),
            kind: role.kind().to_string(),
            lines: Vec::new(),
        }
    }

    fn is_frames(&self) -> bool {
        self.kind == KIND_FRAMES
    }

    /// What the box is called in a message to the model.
    fn name(&self) -> String {
        self.heading.clone().unwrap_or_else(|| self.kind.clone())
    }
}

/// A checked template block: its text and items, and its support boxes in print order.
#[derive(Debug, Clone, PartialEq)]
pub struct TemplateSection {
    pub section: CleanSection,
    pub supports: Vec<CleanSupport>,
}

/// What a template block's support boxes are checked against.
pub struct SupportsCtx<'a> {
    /// The section's boxes, in the sample's order. Empty for a template read before supports:
    /// the block's one scaffold (`scaffold` lines) is then of the `legacy` kind.
    pub supports: &'a [WsTemplateSupport],
    pub legacy: (ScaffoldType, &'a str),
}

fn same_heading(a: &str, b: &str) -> bool {
    norm(a).to_lowercase() == norm(b).to_lowercase()
}

/// Checks a block generated from a template section like [`evaluate_section`], and its support
/// boxes: one box per box of the section (its lines, under the section's heading), or the one
/// scaffold of a template read before supports. Every box holds what its role says
/// (`supports_wellformed`): directions written in a box of sentence frames go to a Directions box
/// (the section's, or a new one before the frames), frames written among directions go to the
/// frames; a box the section has but the output left empty is reported. A question that is a
/// greeting-led frame for sharing aloud (「Hi! In Paragraph 2, …」, also wrapped in a task such as
/// 「Write … using the frame: …」) or only a sentence frame fails `items_wellformed` and leaves
/// the numbering: the greeting frame goes into the sharing-frame box, the other into a box of
/// sentence frames (as [`crate::worksheet::assemble::lift_frames`] does it, the question it
/// supports keeps its place). Boxes left empty are not printed.
pub fn evaluate_template_section(
    raw: &crate::worksheet::template::RawSectionOut,
    ctx: &SectionCtx<'_>,
    sup: &SupportsCtx<'_>,
) -> Eval<TemplateSection> {
    let mut t = Tally::new(&TEMPLATE_SECTION_CHECKS);
    let (mut out, syllable_section) = section_core(raw, ctx, &mut t);
    let mut supports = supports_of(raw, &out.scaffold, sup, syllable_section, ctx, &mut t);
    sort_supports(&mut supports, &mut t);
    lift_template_frames(&mut out.items, &mut supports, ctx.passage, &mut t);
    if !sup.supports.is_empty() {
        report_empty_boxes(&supports, &mut t);
    }
    supports.retain(|s| !s.lines.is_empty());
    finish_section(out, ctx, t, |section| TemplateSection { section, supports })
}

/// The boxes as written: one per box of the section (matched by heading, else by position), or
/// the legacy scaffold. Each line is cleaned and checked like the block's own text.
fn supports_of(
    raw: &crate::worksheet::template::RawSectionOut,
    scaffold: &str,
    sup: &SupportsCtx<'_>,
    syllable_section: bool,
    ctx: &SectionCtx<'_>,
    t: &mut Tally,
) -> Vec<CleanSupport> {
    let mut checked = |lines: &[String]| -> Vec<String> {
        lines
            .iter()
            .map(|l| {
                let mut l = clean(l);
                section_text_checks([("supports", &mut l)], syllable_section, ctx, t);
                l
            })
            .filter(|l| !l.is_empty())
            .collect()
    };
    if sup.supports.is_empty() {
        let (kind_type, kind) = sup.legacy;
        let lines: Vec<String> = scaffold.lines().map(str::to_string).collect();
        return vec![CleanSupport {
            heading: None,
            role: SupportRole::of_kind(kind),
            kind_type,
            kind: kind.to_string(),
            lines: checked(&lines),
        }];
    }
    let mut used = vec![false; raw.supports.len()];
    let mut out = Vec::new();
    for (i, ts) in sup.supports.iter().enumerate() {
        let pick = raw
            .supports
            .iter()
            .enumerate()
            .position(|(j, r)| !used[j] && same_heading(&r.heading, &ts.heading))
            .or_else(|| (i < used.len() && !used[i]).then_some(i));
        let lines = match pick {
            Some(j) => {
                used[j] = true;
                checked(&raw.supports[j].lines)
            }
            None => Vec::new(),
        };
        let mut b = CleanSupport::of_role(Some(ts.heading.clone()), ts.role);
        b.lines = lines;
        out.push(b);
    }
    out
}

/// Every box holds what its role says: directions out of the boxes of frames, frames out of the
/// boxes of directions, each to a box of its own (see [`evaluate_template_section`]). A box of
/// the section that is still empty is reported (the sample prints it).
fn sort_supports(supports: &mut Vec<CleanSupport>, t: &mut Tally) {
    // A greeting-led frame belongs in the sharing-frame box, whatever box it was written in
    // (a template read before supports wrote it in its one scaffold, e.g. 「Words in context:
    // Sentence Frame: Hi! …」). Sent back only when the section has that box to write it in.
    let has_sharing = supports
        .iter()
        .any(|s| s.role == Some(SupportRole::SharingFrame));
    let mut greetings: Vec<(String, String)> = Vec::new();
    for s in supports.iter_mut().filter(|s| {
        s.role != Some(SupportRole::SharingFrame) && s.kind_type != ScaffoldType::Procedural
    }) {
        let name = s.name();
        let mut kept = Vec::new();
        for line in std::mem::take(&mut s.lines) {
            match greeting_frame(&line, "") {
                Some(frame) => greetings.push((name.clone(), frame)),
                None => kept.push(line),
            }
        }
        s.lines = kept;
    }
    for (name, frame) in greetings {
        let problem = format!(
            "「{name}」裡寫了口頭分享用的句型（「{}」）：Hi! 開頭的分享句型寫在分享句型的提示框。",
            clip_chars(&frame, 80)
        );
        let repair = "分享句型移到分享句型方框";
        if has_sharing {
            t.fail("supports_wellformed", problem, repair);
        } else {
            t.fixed("supports_wellformed", problem, repair);
        }
        push_new(&mut frame_box(supports, true).lines, frame);
    }
    let mut directions: Vec<String> = Vec::new();
    for s in supports.iter_mut().filter(|s| s.is_frames()) {
        let (frames, moved) = split_frames(&s.lines);
        if !moved.is_empty() {
            t.fail(
                "supports_wellformed",
                format!(
                    "「{}」是句型框架，卻寫了作答步驟（「{}」）：句型框架只寫有空格 ______ 讓學生完成的句子，不要寫步驟或「Use this frame:」；步驟寫在 Directions（directions）的提示框。",
                    s.name(),
                    clip_chars(&moved.join(" "), 100)
                ),
                "步驟移到 Directions 方框",
            );
            directions.extend(moved);
        }
        s.lines = frames;
    }
    let mut frames: Vec<String> = Vec::new();
    for s in supports
        .iter_mut()
        .filter(|s| s.kind_type == ScaffoldType::Procedural)
    {
        let (steps, moved) = split_directions(&s.lines);
        if !moved.is_empty() {
            t.fail(
                "supports_wellformed",
                format!(
                    "「{}」是作答步驟，卻寫了句型框架（「{}」）：步驟只寫簡短的祈使句，不要有空格；句型框架寫在句型框架的提示框。",
                    s.name(),
                    clip_chars(&moved.join(" "), 100)
                ),
                "句型框架移到句型框架方框",
            );
            frames.extend(moved);
        }
        s.lines = steps;
    }
    if !directions.is_empty() {
        let at = match supports
            .iter()
            .position(|s| s.kind_type == ScaffoldType::Procedural)
        {
            Some(at) => at,
            None => {
                let at = supports.iter().position(|s| s.is_frames()).unwrap_or(0);
                supports.insert(
                    at,
                    CleanSupport::of_role(
                        Some(SupportRole::Directions.default_heading().to_string()),
                        SupportRole::Directions,
                    ),
                );
                at
            }
        };
        for d in directions {
            push_new(&mut supports[at].lines, d);
        }
    }
    for f in frames {
        match greeting_frame(&f, "") {
            Some(g) => push_new(&mut frame_box(supports, true).lines, g),
            None => push_new(&mut frame_box(supports, false).lines, f),
        }
    }
}

/// A box of the section that is still empty is reported (the sample prints it); it is not
/// printed.
fn report_empty_boxes(supports: &[CleanSupport], t: &mut Tally) {
    for s in supports.iter().filter(|s| s.lines.is_empty()) {
        t.fail(
            "supports_wellformed",
            format!(
                "「{}」這個提示框沒有內容；原學習單在這一區有這個提示框，要依它的用途寫出來。",
                s.name()
            ),
            "這個提示框不印",
        );
    }
}

/// Adds `line` unless the box already has it.
fn push_new(lines: &mut Vec<String>, line: String) {
    if !lines.iter().any(|l| norm(l) == norm(&line)) {
        lines.push(line);
    }
}

/// The box a frame goes into: the sharing-frame box for a greeting-led frame, otherwise the box
/// of sentence frames (any box of frames that is not for sharing). One the section does not have
/// is added at the end: 「Sentence Frame for Sharing」, or a box under the kind's own heading.
fn frame_box(supports: &mut Vec<CleanSupport>, sharing: bool) -> &mut CleanSupport {
    let at = if sharing {
        supports
            .iter()
            .position(|s| s.role == Some(SupportRole::SharingFrame))
    } else {
        supports
            .iter()
            .position(|s| s.role == Some(SupportRole::SentenceFrame))
            .or_else(|| {
                supports
                    .iter()
                    .position(|s| s.is_frames() && s.role != Some(SupportRole::SharingFrame))
            })
    };
    let at = match at {
        Some(at) => at,
        None => {
            supports.push(if sharing {
                CleanSupport::of_role(
                    Some(SupportRole::SharingFrame.default_heading().to_string()),
                    SupportRole::SharingFrame,
                )
            } else {
                CleanSupport::of_role(None, SupportRole::SentenceFrame)
            });
            supports.len() - 1
        }
    };
    &mut supports[at]
}

/// Frames written as questions leave the numbering (see [`evaluate_template_section`]). The
/// sharing-frame box gets the first greeting frame only when it has none of its own (the model
/// wrote the frame twice otherwise).
fn lift_template_frames(
    items: &mut Vec<CleanSectionItem>,
    supports: &mut Vec<CleanSupport>,
    passage: &str,
    t: &mut Tally,
) {
    let open = |i: &CleanSectionItem| {
        i.answer_format == FMT_SHORT || i.answer_format == super::FMT_PARAGRAPH
    };
    let mut kept: Vec<CleanSectionItem> = Vec::new();
    for (k, mut item) in std::mem::take(items).into_iter().enumerate() {
        let iname = format!("第 {} 題", k + 1);
        if let Some(frame) = greeting_frame(&item.prompt, passage) {
            let b = frame_box(supports, true);
            t.fail(
                "items_wellformed",
                format!(
                    "{iname}（「{}」）是口頭分享用的句型框架，不是題目：寫在「{}」的提示框（不編題號），不要寫成一題，也不要包成「Write … using the frame: …」「Complete the analysis paragraph: …」這樣的題目；題目不要用 Hi!、Hello! 這類招呼語開頭。",
                    clip_chars(&item.prompt, 80),
                    b.name()
                ),
                "移出題號，印在分享句型的方框（不編號）",
            );
            if b.lines.is_empty() {
                b.lines.push(frame);
            }
            continue;
        }
        if sentence_frame_only(&item.prompt, passage) {
            t.fail(
                "items_wellformed",
                format!(
                    "{iname} 只有句型框架（「{}」），沒有題目：句型框架是語言鷹架，不是一題，寫在句型框架的提示框；prompt 要換成一題針對課文內容的題目，寫出要學生做什麼（完整的題目或指示）；{FRAME_PREFIX_NOTE}。",
                    clip_chars(&item.prompt, 80)
                ),
                "句型框架移出題號，印在這一區的句型框架方框（不編號）",
            );
            push_new(&mut frame_box(supports, false).lines, item.prompt.clone());
            if kept.last().is_some_and(open) {
                continue;
            }
            item.prompt = crate::worksheet::assemble::FRAME_TASK.to_string();
            if !open(&item) {
                item.answer_format = FMT_SHORT.to_string();
                item.options.clear();
                item.organizer = None;
            }
            kept.push(item);
            continue;
        }
        kept.push(item);
    }
    *items = kept;
}

/// The checks of a section's own text: in a section about syllables, the dictionary's division
/// in the instructions and scaffold (an example there, "e.g., be-hav-ior", is a division too);
/// in a section that needs the teacher's material, no named or invented resource (cleared).
fn section_text_checks<const N: usize>(
    fields: [(&str, &mut String); N],
    syllable_section: bool,
    ctx: &SectionCtx<'_>,
    t: &mut Tally,
) {
    for (what, field) in fields {
        if syllable_section && what != "title" {
            let (fixed, corrections) = syllables::correct(field);
            if !corrections.is_empty() {
                t.fixed(
                    "syllables_match_dictionary",
                    format!("{what} 的音節劃分與字典不同：{}", described(&corrections)),
                    "依字典改正",
                );
                *field = fixed;
            }
        }
        if ctx.needs_external && names_a_resource(field) {
            t.fail(
                "no_invented_material",
                format!("{what} 寫出了外部素材（網址或影片網站）；這一區的素材要由教師提供，不可以寫出或編造。"),
                "清空這個欄位",
            );
            field.clear();
        }
    }
}

/// One generated item, cleaned and checked like every item of a template section (fixed
/// answer-format label, well-formed, no answer in the stem, no invented resource, the
/// dictionary's syllables); `None` when it failed (recorded in `t`). An answer quoted from the
/// material gets that quote as its evidence.
fn section_item(
    it: &crate::worksheet::template::RawOutItem,
    iname: &str,
    syllable_section: bool,
    ctx: &SectionCtx<'_>,
    t: &mut Tally,
) -> Option<CleanSectionItem> {
    let format = it.answer_format.trim().to_string();
    if !ANSWER_FORMATS.contains(&format.as_str()) {
        t.fail(
            "labels_from_spec_sets",
            format!(
                "{iname} 的 answer_format「{}」不在允許的清單內。",
                it.answer_format
            ),
            "拿掉這一題",
        );
        return None;
    }
    let mut item = CleanSectionItem {
        answer_format: format,
        prompt: clean_prompt(&it.prompt),
        options: clean_all(&it.options),
        answer: clean(&it.answer),
        evidence: None,
        // A label outside the fixed set (a provider that ignores the schema) is dropped.
        bloom: it.bloom.as_deref().and_then(Bloom::parse),
        organizer: None,
        table: None,
    };
    if !item_checks(&mut item, iname, t) {
        return None;
    }
    if !objective_check(
        (&item.answer_format, &item.prompt, &item.answer),
        iname,
        (ctx.objectives, ctx.passage),
        t,
    ) {
        return None;
    }
    // A template may be in Chinese; a revision (no organizer in its schema) keeps the stored one.
    item.organizer = organizer_of(
        &item.answer_format,
        it.organizer.as_ref(),
        iname,
        (false, false, false),
        t,
    );
    // The revision's schema has no table (asked: false); a template section's has one.
    item.table = table_of(
        &item.answer_format,
        it.table.as_ref(),
        &item.prompt,
        iname,
        (ctx.english_tables, ctx.tables_asked),
        t,
    );
    if ctx.needs_external {
        let texts = std::iter::once(&item.prompt).chain(item.options.iter());
        if texts.into_iter().any(|x| names_a_resource(x)) {
            t.fail(
                "no_invented_material",
                format!(
                    "{iname} 寫出了外部素材（網址或影片網站）；題目只能留空位給教師提供的素材。"
                ),
                "拿掉這一題",
            );
            return None;
        }
    } else {
        item.evidence = verbatim_piece(&item.answer, ctx.passage);
    }
    let about_syllables = syllable_section || syllables::mentions_syllables(&item.prompt);
    if about_syllables && !syllable_answer_from_dictionary(&mut item, iname, t) {
        return None;
    }
    Some(item)
}

// ---------------------------------------------------------------- one block revised (逐區修改)

const REVISION_CHECKS: [&str; 7] = [
    "labels_from_spec_sets",
    "items_wellformed",
    "organizer_wellformed",
    "answer_not_in_prompt",
    CHECK_NOT_AN_OBJECTIVE,
    "no_invented_material",
    "syllables_match_dictionary",
];

/// What `worksheet_block_revise` wrote for one block, before it is merged with the block.
pub struct RevisionDraft<'a> {
    /// The new title and instructions; `None` when the model left them unchanged.
    pub title: Option<&'a str>,
    pub instructions: Option<&'a str>,
    /// The block's title and instructions as they will read (to tell a section about syllables).
    pub effective_title: &'a str,
    pub effective_instructions: &'a str,
    /// The items the model modified or added, with the name the model knows them by ("Q2",
    /// "新增的第 1 題"). Items it kept are the teacher's and are never checked here.
    pub items: Vec<(String, &'a crate::worksheet::template::RawOutItem)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CleanRevision {
    pub title: Option<String>,
    pub instructions: Option<String>,
    /// One entry per item of the draft, in order: the cleaned item, or `None` when it failed.
    pub items: Vec<Option<CleanSectionItem>>,
}

/// Checks a block revision the way [`evaluate_section`] checks a template block, item by item,
/// keeping the position of every item so the caller can tell which change failed (and keep the
/// original there). There is no item-count or has-items check: the kept items are the block's.
pub fn evaluate_revision(draft: &RevisionDraft<'_>, ctx: &SectionCtx<'_>) -> Eval<CleanRevision> {
    let mut t = Tally::new(&REVISION_CHECKS);
    let mut title = draft.title.map(clean);
    let mut instructions = draft.instructions.map(clean);
    let syllable_section =
        syllables::mentions_syllables(title.as_deref().unwrap_or(draft.effective_title))
            || syllables::mentions_syllables(
                instructions
                    .as_deref()
                    .unwrap_or(draft.effective_instructions),
            );
    let mut scratch = (String::new(), String::new());
    section_text_checks(
        [
            ("title", title.as_mut().unwrap_or(&mut scratch.0)),
            (
                "instructions",
                instructions.as_mut().unwrap_or(&mut scratch.1),
            ),
        ],
        syllable_section,
        ctx,
        &mut t,
    );
    let items = draft
        .items
        .iter()
        .map(|(name, it)| section_item(it, name, syllable_section, ctx, &mut t))
        .collect();
    t.finish(Some(CleanRevision {
        title,
        instructions,
        items,
    }))
}

// ---------------------------------------------------------------- questions of one Bloom level

/// One call of `worksheet_bloom_level` (or `worksheet_tiered_level`): the level, the formats it
/// may use, the material and how many questions were asked for.
pub struct BloomCtx<'a> {
    pub bloom: Bloom,
    pub formats: &'a [&'static str],
    /// The material, paragraphs joined.
    pub passage: &'a str,
    /// "P1"…, in material order.
    pub paragraph_codes: &'a [String],
    pub count: usize,
    /// The sheet's learning objectives ([`objective_lines`]): no question may copy one so that it
    /// gives the answer away ([`copied_objective`], [`CHECK_NOT_AN_OBJECTIVE`]).
    pub objectives: &'a [String],
}

#[derive(Debug, Clone, PartialEq)]
pub struct CleanBloomItem {
    /// Format, prompt, options, answer and the verbatim evidence (if any). For a 差異化學習單
    /// question the answer is 進階版's (the full one).
    pub item: CleanSectionItem,
    /// 1-based paragraph numbers the question is about.
    pub paragraph_nos: Vec<usize>,
    pub hint: String,
    /// 差異化學習單: the three versions, in level order; empty otherwise.
    pub tiers: Vec<ItemTier>,
}

/// Linking words for one language function, cumulative per version (T5).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CleanLinking {
    pub function: String,
    pub basic: Vec<String>,
    /// 基礎版's words and the ones 一般版 adds.
    pub standard: Vec<String>,
    /// 一般版's words and the more formal ones 進階版 adds.
    pub advanced: Vec<String>,
}

impl CleanLinking {
    pub fn words(&self, level: Level) -> &[String] {
        match level {
            Level::Basic => &self.basic,
            Level::Standard => &self.standard,
            Level::Advanced => &self.advanced,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct CleanBloomLevel {
    /// The one sentence of prompt version 1 (kept for a prompt edited back to it).
    pub instructions: String,
    /// The written steps, in the order students work (R5); empty when the model gave fewer than
    /// two usable ones (the program writes its own).
    pub steps: Vec<String>,
    /// A worked or partly worked example on a question that is not on the sheet ("" for none).
    pub example: String,
    /// 差異化學習單 only.
    pub linking: Option<CleanLinking>,
    pub items: Vec<CleanBloomItem>,
}

const BLOOM_CHECKS: [&str; 10] = [
    "labels_from_spec_sets",
    "items_wellformed",
    "organizer_wellformed",
    "answer_not_in_prompt",
    CHECK_NOT_AN_OBJECTIVE,
    "evidence_verbatim",
    "item_count_within_plan",
    "has_items",
    "syllables_match_dictionary",
    "steps_written",
];

const TIERED_CHECKS: [&str; 15] = [
    "labels_from_spec_sets",
    "items_wellformed",
    "organizer_wellformed",
    "answer_not_in_prompt",
    CHECK_NOT_AN_OBJECTIVE,
    "evidence_verbatim",
    "item_count_within_plan",
    "has_items",
    "syllables_match_dictionary",
    "steps_written",
    "support_forms_by_version",
    "support_wellformed",
    "no_answer_location",
    "answers_per_version",
    "linking_words_graded",
];

/// The parts of a question that every version shares, as the model wrote them.
struct SharedRaw<'a> {
    answer_format: &'a str,
    prompt: &'a str,
    options: &'a [String],
    answer: &'a str,
    evidence: &'a str,
    paragraph_ids: &'a [String],
    organizer: Option<&'a RawOrganizer>,
    table: Option<&'a RawTable>,
    /// 差異化學習單: cells are pre-filled per version.
    tiered: bool,
}

/// The checks of a question's shared part, the way a template block is checked (the level's
/// formats only, well-formed closed items, no answer in the stem, the dictionary's syllables),
/// plus: a fill-in item has a blank, and a remember / understand question quotes the text word
/// for word as its evidence (cut down to its longest verbatim piece of five words or more; with
/// none the question fails). Other levels need no evidence: one that is not verbatim is cleared.
/// Returns the cleaned item and its paragraph numbers, or `None` when it is dropped.
fn shared_item(
    raw: SharedRaw<'_>,
    iname: &str,
    ctx: &BloomCtx<'_>,
    t: &mut Tally,
) -> Option<(CleanSectionItem, Vec<usize>)> {
    let format = raw.answer_format.trim().to_string();
    if !ctx.formats.contains(&format.as_str()) {
        t.fail(
            "labels_from_spec_sets",
            format!(
                "{iname} 的 answer_format「{}」不能用在這個層次；只能用：{}。",
                raw.answer_format,
                ctx.formats.join("、")
            ),
            "拿掉這一題",
        );
        return None;
    }
    let mut item = CleanSectionItem {
        answer_format: format,
        prompt: clean_prompt(raw.prompt),
        options: clean_all(raw.options),
        answer: clean(raw.answer),
        evidence: None,
        // The level is the call's, not the item's (see `CleanBloomItem`).
        bloom: None,
        organizer: None,
        table: None,
    };
    if item.answer_format == FMT_TRUE_FALSE && item.options.is_empty() {
        item.options = vec!["True".to_string(), "False".to_string()];
    }
    if !item_checks(&mut item, iname, t) {
        return None;
    }
    if !objective_check(
        (&item.answer_format, &item.prompt, &item.answer),
        iname,
        (ctx.objectives, ctx.passage),
        t,
    ) {
        return None;
    }
    let evidence = clean(raw.evidence);
    item.evidence = if evidence.is_empty() {
        None
    } else if let Some(span) = locate_verbatim(&evidence, ctx.passage) {
        Some(span)
    } else {
        verbatim_piece(&evidence, ctx.passage)
    };
    if ctx.bloom.needs_evidence() {
        match &item.evidence {
            None => {
                t.fail(
                    "evidence_verbatim",
                    if evidence.is_empty() {
                        format!("{iname} 沒有 evidence；這個層次的每一題都要從教材逐字複製一段連續文字當作依據。")
                    } else {
                        format!(
                            "{iname} 的 evidence「{}」不是教材的連續片段。請逐字複製一段連續文字（至少 5 個英文字），不可改寫，不可用「...」省略或拼接。",
                            clip_chars(&evidence, 120)
                        )
                    },
                    "拿掉這一題",
                );
                return None;
            }
            Some(piece) if *piece != evidence => t.fixed(
                "evidence_verbatim",
                format!(
                    "{iname} 的 evidence「{}」不完全是教材的連續片段。",
                    clip_chars(&evidence, 120)
                ),
                "只留下其中逐字的一段",
            ),
            Some(_) => {}
        }
    } else if !evidence.is_empty() && item.evidence.as_deref() != Some(evidence.as_str()) {
        t.fixed(
            "evidence_verbatim",
            format!(
                "{iname} 的 evidence「{}」不是教材的連續片段。",
                clip_chars(&evidence, 120)
            ),
            if item.evidence.is_some() {
                "只留下其中逐字的一段"
            } else {
                "清空這一題的依據"
            },
        );
    }
    if syllables::mentions_syllables(&item.prompt)
        && !syllable_answer_from_dictionary(&mut item, iname, t)
    {
        return None;
    }
    item.organizer = organizer_of(
        &item.answer_format,
        raw.organizer,
        iname,
        (
            raw.tiered,
            false,
            ctx.formats.contains(&super::FMT_ORGANIZER),
        ),
        t,
    );
    item.table = table_of(
        &item.answer_format,
        raw.table,
        &item.prompt,
        iname,
        (false, ctx.formats.contains(&super::FMT_TABLE)),
        t,
    );
    let mut paragraph_nos: Vec<usize> = raw
        .paragraph_ids
        .iter()
        .filter_map(|code| ctx.paragraph_codes.iter().position(|c| c == code.trim()))
        .map(|i| i + 1)
        .collect();
    paragraph_nos.sort_unstable();
    paragraph_nos.dedup();
    Some((item, paragraph_nos))
}

/// The written steps (R5): trimmed, empty ones and ones that give an answer away dropped, at
/// most five. Fewer than two usable steps is a failed check (the program then writes its own).
fn clean_steps(raw: &[String], answers: &[&str], t: &mut Tally) -> Vec<String> {
    let mut steps: Vec<String> = Vec::new();
    for s in raw {
        let s = clean(s);
        if s.is_empty() {
            continue;
        }
        if answers.iter().any(|a| contains_answer(&s, a)) {
            t.fixed(
                "steps_written",
                format!("步驟「{}」寫出了題目的答案。", clip_chars(&s, 80)),
                "拿掉這一步",
            );
            continue;
        }
        steps.push(s);
    }
    steps.truncate(super::tiered::MAX_STEPS);
    if steps.len() < super::tiered::MIN_STEPS {
        t.fail(
            "steps_written",
            format!(
                "steps 要依學生實際作答的順序寫 {} 到 {} 個步驟（每個步驟一個陣列元素）。",
                super::tiered::MIN_STEPS,
                super::tiered::MAX_STEPS
            ),
            "",
        );
        return Vec::new();
    }
    steps
}

/// A worked example must not be one of the sheet's answers.
fn clean_example(raw: &str, answers: &[&str], t: &mut Tally) -> String {
    let example = clean(raw);
    if !example.is_empty() && answers.iter().any(|a| contains_answer(&example, a)) {
        t.fixed(
            "steps_written",
            format!("example「{}」寫出了題目的答案。", clip_chars(&example, 80)),
            "拿掉範例",
        );
        return String::new();
    }
    example
}

/// Checks the questions of one level (閱讀理解題): every question's shared checks
/// ([`shared_item`]), its hint, the written steps and the example. More questions than asked
/// for are cut.
pub fn evaluate_bloom_level(raw: &RawBloomLevel, ctx: &BloomCtx<'_>) -> Eval<CleanBloomLevel> {
    let mut t = Tally::new(&BLOOM_CHECKS);
    let mut out = CleanBloomLevel {
        instructions: clean(&raw.instructions),
        ..CleanBloomLevel::default()
    };
    for (ii, it) in raw.items.iter().enumerate() {
        let iname = format!("第 {} 題", ii + 1);
        let shared = SharedRaw {
            answer_format: &it.answer_format,
            prompt: &it.prompt,
            options: &it.options,
            answer: &it.answer,
            evidence: &it.evidence,
            paragraph_ids: &it.paragraph_ids,
            organizer: it.organizer.as_ref(),
            table: it.table.as_ref(),
            tiered: false,
        };
        let Some((item, paragraph_nos)) = shared_item(shared, &iname, ctx, &mut t) else {
            continue;
        };
        out.items.push(CleanBloomItem {
            item,
            paragraph_nos,
            hint: clean(&it.hint).chars().take(HINT_CHARS).collect(),
            tiers: Vec::new(),
        });
    }
    cut_to_count(&mut out, ctx, &mut t);
    let answers: Vec<String> = out.items.iter().map(|i| i.item.answer.clone()).collect();
    let answers: Vec<&str> = answers.iter().map(String::as_str).collect();
    // Version 1 wrote one sentence of instructions and no steps: that is not a failure.
    if !(raw.steps.is_empty() && !out.instructions.is_empty()) {
        out.steps = clean_steps(&raw.steps, &answers, &mut t);
    }
    out.example = clean_example(&raw.example, &answers, &mut t);
    if out.items.is_empty() {
        t.fail(
            "has_items",
            "沒有交出可以用的題目；至少要有 1 題。".to_string(),
            "",
        );
        return t.finish(None);
    }
    t.finish(Some(out))
}

fn cut_to_count(out: &mut CleanBloomLevel, ctx: &BloomCtx<'_>, t: &mut Tally) {
    if out.items.len() > ctx.count {
        t.fixed(
            "item_count_within_plan",
            format!("交出 {} 題，這個層次要 {} 題。", out.items.len(), ctx.count),
            "只留前面幾題",
        );
        out.items.truncate(ctx.count);
    }
}

// ---------------------------------------------------------------- 差異化學習單 (T1–T6)

/// The blanks of a cloze or frame line: three underscores or more (「___」).
fn has_blank(s: &str) -> bool {
    s.contains("___") || s.contains("＿＿＿")
}

/// One or two full sentences: four words or more, ending like a sentence (T6).
fn is_full_sentence(s: &str) -> bool {
    let s = s.trim();
    word_count(s) >= 4
        && s.trim_end_matches(['"', '\'', '”', '’', ')'])
            .ends_with(['.', '!', '?'])
}

/// Words of a list: cleaned, empty ones and repeats (any case) dropped, at most `max`.
fn clean_words(list: &[String], max: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for w in list {
        let w = clean(w);
        if !w.is_empty() && !out.iter().any(|o| o.eq_ignore_ascii_case(&w)) {
            out.push(w);
        }
    }
    out.truncate(max);
    out
}

/// Checks one version of a question (T3–T6). Returns `None` when the version is unusable, which
/// drops the question: a question without its three versions is not differentiated.
fn tier_of(
    level: Level,
    raw: &RawVersion,
    format: &str,
    iname: &str,
    t: &mut Tally,
) -> Option<ItemTier> {
    use super::tiered::{
        allowed_kinds, points_to_answer_location, SupportKind, MAX_SUPPORT_LINES, MAX_WORD_BANK,
    };
    let vname = format!("{iname}的{}", level.label());
    let Some(kind) = SupportKind::parse(&raw.support_kind) else {
        t.fail(
            "support_forms_by_version",
            format!(
                "{vname} support_kind「{}」不是固定的支持形式。",
                clip_chars(&raw.support_kind, 30)
            ),
            "拿掉這一題",
        );
        return None;
    };
    if !allowed_kinds(level).contains(&kind) {
        t.fail(
            "support_forms_by_version",
            format!(
                "{vname}不能用「{}」；只能用：{}。",
                kind.label(),
                allowed_kinds(level)
                    .iter()
                    .map(|k| k.as_str())
                    .collect::<Vec<_>>()
                    .join("、")
            ),
            "拿掉這一題",
        );
        return None;
    }
    let mut support: Vec<String> = Vec::new();
    for line in raw
        .support
        .iter()
        .map(|l| clean(l))
        .filter(|l| !l.is_empty())
    {
        if points_to_answer_location(&line) {
            t.fail(
                "no_answer_location",
                format!(
                    "{vname}的支持「{}」是在指出答案的位置；不可以這樣給提示，請改用克漏字、字庫、句型框架或句首。",
                    clip_chars(&line, 80)
                ),
                "拿掉這一行",
            );
            continue;
        }
        support.push(line);
    }
    support.truncate(MAX_SUPPORT_LINES);
    let mut word_bank = clean_words(&raw.word_bank, MAX_WORD_BANK);
    let mut model_sentence = clean(&raw.model_sentence);
    let malformed = |t: &mut Tally, why: &str| {
        t.fail(
            "support_wellformed",
            format!("{vname}（{}）{why}", kind.label()),
            "拿掉這一題",
        );
    };
    match kind {
        SupportKind::None => {
            if !support.is_empty() || !word_bank.is_empty() {
                t.fixed(
                    "support_wellformed",
                    format!("{vname}選了不給支持，卻寫了支持的內容。"),
                    "清空支持",
                );
            }
            support.clear();
            word_bank.clear();
            model_sentence.clear();
        }
        SupportKind::SentenceStarter => {
            if support.is_empty() {
                malformed(t, "沒有寫出句首。");
                return None;
            }
            if !word_bank.is_empty() {
                t.fixed(
                    "support_wellformed",
                    format!("{vname}只給句首，字庫比句首的支持多。"),
                    "拿掉字庫",
                );
                word_bank.clear();
            }
        }
        SupportKind::WordBank => {
            if word_bank.len() < 2 {
                malformed(t, "word_bank 至少要 2 個字詞。");
                return None;
            }
        }
        SupportKind::SentenceFrame | SupportKind::Cloze => {
            if !support.iter().any(|l| has_blank(l)) {
                malformed(t, "support 要有用 ___ 表示的空格。");
                return None;
            }
            if model_sentence.is_empty() {
                malformed(t, "要先寫出完整的範句（model_sentence），再挖空。");
                return None;
            }
            // T4: the word bank is the words taken out of the model sentence.
            let sentence = model_sentence.to_lowercase();
            let before = word_bank.len();
            word_bank.retain(|w| sentence.contains(&w.to_lowercase()));
            if word_bank.len() < before {
                t.fixed(
                    "support_wellformed",
                    format!("{vname}的字庫有不在範句裡的字詞。"),
                    "只留範句裡的字詞",
                );
            }
            if word_bank.is_empty() {
                malformed(t, "word_bank 要列出從範句挖掉的字詞。");
                return None;
            }
        }
    }
    let answer = clean(&raw.answer);
    if answer.is_empty() {
        t.fail(
            "answers_per_version",
            format!("{vname}沒有參考答案；每個版本都要寫這個版本預期的答案。"),
            "拿掉這一題",
        );
        return None;
    }
    // T6: 一般版 and 進階版 want one or two full sentences (a table or organizer is filled in).
    let open_writing = format != FMT_TABLE && format != super::FMT_ORGANIZER;
    if level != Level::Basic && open_writing && !is_full_sentence(&answer) {
        t.fail(
            "answers_per_version",
            format!(
                "{vname}的參考答案「{}」要是 1–2 個完整的英文句子。",
                clip_chars(&answer, 80)
            ),
            "",
        );
    }
    let scoring = clean(&raw.scoring_zh);
    if scoring.is_empty() {
        t.fail(
            "answers_per_version",
            format!("{vname}沒有評分說明（scoring_zh）。"),
            "",
        );
    }
    Some(ItemTier {
        level,
        support_kind: kind,
        model_sentence,
        support,
        word_bank,
        answer,
        scoring,
    })
}

/// Linking words, cumulative per version (T5): each version keeps the words of the one below
/// and adds its own; a word given twice stays at the lower version.
fn linking_of(raw: &RawLinkingWords, t: &mut Tally) -> Option<CleanLinking> {
    use super::tiered::{FUNCTION_CHARS, MAX_LINKING_WORDS};
    let basic = clean_words(&raw.basic, MAX_LINKING_WORDS);
    let fresh = |list: &[String], have: &[String]| -> Vec<String> {
        clean_words(list, MAX_LINKING_WORDS)
            .into_iter()
            .filter(|w| !have.iter().any(|h| h.eq_ignore_ascii_case(w)))
            .collect()
    };
    let mut standard = basic.clone();
    standard.extend(fresh(&raw.standard_more, &basic));
    let mut advanced = standard.clone();
    advanced.extend(fresh(&raw.advanced_more, &standard));
    if advanced.is_empty() {
        return None;
    }
    if basic.is_empty() {
        t.fail(
            "linking_words_graded",
            "linking_words.basic 要先給基礎版最基本的幾個轉折詞，一般版與進階版再往上加。"
                .to_string(),
            "",
        );
    }
    Some(CleanLinking {
        function: clean(&raw.function).chars().take(FUNCTION_CHARS).collect(),
        basic,
        standard,
        advanced,
    })
}

/// Checks the questions of one level of a 差異化學習單: each question's shared part as for 閱讀理解
/// 題 ([`shared_item`], with 進階版's answer as the one the stem must not give away), then its
/// three versions ([`tier_of`]): a support form each version may use, strictly less support from
/// 基礎版 to 進階版, no hint that points to where the answer is, a cloze or frame cut from a
/// model sentence with its word bank (T4), an answer and a scoring note per version, full
/// sentences above 基礎版 (T6). Then the steps (R5), the example and the linking words (T5).
pub fn evaluate_tiered_level(raw: &RawTieredLevel, ctx: &BloomCtx<'_>) -> Eval<CleanBloomLevel> {
    let mut t = Tally::new(&TIERED_CHECKS);
    let mut out = CleanBloomLevel::default();
    let no_options: Vec<String> = Vec::new();
    'items: for (ii, it) in raw.items.iter().enumerate() {
        let iname = format!("第 {} 題", ii + 1);
        let shared = SharedRaw {
            answer_format: &it.answer_format,
            prompt: &it.prompt,
            options: &no_options,
            answer: &it.advanced.answer,
            evidence: &it.evidence,
            paragraph_ids: &it.paragraph_ids,
            organizer: it.organizer.as_ref(),
            table: it.table.as_ref(),
            tiered: true,
        };
        let Some((item, paragraph_nos)) = shared_item(shared, &iname, ctx, &mut t) else {
            continue;
        };
        let mut tiers: Vec<ItemTier> = Vec::new();
        for level in Level::ALL {
            match tier_of(
                level,
                it.version(level),
                &item.answer_format,
                &iname,
                &mut t,
            ) {
                Some(tier) => tiers.push(tier),
                None => continue 'items,
            }
        }
        let kinds: Vec<_> = tiers.iter().map(|x| x.support_kind).collect();
        if !(kinds[0] > kinds[1] && kinds[1] > kinds[2]) {
            t.fail(
                "support_forms_by_version",
                format!(
                    "{iname}的支持沒有由多到少：基礎版「{}」、一般版「{}」、進階版「{}」。基礎版要最多，進階版最少。",
                    kinds[0].label(),
                    kinds[1].label(),
                    kinds[2].label()
                ),
                "拿掉這一題",
            );
            continue;
        }
        // 一般版 and 進階版 print less support: neither may print the full answer as it.
        for tier in tiers.iter().filter(|x| x.level != Level::Basic) {
            if contains_answer(&tier.support.join(" "), &item.answer) {
                t.fail(
                    "support_wellformed",
                    format!("{iname}的{}支持直接寫出了答案。", tier.level.label()),
                    "拿掉這一題",
                );
                continue 'items;
            }
        }
        out.items.push(CleanBloomItem {
            item,
            paragraph_nos,
            hint: String::new(),
            tiers,
        });
    }
    cut_to_count(&mut out, ctx, &mut t);
    let answers: Vec<String> = out
        .items
        .iter()
        .flat_map(|i| i.tiers.iter().map(|x| x.answer.clone()))
        .collect();
    let answers: Vec<&str> = answers.iter().map(String::as_str).collect();
    out.steps = clean_steps(&raw.steps, &answers, &mut t);
    out.example = clean_example(&raw.example, &answers, &mut t);
    out.linking = linking_of(&raw.linking_words, &mut t);
    if out.items.is_empty() {
        t.fail(
            "has_items",
            "沒有交出可以用的題目；至少要有 1 題。".to_string(),
            "",
        );
        return t.finish(None);
    }
    t.finish(Some(out))
}

/// One question's level as the judge classified it.
#[derive(Debug, Clone, PartialEq)]
pub struct Judged {
    /// Index into the ids the judge was given.
    pub index: usize,
    pub bloom: Bloom,
    pub reason: String,
}

const BLOOM_CHECK_CHECKS: [&str; 2] = ["ids_valid", "every_item_judged"];

/// Checks the judge's output: every id it was given, once, with a level from the fixed set.
/// Unknown ids and levels are dropped, a repeated id keeps its first judgement; a question left
/// out is a failure (one repair call), after which it simply stays unjudged.
pub fn evaluate_bloom_check(raw: &RawBloomCheck, ids: &[String]) -> Eval<Vec<Judged>> {
    let mut t = Tally::new(&BLOOM_CHECK_CHECKS);
    let mut out: Vec<Judged> = Vec::new();
    for j in &raw.items {
        let Some(index) = ids.iter().position(|id| id == j.id.trim()) else {
            t.fail(
                "ids_valid",
                format!("「{}」不是要判斷的題號。", clip_chars(&j.id, 20)),
                "拿掉這一筆",
            );
            continue;
        };
        let Some(bloom) = Bloom::parse(&j.bloom) else {
            t.fail(
                "ids_valid",
                format!(
                    "{} 的 bloom「{}」不是六個層次之一。",
                    ids[index],
                    clip_chars(&j.bloom, 20)
                ),
                "拿掉這一筆",
            );
            continue;
        };
        if out.iter().any(|o| o.index == index) {
            t.fixed(
                "ids_valid",
                format!("{} 判斷了兩次。", ids[index]),
                "只留第一次的判斷",
            );
            continue;
        }
        out.push(Judged {
            index,
            bloom,
            reason: clean(&j.reason).chars().take(REASON_CHARS).collect(),
        });
    }
    let missing: Vec<&str> = ids
        .iter()
        .enumerate()
        .filter(|(i, _)| !out.iter().any(|o| o.index == *i))
        .map(|(_, id)| id.as_str())
        .collect();
    if !missing.is_empty() {
        t.fail(
            "every_item_judged",
            format!(
                "這些題目沒有判斷：{}。每一題都要有一筆。",
                missing.join("、")
            ),
            "",
        );
    }
    out.sort_by_key(|j| j.index);
    if out.is_empty() {
        return t.finish(None);
    }
    t.finish(Some(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worksheet::schemas::{RawProcedure, RawSupport};
    use crate::worksheet::{FMT_PARAGRAPH, FMT_SHORT};

    fn choice_item(options: &[&str]) -> RawItem {
        serde_json::from_value(serde_json::json!({
            "item_id":"q1","answer_format":FMT_CHOICE,"tier":"core","prompt_en":"Choose.",
            "options":options,"answer_en":"One","evidence":null
        }))
        .unwrap()
    }

    #[test]
    fn without_a_habit_the_option_rules_are_exactly_the_old_ones() {
        // The switch off (or no count habit): exactly 4 distinct options, matching ≥ 3, with
        // the same messages as before teacher memory existed.
        let none = OptionHabits::default();
        assert!(item_wellformed(&choice_item(&["One", "Two", "Three", "Four"]), none).is_ok());
        for options in [
            &["One", "Two"][..],
            &["One", "Two", "Three"],
            &["One", "Two", "Three", "Four", "Five"],
            &["One", "Two", "Two", "Four"],
        ] {
            assert_eq!(
                item_wellformed(&choice_item(options), none).unwrap_err(),
                "選擇題要有 4 個不重複的選項"
            );
        }
        assert_eq!(
            item_wellformed(&choice_item(&["Two", "Three", "Four", "Five"]), none).unwrap_err(),
            "選擇題的 answer_en 必須與其中一個選項完全相同"
        );
        let mut matching = choice_item(&["a", "b"]);
        matching.answer_format = FMT_MATCHING.into();
        assert_eq!(
            item_wellformed(&matching, none).unwrap_err(),
            "配對題的 options 至少要 3 個"
        );
        matching.options.push("c".into());
        assert!(item_wellformed(&matching, none).is_ok());
    }

    #[test]
    fn a_confirmed_count_habit_additionally_allows_that_count_only() {
        let three = OptionHabits {
            choice_options: Some(3),
            matching_pairs: Some(2),
        };
        assert!(item_wellformed(&choice_item(&["One", "Two", "Three"]), three).is_ok());
        // The default stays acceptable (the teacher's explicit request for this run wins).
        assert!(item_wellformed(&choice_item(&["One", "Two", "Three", "Four"]), three).is_ok());
        for options in [
            &["One", "Two"][..],
            &["One", "Two", "Three", "Four", "Five"],
        ] {
            assert_eq!(
                item_wellformed(&choice_item(options), three).unwrap_err(),
                "選擇題要有 4 個或 3 個（老師確認的習慣）不重複的選項"
            );
        }
        assert!(item_wellformed(&choice_item(&["One", "Two", "Two"]), three).is_err());
        let mut matching = choice_item(&["a", "b"]);
        matching.answer_format = FMT_MATCHING.into();
        assert!(item_wellformed(&matching, three).is_ok());
        matching.options.pop();
        assert_eq!(
            item_wellformed(&matching, three).unwrap_err(),
            "配對題的 options 至少要 2 個"
        );
        // A habit above the default minimum does not make matching stricter.
        let six = OptionHabits {
            choice_options: Some(5),
            matching_pairs: Some(6),
        };
        let mut matching = choice_item(&["a", "b", "c"]);
        matching.answer_format = FMT_MATCHING.into();
        assert!(item_wellformed(&matching, six).is_ok());
        assert!(
            item_wellformed(&choice_item(&["One", "Two", "Three", "Four", "Five"]), six).is_ok()
        );
    }

    fn out_item(
        format: &str,
        prompt: &str,
        options: &[&str],
        answer: &str,
    ) -> crate::worksheet::template::RawOutItem {
        crate::worksheet::template::RawOutItem {
            answer_format: format.into(),
            prompt: prompt.into(),
            options: options.iter().map(|o| o.to_string()).collect(),
            answer: answer.into(),
            bloom: None,
            organizer: None,
            table: None,
        }
    }

    #[test]
    fn choice_letters_are_stripped_and_letter_answers_become_the_option() {
        let mut options: Vec<String> = [
            "A. To stop fishing",
            "B. To use fewer bags",
            "C. To build a museum",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let mut answer = "B".to_string();
        normalize_choice(&mut options, &mut answer);
        assert_eq!(
            options,
            ["To stop fishing", "To use fewer bags", "To build a museum"]
        );
        assert_eq!(answer, "To use fewer bags");

        let mut options: Vec<String> = ["(A) red", "(B) blue"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let mut answer = "(B) Blue.".to_string();
        normalize_choice(&mut options, &mut answer);
        assert_eq!(answer, "blue");

        // Options that merely start with a capital "A" word are left alone.
        let mut options: Vec<String> = ["A lot of people", "Few people"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let mut answer = "few people".to_string();
        normalize_choice(&mut options, &mut answer);
        assert_eq!(options, ["A lot of people", "Few people"]);
        assert_eq!(answer, "Few people");
    }

    #[test]
    fn syllable_answers_come_from_the_dictionary() {
        let passage = paragraphs().join("\n");
        let raw = crate::worksheet::template::RawSectionOut {
            title: "Task I. Syllable Separation and Oral Reading".into(),
            instructions: "Separate the words into syllables (e.g., be-ha-vior).".into(),
            scaffold: "".into(),
            supports: Vec::new(),
            items: vec![
                out_item(FMT_BLANK, "dramatic", &[], "dra-ma-tic"),
                out_item(FMT_BLANK, "invisible", &[], "in-vis-i-ble"),
                out_item(FMT_BLANK, "Kaohsiung", &[], "Kao-hsi-ung"),
                out_item(
                    FMT_CHOICE,
                    "Which division is right?",
                    &["au-to-ma-tic", "au-to-mat-ic"],
                    "au-to-ma-tic",
                ),
                out_item(
                    FMT_CHOICE,
                    "Which division is right?",
                    &["pat-ience", "pati-ence"],
                    "pat-ience",
                ),
            ],
        };
        let ctx = SectionCtx {
            passage: &passage,
            item_count: 5,
            needs_external: false,
            tables_asked: false,
            english_tables: false,
            objectives: &[],
        };
        let e = evaluate_section(&raw, &ctx);
        let out = e.output.unwrap();
        assert_eq!(
            out.instructions,
            "Separate the words into syllables (e.g., be-hav-ior)."
        );
        let answers: Vec<&str> = out.items.iter().map(|i| i.answer.as_str()).collect();
        assert_eq!(
            answers,
            ["dra-mat-ic", "in-vis-i-ble", "Kao-hsi-ung", "au-to-mat-ic"],
            "corrected, kept, unknown word left alone, choice moved; no right option: dropped"
        );
        let result = e
            .results
            .iter()
            .find(|r| r.name == "syllables_match_dictionary")
            .unwrap();
        assert!(!result.passed, "the dropped choice item fails the check");

        // Outside a syllable section an answer with a hyphen is not touched.
        let other = crate::worksheet::template::RawSectionOut {
            title: "Paragraph 1: Details".into(),
            instructions: "Answer.".into(),
            scaffold: "".into(),
            supports: Vec::new(),
            items: vec![out_item(FMT_BLANK, "Word?", &[], "dra-ma-tic")],
        };
        let out = evaluate_section(&other, &ctx).output.unwrap();
        assert_eq!(out.items[0].answer, "dra-ma-tic");
    }

    #[test]
    fn template_sections_are_checked_and_grounded() {
        let passage = paragraphs().join("\n");
        let raw = crate::worksheet::template::RawSectionOut {
            title: "Paragraph 1: Details".into(),
            instructions: "請閱讀第一段並回答。".into(),
            scaffold: "".into(),
            supports: Vec::new(),
            items: vec![
                out_item(
                    FMT_CHOICE,
                    "Which sentence is in paragraph 1?",
                    &[P1, "No.", "Yes."],
                    "A",
                ),
                out_item(FMT_CHOICE, "Pick one.", &["a", "a"], "a"),
                out_item("探究", "Explore.", &[], "x"),
                out_item("簡答題", &format!("Why? {P2}"), &[], P2),
                out_item("簡答題", "What happened first?", &[], "Something happened."),
                out_item("簡答題", "And then?", &[], "More."),
            ],
        };
        let e = evaluate_section(
            &raw,
            &SectionCtx {
                passage: &passage,
                item_count: 1,
                needs_external: false,
                tables_asked: false,
                english_tables: false,
                objectives: &[],
            },
        );
        assert!(e.needs_repair());
        let out = e.output.unwrap();
        // Chinese instructions are fine here (the template's language decides).
        assert_eq!(out.instructions, "請閱讀第一段並回答。");
        assert_eq!(out.items.len(), 1, "capped at item_count");
        assert_eq!(out.items[0].answer, P1, "a bare letter names the option");
        assert_eq!(out.items[0].evidence.as_deref(), Some(P1));

        let external = crate::worksheet::template::RawSectionOut {
            title: "Watch https://youtube.com/x".into(),
            instructions: "Watch the video your teacher gives you.".into(),
            scaffold: "".into(),
            supports: Vec::new(),
            items: vec![
                out_item(
                    "簡答題",
                    "See www.ted.com/talk and answer: ______",
                    &[],
                    "x",
                ),
                out_item(
                    "簡答題",
                    "After the material, write one key point: ______",
                    &[],
                    P1,
                ),
            ],
        };
        let e = evaluate_section(
            &external,
            &SectionCtx {
                passage: &passage,
                item_count: 3,
                needs_external: true,
                tables_asked: false,
                english_tables: false,
                objectives: &[],
            },
        );
        let out = e.output.unwrap();
        assert_eq!(out.title, "");
        assert_eq!(out.items.len(), 1);
        assert_eq!(out.items[0].evidence, None);
        let empty = crate::worksheet::template::RawSectionOut {
            items: Vec::new(),
            ..external
        };
        let e = evaluate_section(
            &empty,
            &SectionCtx {
                passage: &passage,
                item_count: 3,
                needs_external: true,
                tables_asked: false,
                english_tables: false,
                objectives: &[],
            },
        );
        assert!(e.output.is_none() && e.needs_repair());
    }

    const P1: &str = "Mrs. Lin put an umbrella stand outside her shop on a rainy Monday.";
    const P2: &str =
        "Her neighbors laughed at the idea. They were sure every umbrella would be gone.";
    const P3: &str = "In the first month, twelve umbrellas did disappear. However, something unexpected followed.";

    fn paragraphs() -> Vec<String> {
        vec![P1.to_string(), P2.to_string(), P3.to_string()]
    }

    fn item(format: &str, prompt: &str, answer: &str) -> RawItem {
        RawItem {
            item_id: "x".into(),
            answer_format: format.into(),
            tier: "core".into(),
            prompt_en: prompt.into(),
            options: vec![],
            organizer: None,
            table: None,
            answer_en: answer.into(),
            evidence: None,
            bloom: None,
        }
    }

    fn block(nos: &[i64], function: &str, items: Vec<RawItem>) -> RawBlock {
        RawBlock {
            block_id: "B".into(),
            paragraph_nos: nos.to_vec(),
            title_en: "Read".into(),
            info_function: function.into(),
            practice: None,
            student_tasks: vec!["理解內容".into()],
            objective_refs: vec![],
            instructions_en: "Answer.".into(),
            items,
            design_reason_zh: "理由".into(),
            image_slots: Vec::new(),
        }
    }

    /// The blocks with their Q4 stages written in order, as version 3 of the prompt asks.
    fn staged(mut blocks: Vec<RawBlock>) -> Vec<RawBlock> {
        let n = blocks.len();
        for (i, b) in blocks.iter_mut().enumerate() {
            b.practice = Some(Practice::for_position(i, n).label().to_string());
        }
        blocks
    }

    fn failed(e: &[CheckResult]) -> Vec<&'static str> {
        e.iter().filter(|r| !r.passed).map(|r| r.name).collect()
    }

    fn tasks_ctx<'a>(paragraphs: &'a [String], objectives: &'a [String]) -> TasksCtx<'a> {
        TasksCtx {
            paragraphs,
            objective_ids: objectives,
            item_budget: 6,
            only: None,
            habits: OptionHabits::default(),
            objectives: &[],
        }
    }

    #[test]
    fn good_tasks_pass_and_get_sequential_ids() {
        let p = paragraphs();
        let raw = RawTasks {
            blocks: staged(vec![
                block(
                    &[1],
                    "發現資訊",
                    vec![item("簡答題", "Where was the stand?", "Outside her shop.")],
                ),
                block(
                    &[2],
                    "分析資訊",
                    vec![item(
                        "簡答題",
                        "Why did they laugh?",
                        "They expected theft.",
                    )],
                ),
                block(
                    &[3],
                    "創造資訊",
                    vec![item("段落寫作", "Write your own ending.", "Answers vary.")],
                ),
            ]),
        };
        let e = evaluate_tasks(&raw, &tasks_ctx(&p, &[]));
        assert!(!e.needs_repair(), "{:?}", failed(&e.results));
        let blocks = e.output.unwrap();
        assert_eq!(blocks[2].block_id, "B3");
        assert_eq!(blocks[2].items[0].item_id, "B3-1");
    }

    #[test]
    fn task_organizers_are_cleaned_and_english_only() {
        use crate::worksheet::organizer::{RawOrgCell, RawOrgSection};
        let p = paragraphs();
        let org = |label: &str| RawOrganizer {
            kind: "t_chart".into(),
            title: String::new(),
            center: "not a mind map".into(),
            sections: [label, "After"]
                .iter()
                .map(|l| RawOrgSection {
                    label: l.to_string(),
                    cells: vec![RawOrgCell {
                        answer: "rain".into(),
                        ..RawOrgCell::default()
                    }],
                })
                .collect(),
        };
        let with = |format: &str, label: &str| {
            let mut it = item(format, "Complete the chart.", "Answers vary.");
            it.organizer = Some(org(label));
            RawTasks {
                blocks: staged(vec![block(&[1, 2, 3], "整理資訊", vec![it])]),
            }
        };
        let e = evaluate_tasks(&with("圖像組織", "Before"), &tasks_ctx(&p, &[]));
        assert!(!failed(&e.results).contains(&"english_only"));
        let kept = e.output.unwrap()[0].items[0].organizer.clone().unwrap();
        assert_eq!(kept.kind, "t_chart");
        assert!(kept.center.is_empty(), "the centre is only for a mind map");
        // Chinese in an organizer: a repair call, and the organizer is dropped (a box prints).
        let e = evaluate_tasks(&with("圖像組織", "之前"), &tasks_ctx(&p, &[]));
        assert!(failed(&e.results).contains(&"english_only"));
        assert!(e.output.unwrap()[0].items[0].organizer.is_none());
        // An organizer on another format is dropped without a repair call.
        let e = evaluate_tasks(&with("簡答題", "Before"), &tasks_ctx(&p, &[]));
        assert!(!failed(&e.results).contains(&"organizer_wellformed"));
        assert!(e.output.unwrap()[0].items[0].organizer.is_none());
    }

    #[test]
    fn task_tables_carry_real_headings_and_row_labels() {
        let p = paragraphs();
        let prompt = "Complete a T-chart with two columns: Unconventional Path as a Positive Choice and Speaker Justifying His Decision. For each interpretation, record its claim and one supporting detail.";
        let with = |format: &str, prompt: &str, table: Option<RawTable>| {
            let mut it = item(format, prompt, "Answers vary.");
            it.table = table;
            RawTasks {
                blocks: staged(vec![block(&[1, 2, 3], "整理資訊", vec![it])]),
            }
        };
        let table = |cols: &[&str], rows: &[&str]| RawTable {
            columns: cols.iter().map(|c| c.to_string()).collect(),
            row_labels: rows.iter().map(|c| c.to_string()).collect(),
        };
        // Real headings and labels pass untouched.
        let good = table(&["Path", "Speaker"], &["Claim", "Supporting detail"]);
        let e = evaluate_tasks(
            &with("表格整理", prompt, Some(good.clone())),
            &tasks_ctx(&p, &[]),
        );
        assert!(e.results.iter().all(|r| r.passed), "{:?}", e.problems);
        assert_eq!(e.output.unwrap()[0].items[0].table, Some(good));
        // 「Column 1」: repaired from the question's own words, no repair call.
        let bad = table(&["Column 1", "Column 2"], &["Claim", "Supporting detail"]);
        let e = evaluate_tasks(&with("表格整理", prompt, Some(bad)), &tasks_ctx(&p, &[]));
        assert!(!e.needs_repair(), "{:?}", e.problems);
        assert!(e.repairs.iter().any(|r| r.contains("欄名改用題目裡寫的")));
        let t = e.output.unwrap()[0].items[0].table.clone().unwrap();
        assert_eq!(
            t.columns,
            [
                "Unconventional Path as a Positive Choice",
                "Speaker Justifying His Decision"
            ]
        );
        // The question names nothing and the model wrote placeholders and no labels: a repair
        // call, and the question stays (the teacher fills the headings in).
        let e = evaluate_tasks(
            &with(
                "表格整理",
                "Complete the table.",
                Some(table(&["Column 1", "Column 2"], &[])),
            ),
            &tasks_ctx(&p, &[]),
        );
        assert_eq!(failed(&e.results), ["items_wellformed"]);
        let t = e.output.unwrap()[0].items[0].table.clone().unwrap();
        assert_eq!(t.columns, ["", ""]);
        // Chinese headings in a student table: a repair call, the table is dropped.
        let e = evaluate_tasks(
            &with(
                "表格整理",
                prompt,
                Some(table(&["優點", "Cons"], &["Claim"])),
            ),
            &tasks_ctx(&p, &[]),
        );
        assert_eq!(failed(&e.results), ["items_wellformed"]);
        assert!(e.output.unwrap()[0].items[0].table.is_none());
        // A table on another format is dropped without a repair call.
        let e = evaluate_tasks(
            &with("簡答題", prompt, Some(table(&["A", "B"], &["Claim"]))),
            &tasks_ctx(&p, &[]),
        );
        assert!(!e.needs_repair());
        assert!(e.output.unwrap()[0].items[0].table.is_none());
    }

    #[test]
    fn tasks_go_from_modelling_to_independent_work() {
        let p = paragraphs();
        let three = |stages: [Option<&str>; 3]| RawTasks {
            blocks: vec![
                (1, "發現資訊", stages[0]),
                (2, "分析資訊", stages[1]),
                (3, "創造資訊", stages[2]),
            ]
            .into_iter()
            .map(|(no, f, s)| {
                let mut b = block(&[no], f, vec![item("簡答題", &format!("Q{no}?"), "A.")]);
                b.practice = s.map(str::to_string);
                b
            })
            .collect(),
        };
        // In order: no repair.
        let ok = three([Some("示範"), Some("同儕練習"), Some("獨立應用")]);
        assert!(!evaluate_tasks(&ok, &tasks_ctx(&p, &[])).needs_repair());
        // Backwards, a label outside the four, or missing: one repair call, and the blocks are
        // relabelled in order so the output stays usable.
        for bad in [
            three([Some("獨立應用"), Some("同儕練習"), Some("示範")]),
            three([Some("示範"), Some("討論"), Some("獨立應用")]),
            three([None, None, None]),
        ] {
            let e = evaluate_tasks(&bad, &tasks_ctx(&p, &[]));
            assert_eq!(failed(&e.results), ["practice_in_order"]);
            assert!(e.problems.iter().any(|p| p.contains("示範 → 共同練習")));
            let stages: Vec<String> = e
                .output
                .unwrap()
                .into_iter()
                .map(|b| b.practice.unwrap())
                .collect();
            assert_eq!(stages, ["示範", "同儕練習", "獨立應用"]);
        }
        // Reordered blocks are judged in their new order (the stage follows the position).
        let mut swapped = three([Some("示範"), Some("同儕練習"), Some("獨立應用")]);
        swapped.blocks.swap(0, 2);
        let e = evaluate_tasks(&swapped, &tasks_ctx(&p, &[]));
        let names = failed(&e.results);
        assert!(names.contains(&"blocks_in_paragraph_order"), "{names:?}");
        assert!(!names.contains(&"practice_in_order"), "{names:?}");
        // A single-block regeneration keeps its block's stage (set by the caller).
        let only = [2i64];
        let ctx = TasksCtx {
            only: Some(&only),
            ..tasks_ctx(&p, &[])
        };
        let one = RawTasks {
            blocks: vec![block(&[2], "分析資訊", vec![item("簡答題", "Q?", "A.")])],
        };
        assert!(!evaluate_tasks(&one, &ctx).needs_repair());
    }

    #[test]
    fn tasks_drop_what_fails_and_say_why() {
        let p = paragraphs();
        let mut cjk = item("簡答題", "請回答 Why?", "Because.");
        cjk.tier = "support".into();
        let mut spliced = item(
            "簡答題",
            "What happened in the first month?",
            "Twelve disappeared.",
        );
        spliced.evidence = Some("In the first month ... unexpected followed.".into());
        let mut mcq = item("選擇題", "Pick one.", "A");
        mcq.options = vec!["A".into(), "B".into()];
        let leak = item(
            "簡答題",
            "They were sure every umbrella would be gone. What were they sure about?",
            "every umbrella would be gone",
        );
        let mut labelled = block(
            &[2],
            "發現資訊",
            vec![item("簡答題", "Who laughed?", "The neighbors."), leak],
        );
        labelled.student_tasks.push("探究".into());
        labelled.objective_refs = vec!["O9".into()];
        let raw = RawTasks {
            blocks: vec![
                block(
                    &[1],
                    "發現資訊",
                    vec![item("簡答題", "Where?", "Outside."), cjk, mcq],
                ),
                labelled,
                block(&[3], "發現資訊", vec![spliced]),
                block(&[3], "高層次思考", vec![item("簡答題", "Q?", "A.")]),
            ],
        };
        let objectives = vec!["O1".to_string()];
        let e = evaluate_tasks(&raw, &tasks_ctx(&p, &objectives));
        let names = failed(&e.results);
        for expected in [
            "labels_from_spec_sets",
            "english_only",
            "items_wellformed",
            "answer_not_in_prompt",
            "objective_refs_valid",
            "at_least_3_info_functions",
        ] {
            assert!(names.contains(&expected), "{expected} in {names:?}");
        }
        let blocks = e.output.unwrap();
        assert_eq!(
            blocks.len(),
            3,
            "the block with a label outside the set is gone"
        );
        assert_eq!(blocks[0].items.len(), 1);
        assert_eq!(blocks[1].student_tasks, vec!["理解內容".to_string()]);
        assert!(blocks[1].objective_refs.is_empty());
        assert_eq!(blocks[1].items.len(), 1);
        // The spliced evidence has no verbatim piece of five words: cleared in code, reported
        // as repaired (not as a failure).
        assert_eq!(blocks[2].items[0].evidence, None);
        let evidence = e
            .results
            .iter()
            .find(|r| r.name == "evidence_verbatim")
            .unwrap();
        assert!(evidence.passed);
        assert!(evidence
            .detail
            .as_deref()
            .unwrap()
            .contains("清空這一題的依據"));
        assert!(!e.repairs.is_empty() && !e.problems.is_empty());
    }

    #[test]
    fn blanks_are_recognised_in_every_form_the_sheet_prints() {
        for prompt in [
            "The town was ______.",
            "The town was ___ and ___.",
            "The town was ＿＿＿.",
            "The town was ( ).",
            "The town was (   ).",
            "The town was （　）.",
            "The town was （）.",
        ] {
            assert!(has_blank_marker(prompt), "{prompt}");
        }
        for prompt in [
            "The town was quiet.",
            "The town_was quiet.",
            "Call f() twice.",
            "The town was __ quiet.",
        ] {
            assert!(!has_blank_marker(prompt), "{prompt}");
        }
    }

    #[test]
    fn a_fill_in_without_a_blank_is_blanked_from_its_answer_or_made_a_short_answer() {
        // The teacher's report: the full sentences, the answer's phrases listed apart.
        let prompt = "Narrative writing helps readers understand a sequence of events and the emotions of the characters... When a text creates a vivid image of the scene, it becomes more engaging and persuasive.";
        let answer = "Narrative writing, understand a sequence of events and the emotions of the characters, vivid image of the scene, engaging, persuasive";
        assert_eq!(
            missing_blank(prompt, answer),
            MissingBlank::Blanked(
                "______ helps readers ______... When a text creates a ______, it becomes more ______ and ______."
                    .into()
            )
        );
        // Any case, whole words only, numbering and end punctuation of the answer ignored.
        assert_eq!(
            missing_blank(
                "The stand was outside her shop. Outside, it rained; the outsider waited.",
                "(1) outside."
            ),
            MissingBlank::Blanked(
                "The stand was ______ her shop. ______, it rained; the outsider waited.".into()
            )
        );
        assert_eq!(
            missing_blank(
                "Mrs. Lin put an umbrella stand outside.",
                "1. umbrella stand; 2) a"
            ),
            MissingBlank::Blanked("Mrs. Lin put an ______ outside.".into()),
            "a part shorter than three characters is not blanked"
        );
        // Nothing of the answer in the prompt, or nothing left to read: a 簡答題.
        assert_eq!(
            missing_blank("Mrs. Lin put an umbrella stand", "outside"),
            MissingBlank::ShortAnswer
        );
        assert_eq!(
            missing_blank("Narrative writing.", "narrative writing"),
            MissingBlank::ShortAnswer
        );
        assert_eq!(missing_blank("Why?", ""), MissingBlank::ShortAnswer);
    }

    #[test]
    fn a_fill_in_needs_a_blank_in_every_generator() {
        let p = paragraphs();
        let passage = p.join("\n");
        // 完整學習單 (tasks): a blank passes; none fails and is blanked from the answer; none and
        // no answer in the prompt fails and becomes a 簡答題.
        let raw = RawTasks {
            blocks: staged(vec![block(
                &[1],
                "發現資訊",
                vec![
                    item(
                        "填空題",
                        "Mrs. Lin put an umbrella stand ______ her shop.",
                        "outside",
                    ),
                    item(
                        "填空題",
                        "Mrs. Lin put an umbrella stand outside her shop.",
                        "outside",
                    ),
                    item(
                        "填空題",
                        "What did Mrs. Lin put outside?",
                        "an umbrella stand",
                    ),
                ],
            )]),
        };
        let one = TasksCtx {
            only: Some(&[1]),
            ..tasks_ctx(&p, &[])
        };
        let e = evaluate_tasks(&raw, &one);
        assert_eq!(failed(&e.results), ["items_wellformed"]);
        assert!(
            e.problems.iter().all(|p| p.contains("沒有空格")),
            "{:?}",
            e.problems
        );
        let items = &e.output.unwrap()[0].items;
        assert_eq!(items.len(), 3);
        assert_eq!(
            items[0].prompt_en,
            "Mrs. Lin put an umbrella stand ______ her shop."
        );
        assert_eq!(
            items[1].prompt_en,
            "Mrs. Lin put an umbrella stand ______ her shop."
        );
        assert_eq!(items[1].answer_format, "填空題");
        assert_eq!(items[2].answer_format, FMT_SHORT);
        assert!(e
            .repairs
            .iter()
            .any(|r| r.starts_with("把題目裡參考答案的字詞換成 ______")));
        assert!(e
            .repairs
            .iter()
            .any(|r| r.starts_with("題目裡沒有參考答案的字詞可以挖空，改成簡答題")));
        // With the blank everywhere, nothing fails.
        let raw = RawTasks {
            blocks: staged(vec![block(
                &[1],
                "發現資訊",
                vec![item("填空題", "The stand was outside her ( ).", "shop")],
            )]),
        };
        assert!(!evaluate_tasks(&raw, &one).needs_repair());

        // 照版型產生, 逐區修改 and the prep revise (evaluate_section / evaluate_revision).
        let raw = crate::worksheet::template::RawSectionOut {
            title: "Paragraph 2".into(),
            instructions: "Answer.".into(),
            scaffold: "".into(),
            supports: Vec::new(),
            items: vec![out_item(
                FMT_BLANK,
                "Her neighbors laughed at the idea.",
                &[],
                "laughed",
            )],
        };
        let ctx = SectionCtx {
            passage: &passage,
            item_count: 5,
            needs_external: false,
            tables_asked: false,
            english_tables: false,
            objectives: &[],
        };
        let e = evaluate_section(&raw, &ctx);
        assert_eq!(failed(&e.results), ["items_wellformed"]);
        let out = e.output.unwrap();
        assert_eq!(out.items[0].prompt, "Her neighbors ______ at the idea.");
        assert_eq!(out.items[0].answer_format, FMT_BLANK);

        // Exit ticket: a reflection has no answer to blank: a 簡答題.
        let raw = RawExit {
            estimated_minutes: 3,
            items: vec![
                exit_item(
                    "E1",
                    "形成性評量",
                    "Today I learned that ______.",
                    Some("trust matters"),
                ),
                exit_item(
                    "E2",
                    "學生反思",
                    "The hardest part of today was the reading.",
                    None,
                ),
            ],
        };
        let e = evaluate_exit(
            &raw,
            &ExitCtx {
                objective_ids: &[],
                task_prompts: &[],
                passage: "",
                objectives: &[],
            },
        );
        assert_eq!(failed(&e.results), ["items_wellformed"]);
        let items = e.output.unwrap();
        assert_eq!(items[0].answer_format, FMT_BLANK);
        assert_eq!(items[1].answer_format, FMT_SHORT);
    }

    #[test]
    fn a_sentence_frame_alone_is_not_a_question() {
        let passage = "I went to the market with my mother. The neighbors thought the umbrellas would be gone.";
        for frame in [
            // The teacher's report: printed as question 20.
            "Hi! In the 3rd paragraph, the author describes ______. After reading it carefully, I identified ______ as one of the main characteristics of narrative writing, as seen in the text where ______.",
            "I think ______ because ______.",
            "Today I learned that ___.",
            "In my opinion, the ending is ______ because ______.",
        ] {
            assert!(sentence_frame_only(frame, passage), "{frame}");
        }
        for question in [
            // Not the student's voice: a fill-in question about the text.
            "The neighbors thought the ______ would be gone.",
            // First person, but quoted from the material: a cloze.
            "I ______ to the market with my mother.",
            // A question or an instruction of its own, with a frame to use.
            "What do I think about the ______?",
            "Use the frame to answer: I think ______ because ______.",
            "Complete the sentence: I identified ______ as a feature of the story.",
            "Explain your choice. I chose ______ because ______.",
            // No blank to complete.
            "I think the story is sad.",
        ] {
            assert!(!sentence_frame_only(question, passage), "{question}");
        }
    }

    #[test]
    fn an_exit_question_that_is_only_a_frame_is_sent_back() {
        let frame = "Hi! In the 3rd paragraph, the author describes ______. After reading it carefully, I identified ______ as one of the main characteristics of narrative writing.";
        let mut first = exit_item("E1", "形成性評量", frame, Some("the setting"));
        first.answer_format = FMT_SHORT.into();
        let mut second = exit_item(
            "E2",
            "學生反思",
            "Which part of today's text was the hardest for you?",
            None,
        );
        second.answer_format = FMT_SHORT.into();
        let raw = RawExit {
            estimated_minutes: 3,
            items: vec![first, second],
        };
        let e = evaluate_exit(
            &raw,
            &ExitCtx {
                objective_ids: &[],
                task_prompts: &[],
                passage: &paragraphs().join("\n"),
                objectives: &[],
            },
        );
        assert_eq!(failed(&e.results), ["items_wellformed"]);
        assert!(e.problems[0].contains("只有句型框架"), "{:?}", e.problems);
        assert!(e.problems[0].contains("招呼語"));
        assert!(e.repairs[0].starts_with("句型框架移出題號"));
        // Kept: the block is assembled with the frame in its box (assemble::lift_frames).
        assert_eq!(e.output.unwrap()[0].prompt_en, frame);
    }

    fn exit_item(id: &str, purpose: &str, prompt: &str, answer: Option<&str>) -> RawExitItem {
        RawExitItem {
            item_id: id.into(),
            purpose: purpose.into(),
            prompt_en: prompt.into(),
            answer_format: FMT_BLANK.into(),
            options: vec![],
            student_tasks: vec!["學習反思".into()],
            objective_refs: vec![],
            sample_answer_en: answer.map(str::to_string),
        }
    }

    #[test]
    fn spliced_evidence_is_cut_to_its_verbatim_piece_without_a_repair_call() {
        let p = paragraphs();
        let passage = p.join("\n");
        // Two sentences that are not next to each other (the model's habit), and a "..." splice.
        assert_eq!(
            verbatim_piece(
                "Mrs. Lin put an umbrella stand outside her shop on a rainy Monday. However, something unexpected followed.",
                &passage
            )
            .as_deref(),
            Some("Mrs. Lin put an umbrella stand outside her shop on a rainy Monday.")
        );
        assert_eq!(
            verbatim_piece(
                "In the first month, twelve umbrellas did disappear … something unexpected",
                &passage
            )
            .as_deref(),
            Some("In the first month, twelve umbrellas did disappear")
        );
        assert_eq!(
            verbatim_piece("Her neighbors ... the idea.", &passage),
            None
        );
        assert_eq!(
            verbatim_piece("Nothing like the text at all here.", &passage),
            None
        );

        let mut spliced = item(
            "簡答題",
            "What happened in the first month?",
            "Twelve went.",
        );
        spliced.evidence = Some(
            "In the first month, twelve umbrellas did disappear. ... something unexpected followed."
                .into(),
        );
        let raw = RawTasks {
            blocks: staged(vec![
                block(&[1], "發現資訊", vec![item("簡答題", "Where?", "Outside.")]),
                block(&[2], "分析資訊", vec![spliced]),
                block(
                    &[3],
                    "創造資訊",
                    vec![item("段落寫作", "Write.", "Varies.")],
                ),
            ]),
        };
        let e = evaluate_tasks(&raw, &tasks_ctx(&p, &[]));
        assert!(!e.needs_repair(), "{:?}", failed(&e.results));
        let blocks = e.output.unwrap();
        assert_eq!(
            blocks[1].items[0].evidence.as_deref(),
            Some("In the first month, twelve umbrellas did disappear.")
        );
        assert!(e.repairs.iter().any(|r| r.contains("只留下其中逐字的一段")));
        // If a repair call is made for another reason, the model hears about it too.
        assert!(e.problems.iter().any(|p| p.contains("evidence")));
    }

    #[test]
    fn tasks_report_uncovered_paragraphs_and_trim_to_the_budget() {
        let p = paragraphs();
        let many: Vec<RawItem> = (0..12)
            .map(|i| item("簡答題", &format!("Question {i}?"), "Yes."))
            .collect();
        let raw = RawTasks {
            blocks: vec![
                block(&[2], "發現資訊", many),
                block(&[1], "分析資訊", vec![item("簡答題", "Q?", "A.")]),
            ],
        };
        let mut ctx = tasks_ctx(&p, &[]);
        ctx.item_budget = 4;
        let e = evaluate_tasks(&raw, &ctx);
        let names = failed(&e.results);
        assert!(names.contains(&"all_paragraphs_covered"));
        assert!(names.contains(&"blocks_in_paragraph_order"));
        assert!(names.contains(&"item_count_within_budget"));
        let blocks = e.output.unwrap();
        assert_eq!(blocks[0].paragraph_nos, vec![1]);
        let core: usize = blocks.iter().map(|b| b.items.len()).sum();
        assert_eq!(core, 6, "1.5 times the budget of 4");
        assert_eq!(uncovered_paragraphs(&blocks, 3), vec![3]);
    }

    #[test]
    fn warmup_keeps_only_candidates_and_levels_from_data() {
        let p = paragraphs();
        let candidates = vec![
            Candidate {
                word: "disappear".into(),
                level: Some(2),
                paragraph_no: 3,
                pos: "v.".into(),
                sentence: "In the first month, twelve umbrellas did disappear.".into(),
            },
            Candidate {
                word: "unexpected".into(),
                level: Some(4),
                paragraph_no: 3,
                pos: "adj.".into(),
                sentence: "However, something unexpected followed.".into(),
            },
        ];
        let raw = RawWarmup {
            background_activities: vec![RawBackground {
                title_en: "Before you read".into(),
                instructions_en: "Talk to a partner.".into(),
                prompts_en: vec!["Have you ever lent something to a stranger?".into()],
                student_tasks: vec!["討論／分享".into(), "合作活動".into()],
                answer_format: "簡答題".into(),
                purpose_zh: "連結個人經驗".into(),
            }],
            key_vocabulary: vec![
                RawKeyWord {
                    word: "Disappear".into(),
                    paragraph_no: 3,
                    context_sentence: "twelve umbrellas did disappear".into(),
                    student_friendly_definition_en: "to go away".into(),
                    pos: "verb".into(),
                    meaning_zh: "消失".into(),
                },
                RawKeyWord {
                    word: "unexpected".into(),
                    paragraph_no: 1,
                    context_sentence: "made up sentence".into(),
                    student_friendly_definition_en: "沒想到的".into(),
                    pos: String::new(),
                    meaning_zh: String::new(),
                },
                RawKeyWord {
                    word: "trust".into(),
                    paragraph_no: 1,
                    context_sentence: "x".into(),
                    student_friendly_definition_en: "belief".into(),
                    pos: "n.".into(),
                    meaning_zh: "belief".into(),
                },
            ],
        };
        let e = evaluate_warmup(
            &raw,
            &WarmupCtx {
                paragraphs: &p,
                candidates: &candidates,
                vocab_count: 6,
            },
        );
        let names = failed(&e.results);
        for expected in [
            "labels_from_spec_sets",
            "vocab_only_from_candidates",
            "english_only",
        ] {
            assert!(names.contains(&expected), "{expected} in {names:?}");
        }
        // A context sentence that is not verbatim is replaced by the one the code found:
        // repaired, not a reason for another call.
        let context = e
            .results
            .iter()
            .find(|r| r.name == "context_sentence_verbatim")
            .unwrap();
        assert!(context.passed && context.detail.is_some());
        let w = e.output.unwrap();
        assert_eq!(
            w.activities[0].student_tasks,
            vec!["討論／分享".to_string()]
        );
        assert_eq!(w.words.len(), 2);
        assert_eq!(
            (w.words[0].word.as_str(), w.words[0].level),
            ("disappear", Some(2))
        );
        assert_eq!(
            w.words[1].context,
            "However, something unexpected followed."
        );
        assert_eq!(w.words[1].definition, "");
        // The model's part of speech is normalised and checked against the list's.
        assert_eq!(
            (w.words[0].pos.as_str(), w.words[0].meaning_zh.as_str()),
            ("v.", "消失")
        );
        assert_eq!(w.words[1].pos, "adj.", "none given: the list's single one");
        assert_eq!(w.words[1].meaning_zh, "");
    }
    use crate::worksheet::schemas::{RawBackground, RawKeyWord};

    #[test]
    fn scaffolds_need_a_procedure_and_must_not_give_the_answer() {
        let targets = vec!["warm_up".to_string(), "B1".to_string(), "B2".to_string()];
        let answers = vec!["because a stranger once shared his umbrella".to_string()];
        let steps = |s: &[&str]| RawProcedure {
            kind: "清楚的分步指示及完成順序".into(),
            steps_en: s.iter().map(|x| x.to_string()).collect(),
        };
        let raw = RawScaffolds {
            scaffolds: vec![
                RawScaffoldSet {
                    item_frames: Vec::new(),
                    item_hints: Vec::new(),
                    target: "warm_up".into(),
                    // Directions written among the frames (a teacher's 「Sentence frames」 box).
                    verbal: vec![RawSupport {
                        kind: "句型框架".into(),
                        content_en: "Circle the key word in the title.\nUse this frame: I think ___ because ___.".into(),
                    }],
                    instructional: vec![],
                    procedural: vec![steps(&["Read.", "Talk."])],
                },
                RawScaffoldSet {
                    item_frames: Vec::new(),
                    item_hints: Vec::new(),
                    target: "B1".into(),
                    verbal: vec![
                        RawSupport {
                            kind: "句型框架".into(),
                            content_en: "She started it ___ because ___.".into(),
                        },
                        RawSupport {
                            kind: "作答示例".into(),
                            content_en:
                                "She did it because a stranger once shared his umbrella with her."
                                    .into(),
                        },
                        RawSupport {
                            kind: "自創".into(),
                            content_en: "x".into(),
                        },
                    ],
                    instructional: vec![RawSupport {
                        kind: "圖像組織".into(),
                        content_en: "請畫表格".into(),
                    }],
                    procedural: vec![],
                },
                RawScaffoldSet {
                    item_frames: Vec::new(),
                    item_hints: Vec::new(),
                    target: "B9".into(),
                    verbal: vec![],
                    instructional: vec![],
                    procedural: vec![steps(&["Go."])],
                },
            ],
        };
        let e = evaluate_scaffolds(
            &raw,
            &ScaffoldsCtx {
                targets: &targets,
                answers: &answers,
                items: &[],
            },
        );
        let names = failed(&e.results);
        for expected in SCAFFOLD_CHECKS {
            assert!(names.contains(&expected), "{expected} in {names:?}");
        }
        let sets = e.output.unwrap();
        assert_eq!(sets.len(), 2);
        assert_eq!(sets[1].verbal.len(), 1);
        assert!(sets[1].instructional.is_empty() && sets[1].procedural.is_empty());
        // The direction joined the steps; the frame stays, without its lead.
        assert_eq!(
            sets[0].procedural[0].steps_en,
            ["Read.", "Talk.", "Circle the key word in the title."]
        );
        assert_eq!(sets[0].verbal[0].content_en, "I think ___ because ___.");
    }

    #[test]
    fn chinese_glosses_are_taken_out_of_a_word_bank() {
        assert_eq!(
            without_cjk_glosses("Word Bank: behavior (行為), automatic（自動的）, effort (try)")
                .as_deref(),
            Some("Word Bank: behavior, automatic, effort (try)")
        );
        assert_eq!(without_cjk_glosses("請用這個句型 (frame)"), None);
        assert_eq!(without_cjk_glosses("(行為)"), None);
        assert_eq!(without_cjk_glosses("unclosed (行為"), None);

        let raw = RawScaffolds {
            scaffolds: vec![RawScaffoldSet {
                item_frames: Vec::new(),
                item_hints: Vec::new(),
                target: "B1".into(),
                verbal: vec![RawSupport {
                    kind: "字詞表".into(),
                    content_en: "Word Bank: habit (習慣), routine (常規)".into(),
                }],
                instructional: vec![],
                procedural: vec![RawProcedure {
                    kind: "清楚的分步指示及完成順序".into(),
                    steps_en: vec!["Read.".into(), "Write.".into()],
                }],
            }],
        };
        let e = evaluate_scaffolds(
            &raw,
            &ScaffoldsCtx {
                targets: &["B1".to_string()],
                answers: &[],
                items: &[],
            },
        );
        assert!(!e.needs_repair(), "{:?}", failed(&e.results));
        assert_eq!(
            e.output.unwrap()[0].verbal[0].content_en,
            "Word Bank: habit, routine"
        );
    }

    #[test]
    fn scaffold_targets_are_matched_however_they_are_spelled() {
        let set = |target: &str| RawScaffoldSet {
            item_frames: Vec::new(),
            item_hints: Vec::new(),
            target: target.into(),
            verbal: vec![],
            instructional: vec![],
            procedural: vec![RawProcedure {
                kind: "清楚的分步指示及完成順序".into(),
                steps_en: vec!["Read.".into(), "Write.".into()],
            }],
        };
        assert_eq!(
            canonical_target("Warm-up", &["warm_up".into()]).as_deref(),
            Some("warm_up")
        );
        assert_eq!(
            canonical_target("[b2]", &["B1".into(), "B2".into()]).as_deref(),
            Some("B2")
        );
        assert_eq!(canonical_target("", &["B1".into()]), None);

        // The warm-up failed, so only the blocks were sent; the model still wrote a warm-up
        // set (the prompt describes its target) and spelled the blocks its own way.
        let targets = vec!["B1".to_string(), "B2".to_string()];
        let raw = RawScaffolds {
            scaffolds: vec![set("warmup"), set("b1"), set("[B2]"), set("B2")],
        };
        let e = evaluate_scaffolds(
            &raw,
            &ScaffoldsCtx {
                targets: &targets,
                answers: &[],
                items: &[],
            },
        );
        assert!(!e.needs_repair(), "{:?}", failed(&e.results));
        let sets = e.output.unwrap();
        let names: Vec<&str> = sets.iter().map(|s| s.target.as_str()).collect();
        assert_eq!(names, ["B1", "B2"]);
        let check = e
            .results
            .iter()
            .find(|r| r.name == "targets_match_blocks_and_warmup")
            .unwrap();
        assert!(check.detail.as_deref().unwrap().contains("沒有暖身"));

        // With the warm-up requested, "warm-up" is its set; a made-up target still fails.
        let targets = vec!["warm_up".to_string(), "B1".to_string()];
        let raw = RawScaffolds {
            scaffolds: vec![set("Warm-up"), set("B1"), set("B7")],
        };
        let e = evaluate_scaffolds(
            &raw,
            &ScaffoldsCtx {
                targets: &targets,
                answers: &[],
                items: &[],
            },
        );
        assert_eq!(failed(&e.results), ["targets_match_blocks_and_warmup"]);
        let sets = e.output.unwrap();
        assert_eq!(sets[0].target, "warm_up");
        assert_eq!(sets.len(), 2);
    }

    #[test]
    fn exit_ticket_needs_both_purposes() {
        let ex = |purpose: &str, prompt: &str, sample: Option<&str>| RawExitItem {
            item_id: "x".into(),
            purpose: purpose.into(),
            prompt_en: prompt.into(),
            answer_format: "簡答題".into(),
            options: vec![],
            student_tasks: vec!["學習反思".into()],
            objective_refs: vec!["O1".into(), "O7".into()],
            sample_answer_en: sample.map(str::to_string),
        };
        let objectives = vec!["O1".to_string()];
        let prompts = vec!["Why did they laugh?".to_string()];
        let ctx = ExitCtx {
            objective_ids: &objectives,
            task_prompts: &prompts,
            passage: "",
            objectives: &[],
        };
        let good = RawExit {
            estimated_minutes: 3,
            items: vec![
                ex(
                    "形成性評量",
                    "Explain the main idea in one sentence.",
                    Some("Trust grows when it is given."),
                ),
                ex("學生反思", "What was hard today?", None),
            ],
        };
        let e = evaluate_exit(&good, &ctx);
        assert!(!e.needs_repair(), "{:?}", failed(&e.results));
        let items = e.output.unwrap();
        assert_eq!(items[0].objective_refs, vec!["O1".to_string()]);
        assert_eq!(items[1].item_id, "E2");

        let bad = RawExit {
            estimated_minutes: 3,
            items: vec![
                ex("形成性評量", "why did they laugh?", None),
                ex("形成性評量", "今天學到什麼？", None),
            ],
        };
        let e = evaluate_exit(&bad, &ctx);
        let names = failed(&e.results);
        for expected in [
            "english_only",
            "not_a_repeat_of_main_task",
            "item_count_2_4",
            "has_formative_item",
            "has_reflection_or_question_item",
        ] {
            assert!(names.contains(&expected), "{expected} in {names:?}");
        }
        assert!(e.output.is_none());
    }

    #[test]
    fn exit_ticket_checks_every_objective() {
        let ex = |purpose: &str, prompt: &str, refs: &[&str]| RawExitItem {
            item_id: "x".into(),
            purpose: purpose.into(),
            prompt_en: prompt.into(),
            answer_format: "簡答題".into(),
            options: vec![],
            student_tasks: vec![],
            objective_refs: refs.iter().map(|r| r.to_string()).collect(),
            sample_answer_en: (purpose == "形成性評量").then(|| "A sample.".to_string()),
        };
        let objectives = vec!["O1".to_string(), "O2".to_string(), "O3".to_string()];
        let ctx = ExitCtx {
            objective_ids: &objectives,
            task_prompts: &[],
            passage: "",
            objectives: &[],
        };
        let raw = RawExit {
            estimated_minutes: 3,
            items: vec![
                ex("形成性評量", "Explain the main idea.", &["O1"]),
                ex("學生反思", "Which signal word helped you most?", &["O3"]),
            ],
        };
        let e = evaluate_exit(&raw, &ctx);
        assert_eq!(failed(&e.results), ["covers_every_objective"]);
        assert!(e.problems[0].contains("O2"), "{:?}", e.problems);
        // Still usable: after the repair call the teacher gets it, with a notice.
        let items = e.output.unwrap();
        assert_eq!(uncovered_objectives(&items, &objectives), ["O2"]);

        // One item may check two objectives.
        let raw = RawExit {
            estimated_minutes: 3,
            items: vec![
                ex(
                    "形成性評量",
                    "Explain the main idea with because.",
                    &["O1", "O2"],
                ),
                ex("學生反思", "Which signal word helped you most?", &["O3"]),
            ],
        };
        assert!(!evaluate_exit(&raw, &ctx).needs_repair());
        // Without objectives there is nothing to cover.
        let none = ExitCtx {
            objective_ids: &[],
            task_prompts: &[],
            passage: "",
            objectives: &[],
        };
        assert!(!evaluate_exit(&raw, &none).needs_repair());
    }

    fn bloom_item(
        format: &str,
        prompt: &str,
        options: &[&str],
        answer: &str,
        evidence: &str,
    ) -> crate::worksheet::bloom::RawBloomItem {
        crate::worksheet::bloom::RawBloomItem {
            answer_format: format.into(),
            prompt: prompt.into(),
            options: options.iter().map(|o| o.to_string()).collect(),
            answer: answer.into(),
            evidence: evidence.into(),
            paragraph_ids: vec!["P2".into(), "P9".into()],
            hint: "Look at paragraph 2.".into(),
            organizer: None,
            table: None,
        }
    }

    #[test]
    fn remember_questions_must_quote_the_text() {
        let passage = [P1, P2, P3].join("\n");
        let codes = crate::worksheet::bloom::paragraph_codes(3);
        let ctx = |bloom, count| BloomCtx {
            bloom,
            formats: bloom_formats(bloom),
            passage: &passage,
            paragraph_codes: &codes,
            count,
            objectives: &[],
        };
        fn bloom_formats(b: Bloom) -> &'static [&'static str] {
            b.formats()
        }
        // Prompt version 1: one sentence of instructions, no steps (not a failure).
        let raw = RawBloomLevel {
            instructions: "Answer the questions.".into(),
            steps: vec![],
            example: String::new(),
            items: vec![
                // verbatim
                bloom_item(
                    FMT_SHORT,
                    "Where did Mrs. Lin put the stand?",
                    &[],
                    "Outside her shop.",
                    P1,
                ),
                // not in the text at all: fails, repair
                bloom_item(
                    FMT_SHORT,
                    "What day was it?",
                    &[],
                    "Monday.",
                    "It was a sunny Tuesday in the city.",
                ),
                // a spliced quote with a verbatim piece: cut down, no repair needed for it
                bloom_item(
                    FMT_SHORT,
                    "What happened?",
                    &[],
                    "Umbrellas disappeared.",
                    "In the first month, twelve umbrellas did disappear ... and so on",
                ),
                // a format the level may not use
                bloom_item(FMT_PARAGRAPH, "Write about it.", &[], "Answers vary.", P1),
                // a fill-in item without a blank, its answer nowhere in it: a 簡答題
                bloom_item(
                    FMT_BLANK,
                    "Mrs. Lin put an umbrella stand",
                    &[],
                    "outside",
                    P1,
                ),
            ],
        };
        let e = evaluate_bloom_level(&raw, &ctx(Bloom::Remember, 8));
        assert!(e.needs_repair());
        let failed: Vec<&str> = e
            .results
            .iter()
            .filter(|r| !r.passed)
            .map(|r| r.name)
            .collect();
        assert_eq!(
            failed,
            [
                "labels_from_spec_sets",
                "items_wellformed",
                "evidence_verbatim"
            ]
        );
        let out = e.output.unwrap();
        assert_eq!(out.items.len(), 3);
        assert_eq!(out.items[0].item.evidence.as_deref(), Some(P1));
        assert_eq!(
            out.items[1].item.evidence.as_deref(),
            Some("In the first month, twelve umbrellas did disappear")
        );
        assert_eq!(out.items[2].item.answer_format, FMT_SHORT);
        assert!(e.repairs.iter().any(|r| r.contains("改成簡答題")));
        assert_eq!(out.items[0].paragraph_nos, [2], "unknown codes are dropped");
        assert_eq!(out.items[0].hint, "Look at paragraph 2.");

        // Analyze needs no evidence: one that is not verbatim is cleared without a repair.
        let raw = RawBloomLevel {
            instructions: String::new(),
            steps: vec![
                "Read the question and think about why the writer says it.".into(),
                "Write your answer in one or two sentences.".into(),
            ],
            example: String::new(),
            items: vec![
                bloom_item(
                    FMT_SHORT,
                    "Why does the writer mention the first month?",
                    &[],
                    "To show the change.",
                    "made up words that are not there",
                ),
                bloom_item(
                    FMT_CHOICE,
                    "What is the purpose of paragraph 3?",
                    &["To warn", "To surprise", "To list", "To end"],
                    "B",
                    "",
                ),
                bloom_item(FMT_SHORT, "Third question?", &[], "x", ""),
            ],
        };
        let e = evaluate_bloom_level(&raw, &ctx(Bloom::Analyze, 2));
        assert!(!e.needs_repair(), "{:?}", e.results);
        let out = e.output.unwrap();
        assert_eq!(out.items.len(), 2, "cut to the count");
        assert_eq!(out.items[0].item.evidence, None);
        assert_eq!(
            out.items[1].item.answer, "To surprise",
            "a letter names its option"
        );
        assert!(e.repairs.iter().any(|r| r.contains("只留前面幾題")));

        // Nothing usable: no output.
        let raw = RawBloomLevel {
            instructions: String::new(),
            steps: vec![],
            example: String::new(),
            items: vec![bloom_item("探究", "x", &[], "y", "")],
        };
        assert!(evaluate_bloom_level(&raw, &ctx(Bloom::Create, 1))
            .output
            .is_none());
    }

    // ------------------------------------------------------------ 差異化學習單

    fn version(
        kind: &str,
        model: &str,
        support: &[&str],
        bank: &[&str],
        answer: &str,
    ) -> RawVersion {
        RawVersion {
            support_kind: kind.into(),
            model_sentence: model.into(),
            support: support.iter().map(|s| s.to_string()).collect(),
            word_bank: bank.iter().map(|s| s.to_string()).collect(),
            answer: answer.into(),
            scoring_zh: "內容正確即給分".into(),
        }
    }

    /// A well-formed 差異化學習單 question: a frame for 基礎版, a starter for 一般版, nothing for
    /// 進階版.
    fn tiered_item(prompt: &str) -> crate::worksheet::tiered::RawTieredItem {
        crate::worksheet::tiered::RawTieredItem {
            answer_format: FMT_SHORT.into(),
            prompt: prompt.into(),
            evidence: P2.into(),
            paragraph_ids: vec!["P2".into()],
            organizer: None,
            table: None,
            advanced: version(
                "none",
                "",
                &[],
                &[],
                "They were sure that every umbrella would be gone within a week.",
            ),
            standard: version(
                "sentence_starter",
                "",
                &["The neighbors thought that …"],
                &[],
                "They thought every umbrella would be gone.",
            ),
            basic: version(
                "sentence_frame",
                "The neighbors thought the umbrellas would be gone.",
                &["The neighbors thought the ___ would be ___."],
                &["umbrellas", "gone"],
                "umbrellas, gone",
            ),
        }
    }

    fn tiered_raw(items: Vec<crate::worksheet::tiered::RawTieredItem>) -> RawTieredLevel {
        RawTieredLevel {
            items,
            steps: vec![
                "Read the question first.".into(),
                "Underline the words in the text that help you.".into(),
                "Write your answer with the help under the question.".into(),
            ],
            example: "Q: Who is the story about? A: It is about a shop owner.".into(),
            linking_words: RawLinkingWords {
                function: "giving reasons".into(),
                basic: vec!["because".into(), "so".into()],
                standard_more: vec!["since".into(), "because".into()],
                advanced_more: vec!["consequently".into()],
            },
        }
    }

    fn tiered_ctx<'a>(passage: &'a str, codes: &'a [String], count: usize) -> BloomCtx<'a> {
        BloomCtx {
            bloom: Bloom::Understand,
            formats: &[FMT_SHORT],
            passage,
            paragraph_codes: codes,
            count,
            objectives: &[],
        }
    }

    #[test]
    fn a_differentiated_question_is_written_once_with_three_versions() {
        let passage = [P1, P2, P3].join("\n");
        let codes = crate::worksheet::bloom::paragraph_codes(3);
        let e = evaluate_tiered_level(
            &tiered_raw(vec![tiered_item(
                "What did the neighbors think would happen?",
            )]),
            &tiered_ctx(&passage, &codes, 1),
        );
        assert!(!e.needs_repair(), "{:?}", e.problems);
        let out = e.output.unwrap();
        let q = &out.items[0];
        assert_eq!(q.item.prompt, "What did the neighbors think would happen?");
        let levels: Vec<Level> = q.tiers.iter().map(|t| t.level).collect();
        assert_eq!(levels, Level::ALL, "one part per version, in level order");
        use crate::worksheet::SupportKind::*;
        let kinds: Vec<_> = q.tiers.iter().map(|t| t.support_kind).collect();
        assert_eq!(kinds, [SentenceFrame, SentenceStarter, None]);
        // T6: per-version answers; the shared answer is 進階版's full one.
        assert_eq!(
            q.tiers[0].answer, "umbrellas, gone",
            "基礎版 accepts a phrase"
        );
        assert_eq!(q.item.answer, q.tiers[2].answer);
        assert!(q.tiers.iter().all(|t| !t.scoring.is_empty()));
        // R5 steps, the example, and the linking words, cumulative per version (T5).
        assert_eq!(out.steps.len(), 3);
        assert!(!out.example.is_empty());
        let l = out.linking.unwrap();
        assert_eq!(l.basic, ["because", "so"]);
        assert_eq!(l.standard, ["because", "so", "since"], "repeats stay lower");
        assert_eq!(l.advanced, ["because", "so", "since", "consequently"]);
    }

    #[test]
    fn support_must_go_down_and_never_point_to_the_answer() {
        let passage = [P1, P2, P3].join("\n");
        let codes = crate::worksheet::bloom::paragraph_codes(3);
        let ctx = tiered_ctx(&passage, &codes, 5);
        // 1. As much support in 一般版 as in 基礎版: dropped, repair.
        let mut flat = tiered_item("Question one?");
        flat.standard = flat.basic.clone();
        // 2. A form 進階版 may not use: dropped.
        let mut rich = tiered_item("Question two?");
        rich.advanced = rich.basic.clone();
        // 3. A hint that gives the answer's location: the line goes, the question stays.
        let mut located = tiered_item("Question three?");
        located.basic.support.push("Look at paragraph 2.".into());
        // 4. A frame without its model sentence: dropped.
        let mut no_model = tiered_item("Question four?");
        no_model.basic.model_sentence = String::new();
        // 5. 一般版 answer that is not a sentence: kept, but a repair is asked for.
        let mut phrase = tiered_item("Question five?");
        phrase.standard.answer = "gone".into();
        let e = evaluate_tiered_level(
            &tiered_raw(vec![flat, rich, located, no_model, phrase]),
            &ctx,
        );
        assert!(e.needs_repair());
        let failed: Vec<&str> = e
            .results
            .iter()
            .filter(|r| !r.passed)
            .map(|r| r.name)
            .collect();
        assert_eq!(
            failed,
            [
                "support_forms_by_version",
                "support_wellformed",
                "no_answer_location",
                "answers_per_version"
            ]
        );
        let out = e.output.unwrap();
        let prompts: Vec<&str> = out.items.iter().map(|i| i.item.prompt.as_str()).collect();
        assert_eq!(prompts, ["Question three?", "Question five?"]);
        let kept = &out.items[0].tiers[0];
        assert_eq!(
            kept.support,
            ["The neighbors thought the ___ would be ___."]
        );
        assert!(out
            .items
            .iter()
            .flat_map(|i| &i.tiers)
            .flat_map(|t| &t.support)
            .all(|l| !crate::worksheet::tiered::points_to_answer_location(l)));
    }

    #[test]
    fn a_frame_is_cut_from_its_model_sentence() {
        let passage = [P1, P2, P3].join("\n");
        let codes = crate::worksheet::bloom::paragraph_codes(3);
        let ctx = tiered_ctx(&passage, &codes, 2);
        // A word bank word that is not in the model sentence goes (T4).
        let mut extra = tiered_item("Question one?");
        extra.basic.word_bank.push("laughed".into());
        // No blanks in the frame: dropped.
        let mut no_blank = tiered_item("Question two?");
        no_blank.basic.support = vec!["The neighbors thought the umbrellas would be gone.".into()];
        let e = evaluate_tiered_level(&tiered_raw(vec![extra, no_blank]), &ctx);
        let out = e.output.unwrap();
        assert_eq!(out.items.len(), 1);
        assert_eq!(out.items[0].tiers[0].word_bank, ["umbrellas", "gone"]);
        assert!(e.repairs.iter().any(|r| r.contains("只留範句裡的字詞")));
        // A starter that carries a word bank loses it; `none` keeps nothing.
        let mut item = tiered_item("Question three?");
        item.standard.word_bank = vec!["gone".into()];
        item.advanced.support = vec!["Think about it.".into()];
        let e = evaluate_tiered_level(&tiered_raw(vec![item]), &ctx);
        let out = e.output.unwrap();
        assert!(out.items[0].tiers[1].word_bank.is_empty());
        assert!(out.items[0].tiers[2].support.is_empty());
    }

    #[test]
    fn steps_are_required_and_must_not_give_answers_away() {
        let passage = [P1, P2, P3].join("\n");
        let codes = crate::worksheet::bloom::paragraph_codes(3);
        let ctx = tiered_ctx(&passage, &codes, 1);
        let mut raw = tiered_raw(vec![tiered_item("What did they think?")]);
        raw.steps = vec!["Read the question.".into()];
        raw.example = "Example: They thought every umbrella would be gone.".into();
        let e = evaluate_tiered_level(&raw, &ctx);
        assert!(e.needs_repair());
        let out = e.output.unwrap();
        assert!(
            out.steps.is_empty(),
            "one step is not enough: the program writes them"
        );
        assert_eq!(out.example, "", "an example that gives an answer goes");
    }

    #[test]
    fn the_judge_must_judge_every_question_once() {
        use crate::worksheet::bloom::RawJudgement;
        let ids: Vec<String> = ["Q1", "Q2", "Q3"].iter().map(|s| s.to_string()).collect();
        let j = |id: &str, bloom: &str| RawJudgement {
            id: id.into(),
            bloom: bloom.into(),
            reason: "理由".into(),
        };
        let all = RawBloomCheck {
            items: vec![
                j("Q2", "analyze"),
                j("Q1", "remember"),
                j("Q3", "create"),
                j("Q3", "apply"),
            ],
        };
        let e = evaluate_bloom_check(&all, &ids);
        assert!(!e.needs_repair());
        let out = e.output.unwrap();
        assert_eq!(
            out.iter().map(|x| (x.index, x.bloom)).collect::<Vec<_>>(),
            [
                (0, Bloom::Remember),
                (1, Bloom::Analyze),
                (2, Bloom::Create)
            ]
        );
        let partial = RawBloomCheck {
            items: vec![j("Q1", "remember"), j("Q7", "apply"), j("Q2", "thinking")],
        };
        let e = evaluate_bloom_check(&partial, &ids);
        assert!(e.needs_repair());
        assert_eq!(e.output.unwrap().len(), 1);
        assert!(e.problems.iter().any(|p| p.contains("Q2、Q3")));
        assert!(evaluate_bloom_check(&RawBloomCheck { items: vec![] }, &ids)
            .output
            .is_none());
    }

    // ------------------------------------------------------------ support boxes, greetings, 是非題

    /// The sharing frame of the teacher's report.
    const SHARING: &str = "Hi! In Paragraph 1, the author describes _____. After reading it carefully, I identified _____ as one of the main characteristics of informative writing.";

    #[test]
    fn a_greeting_frame_is_found_alone_or_wrapped_in_a_task() {
        let passage = "Bees carry pollen from flower to flower. Hi Tom, I will be ______ late.";
        assert_eq!(greeting_frame(SHARING, passage).as_deref(), Some(SHARING));
        for wrapped in [
            format!("Write a short analysis paragraph using the frame: {SHARING}"),
            format!("Complete the analysis paragraph: {SHARING}"),
            format!("Complete the analysis paragraph：\n“{SHARING}”"),
        ] {
            assert_eq!(
                greeting_frame(&wrapped, passage).as_deref(),
                Some(SHARING),
                "{wrapped}"
            );
        }
        assert!(greeting_frame("Hello, my name is ______.", passage).is_some());
        for not_one in [
            // No blank.
            "Hi! In Paragraph 1, the author describes bees.",
            // A question that happens to start with "Hi…".
            "History tells us ______ about bees.",
            // A cloze of a letter in the material.
            "Hi Tom, I will be ______ late.",
            "What does the author describe in Paragraph 1? ______",
        ] {
            assert!(greeting_frame(not_one, passage).is_none(), "{not_one}");
        }
    }

    #[test]
    fn directions_and_frames_are_told_apart() {
        // The box printed under 「Sentence frames」 in the teacher's report.
        let written: Vec<String> = [
            "Identify the common idea about bees in the first sentence.",
            "Look for the sentence that introduces the paragraph's main point.",
            "Use a detail about pollen, seeds, or fruit as evidence.",
            "Use this frame: The author describes _____ to show _____.",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let (frames, directions) = split_frames(&written);
        assert_eq!(frames, ["The author describes _____ to show _____."]);
        assert_eq!(directions, written[..3]);
        // Sentences of one line; a greeting stays with its frame; a starter without a blank stays.
        let (frames, directions) = split_frames(&[
            "1. Underline the topic sentence. The topic sentence is ______ because ______.".into(),
            SHARING.into(),
            "- I agree with the author.".into(),
        ]);
        assert_eq!(
            frames,
            [
                "The topic sentence is ______ because ______.",
                SHARING,
                "I agree with the author."
            ]
        );
        assert_eq!(directions, ["Underline the topic sentence."]);
        // A box of directions with a frame in it.
        let (steps, frames) = split_directions(&[
            "Step 1: Read the paragraph.".into(),
            "Use the frame: The author describes ______.".into(),
        ]);
        assert_eq!(steps, ["Read the paragraph."]);
        assert_eq!(frames, ["The author describes ______."]);
        assert!(direction_line("Circle two details."));
        assert!(!direction_line("Circle the ______."));
        assert!(!direction_line("Why is it the topic sentence?"));
        assert!(!direction_line("The author describes bees."));
    }

    #[test]
    fn a_true_false_item_is_a_statement_to_judge() {
        let opinion = "Do you think this sentence serves as a topic sentence?";
        assert!(true_false_question(opinion));
        assert!(true_false_question(
            "Is the second sentence the topic sentence?"
        ));
        assert!(true_false_question(
            "In your opinion, the ending is the best part."
        ));
        assert!(!true_false_question(
            "The second sentence of Paragraph 1 is the topic sentence."
        ));
        assert_eq!(
            true_false_fix(opinion),
            TrueFalseFix::Statement("This sentence serves as a topic sentence.".into())
        );
        assert_eq!(
            true_false_fix("Is it true that bees carry pollen?"),
            TrueFalseFix::Statement("Bees carry pollen.".into())
        );
        assert_eq!(
            true_false_fix("Do you agree with the author?"),
            TrueFalseFix::YesNo(format!("Do you agree with the author? {YES_NO_REASON}"))
        );

        // A template block (and every path that shares `item_checks`).
        let raw = crate::worksheet::template::RawSectionOut {
            title: "Paragraph 1".into(),
            instructions: "Answer.".into(),
            scaffold: String::new(),
            supports: Vec::new(),
            items: vec![
                out_item(FMT_TRUE_FALSE, opinion, &["True", "False"], "True"),
                out_item(
                    FMT_TRUE_FALSE,
                    "Is the second sentence the topic sentence?",
                    &["True", "False"],
                    "False",
                ),
            ],
        };
        let ctx = SectionCtx {
            passage: "",
            item_count: 3,
            needs_external: false,
            tables_asked: false,
            english_tables: false,
            objectives: &[],
        };
        let e = evaluate_section(&raw, &ctx);
        assert_eq!(failed(&e.results), ["items_wellformed"]);
        assert!(e.problems[0].contains("直述句"), "{:?}", e.problems);
        let items = e.output.unwrap().items;
        assert_eq!(
            (items[0].answer_format.as_str(), items[0].prompt.as_str()),
            (FMT_TRUE_FALSE, "This sentence serves as a topic sentence.")
        );
        assert_eq!(items[0].answer, "True");
        assert_eq!(items[1].answer_format, FMT_SHORT);
        assert!(items[1].prompt.ends_with(YES_NO_REASON));
        assert!(items[1].options.is_empty());
        assert_eq!(items[1].answer, "No.");

        // The main tasks.
        let p = paragraphs();
        let mut tf = item(FMT_TRUE_FALSE, opinion, "True");
        tf.options = vec!["True".into(), "False".into()];
        let raw = RawTasks {
            blocks: staged(vec![
                block(&[1], "發現資訊", vec![tf]),
                block(
                    &[2],
                    "分析資訊",
                    vec![item("簡答題", "Why did they laugh?", "They were sure.")],
                ),
                block(
                    &[3],
                    "整理資訊",
                    vec![item("簡答題", "What happened?", "Twelve disappeared.")],
                ),
            ]),
        };
        let e = evaluate_tasks(&raw, &tasks_ctx(&p, &[]));
        assert_eq!(failed(&e.results), ["items_wellformed"]);
        let blocks = e.output.unwrap();
        assert_eq!(
            blocks[0].items[0].prompt_en,
            "This sentence serves as a topic sentence."
        );
    }

    fn box_of(heading: &str, role: SupportRole) -> WsTemplateSupport {
        WsTemplateSupport {
            heading: heading.into(),
            role,
            pattern: String::new(),
        }
    }

    fn written(heading: &str, lines: &[&str]) -> crate::worksheet::template::RawSupportOut {
        crate::worksheet::template::RawSupportOut {
            heading: heading.into(),
            lines: lines.iter().map(|l| l.to_string()).collect(),
        }
    }

    fn printed(out: &TemplateSection) -> Vec<(Option<&str>, Vec<&str>)> {
        out.supports
            .iter()
            .map(|s| {
                (
                    s.heading.as_deref(),
                    s.lines.iter().map(String::as_str).collect(),
                )
            })
            .collect()
    }

    /// The shapes of the teacher's report: an opinion as a 是非題, the sharing frame numbered
    /// twice (once wrapped in a writing task), and the directions printed as sentence frames.
    fn reported_items() -> Vec<crate::worksheet::template::RawOutItem> {
        vec![
            out_item(
                FMT_TRUE_FALSE,
                "Do you think this sentence serves as a topic sentence?",
                &["True", "False"],
                "True",
            ),
            out_item(
                FMT_SHORT,
                "What do bees carry from flower to flower?",
                &[],
                "Pollen.",
            ),
            out_item(
                FMT_PARAGRAPH,
                &format!("Write a short analysis paragraph using the frame: {SHARING}"),
                &[],
                "Answers will vary.",
            ),
            out_item(FMT_SHORT, SHARING, &[], "Answers will vary."),
        ]
    }

    const REPORTED_BOX: [&str; 4] = [
        "Identify the common idea about bees in the first sentence.",
        "Look for the sentence that introduces the paragraph's main point.",
        "Use a detail about pollen, seeds, or fruit as evidence.",
        "Use this frame: The author describes _____ to show _____.",
    ];

    #[test]
    fn a_template_block_prints_each_box_of_the_sample_apart() {
        let boxes = [
            box_of("Directions", SupportRole::Directions),
            box_of("Sentence Frames", SupportRole::SentenceFrame),
            box_of("Sentence Frame for Sharing", SupportRole::SharingFrame),
        ];
        let sup = SupportsCtx {
            supports: &boxes,
            legacy: (ScaffoldType::Verbal, KIND_GUIDING_FOR_TESTS),
        };
        let ctx = SectionCtx {
            passage: "Bees carry pollen from flower to flower.",
            item_count: 3,
            needs_external: false,
            tables_asked: false,
            english_tables: false,
            objectives: &[],
        };
        // Written well: each box under its heading, in the sample's order (matched by heading).
        let raw = crate::worksheet::template::RawSectionOut {
            title: "Paragraph 1".into(),
            instructions: "Read paragraph 1.".into(),
            scaffold: String::new(),
            supports: vec![
                written(
                    "Sentence Frames",
                    &["The author describes _____ to show _____."],
                ),
                written("Directions", &["Underline the topic sentence."]),
                written("Sentence Frame for Sharing", &[SHARING]),
            ],
            items: vec![out_item(
                FMT_SHORT,
                "What do bees carry from flower to flower?",
                &[],
                "Pollen.",
            )],
        };
        let e = evaluate_template_section(&raw, &ctx, &sup);
        assert!(!e.needs_repair(), "{:?}", e.problems);
        let out = e.output.unwrap();
        assert_eq!(
            printed(&out),
            [
                (Some("Directions"), vec!["Underline the topic sentence."]),
                (
                    Some("Sentence Frames"),
                    vec!["The author describes _____ to show _____."]
                ),
                (Some("Sentence Frame for Sharing"), vec![SHARING]),
            ]
        );
        assert_eq!(out.supports[0].kind_type, ScaffoldType::Procedural);
        assert_eq!(out.supports[2].kind, KIND_FRAMES);

        // The reported shapes: the directions leave the frames for the Directions box, the
        // sharing frame leaves the numbering for its box, the opinion becomes a statement.
        let bad = crate::worksheet::template::RawSectionOut {
            supports: vec![
                written("Directions", &[]),
                written("Sentence Frames", &REPORTED_BOX),
                written("Sentence Frame for Sharing", &[]),
            ],
            items: reported_items(),
            ..raw.clone()
        };
        let e = evaluate_template_section(&bad, &ctx, &sup);
        let names = failed(&e.results);
        assert_eq!(names, ["items_wellformed", "supports_wellformed"]);
        assert!(e
            .problems
            .iter()
            .any(|p| p.contains("口頭分享用的句型框架")));
        assert!(e.problems.iter().any(|p| p.contains("卻寫了作答步驟")));
        let out = e.output.unwrap();
        assert_eq!(
            printed(&out),
            [
                (Some("Directions"), REPORTED_BOX[..3].to_vec()),
                (
                    Some("Sentence Frames"),
                    vec!["The author describes _____ to show _____."]
                ),
                (Some("Sentence Frame for Sharing"), vec![SHARING]),
            ]
        );
        let prompts: Vec<&str> = out
            .section
            .items
            .iter()
            .map(|i| i.prompt.as_str())
            .collect();
        assert_eq!(
            prompts,
            [
                "This sentence serves as a topic sentence.",
                "What do bees carry from flower to flower?"
            ]
        );
        assert!(out
            .section
            .items
            .iter()
            .all(|i| greeting_frame(&i.prompt, "").is_none()));

        // A box the output left empty is reported and not printed.
        let empty = crate::worksheet::template::RawSectionOut {
            supports: vec![
                written("Directions", &["Underline the topic sentence."]),
                written("Sentence Frames", &[]),
                written("Sentence Frame for Sharing", &[SHARING]),
            ],
            ..raw
        };
        let e = evaluate_template_section(&empty, &ctx, &sup);
        assert_eq!(failed(&e.results), ["supports_wellformed"]);
        assert_eq!(e.output.unwrap().supports.len(), 2);
    }

    const KIND_GUIDING_FOR_TESTS: &str = "引導問題";

    #[test]
    fn a_template_read_before_supports_gets_the_same_boxes() {
        // The teacher's template as it was stored: 句型框架 first, no boxes, the sharing frame
        // among the questions.
        let sup = SupportsCtx {
            supports: &[],
            legacy: (ScaffoldType::Verbal, KIND_FRAMES),
        };
        let ctx = SectionCtx {
            passage: "Bees carry pollen from flower to flower.",
            item_count: 4,
            needs_external: false,
            tables_asked: false,
            english_tables: false,
            objectives: &[],
        };
        let raw = crate::worksheet::template::RawSectionOut {
            title: "Paragraph 1".into(),
            instructions: "Read paragraph 1.".into(),
            scaffold: REPORTED_BOX.join("\n"),
            supports: Vec::new(),
            items: reported_items(),
        };
        let e = evaluate_template_section(&raw, &ctx, &sup);
        assert_eq!(
            failed(&e.results),
            ["items_wellformed", "supports_wellformed"]
        );
        let out = e.output.unwrap();
        assert_eq!(
            printed(&out),
            [
                (Some("Directions"), REPORTED_BOX[..3].to_vec()),
                (None, vec!["The author describes _____ to show _____."]),
                (Some("Sentence Frame for Sharing"), vec![SHARING]),
            ]
        );
        assert_eq!(out.section.items.len(), 2);
        // A plain frame in the student's voice is lifted as #71 does it: right after an open
        // question its number goes.
        let raw = crate::worksheet::template::RawSectionOut {
            scaffold: "The author says ______.".into(),
            items: vec![
                out_item(FMT_SHORT, "Why do bees matter?", &[], "They carry pollen."),
                out_item(
                    FMT_SHORT,
                    "I think ______ because ______.",
                    &[],
                    "Answers vary.",
                ),
            ],
            ..raw
        };
        let e = evaluate_template_section(&raw, &ctx, &sup);
        let out = e.output.unwrap();
        assert_eq!(out.section.items.len(), 1);
        // A first kind that is not 句型框架 (a real-model run on main): the sharing frame written
        // in its box goes to its own box, without a repair call (the scaffold had no other place).
        let words = SupportsCtx {
            supports: &[],
            legacy: (ScaffoldType::Verbal, "透過上下文解釋詞義"),
        };
        let raw_words = crate::worksheet::template::RawSectionOut {
            scaffold: "Step 1: Look for the sentence that tells the main idea.\nSentence Frame: Hi! In Paragraph 1, the author describes ______.".into(),
            items: vec![out_item(FMT_SHORT, "Why do bees matter?", &[], "They carry pollen.")],
            ..raw.clone()
        };
        let e = evaluate_template_section(&raw_words, &ctx, &words);
        assert!(!e.needs_repair(), "{:?}", e.problems);
        assert_eq!(
            printed(&e.output.unwrap()),
            [
                (
                    None,
                    vec!["Step 1: Look for the sentence that tells the main idea."]
                ),
                (
                    Some("Sentence Frame for Sharing"),
                    vec!["Hi! In Paragraph 1, the author describes ______."]
                ),
            ]
        );
        assert_eq!(
            printed(&out),
            [(
                None,
                vec!["The author says ______.", "I think ______ because ______."]
            )]
        );
    }
}

#[cfg(test)]
mod item_frame_tests {
    use super::*;
    use crate::worksheet::schemas::{RawItemFrame, RawItemHint, RawScaffoldSet};

    const FRAME: &str = "Hello, everyone. My answer is ___ because ___. Thank you.";

    #[test]
    fn a_frame_has_a_blank_english_and_no_answer() {
        assert_eq!(item_frame(FRAME, "").unwrap(), FRAME);
        assert!(item_frame("Say your answer in a full sentence.", "")
            .unwrap_err()
            .contains("空格"));
        assert!(item_frame("我的答案是 ___。", "")
            .unwrap_err()
            .contains("中文"));
        assert!(item_frame("", "").is_err());
        assert!(item_frame(
            "The street was empty because the shops had closed ___.",
            "The street was empty because the shops had closed"
        )
        .unwrap_err()
        .contains("答案"));
        assert!(item_frame(&format!("{FRAME} {}", "x".repeat(700)), "").is_err());
        assert!(item_hint("Look at the second stanza.", "").is_ok());
        assert!(item_hint("提示", "").is_err());
    }

    fn set(frames: &[(i64, &str)], hints: &[(i64, &str)]) -> RawScaffolds {
        RawScaffolds {
            scaffolds: vec![RawScaffoldSet {
                target: "B1".into(),
                verbal: Vec::new(),
                instructional: Vec::new(),
                procedural: vec![crate::worksheet::schemas::RawProcedure {
                    kind: KIND_STEPS.into(),
                    steps_en: vec!["Read.".into(), "Write.".into()],
                }],
                item_frames: frames
                    .iter()
                    .map(|(n, f)| RawItemFrame {
                        item_no: *n,
                        frame_en: (*f).into(),
                    })
                    .collect(),
                item_hints: hints
                    .iter()
                    .map(|(n, h)| RawItemHint {
                        item_no: *n,
                        hint_en: (*h).into(),
                    })
                    .collect(),
            }],
        }
    }

    fn ctx_items() -> Vec<TargetItems> {
        vec![TargetItems {
            target: "B1".into(),
            items: vec![
                ("簡答題".into(), "Because it rained.".into()),
                ("選擇題".into(), "B".into()),
                ("段落寫作".into(), "".into()),
            ],
        }]
    }

    fn run(raw: &RawScaffolds) -> Eval<Vec<RawScaffoldSet>> {
        let items = ctx_items();
        evaluate_scaffolds(
            raw,
            &ScaffoldsCtx {
                targets: &["B1".to_string()],
                answers: &[],
                items: &items,
            },
        )
    }

    #[test]
    fn frames_stay_on_open_questions_only() {
        let e = run(&set(
            &[(1, FRAME), (2, FRAME), (3, FRAME), (9, FRAME), (1, FRAME)],
            &[(2, "Look at paragraph 2."), (2, "Again.")],
        ));
        assert!(!e.needs_repair(), "{:?}", e.problems);
        let s = &e.output.unwrap()[0];
        let nos: Vec<i64> = s.item_frames.iter().map(|f| f.item_no).collect();
        assert_eq!(nos, [1, 3]);
        assert_eq!(s.item_hints.len(), 1);
        assert_eq!(s.item_hints[0].hint_en, "Look at paragraph 2.");
    }

    #[test]
    fn a_frame_without_a_blank_is_sent_back_and_dropped() {
        let e = run(&set(&[(1, "Say it in a sentence.")], &[]));
        assert!(e.needs_repair());
        assert!(e.problems[0].contains("第 1 題"), "{:?}", e.problems);
        assert!(e.output.unwrap()[0].item_frames.is_empty());
    }

    #[test]
    fn the_sop_step_takes_no_frames() {
        let raw = set(&[(1, FRAME)], &[(1, "Look.")]);
        let e = evaluate_scaffolds(
            &raw,
            &ScaffoldsCtx {
                targets: &["B1".to_string()],
                answers: &[],
                items: &[],
            },
        );
        assert!(!e.needs_repair());
        let s = &e.output.unwrap()[0];
        assert!(s.item_frames.is_empty() && s.item_hints.is_empty());
    }
}

#[cfg(test)]
mod objective_copy_tests {
    use super::*;

    /// The teacher's two objectives of the reported sheet (a poem; the sheet printed both).
    const CONTENT: &str = "I can explain how the two roads symbolize life choices.";
    const LANGUAGE: &str = "I can use clues such as \"both,\" \"one,\" \"Yet,\" and \"ever\" to infer that choices have limits and cannot be reversed.";
    const POEM: &str = "Two roads diverged in a yellow wood, And sorry I could not travel both. \
         Then took the other, as just as fair, And having perhaps the better claim, Because it \
         was grassy and wanted wear. Yet knowing how way leads on to way, I doubted if I should \
         ever come back. I took the one less traveled by, And that has made all the difference.";

    fn objectives() -> Vec<String> {
        vec![CONTENT.to_string(), LANGUAGE.to_string()]
    }

    fn copied(format: &str, prompt: &str, answer: &str) -> Option<String> {
        copied_objective(format, prompt, answer, &objectives(), POEM).map(str::to_string)
    }

    fn out_item(
        format: &str,
        prompt: &str,
        answer: &str,
    ) -> crate::worksheet::template::RawOutItem {
        crate::worksheet::template::RawOutItem {
            answer_format: format.into(),
            prompt: prompt.into(),
            options: Vec::new(),
            answer: answer.into(),
            bloom: None,
            organizer: None,
            table: None,
        }
    }

    fn failed(e: &[CheckResult]) -> Vec<&'static str> {
        e.iter().filter(|r| !r.passed).map(|r| r.name).collect()
    }

    /// Items 3 and 4 of the reported sheet, as the repair call wrote them (a lead in front of the
    /// blanked objective), and as the first call did (the bare frame).
    const REPAIRED_CONTENT: &str = "Complete the content-goal self-assessment statement with the key idea: I can explain how the two roads symbolize life ______.";
    const REPAIRED_LANGUAGE: &str = "Complete the language-goal self-assessment statement: I can use clues such as \"both,\" \"one,\" \"Yet,\" and \"ever\" to infer that choices have ______ and cannot be reversed.";

    #[test]
    fn the_reported_items_copy_the_objectives() {
        assert_eq!(
            copied(FMT_BLANK, REPAIRED_CONTENT, "choices").as_deref(),
            Some(CONTENT)
        );
        assert_eq!(
            copied(FMT_BLANK, REPAIRED_LANGUAGE, "limits").as_deref(),
            Some(LANGUAGE)
        );
        // The first call's bare frames, and the same as other closed formats.
        assert_eq!(
            copied(
                FMT_BLANK,
                "I can explain how the two roads symbolize life ______.",
                "choices"
            )
            .as_deref(),
            Some(CONTENT)
        );
        assert!(copied(
            FMT_TRUE_FALSE,
            "I can explain how the two roads symbolize life choices.",
            "True"
        )
        .is_some());
        assert!(copied(
            FMT_CHOICE,
            "Which word completes the goal: I can explain how the two roads symbolize life ______?",
            "choices"
        )
        .is_some());
    }

    #[test]
    fn a_self_rating_or_reflection_without_an_answer_may_quote_an_objective() {
        for answer in ["", "Answers will vary.", "answers may vary", "依學生回答"] {
            assert_eq!(
                copied(FMT_BLANK, REPAIRED_CONTENT, answer),
                None,
                "{answer}"
            );
            assert_eq!(
                copied(
                    FMT_SHORT,
                    "Rate yourself from 1 to 4: I can explain how the two roads symbolize life choices.",
                    answer
                ),
                None,
                "{answer}"
            );
        }
        // No objectives: nothing to copy.
        assert_eq!(
            copied_objective(FMT_BLANK, REPAIRED_CONTENT, "choices", &[], POEM),
            None
        );
    }

    #[test]
    fn a_question_about_the_text_passes() {
        for (format, prompt, answer) in [
            // A cloze of the poem.
            (
                FMT_BLANK,
                "The road the speaker took was grassy and wanted ______.",
                "wear",
            ),
            (FMT_BLANK, "Two roads diverged in a yellow ______.", "wood"),
            // Five words of an objective in a row are not a copy.
            (FMT_BLANK, "Explain how the two roads ______.", "differ"),
            // Six words of the objective, but the objective does not give the answer away.
            (
                FMT_BLANK,
                "The two roads symbolize life choices because ______.",
                "the speaker can take only one",
            ),
            (
                FMT_TRUE_FALSE,
                "The two roads symbolize life choices that the speaker can easily undo later.",
                "False",
            ),
            (
                FMT_CHOICE,
                "What do the two roads stand for in the poem?",
                "Choices in life",
            ),
            // An open question that asks what the objective aims at, with a real answer.
            (
                FMT_SHORT,
                "Explain how the two roads symbolize life choices.",
                "Each road stands for a choice; once the speaker takes one, he cannot go back.",
            ),
        ] {
            assert_eq!(copied(format, prompt, answer), None, "{prompt}");
        }
    }

    #[test]
    fn a_line_the_objective_quotes_from_the_text_is_not_a_copy() {
        let objectives =
            vec!["I can explain what \"I took the one less traveled by\" means.".to_string()];
        assert_eq!(
            copied_objective(
                FMT_BLANK,
                "I took the one less traveled ______.",
                "by",
                &objectives,
                POEM
            ),
            None
        );
        // Without the poem it would be one.
        assert!(copied_objective(
            FMT_BLANK,
            "I took the one less traveled ______.",
            "by",
            &objectives,
            ""
        )
        .is_some());
    }

    #[test]
    fn case_punctuation_and_blanks_do_not_hide_a_copy() {
        for (prompt, answer) in [
            (
                "i CAN explain—how the TWO roads symbolize life (   )",
                "choices",
            ),
            (
                "I can explain how the two roads symbolize life ＿＿＿＿.",
                "choices",
            ),
            (
                "\"I can explain how the two roads, symbolize life:\" ________",
                "choices",
            ),
            // The blank in the middle: it stands for the word it took away.
            (
                "Infer that choices have ______ and cannot be reversed.",
                "limits",
            ),
            // A blank for two words is filled with the answer.
            (
                "Infer that choices ______ and cannot be reversed.",
                "have limits",
            ),
            // Two blanks, two parts.
            (
                "To infer that choices ______ limits and cannot be ______.",
                "have, reversed",
            ),
        ] {
            assert!(copied(FMT_BLANK, prompt, answer).is_some(), "{prompt}");
        }
    }

    #[test]
    fn an_open_question_fails_only_when_its_answer_is_a_word_of_the_objective() {
        let prompt =
            "Which word finishes the goal \"I can explain how the two roads symbolize life\"?";
        assert_eq!(
            copied(FMT_SHORT, prompt, "choices").as_deref(),
            Some(CONTENT)
        );
        assert_eq!(
            copied(FMT_SHORT, prompt, "Life choices.").as_deref(),
            Some(CONTENT)
        );
        assert_eq!(
            copied(FMT_SHORT, prompt, "The speaker has to pick one road."),
            None
        );
    }

    #[test]
    fn chinese_counts_two_characters_as_a_word() {
        let objectives = vec!["能說明兩條路如何象徵人生的選擇".to_string()];
        assert!(copied_objective(
            FMT_BLANK,
            "能說明兩條路如何象徵人生的______",
            "選擇",
            &objectives,
            ""
        )
        .is_some());
        assert!(copied_objective(FMT_BLANK, "兩條路象徵______", "選擇", &objectives, "").is_none());
    }

    #[test]
    fn the_lines_of_the_objectives_are_read_once() {
        let lines = objective_lines([
            (CONTENT, CONTENT),
            ("學生能說明兩條路的象徵\n學生能推論", " I can infer. "),
            ("", ""),
        ]);
        assert_eq!(
            lines,
            [
                CONTENT,
                "學生能說明兩條路的象徵",
                "學生能推論",
                "I can infer."
            ]
        );
    }

    #[test]
    fn the_repair_note_says_why_and_what_to_write_instead() {
        assert_eq!(
            objective_copy_note("第 3 題", CONTENT),
            format!("第 3 題 照抄了學習目標（{CONTENT}）：學習目標只供理解教學重點，學生在學習單上方已看得到，請換成一題針對課文內容的題目。")
        );
    }

    #[test]
    fn a_template_block_drops_a_copied_objective_even_with_a_lead() {
        let objectives = objectives();
        let ctx = SectionCtx {
            passage: POEM,
            item_count: 3,
            needs_external: false,
            tables_asked: false,
            english_tables: false,
            objectives: &objectives,
        };
        let sup = SupportsCtx {
            supports: &[],
            legacy: (ScaffoldType::Verbal, KIND_FRAMES),
        };
        let content = out_item(
            FMT_BLANK,
            "The speaker chose the road that was grassy and wanted ______.",
            "wear",
        );
        let raw = crate::worksheet::template::RawSectionOut {
            title: "Concept Introduction".into(),
            instructions: "Complete the sentences.".into(),
            scaffold: String::new(),
            supports: Vec::new(),
            items: vec![
                content.clone(),
                out_item(FMT_BLANK, REPAIRED_CONTENT, "choices"),
                out_item(FMT_BLANK, REPAIRED_LANGUAGE, "limits"),
            ],
        };
        let e = evaluate_template_section(&raw, &ctx, &sup);
        assert_eq!(failed(&e.results), [CHECK_NOT_AN_OBJECTIVE]);
        assert_eq!(
            e.problems,
            [
                objective_copy_note("第 2 題", CONTENT),
                objective_copy_note("第 3 題", LANGUAGE)
            ]
        );
        assert!(e.repairs.iter().all(|r| r.starts_with("拿掉這一題")));
        let out = e.output.unwrap();
        assert_eq!(out.section.items.len(), 1);
        assert_eq!(out.section.items[0].prompt, content.prompt);

        // The first call's bare frame: refused as a copy, not lifted into the frames box.
        let bare = crate::worksheet::template::RawSectionOut {
            items: vec![
                content.clone(),
                out_item(
                    FMT_BLANK,
                    "I can explain how the two roads symbolize life ______.",
                    "choices",
                ),
            ],
            ..raw.clone()
        };
        let e = evaluate_template_section(&bare, &ctx, &sup);
        assert_eq!(failed(&e.results), [CHECK_NOT_AN_OBJECTIVE]);
        let out = e.output.unwrap();
        assert_eq!(out.section.items.len(), 1);
        assert!(out
            .supports
            .iter()
            .all(|s| s.lines.iter().all(|l| !l.contains("symbolize"))));

        // A self-rating with no answer stays (here a frame of its own: lifted as before).
        let rating = crate::worksheet::template::RawSectionOut {
            items: vec![
                content,
                out_item(
                    FMT_SHORT,
                    "Rate yourself from 1 to 4: I can explain how the two roads symbolize life choices.",
                    "Answers will vary.",
                ),
            ],
            ..raw
        };
        let e = evaluate_template_section(&rating, &ctx, &sup);
        assert!(!e.needs_repair(), "{:?}", e.problems);
        assert_eq!(e.output.unwrap().section.items.len(), 2);
    }

    #[test]
    fn a_frame_note_asks_for_a_question_about_the_text_not_a_lead() {
        let ctx = SectionCtx {
            passage: POEM,
            item_count: 3,
            needs_external: false,
            tables_asked: false,
            english_tables: false,
            objectives: &[],
        };
        let sup = SupportsCtx {
            supports: &[],
            legacy: (ScaffoldType::Verbal, KIND_FRAMES),
        };
        let raw = crate::worksheet::template::RawSectionOut {
            title: "Concept Introduction".into(),
            instructions: "Complete the sentences.".into(),
            scaffold: String::new(),
            supports: Vec::new(),
            items: vec![out_item(
                FMT_SHORT,
                "I think the speaker feels ______ because ______.",
                "Answers will vary.",
            )],
        };
        let e = evaluate_template_section(&raw, &ctx, &sup);
        assert_eq!(failed(&e.results), ["items_wellformed"]);
        assert!(e.problems[0].contains("只有句型框架"), "{:?}", e.problems);
        assert!(e.problems[0].contains("換成一題針對課文內容的題目"));
        assert!(e.problems[0].contains("Complete the statement:"));
    }

    #[test]
    fn tasks_and_exit_tickets_drop_a_copied_objective() {
        let paragraphs = vec![POEM.to_string()];
        let objectives = objectives();
        let mut copy = RawItem {
            item_id: "x".into(),
            answer_format: FMT_BLANK.into(),
            tier: "core".into(),
            prompt_en: REPAIRED_CONTENT.into(),
            options: vec![],
            organizer: None,
            table: None,
            answer_en: "choices".into(),
            evidence: None,
            bloom: None,
        };
        let mut fine = copy.clone();
        fine.prompt_en = "The speaker chose the road that was grassy and wanted ______.".into();
        fine.answer_en = "wear".into();
        let raw = RawTasks {
            blocks: vec![RawBlock {
                block_id: "B1".into(),
                paragraph_nos: vec![1],
                title_en: "Read".into(),
                info_function: "分析資訊".into(),
                practice: None,
                student_tasks: vec!["理解內容".into()],
                objective_refs: vec![],
                instructions_en: "Answer.".into(),
                items: vec![fine.clone(), copy.clone()],
                design_reason_zh: "理由".into(),
                image_slots: Vec::new(),
            }],
        };
        let only = [1];
        let ctx = TasksCtx {
            paragraphs: &paragraphs,
            objective_ids: &[],
            item_budget: 6,
            only: Some(&only),
            habits: OptionHabits::default(),
            objectives: &objectives,
        };
        let e = evaluate_tasks(&raw, &ctx);
        assert_eq!(failed(&e.results), [CHECK_NOT_AN_OBJECTIVE]);
        assert_eq!(e.problems, [objective_copy_note("B1 第 2 題", CONTENT)]);
        assert_eq!(e.output.unwrap()[0].items.len(), 1);
        // The same item without an answer is a self-rating: kept.
        copy.answer_en = "Answers will vary.".into();
        let raw = RawTasks {
            blocks: vec![RawBlock {
                items: vec![fine, copy],
                ..raw.blocks[0].clone()
            }],
        };
        assert!(failed(&evaluate_tasks(&raw, &ctx).results).is_empty());

        // Exit ticket: a formative item may not copy; a reflection may quote.
        let exit_item = |purpose: &str, prompt: &str, answer: Option<&str>| RawExitItem {
            item_id: "E".into(),
            purpose: purpose.into(),
            prompt_en: prompt.into(),
            answer_format: FMT_SHORT.into(),
            options: vec![],
            student_tasks: vec![],
            objective_refs: vec![],
            sample_answer_en: answer.map(str::to_string),
        };
        let exit = RawExit {
            estimated_minutes: 3,
            items: vec![
                exit_item(
                    "形成性評量",
                    "Why does the speaker say the choice made all the difference?",
                    Some("Because he cannot go back."),
                ),
                exit_item("形成性評量", REPAIRED_LANGUAGE, Some("limits")),
                exit_item(
                    "學生反思",
                    "How sure are you? I can explain how the two roads symbolize life choices. (1–4)",
                    Some("4"),
                ),
            ],
        };
        let ctx = ExitCtx {
            objective_ids: &[],
            task_prompts: &[],
            passage: POEM,
            objectives: &objectives,
        };
        let e = evaluate_exit(&exit, &ctx);
        assert_eq!(failed(&e.results), [CHECK_NOT_AN_OBJECTIVE]);
        assert_eq!(e.problems, [objective_copy_note("第 2 題", LANGUAGE)]);
        let kept: Vec<String> = e.output.unwrap().into_iter().map(|i| i.purpose).collect();
        assert_eq!(kept, ["形成性評量", "學生反思"]);
    }
}
