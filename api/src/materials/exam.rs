//! Exam papers (試卷), rebuilt into the paragraphs a teacher picks (issue #135).
//!
//! An exam paper's text layer is spaced so widely that the PDF extractor puts a blank line after
//! every line: read as an ordinary page, every line became a paragraph, the cover alone came out
//! as a dozen paragraphs and a 題組's passage was cut into lines. When the lines look like an
//! exam paper ([`looks_like_exam`]), [`rebuild`] reads them across page breaks instead:
//!
//! | what | paragraphs |
//! |---|---|
//! | cover text before the first part, section, 題組 or question 1 (作答注意事項, 計分方式, 著作權說明) | one per item; `front` when it reads like a cover |
//! | a part or section heading (第壹部分、…, 一、詞彙題) | one |
//! | 說明： / 提示： with its wrapped lines and numbered items | one |
//! | 「第 31 至 34 題為題組」 (or "Questions 31-34 …") | one, opening the group `31-34` |
//! | the passage after it, up to its first question | one, in the group |
//! | a question (`31.`) with its options and wrapped lines | one; in the group while its number is in range |
//! | a sub-question (`(1)`) | one |
//!
//! A 題組 without its 「為題組」 line (common in school exams) is found too: a passage of at least
//! [`MIN_PASSAGE_CHARS`] followed directly by questions becomes the group of those questions.
//!
//! Page furniture goes first: page numbers (「第 1 頁 共 7 頁」, `- 3 -`), 「背面尚有試題」, and a
//! running header or footer repeated at the edge of most pages. The same rules serve the
//! paragraphs OCR read from a scanned exam (`lines_are_paragraphs`), where every line already is a
//! paragraph. Every line also goes through [`normalize_line`].

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;

use super::extract::Block;
use crate::retrieval::is_cjk;
use crate::sop::wrap::{ends_sentence, join_wrapped, join_wrapped_within, line_width, SHORT_LINE};

/// The lines of one page in reading order (from OCR: one paragraph per line).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PageLines {
    pub page: i32,
    pub lines: Vec<String>,
}

/// Lines at most this far from the top or the bottom of a page may be page furniture.
const EDGE_LINES: usize = 3;
/// After its last option, a question goes on only while its lines reach this share of the widest
/// line and do not end a sentence (an option wrapped onto the next line).
const FULL_LINE: f32 = 0.7;
/// Text at least this long directly before a run of questions is their passage.
pub const MIN_PASSAGE_CHARS: usize = 150;
/// Words that mark the text before the first question as an exam cover.
const COVER_WORDS: [&str; 13] = [
    "注意事項",
    "考試時間",
    "作答方式",
    "計分方式",
    "答題卷",
    "答案卡",
    "答案卷",
    "著作權",
    "翻閱",
    "鈴響",
    "准考證",
    "座號",
    "姓名",
];

/// 「第 31 至 34 題為題組」, 「第31題至第34題為題組」, "Questions 31-34 are based on …".
static GROUP_MARK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?:第\s*(\d{1,3})\s*題?\s*(?:至|到|~|～|-|－|–|—)\s*第?\s*(\d{1,3})\s*題\s*為\s*題\s*組|(?i:questions?)\s+(\d{1,3})\s*(?:-|–|—|~|to|through)\s*(\d{1,3})\b)",
    )
    .unwrap()
});
/// `31.`, `31．`, `31、` (not `3.5`).
static QUESTION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d{1,3})\s*[.．、](?:\s|$|[^\d])").unwrap());
/// `(1)`, `（2）`.
static SUB_QUESTION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[(（]\s*\d{1,2}\s*[)）]").unwrap());
/// An option marker anywhere in a line: `(A)` … `(E)`.
static OPTION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[(（]\s*([A-E])\s*[)）]").unwrap());
static OPTION_START: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[(（]\s*[A-E]\s*[)）]").unwrap());
/// 「第壹部分、選擇題」, 「一、詞彙題」, 「貳、非選擇題」.
static SECTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?:第\s*[壹貳參肆伍陸柒捌玖拾一二三四五六七八九十]{1,3}\s*部\s*分|[壹貳參肆伍陸柒捌玖拾一二三四五六七八九十]{1,3}\s*[、.．])",
    )
    .unwrap()
});
/// 說明： / 提示： / 注意：
static NOTE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:說\s*明|提\s*示|注\s*意)\s*[：:]").unwrap());
/// A note whose items are numbered from its first line: 「說明：1. 請將……」.
static NOTE_FIRST_ITEM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\S{2,4}\s*[：:]\s*(\d{1,2})\s*[.．、]").unwrap());
/// A cover label: 「考試時間：」, 「作答方式：」, 「著作權說明：」.
static LABEL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\p{Han}{2,8}\s*[：:]").unwrap());
static BULLET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[・•●◎※○■□◆◇★☆＊*‧·▪►]").unwrap());
/// A whole line that is page furniture.
static FURNITURE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?:第\s*\d{1,3}\s*頁(?:\s*[,，/、]?\s*共\s*\d{1,3}\s*頁)?|共\s*\d{1,3}\s*頁|[-－—–]\s*\d{1,3}\s*[-－—–]|\d{1,3}|\d{1,3}\s*/\s*\d{1,3}|(?i:page)\s*\d{1,3}(?:\s*(?i:of)\s*\d{1,3})?|[-－—–\s]*(?:試題結束|試題到此結束|背面尚有試題|背面還有試題|請翻頁|請翻面)[-－—–\s]*)$",
    )
    .unwrap()
});

// ---------------------------------------------------------------- lines

/// One PDF line made comparable: the Kangxi radicals some PDF fonts map their characters to
/// (「⽂」 U+2F42 for 「文」, 「⾄」 for 「至」) become the ideographs, and a letter-spaced heading
/// (「作 答 注 意 事 項」, 「M a n n a   模 擬 試 卷」) is closed up. Trimmed.
pub fn normalize_line(line: &str) -> String {
    let unified: String = line.chars().map(unify_radical).collect();
    close_letter_spacing(unified.trim())
}

fn unify_radical(c: char) -> char {
    if !(0x2E80..=0x2FDF).contains(&(c as u32)) {
        return c;
    }
    let mut parts = Vec::new();
    unicode_normalization::char::decompose_compatible(c, |d| parts.push(d));
    match parts.as_slice() {
        [one] => *one,
        _ => c,
    }
}

/// At least four one-character tokens, one of them Chinese: the letters of a spaced heading.
/// Words were parted by wider gaps (two spaces or more).
fn close_letter_spacing(line: &str) -> String {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let spaced = tokens.len() >= 4
        && tokens.iter().all(|t| t.chars().count() == 1)
        && tokens.iter().any(|t| t.chars().all(is_cjk));
    if !spaced {
        return line.to_string();
    }
    line.split("  ")
        .map(|w| w.split_whitespace().collect::<String>())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn group_range(line: &str) -> Option<(u32, u32)> {
    let c = GROUP_MARK.captures(line)?;
    let num = |i: usize| c.get(i).and_then(|m| m.as_str().parse::<u32>().ok());
    let first = num(1).or_else(|| num(3))?;
    let last = num(2).or_else(|| num(4))?;
    (first <= last).then_some((first, last))
}

fn question_no(line: &str) -> Option<u32> {
    QUESTION.captures(line)?.get(1)?.as_str().parse().ok()
}

/// The last option letter in a line (`C` for 「(C) careless」).
fn last_option(line: &str) -> Option<char> {
    OPTION
        .captures_iter(line)
        .filter_map(|c| c.get(1)?.as_str().chars().next())
        .max()
}

fn ends_with_colon(line: &str) -> bool {
    line.trim_end().ends_with(['：', ':'])
}

fn is_cover(line: &str) -> bool {
    COVER_WORDS.iter().any(|w| line.contains(w))
}

/// Lines that are exam structure, never page furniture however often they repeat.
fn is_structural(line: &str) -> bool {
    group_range(line).is_some()
        || question_no(line).is_some()
        || SUB_QUESTION.is_match(line)
        || OPTION_START.is_match(line)
        || SECTION.is_match(line)
        || LABEL.is_match(line)
        || BULLET.is_match(line)
}

/// A line compared across pages: spaces collapsed and digits blanked, so 「英文考科 第 2 頁」 on
/// page 2 matches 「英文考科 第 3 頁」 on page 3.
fn furniture_key(line: &str) -> String {
    line.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .map(|c| if c.is_ascii_digit() { '#' } else { c })
        .collect()
}

/// The pages' lines, normalized and without page furniture, each with its page.
fn content_lines(pages: &[PageLines]) -> Vec<(i32, String)> {
    let pages: Vec<(i32, Vec<String>)> = pages
        .iter()
        .map(|p| {
            let lines = p
                .lines
                .iter()
                .map(|l| normalize_line(l))
                .filter(|l| !l.is_empty())
                .collect();
            (p.page, lines)
        })
        .collect();
    let at_edge = |i: usize, n: usize| i < EDGE_LINES || i + EDGE_LINES >= n;
    let mut seen: HashMap<String, HashSet<i32>> = HashMap::new();
    for (page, lines) in &pages {
        for (i, l) in lines.iter().enumerate() {
            if at_edge(i, lines.len()) && !is_structural(l) {
                seen.entry(furniture_key(l)).or_default().insert(*page);
            }
        }
    }
    // A running header or footer is on most of the pages after the cover.
    let others = pages.len().saturating_sub(1);
    let repeated = |l: &str| {
        seen.get(&furniture_key(l))
            .is_some_and(|on| on.len() >= 2 && on.len() * 2 > others)
    };
    let mut out = Vec::new();
    for (page, lines) in &pages {
        for (i, l) in lines.iter().enumerate() {
            let furniture = at_edge(i, lines.len())
                && (FURNITURE.is_match(l) || (!is_structural(l) && repeated(l)));
            if !furniture {
                out.push((*page, l.clone()));
            }
        }
    }
    out
}

/// Whether normalized lines read like an exam paper: a 「為題組」 line with questions counting up,
/// or at least five questions counting up with options.
pub fn looks_like_exam(lines: &[&str]) -> bool {
    let marks = lines.iter().filter(|l| group_range(l).is_some()).count();
    let options = lines.iter().filter(|l| OPTION.is_match(l)).count();
    let (mut best, mut run, mut last) = (0, 0, 0);
    for n in lines.iter().filter_map(|l| question_no(l)) {
        run = if n == last + 1 { run + 1 } else { 1 };
        last = n;
        best = best.max(run);
    }
    (marks >= 1 && best >= 2) || (best >= 5 && options >= 3)
}

// ---------------------------------------------------------------- rebuilding

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Text before the exam proper (the cover, or a title).
    Front,
    Heading,
    Note,
    Mark,
    Passage,
    Question,
    Other,
}

/// A paragraph being read.
#[derive(Debug)]
struct Para {
    kind: Kind,
    page: i32,
    lines: Vec<String>,
    group: Option<String>,
    front: bool,
    /// The question number (numbered questions only).
    no: Option<u32>,
    /// The last option letter read so far (questions only).
    last_option: Option<char>,
}

/// A finished paragraph.
#[derive(Debug, Clone)]
struct Piece {
    kind: Kind,
    text: String,
    page: i32,
    group: Option<String>,
    front: bool,
    no: Option<u32>,
}

#[derive(Debug)]
struct Group {
    first: u32,
    last: u32,
    key: String,
}

struct Rebuild<'a> {
    whole_lines: bool,
    widest: &'a HashMap<i32, usize>,
    out: Vec<Piece>,
    cur: Option<Para>,
    /// The number of the last question read.
    last_q: u32,
    /// A section heading came after the last question: numbering may start again.
    fresh_section: bool,
    group: Option<Group>,
    /// Inside a 題組, after its 「為題組」 line and before its first question.
    passage_open: bool,
    /// The last numbered item of the current note (「說明：1. … 2. …」).
    note_items: u32,
}

impl<'a> Rebuild<'a> {
    fn new(whole_lines: bool, widest: &'a HashMap<i32, usize>) -> Self {
        Self {
            whole_lines,
            widest,
            out: Vec::new(),
            cur: None,
            last_q: 0,
            fresh_section: true,
            group: None,
            passage_open: false,
            note_items: 0,
        }
    }

    fn widest_on(&self, page: i32) -> usize {
        self.widest.get(&page).copied().unwrap_or(0)
    }

    fn cur_kind(&self) -> Option<Kind> {
        self.cur.as_ref().map(|p| p.kind)
    }

    fn start(&mut self, kind: Kind, page: i32, line: &str, group: Option<String>, no: Option<u32>) {
        self.flush();
        self.cur = Some(Para {
            kind,
            page,
            lines: vec![line.to_string()],
            group,
            front: false,
            no,
            last_option: last_option(line),
        });
    }

    fn append(&mut self, line: &str) {
        if let Some(p) = self.cur.as_mut() {
            p.lines.push(line.to_string());
            p.last_option = p.last_option.max(last_option(line));
        }
    }

    fn flush(&mut self) {
        let Some(p) = self.cur.take() else {
            return;
        };
        let lines: Vec<&str> = p.lines.iter().map(String::as_str).collect();
        let texts = match p.kind {
            Kind::Front | Kind::Other if self.whole_lines => p.lines.clone(),
            Kind::Front | Kind::Other => join_wrapped_within(&lines, true, self.widest_on(p.page)),
            _ => join_wrapped(&lines, false),
        };
        for text in texts.into_iter().filter(|t| !t.is_empty()) {
            self.out.push(Piece {
                kind: p.kind,
                text,
                page: p.page,
                group: p.group.clone(),
                front: p.front,
                no: p.no,
            });
        }
    }

    /// The group, if its questions are not all read yet.
    fn open_group_key(&mut self) -> Option<String> {
        if self.group.as_ref().is_some_and(|g| self.last_q >= g.last) {
            self.group = None;
        }
        self.group.as_ref().map(|g| g.key.clone())
    }

    /// Text before the exam proper. Bullets and labels start an item; a label ending in a colon
    /// stands alone; the rest is joined by the wrapped-line rule at flush.
    fn front_line(&mut self, page: i32, line: &str, cover: bool) {
        let starts = BULLET.is_match(line) || LABEL.is_match(line);
        if self.cur_kind() == Some(Kind::Front) && !starts {
            self.append(line);
        } else {
            self.start(Kind::Front, page, line, None, None);
            if let Some(p) = self.cur.as_mut() {
                p.front = cover;
            }
        }
        if ends_with_colon(line) {
            self.flush();
        }
    }

    /// Whether `n.` starts the next question rather than an item of a note or a number in a
    /// passage.
    fn accepts(&self, n: u32) -> bool {
        if self.cur_kind() == Some(Kind::Note) && self.note_items > 0 && n == self.note_items + 1 {
            return false;
        }
        let in_group = self
            .group
            .as_ref()
            .is_some_and(|g| (g.first..=g.last).contains(&n) && n > self.last_q);
        if self.passage_open {
            // A numbered list inside the passage is not its questions.
            return in_group;
        }
        // A question number may be lost in extraction, so one gap is allowed.
        (self.last_q + 1..=self.last_q + 2).contains(&n)
            || (self.fresh_section && (n == 1 || n > self.last_q))
            || in_group
    }

    /// After its last option, a question ends at a line that is not an option, unless the
    /// option was wrapped onto it.
    fn question_ends_before(&self, line: &str) -> bool {
        let Some(p) = self.cur.as_ref() else {
            return true;
        };
        if OPTION_START.is_match(line) || p.last_option.is_none_or(|o| o < 'D') {
            return false;
        }
        if self.whole_lines {
            return true;
        }
        let prev = p.lines.last().map(String::as_str).unwrap_or("");
        ends_sentence(prev) || (line_width(prev) as f32) < self.widest_on(p.page) as f32 * FULL_LINE
    }

    /// Whether a note goes on into this line: its next numbered item, or (in a PDF) a line
    /// wrapped from the one before, which did not end a sentence well short of the margin.
    fn note_goes_on(&mut self, line: &str) -> bool {
        if let Some(n) = question_no(line) {
            if self.note_items > 0 && n == self.note_items + 1 {
                self.note_items = n;
                return true;
            }
        }
        if self.whole_lines {
            return false;
        }
        let Some(p) = self.cur.as_ref() else {
            return false;
        };
        let prev = p.lines.last().map(String::as_str).unwrap_or("");
        let short = (line_width(prev) as f32) < self.widest_on(p.page) as f32 * SHORT_LINE;
        !(ends_sentence(prev) && short)
    }

    fn body_line(&mut self, page: i32, line: &str) {
        if let Some((first, last)) = group_range(line) {
            let key = format!("{first}-{last}");
            self.group = Some(Group {
                first,
                last,
                key: key.clone(),
            });
            self.passage_open = true;
            self.start(Kind::Mark, page, line, Some(key), None);
            self.flush();
            return;
        }
        if SECTION.is_match(line) {
            self.group = None;
            self.passage_open = false;
            self.fresh_section = true;
            self.start(Kind::Heading, page, line, None, None);
            self.flush();
            return;
        }
        if let Some(n) = question_no(line).filter(|&n| self.accepts(n)) {
            self.passage_open = false;
            if self
                .group
                .as_ref()
                .is_some_and(|g| n < g.first || n > g.last)
            {
                self.group = None;
            }
            self.last_q = n;
            self.fresh_section = false;
            let group = self.group.as_ref().map(|g| g.key.clone());
            self.start(Kind::Question, page, line, group, Some(n));
            return;
        }
        if NOTE.is_match(line) {
            self.group = None;
            self.passage_open = false;
            self.note_items = NOTE_FIRST_ITEM
                .captures(line)
                .and_then(|c| c.get(1)?.as_str().parse().ok())
                .unwrap_or(0);
            self.start(Kind::Note, page, line, None, None);
            if ends_with_colon(line) {
                self.flush();
            }
            return;
        }
        if !self.passage_open && SUB_QUESTION.is_match(line) {
            let group = self.open_group_key();
            self.start(Kind::Question, page, line, group, None);
            return;
        }
        let goes_on = match self.cur_kind() {
            Some(Kind::Passage) => true,
            Some(Kind::Question) => !self.question_ends_before(line),
            Some(Kind::Note) => self.note_goes_on(line),
            Some(Kind::Other) => !(BULLET.is_match(line) || LABEL.is_match(line)),
            _ => false,
        };
        if goes_on {
            self.append(line);
        } else if self.passage_open {
            let group = self.group.as_ref().map(|g| g.key.clone());
            self.start(Kind::Passage, page, line, group, None);
        } else {
            self.open_group_key();
            self.start(Kind::Other, page, line, None, None);
        }
    }
}

/// A 題組 without its 「為題組」 line: text of at least [`MIN_PASSAGE_CHARS`] directly followed by
/// questions outside any group becomes one passage, grouped with those questions.
fn implicit_groups(pieces: Vec<Piece>) -> Vec<Piece> {
    let is_text = |p: &Piece| p.kind == Kind::Other && !p.front;
    let is_free_question =
        |p: &Piece| p.kind == Kind::Question && p.group.is_none() && p.no.is_some();
    let mut out = Vec::with_capacity(pieces.len());
    let mut i = 0;
    while i < pieces.len() {
        if !is_text(&pieces[i]) {
            out.push(pieces[i].clone());
            i += 1;
            continue;
        }
        let text_end = (i..pieces.len())
            .find(|&j| !is_text(&pieces[j]))
            .unwrap_or(pieces.len());
        let questions_end = (text_end..pieces.len())
            .find(|&j| !is_free_question(&pieces[j]))
            .unwrap_or(pieces.len());
        let chars: usize = pieces[i..text_end]
            .iter()
            .map(|p| p.text.chars().count())
            .sum();
        if questions_end > text_end && chars >= MIN_PASSAGE_CHARS {
            let first = pieces[text_end].no.unwrap_or_default();
            let last = pieces[questions_end - 1].no.unwrap_or_default();
            let key = format!("{first}-{last}");
            let texts: Vec<&str> = pieces[i..text_end]
                .iter()
                .map(|p| p.text.as_str())
                .collect();
            out.push(Piece {
                kind: Kind::Passage,
                text: join_wrapped(&texts, false).concat(),
                page: pieces[i].page,
                group: Some(key.clone()),
                front: false,
                no: None,
            });
            out.extend(pieces[text_end..questions_end].iter().map(|q| Piece {
                group: Some(key.clone()),
                ..q.clone()
            }));
            i = questions_end;
        } else {
            out.extend(pieces[i..text_end].iter().cloned());
            i = text_end;
        }
    }
    out
}

/// The paragraphs of an exam paper (see the module table), or `None` when the pages do not look
/// like one. `lines_are_paragraphs`: the lines are OCR paragraphs, not wrapped PDF lines.
pub fn rebuild(pages: &[PageLines], lines_are_paragraphs: bool) -> Option<Vec<Block>> {
    let lines = content_lines(pages);
    let texts: Vec<&str> = lines.iter().map(|(_, l)| l.as_str()).collect();
    if !looks_like_exam(&texts) {
        return None;
    }
    let mut widest: HashMap<i32, usize> = HashMap::new();
    for (page, line) in &lines {
        let w = widest.entry(*page).or_default();
        *w = (*w).max(line_width(line));
    }
    // The exam proper starts at its first part or section heading, 題組 or question 1.
    let body = texts
        .iter()
        .position(|l| {
            group_range(l).is_some()
                || SECTION.is_match(l)
                || (question_no(l) == Some(1) && !is_cover(l))
        })
        .unwrap_or(0);
    let cover = texts[..body].iter().any(|l| is_cover(l));
    let mut r = Rebuild::new(lines_are_paragraphs, &widest);
    for (i, (page, line)) in lines.iter().enumerate() {
        if i < body {
            r.front_line(*page, line, cover);
        } else {
            r.body_line(*page, line);
        }
    }
    r.flush();
    Some(
        implicit_groups(r.out)
            .into_iter()
            .map(|p| Block {
                text: p.text,
                page: Some(p.page),
                group: p.group,
                front: p.front,
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(page: i32, text: &str) -> PageLines {
        PageLines {
            page,
            lines: text.lines().map(str::to_string).collect(),
        }
    }

    fn texts(blocks: &[Block]) -> Vec<&str> {
        blocks.iter().map(|b| b.text.as_str()).collect()
    }

    fn in_group<'a>(blocks: &'a [Block], key: &str) -> Vec<&'a str> {
        blocks
            .iter()
            .filter(|b| b.group.as_deref() == Some(key))
            .map(|b| b.text.as_str())
            .collect()
    }

    #[test]
    fn kangxi_radicals_and_letter_spacing_are_normalized() {
        assert_eq!(
            normalize_line("  英⽂考科 第 6 ⾄ 10 題  "),
            "英文考科 第 6 至 10 題"
        );
        assert_eq!(normalize_line("作 答 注 意 事 項"), "作答注意事項");
        assert_eq!(normalize_line("M a n n a   模 擬 試 卷"), "Manna 模擬試卷");
        assert_eq!(
            normalize_line("請 於 考 試 開 始 鈴 響 時 ， 始 得 翻 閱 本 試 題 本"),
            "請於考試開始鈴響時，始得翻閱本試題本"
        );
        // Not a spaced heading: letters alone, or ordinary words.
        assert_eq!(normalize_line("A B C D"), "A B C D");
        assert_eq!(
            normalize_line("(A) eagerly (B) rarely"),
            "(A) eagerly (B) rarely"
        );
    }

    #[test]
    fn page_numbers_and_running_headers_are_dropped() {
        let pages = [
            page(1, "Title page\n作答注意事項"),
            page(
                2,
                "英文考科 第 1 頁\n1. One?\n(A) a (B) b (C) c (D) d\n- 1 -",
            ),
            page(
                3,
                "英文考科 第 2 頁\n2. Two?\n(A) a (B) b (C) c (D) d\n- 2 -",
            ),
            page(
                4,
                "英文考科 第 3 頁\n3. Three?\n(A) a (B) b (C) c (D) d\n背面尚有試題",
            ),
        ];
        let kept: Vec<String> = content_lines(&pages).into_iter().map(|(_, l)| l).collect();
        assert_eq!(
            kept,
            [
                "Title page",
                "作答注意事項",
                "1. One?",
                "(A) a (B) b (C) c (D) d",
                "2. Two?",
                "(A) a (B) b (C) c (D) d",
                "3. Three?",
                "(A) a (B) b (C) c (D) d",
            ],
            "options repeat at the edges too, but they are exam structure"
        );
    }

    #[test]
    fn ordinary_texts_are_not_exam_papers() {
        let story = [
            "Mrs. Lin put an umbrella stand outside her shop.",
            "1. It rained.",
            "2. Nobody took one.",
        ];
        assert!(!looks_like_exam(&story));
        let recipe = [
            "1. Wash the rice.",
            "2. Boil it.",
            "3. Wait.",
            "4. Eat.",
            "5. Smile.",
        ];
        assert!(!looks_like_exam(&recipe), "numbers without options");
        assert!(rebuild(&[page(1, &story.join("\n"))], false).is_none());
    }

    /// What OCR gives for an exam page: one paragraph per line, the passage in several.
    #[test]
    fn ocr_paragraphs_of_an_exam_are_grouped() {
        let pages = [
            page(
                1,
                "第壹部分、選擇題\n\
                 一、閱讀測驗\n\
                 說明：第 31 題至第 34 題，每題請選出最適當的一個選項。\n\
                 第 31 至 32 題為題組\n\
                 Most people think of bees as busy workers that never rest.\n\
                 In one study, researchers kept a group of bees awake for a night.\n\
                 31. What is the passage mainly about?\n\
                 (A) Flowers.\n(B) Sleep.\n(C) Farmers.\n(D) Dances.",
            ),
            page(
                2,
                "32. What happened to the tired bees?\n\
                 (A) They slept.\n(B) They left.\n(C) They erred.\n(D) They sang.\n\
                 第貳部分、非選擇題\n\
                 (1) 許多學生喜歡在週末到圖書館讀書。",
            ),
        ];
        let b = rebuild(&pages, true).expect("an exam");
        assert_eq!(
            in_group(&b, "31-32"),
            [
                "第 31 至 32 題為題組",
                "Most people think of bees as busy workers that never rest. In one study, researchers kept a group of bees awake for a night.",
                "31. What is the passage mainly about? (A) Flowers. (B) Sleep. (C) Farmers. (D) Dances.",
                "32. What happened to the tired bees? (A) They slept. (B) They left. (C) They erred. (D) They sang.",
            ]
        );
        assert_eq!(
            b.iter().find(|x| x.text.starts_with("32.")).unwrap().page,
            Some(2)
        );
        assert_eq!(
            texts(&b)[b.len() - 2..],
            ["第貳部分、非選擇題", "(1) 許多學生喜歡在週末到圖書館讀書。"]
        );
        assert!(b.iter().all(|x| !x.front));
    }

    #[test]
    fn a_passage_without_its_group_line_is_still_grouped() {
        let passage = "Every year on the second Saturday of May, the town of Riverside holds a \
                       Repair Day. Residents bring broken lamps, toasters and bicycles to the town \
                       hall, where volunteers fix them for free.";
        let text = format!(
            "一、閱讀測驗\n\
             1. Pick one.\n(A) a (B) b (C) c (D) d\n\
             2. Pick two.\n(A) a (B) b (C) c (D) d\n\
             {passage}\n\
             3. When is Repair Day held?\n(A) May.\n(B) June.\n(C) July.\n(D) Never.\n\
             4. Who fixes things?\n(A) Volunteers.\n(B) Shops.\n(C) Nobody.\n(D) Robots.\n\
             5. Where?\n(A) Hall.\n(B) Park.\n(C) Shop.\n(D) Home."
        );
        let b = rebuild(&[page(1, &text)], true).expect("an exam");
        let g = in_group(&b, "3-5");
        assert_eq!(g.len(), 4, "{b:#?}");
        assert_eq!(
            g[0],
            passage.split_whitespace().collect::<Vec<_>>().join(" ")
        );
        assert!(g[1].starts_with("3. When") && g[2].starts_with("4. Who"));
        // Questions 1 and 2 have no passage: no group.
        assert!(b
            .iter()
            .filter(|x| x.text.starts_with('1') || x.text.starts_with('2'))
            .all(|x| x.group.is_none()));
    }

    #[test]
    fn numbers_inside_a_passage_or_a_note_are_not_questions() {
        let text = "一、閱讀測驗\n\
             說明：1. 請依文意作答。\n\
             2. 每題 2 分。\n\
             第 11 至 11 題為題組\n\
             To make rice, follow these steps:\n\
             1. Wash the rice.\n\
             2. Boil it for twenty minutes.\n\
             11. What should you do first?\n(A) Wash.\n(B) Boil.\n(C) Eat.\n(D) Sleep.";
        let b = rebuild(&[page(1, text)], true).expect("an exam");
        assert_eq!(
            texts(&b),
            [
                "一、閱讀測驗",
                "說明：1. 請依文意作答。2. 每題 2 分。",
                "第 11 至 11 題為題組",
                "To make rice, follow these steps: 1. Wash the rice. 2. Boil it for twenty minutes.",
                "11. What should you do first? (A) Wash. (B) Boil. (C) Eat. (D) Sleep.",
            ]
        );
    }

    /// The fake exam paper in `tests/fixtures` (made by us, laid out like a 學測 paper): read from
    /// the PDF the way an upload is.
    #[test]
    fn the_fake_exam_pdf_reads_as_a_teacher_picks_it() {
        let data = include_bytes!("../../tests/fixtures/fake_exam.pdf");
        let e = super::super::extract::extract(
            crate::materials::file_type_for_name("fake_exam.pdf").unwrap(),
            data,
        );
        let b = &e.blocks;
        let all = texts(b);
        // The cover: a few items, all left out by default.
        let front: Vec<&str> = b
            .iter()
            .filter(|x| x.front)
            .map(|x| x.text.as_str())
            .collect();
        assert!(front.len() < 16, "{front:#?}");
        assert!(b
            .iter()
            .take(front.len())
            .all(|x| x.front && x.page == Some(1)));
        assert!(front.contains(&"作答注意事項"), "{front:#?}");
        assert!(front.contains(
            &"・選擇題用 2B 鉛筆在「答題卷」上作答；更正時，應以橡皮擦擦拭，切勿使用修正液（帶）。"
        ));
        assert!(front
            .iter()
            .any(|t| t.starts_with("著作權說明：") && t.ends_with("可自由用於測試。")));
        assert!(b.iter().filter(|x| !x.front).all(|x| x.page != Some(1)));
        // No page furniture, no Kangxi radicals.
        assert!(
            all.iter()
                .all(|t| !t.contains("第 1 頁") && !t.contains("Manna 模擬試卷 英文考科")),
            "{all:#?}"
        );
        assert!(all
            .iter()
            .all(|t| !t.chars().any(|c| ('\u{2F00}'..='\u{2FDF}').contains(&c))));
        // Headings and notes stand alone.
        for heading in [
            "第壹部分、選擇題（占 62 分）",
            "一、詞彙題（占 10 分）",
            "二、綜合測驗（占 10 分）",
            "三、閱讀測驗（占 16 分）",
            "第貳部分、非選擇題（占 28 分）",
        ] {
            assert!(all.contains(&heading), "{heading} in {all:#?}");
        }
        assert!(all
            .iter()
            .any(|t| t.starts_with("說明：第 1 題至第 5 題") && t.ends_with("該題以零分計算。")));
        // Questions 1-5: one paragraph each, options included, no group.
        let q4 = b.iter().find(|x| x.text.starts_with("4. ")).unwrap();
        assert_eq!(q4.text, "4. After weeks of practice, Ken finally felt ________ enough to sing in front of the whole school. (A) confident (B) curious (C) careless (D) crowded");
        assert_eq!(q4.group, None);
        // 題組 6-10: the line, the passage as one paragraph (page 2), questions 6-10 (page 3).
        let g = in_group(b, "6-10");
        assert_eq!(g.len(), 7, "{g:#?}");
        assert_eq!(g[0], "第 6 至 10 題為題組");
        assert!(
            g[1].starts_with("Mrs. Lin put") && g[1].ends_with("for its umbrellas."),
            "{}",
            g[1]
        );
        assert_eq!(g[2], "6. (A) laughed (B) cried (C) waited (D) pointed");
        assert!(g[6].starts_with("10. "));
        assert_eq!(
            b.iter()
                .find(|x| x.text.starts_with("Mrs. Lin"))
                .unwrap()
                .page,
            Some(2)
        );
        assert_eq!(
            b.iter().find(|x| x.text.starts_with("6. ")).unwrap().page,
            Some(3)
        );
        // 題組 11-14: three printed paragraphs make one passage; its questions are on the next page.
        let g = in_group(b, "11-14");
        assert_eq!(g.len(), 6, "{g:#?}");
        assert!(
            g[1].starts_with("Most people think")
                && g[1].contains("less food. The findings")
                && g[1].ends_with("think again.")
        );
        assert!(g[2].starts_with("11. What is the passage mainly about? (A) How bees"));
        assert!(g[5].ends_with("(D) Plant more flowers near the hives."));
        // 題組 15-18 in two columns: the question wrapped over two lines is one paragraph.
        let g = in_group(b, "15-18");
        assert_eq!(g.len(), 6, "{g:#?}");
        assert!(
            g[1].starts_with("Every year on the second Saturday")
                && g[1].ends_with("as volunteers.")
        );
        assert!(g[5].starts_with("18. Which of the following is TRUE about the visitors? (A)"));
        // Part two: notes keep their numbered items; sub-questions stand alone.
        assert!(all.contains(&"說明：1. 請將以下中文句子譯成正確而通順達意的英文，並將答案寫在「答題卷」上。2. 請依序作答，並標明子題號 (1)、(2)。每題 4 分，共 8 分。"), "{all:#?}");
        assert!(all.contains(&"(1) 許多學生喜歡在週末到圖書館讀書，因為那裡安靜又涼爽。"));
        assert!(all
            .iter()
            .any(|t| t.starts_with("提示：你的學校") && t.ends_with("順利運作。")));
        // Every question is its own paragraph, and nothing else starts like one.
        let questions: Vec<u32> = all.iter().filter_map(|t| question_no(t)).collect();
        assert_eq!(questions, (1..=18).collect::<Vec<_>>());
        assert!(b.len() < 60, "{} paragraphs: {all:#?}", b.len());
    }
}
