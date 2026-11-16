//! chandra's layout-labelled HTML → blocks → paragraphs. Pure functions.
//!
//! chandra answers one page with a run of `<div data-bbox="x0 y0 x1 y1" data-label="…">…</div>`
//! in reading order. Each labelled element becomes one [`OcrBlock`] (all of them are kept, for
//! later use: Phase 2 splits exam papers by `Section-Header`); [`paragraphs`] then makes the
//! material's paragraphs:
//!
//! | label | paragraphs |
//! |---|---|
//! | `Section-Header` | one (the block keeps the label, so it is still known as a heading) |
//! | `Text`, `Caption`, `Footnote` and any other label | one |
//! | `List-Group` | one per `<li>` |
//! | `Table` | one per `<tr>`, cells joined with ` \| ` |
//! | `Page-Header`, `Page-Footer`, `Picture` (also `Image`, `Figure`) | none; pictures are counted |
//!
//! Tags are stripped, entities decoded, zero-width characters dropped and whitespace collapsed
//! (no space is put back between two CJK characters). The order is the order chandra returned.
//! HTML without any labelled element (plain HTML or markdown) becomes one `Text` block per line.

use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

pub const LABEL_SECTION_HEADER: &str = "Section-Header";
pub const LABEL_TEXT: &str = "Text";
pub const LABEL_LIST: &str = "List-Group";
pub const LABEL_TABLE: &str = "Table";
/// Labels that never become text.
pub const SKIPPED_LABELS: [&str; 5] = ["Page-Header", "Page-Footer", "Picture", "Image", "Figure"];
/// Labels counted as pictures.
pub const PICTURE_LABELS: [&str; 3] = ["Picture", "Image", "Figure"];

/// One labelled element of a page. Tables and lists keep one row or item per line (`\n`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OcrBlock {
    pub label: String,
    pub text: String,
    /// `[x0, y0, x1, y1]` as chandra gave it; `null` when it gave none.
    pub bbox: Option<[i64; 4]>,
}

/// One page of the stored OCR result (`materials.ocr_pages`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OcrPage {
    pub page: i32,
    pub blocks: Vec<OcrBlock>,
    /// Pictures on the page (「此頁有 n 張圖」): metadata, never part of the text.
    #[serde(default)]
    pub pictures: usize,
    /// Why the page could not be read; `blocks` is then empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl OcrPage {
    pub fn read(page: i32, html: &str) -> Self {
        let blocks = blocks(html);
        Self {
            page,
            pictures: pictures(&blocks),
            blocks,
            error: None,
        }
    }

    pub fn failed(page: i32, error: impl Into<String>) -> Self {
        Self {
            page,
            blocks: Vec::new(),
            pictures: 0,
            error: Some(error.into()),
        }
    }
}

// ---------------------------------------------------------------- tokens

#[derive(Debug, Clone, PartialEq)]
enum Token<'a> {
    Open { name: String, attrs: &'a str },
    Close { name: String },
    Text(&'a str),
}

const VOID: [&str; 13] = [
    "br", "img", "hr", "input", "meta", "link", "col", "area", "base", "embed", "source", "track",
    "wbr",
];
/// Tags that separate words (an inline tag such as `<sub>` does not).
const BREAKING: [&str; 22] = [
    "p",
    "div",
    "br",
    "li",
    "ul",
    "ol",
    "tr",
    "td",
    "th",
    "table",
    "thead",
    "tbody",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "section",
    "blockquote",
    "pre",
    "hr",
];

/// The end of a tag that starts at `start` (`<`), skipping `>` inside quoted attribute values.
fn tag_end(s: &str, start: usize) -> Option<usize> {
    let mut quote: Option<u8> = None;
    for (i, b) in s.as_bytes().iter().enumerate().skip(start + 1) {
        match (quote, *b) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, b'"') | (None, b'\'') => quote = Some(*b),
            (None, b'>') => return Some(i),
            _ => {}
        }
    }
    None
}

fn tokens(html: &str) -> Vec<Token<'_>> {
    let mut out = Vec::new();
    let mut i = 0;
    let mut text_start = 0;
    let bytes = html.as_bytes();
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        let rest = &html[i..];
        let next = rest.as_bytes().get(1).copied().unwrap_or(b' ');
        let is_tag = next.is_ascii_alphabetic() || next == b'/' || next == b'!' || next == b'?';
        if !is_tag {
            i += 1;
            continue;
        }
        if text_start < i {
            out.push(Token::Text(&html[text_start..i]));
        }
        if rest.starts_with("<!--") {
            let end = rest.find("-->").map_or(html.len(), |e| i + e + 3);
            i = end;
            text_start = i;
            continue;
        }
        let Some(end) = tag_end(html, i) else {
            // An unclosed `<`: the rest is text.
            text_start = i;
            break;
        };
        let inner = &html[i + 1..end];
        if let Some(name) = inner.strip_prefix('/') {
            let name: String = name
                .trim()
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
                .collect();
            out.push(Token::Close {
                name: name.to_ascii_lowercase(),
            });
        } else if !inner.starts_with('!') && !inner.starts_with('?') {
            let name_len = inner
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
                .unwrap_or(inner.len());
            let name = inner[..name_len].to_ascii_lowercase();
            let attrs = inner[name_len..].trim_end_matches('/');
            let self_closing = inner.ends_with('/') || VOID.contains(&name.as_str());
            out.push(Token::Open {
                name: name.clone(),
                attrs,
            });
            if self_closing {
                out.push(Token::Close { name });
            }
        }
        i = end + 1;
        text_start = i;
    }
    if text_start < html.len() {
        out.push(Token::Text(&html[text_start..]));
    }
    out
}

static ATTR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(?:^|\s)([a-z_:][-a-z0-9_:.]*)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'>]+))"#)
        .expect("valid regex")
});

fn attr(attrs: &str, name: &str) -> Option<String> {
    ATTR.captures_iter(attrs)
        .find(|c| c[1].eq_ignore_ascii_case(name))
        .map(|c| {
            let raw = c
                .get(2)
                .or_else(|| c.get(3))
                .or_else(|| c.get(4))
                .map_or("", |m| m.as_str());
            decode_entities(raw)
        })
}

fn parse_bbox(raw: &str) -> Option<[i64; 4]> {
    let nums: Vec<f64> = raw
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|p| !p.is_empty())
        .map(|p| p.parse::<f64>())
        .collect::<Result<_, _>>()
        .ok()?;
    if nums.len() != 4 || nums.iter().any(|n| !n.is_finite()) {
        return None;
    }
    Some([
        nums[0].round() as i64,
        nums[1].round() as i64,
        nums[2].round() as i64,
        nums[3].round() as i64,
    ])
}

// ---------------------------------------------------------------- text

/// Decodes `&amp;`, `&#39;`, `&#x4E2D;` and the common named entities; anything else stays.
pub fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let end = rest
            .char_indices()
            .take(12)
            .find(|(_, c)| *c == ';')
            .map(|(i, _)| i);
        let decoded = end.and_then(|end| {
            let name = &rest[1..end];
            let c = if let Some(num) = name.strip_prefix('#') {
                let code = match num.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok(),
                    None => num.parse::<u32>().ok(),
                };
                code.and_then(char::from_u32)
            } else {
                named_entity(name)
            };
            c.map(|c| (c, end))
        });
        match decoded {
            Some((c, end)) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn named_entity(name: &str) -> Option<char> {
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        "ensp" | "emsp" | "thinsp" => ' ',
        "ndash" => '–',
        "mdash" => '—',
        "hellip" => '…',
        "lsquo" => '‘',
        "rsquo" => '’',
        "ldquo" => '“',
        "rdquo" => '”',
        "middot" => '·',
        "bull" => '•',
        "times" => '×',
        "divide" => '÷',
        "deg" => '°',
        "plusmn" => '±',
        "copy" => '©',
        "reg" => '®',
        "laquo" => '«',
        "raquo" => '»',
        "larr" => '←',
        "rarr" => '→',
        "uarr" => '↑',
        "darr" => '↓',
        "le" => '≤',
        "ge" => '≥',
        "ne" => '≠',
        _ => return None,
    })
}

/// Zero-width and other invisible format characters (also used by `poor_text`).
pub fn is_format_char(c: char) -> bool {
    matches!(c, '\u{200B}'..='\u{200F}' | '\u{FEFF}' | '\u{2060}')
}

fn is_cjk(c: char) -> bool {
    matches!(c,
        '\u{3000}'..='\u{303F}'   // CJK punctuation
        | '\u{3040}'..='\u{30FF}' // kana
        | '\u{3100}'..='\u{312F}' // bopomofo
        | '\u{3400}'..='\u{4DBF}'
        | '\u{4E00}'..='\u{9FFF}'
        | '\u{F900}'..='\u{FAFF}'
        | '\u{FF00}'..='\u{FFEF}' // full-width forms
        | '\u{20000}'..='\u{2FFFF}')
}

/// Collapses whitespace runs to one space, and to nothing between two CJK characters (a line
/// break inside a Chinese sentence is not a word break). Format characters are dropped.
pub fn collapse(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .filter(|c| !is_format_char(*c) && (!c.is_control() || c.is_whitespace()))
        .collect();
    let mut out = String::with_capacity(cleaned.len());
    for word in cleaned.split_whitespace() {
        if let (Some(a), Some(b)) = (out.chars().last(), word.chars().next()) {
            if !(is_cjk(a) && is_cjk(b)) {
                out.push(' ');
            }
        }
        out.push_str(word);
    }
    out
}

/// The text of some tokens as one line: breaking tags become spaces.
fn plain(tokens: &[Token<'_>]) -> String {
    let mut raw = String::new();
    for t in tokens {
        match t {
            Token::Text(s) => raw.push_str(&decode_entities(s)),
            Token::Open { name, .. } | Token::Close { name } => {
                if BREAKING.contains(&name.as_str()) {
                    raw.push(' ');
                }
            }
        }
    }
    collapse(&raw)
}

/// `<tr>` rows, cells joined with ` | `. `None` when there is no row.
fn table_rows(tokens: &[Token<'_>]) -> Option<Vec<String>> {
    let mut rows: Vec<String> = Vec::new();
    let mut row: Option<Vec<String>> = None;
    let mut cell: Option<String> = None;
    let mut saw_row = false;
    let close_cell = |row: &mut Option<Vec<String>>, cell: &mut Option<String>| {
        if let Some(c) = cell.take() {
            row.get_or_insert_with(Vec::new).push(collapse(&c));
        }
    };
    let close_row = |rows: &mut Vec<String>, row: &mut Option<Vec<String>>| {
        if let Some(cells) = row.take() {
            if cells.iter().any(|c| !c.is_empty()) {
                rows.push(cells.join(" | "));
            }
        }
    };
    for t in tokens {
        match t {
            Token::Open { name, .. } if name == "tr" => {
                close_cell(&mut row, &mut cell);
                close_row(&mut rows, &mut row);
                row = Some(Vec::new());
                saw_row = true;
            }
            Token::Open { name, .. } if name == "td" || name == "th" => {
                close_cell(&mut row, &mut cell);
                cell = Some(String::new());
            }
            Token::Close { name } if name == "td" || name == "th" => {
                close_cell(&mut row, &mut cell);
            }
            Token::Close { name } if name == "tr" => {
                close_cell(&mut row, &mut cell);
                close_row(&mut rows, &mut row);
            }
            Token::Open { name, .. } | Token::Close { name } => {
                if let Some(c) = cell.as_mut() {
                    if BREAKING.contains(&name.as_str()) {
                        c.push(' ');
                    }
                }
            }
            Token::Text(s) => {
                if let Some(c) = cell.as_mut() {
                    c.push_str(&decode_entities(s));
                }
            }
        }
    }
    close_cell(&mut row, &mut cell);
    close_row(&mut rows, &mut row);
    saw_row.then_some(rows)
}

/// One entry per `<li>` (a nested item is its own entry, in reading order); text outside any
/// item is an entry of its own. `None` when there is no item.
fn list_items(tokens: &[Token<'_>]) -> Option<Vec<String>> {
    let mut items: Vec<String> = Vec::new();
    let mut open: Vec<usize> = Vec::new();
    let mut loose: Option<usize> = None;
    let mut saw_item = false;
    for t in tokens {
        match t {
            Token::Open { name, .. } if name == "li" => {
                items.push(String::new());
                open.push(items.len() - 1);
                loose = None;
                saw_item = true;
            }
            Token::Close { name } if name == "li" => {
                open.pop();
                loose = None;
            }
            Token::Open { name, .. } | Token::Close { name } => {
                if BREAKING.contains(&name.as_str()) {
                    match open.last() {
                        Some(&i) => items[i].push(' '),
                        None => loose = None,
                    }
                }
            }
            Token::Text(s) => {
                let text = decode_entities(s);
                match open.last() {
                    Some(&i) => items[i].push_str(&text),
                    None if text.trim().is_empty() => {}
                    None => {
                        let i = *loose.get_or_insert_with(|| {
                            items.push(String::new());
                            items.len() - 1
                        });
                        items[i].push_str(&text);
                    }
                }
            }
        }
    }
    saw_item.then(|| {
        items
            .iter()
            .map(|i| collapse(i))
            .filter(|i| !i.is_empty())
            .collect()
    })
}

fn block_text(label: &str, tokens: &[Token<'_>]) -> String {
    let lines = match label {
        LABEL_TABLE => table_rows(tokens),
        LABEL_LIST => list_items(tokens),
        _ => None,
    };
    match lines {
        Some(lines) => lines.join("\n"),
        None => plain(tokens),
    }
}

// ---------------------------------------------------------------- blocks

/// A labelled element being read: its own element sits at `depth` of the open-element stack.
struct OpenBlock<'a> {
    label: String,
    bbox: Option<[i64; 4]>,
    depth: usize,
    inner: Vec<Token<'a>>,
}

impl OpenBlock<'_> {
    fn close(self) -> OcrBlock {
        OcrBlock {
            text: block_text(&self.label, &self.inner),
            label: self.label,
            bbox: self.bbox,
        }
    }
}

/// The labelled blocks of one page, in the order chandra returned them.
pub fn blocks(html: &str) -> Vec<OcrBlock> {
    let tokens = tokens(html);
    let mut out = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let mut current: Option<OpenBlock<'_>> = None;
    let mut labelled = false;
    for t in tokens.iter() {
        match t {
            Token::Open { name, attrs } => {
                match current.as_mut() {
                    Some(block) => block.inner.push(t.clone()),
                    None => {
                        if let Some(label) = attr(attrs, "data-label") {
                            labelled = true;
                            current = Some(OpenBlock {
                                label: label.trim().to_string(),
                                bbox: attr(attrs, "data-bbox").and_then(|b| parse_bbox(&b)),
                                depth: stack.len(),
                                inner: Vec::new(),
                            });
                        }
                    }
                }
                stack.push(name.clone());
            }
            Token::Close { name } => {
                // Pop to the matching element; a stray close tag is only text of the block.
                let Some(at) = stack.iter().rposition(|n| n == name) else {
                    if let Some(block) = current.as_mut() {
                        block.inner.push(t.clone());
                    }
                    continue;
                };
                stack.truncate(at);
                match current.take() {
                    Some(block) if at <= block.depth => out.push(block.close()),
                    Some(mut block) => {
                        block.inner.push(t.clone());
                        current = Some(block);
                    }
                    None => {}
                }
            }
            Token::Text(_) => {
                if let Some(block) = current.as_mut() {
                    block.inner.push(t.clone());
                }
            }
        }
    }
    if let Some(block) = current.take() {
        out.push(block.close());
    }
    if labelled {
        out
    } else {
        unlabelled(&tokens)
    }
}

/// No labelled element: one `Text` block per line (a markdown `#` line is a header).
fn unlabelled(tokens: &[Token<'_>]) -> Vec<OcrBlock> {
    let mut raw = String::new();
    for t in tokens {
        match t {
            Token::Text(s) => raw.push_str(&decode_entities(s)),
            Token::Open { name, .. } | Token::Close { name } => {
                if BREAKING.contains(&name.as_str()) {
                    raw.push('\n');
                }
            }
        }
    }
    raw.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            let (label, text) = match trimmed.strip_prefix('#') {
                Some(h) => (LABEL_SECTION_HEADER, h.trim_start_matches('#')),
                None => (LABEL_TEXT, trimmed),
            };
            let text = collapse(text);
            (!text.is_empty()).then(|| OcrBlock {
                label: label.to_string(),
                text,
                bbox: None,
            })
        })
        .collect()
}

/// The paragraphs a page's blocks give, in order (see the module table).
pub fn paragraphs(blocks: &[OcrBlock]) -> Vec<String> {
    blocks
        .iter()
        .filter(|b| !SKIPPED_LABELS.contains(&b.label.as_str()))
        .flat_map(|b| b.text.split('\n'))
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

pub fn pictures(blocks: &[OcrBlock]) -> usize {
    blocks
        .iter()
        .filter(|b| PICTURE_LABELS.contains(&b.label.as_str()))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What chandra returned for a textbook page in the probe, rewritten with our own words.
    const PAGE: &str = r#"<div data-bbox="12 8 980 40" data-label="Page-Header"><p>Unit 3 &middot; Reading</p></div>
<div data-bbox="60 60 940 110" data-label="Section-Header"><h2>Lesson 3: The Umbrella Stand</h2></div>
<div data-bbox="60 120 940 260" data-label="Text"><p>Mrs. Lin put an umbrella stand
outside her shop.&nbsp;Nobody took one &amp; the sign stayed.</p></div>
<div data-bbox="60 270 940 380" data-label="List-Group"><ul><li>First, read the title.</li>
<li>Then <b>circle</b> the new words.</li></ul></div>
<div data-bbox="300 390 700 600" data-label="Picture"><img alt="an umbrella stand"/></div>
<div data-bbox="300 610 700 640" data-label="Caption"><p>Figure 1. The stand.</p></div>
<div data-bbox="60 650 940 820" data-label="Table"><table><thead><tr><th>Word</th><th>Meaning</th></tr></thead>
<tbody><tr><td>trust</td><td>相信</td></tr><tr><td>stand</td><td>架子</td></tr><tr><td></td><td></td></tr></tbody></table></div>
<div data-bbox="60 830 940 900" data-label="Footnote"><p>* Adapted for this class.</p></div>
<div data-bbox="450 960 550 990" data-label="Page-Footer">12</div>"#;

    #[test]
    fn every_label_maps_as_documented() {
        let b = blocks(PAGE);
        let labels: Vec<&str> = b.iter().map(|x| x.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "Page-Header",
                "Section-Header",
                "Text",
                "List-Group",
                "Picture",
                "Caption",
                "Table",
                "Footnote",
                "Page-Footer"
            ]
        );
        assert_eq!(b[1].bbox, Some([60, 60, 940, 110]));
        assert_eq!(b[0].text, "Unit 3 · Reading", "kept in the blocks");
        assert_eq!(
            paragraphs(&b),
            [
                "Lesson 3: The Umbrella Stand",
                "Mrs. Lin put an umbrella stand outside her shop. Nobody took one & the sign stayed.",
                "First, read the title.",
                "Then circle the new words.",
                "Figure 1. The stand.",
                "Word | Meaning",
                "trust | 相信",
                "stand | 架子",
                "* Adapted for this class.",
            ]
        );
        assert_eq!(pictures(&b), 1);
    }

    #[test]
    fn tables_keep_rows_and_empty_cells() {
        let html = r#"<div data-label="Table"><table><tr><td>A</td><td></td><td>C &lt;3</td></tr>
            <tr><td colspan="3">合計<br>十二</td></tr></table></div>"#;
        let b = blocks(html);
        assert_eq!(b[0].text, "A |  | C <3\n合計十二");
        assert_eq!(b[0].bbox, None);
        assert_eq!(paragraphs(&b), ["A |  | C <3", "合計十二"]);
        // A table label without rows is read as text.
        let b = blocks(r#"<div data-label="Table">| a | b |</div>"#);
        assert_eq!(paragraphs(&b), ["| a | b |"]);
    }

    #[test]
    fn lists_give_one_paragraph_per_item_in_order() {
        let html = r#"<div data-label="List-Group"><p>Before you read:</p><ol>
            <li>Look at the picture.<ul><li>Who is there?</li></ul></li>
            <li>Guess the topic.</li></ol></div>"#;
        assert_eq!(
            paragraphs(&blocks(html)),
            [
                "Before you read:",
                "Look at the picture.",
                "Who is there?",
                "Guess the topic."
            ]
        );
    }

    #[test]
    fn entities_whitespace_and_chinese_line_breaks() {
        assert_eq!(
            decode_entities("a &amp; b &lt;c&gt; &#39;d&#x27; &#x4E2D; &unknown; & e"),
            "a & b <c> 'd' 中 &unknown; & e"
        );
        let html = "<div data-label=\"Text\"><p>這是一個\n句子，\u{200B}沒有\n斷開。 H<sub>2</sub>O is water.</p></div>";
        assert_eq!(
            paragraphs(&blocks(html)),
            ["這是一個句子，沒有斷開。 H2O is water."]
        );
        assert_eq!(collapse("  one\n\ttwo  "), "one two");
        assert_eq!(collapse("English 中文"), "English 中文");
    }

    #[test]
    fn reading_order_is_chandras_not_the_bbox() {
        let html = r#"<div data-bbox="0 500 10 510" data-label="Text">second on paper, first here</div>
            <div data-bbox="0 0 10 10" data-label="Text">first on paper, second here</div>"#;
        assert_eq!(
            paragraphs(&blocks(html)),
            ["second on paper, first here", "first on paper, second here"]
        );
    }

    #[test]
    fn nested_divs_attribute_quoting_and_broken_html() {
        let html = r#"<div data-label='Text' data-bbox="1,2,3,4"><div><span title="a > b">Inner</span> text</div></div>
            <div data-label=Section-Header>Part B</div>
            <div data-label="Text"><p>Unclosed at the end"#;
        let b = blocks(html);
        assert_eq!(b.len(), 3);
        assert_eq!(b[0].bbox, Some([1, 2, 3, 4]));
        assert_eq!(b[0].text, "Inner text");
        assert_eq!(b[1].label, "Section-Header");
        assert_eq!(b[2].text, "Unclosed at the end");
        // Unknown labels are text; a stray close tag does not end the block early.
        let b = blocks(r#"<div data-label="Equation-Block">x = 1</span> + 2</div>"#);
        assert_eq!(paragraphs(&b), ["x = 1 + 2"]);
    }

    #[test]
    fn output_without_labels_is_read_line_by_line() {
        let b = blocks("# Lesson 3\n\nFirst line.\n<p>Second &amp; last.</p>");
        assert_eq!(
            b.iter()
                .map(|x| (x.label.as_str(), x.text.as_str()))
                .collect::<Vec<_>>(),
            [
                ("Section-Header", "Lesson 3"),
                ("Text", "First line."),
                ("Text", "Second & last.")
            ]
        );
        assert!(blocks("").is_empty());
        assert!(blocks("   ").is_empty());
    }

    #[test]
    fn pages_round_trip_through_json() {
        let page = OcrPage::read(2, PAGE);
        assert_eq!(page.pictures, 1);
        let json = serde_json::to_value(&page).unwrap();
        assert!(json.get("error").is_none());
        assert_eq!(json["blocks"][1]["label"], "Section-Header");
        assert_eq!(
            json["blocks"][1]["bbox"],
            serde_json::json!([60, 60, 940, 110])
        );
        let back: OcrPage = serde_json::from_value(json).unwrap();
        assert_eq!(back, page);
        let failed = OcrPage::failed(3, "辨識逾時");
        assert_eq!(serde_json::to_value(&failed).unwrap()["error"], "辨識逾時");
    }
}
