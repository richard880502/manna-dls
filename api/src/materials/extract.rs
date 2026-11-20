//! Text extraction for uploaded documents: TXT, DOCX and text-based PDF, all in pure Rust.
//! A scanned PDF has no text layer and ends as `no_text`; it, a PDF whose text layer looks
//! broken (`poor_text`) and every image are then read by OCR in the background
//! (`super::ocr`). Audio and video are stored and previewed, never analysed.
//!
//! Positions: every block knows the page it starts on (PDF) and its order; paragraphs are then
//! numbered by `sop::assign_paragraph_ids`, so a position is 「第 n 段（第 p 頁）」.

use std::io::Read;

use quick_xml::events::Event;
use quick_xml::Reader;

use super::{FileType, MAX_EXTRACTED_CHARS};
use crate::sop::split_paragraphs;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextStatus {
    /// There is text to read and annotate.
    Ready,
    /// A document without a text layer (typically a scanned PDF).
    NoText,
    /// The document could not be read (damaged, encrypted, unsupported).
    Failed,
    /// Images, audio, video and links: nothing to extract.
    NotApplicable,
}

impl TextStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::NoText => "no_text",
            Self::Failed => "failed",
            Self::NotApplicable => "not_applicable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub text: String,
    /// 1-based page (PDF only).
    pub page: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extracted {
    pub status: TextStatus,
    pub blocks: Vec<Block>,
    pub page_count: Option<i32>,
    /// The document had more text than [`MAX_EXTRACTED_CHARS`]; the rest was left out.
    pub truncated: bool,
    /// A PDF whose text layer looks broken ([`super::ocr::poor_text`]): read again by OCR.
    pub poor_text: bool,
}

impl Extracted {
    fn empty(status: TextStatus) -> Self {
        Self {
            status,
            blocks: Vec::new(),
            page_count: None,
            truncated: false,
            poor_text: false,
        }
    }

    pub fn from_blocks(blocks: Vec<Block>, page_count: Option<i32>) -> Self {
        let mut kept = Vec::new();
        let mut chars = 0usize;
        let mut truncated = false;
        for block in blocks {
            let len = block.text.chars().count();
            if chars + len > MAX_EXTRACTED_CHARS {
                truncated = true;
                break;
            }
            chars += len;
            kept.push(block);
        }
        Self {
            status: if kept.is_empty() {
                TextStatus::NoText
            } else {
                TextStatus::Ready
            },
            blocks: kept,
            page_count,
            truncated,
            poor_text: false,
        }
    }

    /// The paragraphs joined the way a pasted text would look.
    pub fn text(&self) -> String {
        self.blocks
            .iter()
            .map(|b| b.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

/// Extracts the text of an uploaded file. CPU-bound: call it from `spawn_blocking`. Never
/// panics outward: a parser panic on a malformed file becomes `Failed`.
pub fn extract(file_type: FileType, data: &[u8]) -> Extracted {
    let run = || match file_type.ext {
        "txt" => extract_txt(data),
        "docx" => extract_docx(data),
        "pdf" => extract_pdf(data),
        _ => Extracted::empty(TextStatus::NotApplicable),
    };
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)) {
        Ok(extracted) => extracted,
        Err(_) => {
            tracing::warn!(ext = file_type.ext, "text extraction panicked");
            Extracted::empty(TextStatus::Failed)
        }
    }
}

// ---------------------------------------------------------------- TXT

/// Decodes a text file: UTF-8 (with or without BOM), UTF-16 with BOM, otherwise Big5 (what
/// older Windows editors in Taiwan save). `None` when it is not text.
pub fn decode_text(data: &[u8]) -> Option<String> {
    let looks_like_text = |s: &str| {
        !s.chars().any(|c| {
            c == '\u{FFFD}' || (c.is_control() && !matches!(c, '\n' | '\r' | '\t' | '\u{c}'))
        })
    };
    if let Some(rest) = data.strip_prefix(b"\xEF\xBB\xBF") {
        return std::str::from_utf8(rest)
            .ok()
            .filter(|s| looks_like_text(s))
            .map(str::to_string);
    }
    if data.starts_with(b"\xFF\xFE") || data.starts_with(b"\xFE\xFF") {
        let (text, _, had_errors) = encoding_rs::UTF_16LE.decode(data);
        return (!had_errors && looks_like_text(&text)).then(|| text.into_owned());
    }
    if let Ok(s) = std::str::from_utf8(data) {
        return looks_like_text(s).then(|| s.to_string());
    }
    let (text, _, had_errors) = encoding_rs::BIG5.decode(data);
    (!had_errors && looks_like_text(&text)).then(|| text.into_owned())
}

fn extract_txt(data: &[u8]) -> Extracted {
    let Some(text) = decode_text(data) else {
        return Extracted::empty(TextStatus::Failed);
    };
    let blocks = split_paragraphs(&text)
        .into_iter()
        .map(|text| Block { text, page: None })
        .collect();
    Extracted::from_blocks(blocks, None)
}

// ---------------------------------------------------------------- DOCX

/// `word/document.xml` is read up to this many bytes (a zip can inflate without bound).
const MAX_DOCX_XML_BYTES: u64 = 64 * 1024 * 1024;

fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn extract_docx(data: &[u8]) -> Extracted {
    let Ok(mut archive) = zip::ZipArchive::new(std::io::Cursor::new(data)) else {
        return Extracted::empty(TextStatus::Failed);
    };
    let mut xml = Vec::new();
    {
        let Ok(file) = archive.by_name("word/document.xml") else {
            return Extracted::empty(TextStatus::Failed);
        };
        if file
            .take(MAX_DOCX_XML_BYTES + 1)
            .read_to_end(&mut xml)
            .is_err()
            || xml.len() as u64 > MAX_DOCX_XML_BYTES
        {
            return Extracted::empty(TextStatus::Failed);
        }
    }
    match docx_paragraphs(&xml) {
        Some(paragraphs) => Extracted::from_blocks(
            paragraphs
                .into_iter()
                .map(|text| Block { text, page: None })
                .collect(),
            None,
        ),
        None => Extracted::empty(TextStatus::Failed),
    }
}

/// The text of every `w:p` (body paragraphs, table cells, text boxes), in document order.
/// Tabs and line breaks inside a paragraph become spaces; deleted (tracked) text is skipped.
fn docx_paragraphs(xml: &[u8]) -> Option<Vec<String>> {
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut out = Vec::new();
    // Paragraphs can nest (a text box inside a paragraph): keep a stack.
    let mut stack: Vec<String> = Vec::new();
    let mut in_text = false;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => match e.name().as_ref() {
                "w:p" => stack.push(String::new()),
                "w:t" => in_text = true,
                _ => {}
            },
            Ok(Event::Empty(e)) => {
                if matches!(e.name().as_ref(), "w:tab" | "w:br" | "w:cr") {
                    if let Some(current) = stack.last_mut() {
                        current.push(' ');
                    }
                }
            }
            Ok(Event::End(e)) => match e.name().as_ref() {
                "w:p" => {
                    if let Some(text) = stack.pop() {
                        let text = collapse_ws(&text);
                        if !text.is_empty() {
                            out.push(text);
                        }
                    }
                }
                "w:t" => in_text = false,
                _ => {}
            },
            Ok(Event::Text(t)) if in_text => {
                if let Some(current) = stack.last_mut() {
                    current.push_str(&t);
                }
            }
            Ok(Event::GeneralRef(r)) if in_text => {
                let name: String = r.to_string();
                let resolved = match r.resolve_char_ref() {
                    Ok(Some(c)) => Some(c.to_string()),
                    _ => quick_xml::escape::resolve_xml_entity(&name).map(str::to_string),
                };
                if let (Some(current), Some(text)) = (stack.last_mut(), resolved) {
                    current.push_str(&text);
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => return None,
        }
        buf.clear();
    }
    Some(out)
}

// ---------------------------------------------------------------- PDF

fn extract_pdf(data: &[u8]) -> Extracted {
    let pages = match pdf_extract::extract_text_from_mem_by_pages(data) {
        Ok(pages) => pages,
        Err(e) => {
            tracing::info!(error = %e, "PDF text extraction failed");
            return Extracted::empty(TextStatus::Failed);
        }
    };
    let page_count = i32::try_from(pages.len()).ok();
    let mut blocks = Vec::new();
    for (i, page) in pages.iter().enumerate() {
        let page_no = i as i32 + 1;
        blocks.extend(page_blocks(page).into_iter().map(|text| Block {
            text,
            page: Some(page_no),
        }));
    }
    let mut extracted = Extracted::from_blocks(blocks, page_count);
    extracted.poor_text = extracted.status == TextStatus::Ready
        && super::ocr::poor_text(&pages.join("\n"), pages.len());
    extracted
}

/// Paragraphs of one PDF page. A PDF only has lines: blank lines part paragraphs when the
/// extractor produced any; otherwise a line that ends a sentence and stops well short of the
/// right margin closes a paragraph. Words hyphenated across lines are joined. (The rule is
/// shared with pasted text that looks hard-wrapped: `sop::wrap`.)
pub fn page_blocks(page: &str) -> Vec<String> {
    let text = page.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = text.split('\n').map(str::trim).collect();
    let has_blank = {
        let first = lines.iter().position(|l| !l.is_empty());
        let last = lines.iter().rposition(|l| !l.is_empty());
        match (first, last) {
            (Some(a), Some(b)) => lines[a..=b].iter().any(|l| l.is_empty()),
            _ => false,
        }
    };
    crate::sop::wrap::join_wrapped(&lines, !has_blank)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::materials::file_type_for_name;

    #[test]
    fn text_files_decode_utf8_utf16_and_big5() {
        assert_eq!(decode_text("Hello 你好".as_bytes()).unwrap(), "Hello 你好");
        assert_eq!(decode_text(b"\xEF\xBB\xBFHi").unwrap(), "Hi");
        let utf16: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain("Hi 你".encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        assert_eq!(decode_text(&utf16).unwrap(), "Hi 你");
        // 「你好」 in Big5.
        assert_eq!(decode_text(b"\xA7\x41\xA6\x6E").unwrap(), "你好");
        assert!(decode_text(b"\x00\x01\x02binary").is_none());
    }

    #[test]
    fn txt_becomes_paragraphs() {
        let e = extract(
            file_type_for_name("a.txt").unwrap(),
            b"First paragraph\nwrapped.\n\nSecond one.\n",
        );
        assert_eq!(e.status, TextStatus::Ready);
        assert_eq!(
            e.blocks.iter().map(|b| b.text.as_str()).collect::<Vec<_>>(),
            ["First paragraph wrapped.", "Second one."]
        );
        assert_eq!(e.text(), "First paragraph wrapped.\n\nSecond one.");
        let empty = extract(file_type_for_name("a.txt").unwrap(), b"  \n ");
        assert_eq!(empty.status, TextStatus::NoText);
    }

    #[test]
    fn a_hard_wrapped_txt_is_joined_like_a_pasted_text() {
        let txt = "Mrs. Lin put an umbrella stand outside her shop on a rainy\n\
            Monday morning. Nobody took one, and the stand stayed full\n\
            until the evening, when it stopped.\n\
            In the first month, twelve of the thirty umbrellas did go\n\
            missing. However, something unexpected followed, because\n\
            people brought their own umbrellas back.\n\
            By spring the stand was always full again, and Mrs. Lin\n\
            never bought a single new umbrella for it.\n";
        let e = extract(file_type_for_name("a.txt").unwrap(), txt.as_bytes());
        assert_eq!(e.blocks.len(), 3, "{:?}", e.blocks);
        // A list stays a list.
        let list = extract(
            file_type_for_name("a.txt").unwrap(),
            b"Apples\nPears\nPlums\nCherries\nGrapes\nMelons\n",
        );
        assert_eq!(list.blocks.len(), 6);
    }

    #[test]
    fn docx_paragraphs_keep_order_entities_and_skip_deleted_text() {
        let xml = r#"<?xml version="1.0"?><w:document xmlns:w="x"><w:body>
            <w:p><w:r><w:t>Tom &amp; Jerry</w:t></w:r><w:r><w:tab/><w:t xml:space="preserve"> ran&#x21;</w:t></w:r></w:p>
            <w:p></w:p>
            <w:tbl><w:tr><w:tc><w:p><w:r><w:t>Cell one</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
            <w:p><w:r><w:t>Line</w:t><w:br/><w:t>two</w:t></w:r><w:del><w:r><w:delText>gone</w:delText></w:r></w:del></w:p>
        </w:body></w:document>"#;
        assert_eq!(
            docx_paragraphs(xml.as_bytes()).unwrap(),
            ["Tom & Jerry ran!", "Cell one", "Line two"]
        );
        assert_eq!(
            extract(
                file_type_for_name("a.docx").unwrap(),
                b"PK\x03\x04 not a zip"
            )
            .status,
            TextStatus::Failed
        );
    }

    #[test]
    fn pdf_lines_are_joined_into_paragraphs() {
        // With blank lines: they part the paragraphs; hyphenated words are joined.
        let page = "The umbrella stand was an experi-\nment in trust.\n\nIt worked.\n";
        assert_eq!(
            page_blocks(page),
            [
                "The umbrella stand was an experiment in trust.",
                "It worked."
            ]
        );
        // Without: a short line that ends a sentence closes the paragraph.
        let page = "Mrs. Lin put an umbrella stand outside her shop on a rainy\n\
                    Monday morning. Nobody took one.\n\
                    In the first month, twelve of the thirty umbrellas did go\n\
                    missing. However, something unexpected followed.\n";
        assert_eq!(page_blocks(page).len(), 2);
        assert!(page_blocks("  \n\n ").is_empty());
    }

    #[test]
    fn long_documents_are_cut_and_marked() {
        let blocks = (0..40)
            .map(|_| Block {
                text: "x".repeat(10_000),
                page: None,
            })
            .collect();
        let e = Extracted::from_blocks(blocks, None);
        assert!(e.truncated);
        assert_eq!(e.blocks.len(), MAX_EXTRACTED_CHARS / 10_000);
    }

    #[test]
    fn a_broken_pdf_fails_quietly() {
        let e = extract(file_type_for_name("a.pdf").unwrap(), b"%PDF-1.4 garbage");
        assert_eq!(e.status, TextStatus::Failed);
    }
}
