//! DOCX and PDF of one version (level) for one audience, made on the server from the same
//! projection the editor shows ([`super::assemble::project`]) and streamed back; nothing is
//! stored. Choices are explained in docs/ENGINEERING_REPLY_WORKSHEET.md:
//!
//! * DOCX is WordprocessingML written directly into a zip (`zip` crate): plain paragraphs,
//!   tables and paragraph borders, all native Word objects the teacher can edit. Fonts are named,
//!   not embedded (Times New Roman for English, 新細明體 for Chinese, as on school PCs).
//! * PDF is drawn with `krilla` (pure Rust, no system libraries), with a small line-breaking
//!   layout of our own. Libertinus Serif (OFL) is compiled into the binary and subset into every
//!   PDF; Chinese characters (teacher notes) need a CJK font file, taken from
//!   `WORKSHEET_CJK_FONT` or a few well-known system paths (the Docker image installs
//!   文泉驛微米黑). Without one, a sheet that contains Chinese is refused with a clear message
//!   instead of printing empty boxes.

use std::collections::HashMap;
use std::io::Write as _;
use std::sync::OnceLock;

mod organizer;

use super::assemble::{
    printed_answer, printed_option, Sheet, SheetBlock, SheetImage, SheetItem, SheetItemSupport,
    SheetTable, SheetWord,
};
use super::organizer::SheetOrganizer;
use super::tiered::{student_version_name, SupportKind};
use super::{Audience, BlockKind, FMT_ORGANIZER, FMT_TABLE};
use crate::objectives::printed_objective;
use crate::retrieval::is_cjk;

pub const DOCX_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
pub const PDF_TYPE: &str = "application/pdf";

/// A stored picture for an export: JPEG bytes and its size in pixels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportImage {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// The pictures a sheet prints, by `image_id` (`images::bank_for`). A picture of the sheet that
/// is not here prints as an empty box.
pub type ImageBank = HashMap<i64, ExportImage>;

/// Printed under a generated picture in the teacher version (doc §6.4). English, so a teacher
/// PDF needs no CJK font for it.
pub const AI_LABEL: &str = "AI-generated picture";

/// What a picture without one prints in its box: the purpose in the teacher version, nothing in
/// the student version.
pub fn placeholder_text(img: &SheetImage) -> String {
    match &img.purpose {
        Some(p) if !p.trim().is_empty() => format!("請放一張圖：{}", p.trim()),
        Some(_) => "請放一張圖".to_string(),
        None => String::new(),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("no CJK font")]
    NoCjkFont,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// The printed name of a block: "A. Warm-up"; the exit ticket has no letter. A title that
/// already carries the letter (「Part A · Find the Facts」) is printed as it is.
fn heading(b: &SheetBlock) -> String {
    let lettered = super::bloom::split_part_title(&b.title).is_some_and(|(l, _)| l == b.label);
    if b.label.is_empty() || lettered {
        b.title.clone()
    } else if b.title.is_empty() {
        format!("{}.", b.label)
    } else {
        format!("{}. {}", b.label, b.title)
    }
}

/// Where a question is answered: a table (表格整理), a drawn organizer (圖像組織 with one), a
/// box (圖像組織 without one) or answer lines.
enum AnswerSpace<'a> {
    Table(SheetTable),
    Organizer(&'a SheetOrganizer),
    Box,
    Lines(u8),
}

fn answer_space(item: &SheetItem) -> AnswerSpace<'_> {
    if let Some(o) = &item.organizer {
        return AnswerSpace::Organizer(o);
    }
    match (&item.table, item.answer_format.as_deref()) {
        (Some(t), _) => AnswerSpace::Table(t.clone()),
        // A sheet assembled before tables had their own size: the grid it always printed.
        (None, Some(f)) if f == FMT_TABLE => AnswerSpace::Table(SheetTable {
            columns: vec![String::new(); 2],
            rows: super::TABLE_ROWS,
            row_labels: Vec::new(),
        }),
        (None, Some(f)) if f == FMT_ORGANIZER => AnswerSpace::Box,
        _ => AnswerSpace::Lines(item.lines),
    }
}

const META_LINE: &str = "Class ________   No. ________   Name ____________________";
/// Printed above the original paragraphs of a block that also has a simplified text (T8).
const ORIGINAL_TEXT: &str = "Original text (read this first; the simplified text below is a help)";
/// Heading of a block's Glossary (詞彙級別檢查「提供字義」), printed at the end of the block.
pub const GLOSSARY: &str = "Glossary";

/// The name line, with the version's neutral code when the worksheet has several: students see
/// 「Version A」, never the level's name (T9).
fn meta_line(sheet: &Sheet) -> String {
    match &sheet.version_code {
        Some(code) => format!("{META_LINE}   {}", student_version_name(code)),
        None => META_LINE.to_string(),
    }
}

fn teacher_banner(sheet: &Sheet) -> String {
    let code = sheet
        .version_code
        .as_deref()
        .map(|c| format!(" (students' sheet: {})", student_version_name(c)))
        .unwrap_or_default();
    format!(
        "Teacher reference · {} version{code} · answers and notes are not on the student sheet",
        sheet.level.english()
    )
}

/// The PDF footer's label: the student's sheet carries the neutral code only (T9); the teacher's
/// keeps the level's name.
fn footer_label(sheet: &Sheet) -> Option<String> {
    let code = sheet.version_code.as_deref().map(student_version_name);
    match (sheet.audience, code) {
        (Audience::Student, code) => code,
        (Audience::Teacher, Some(code)) => Some(format!("{} ({code})", sheet.level.english())),
        (Audience::Teacher, None) => Some(sheet.level.english().to_string()),
    }
}

/// The lines of a support box: the first prints after its heading, each other on a line of its
/// own (a Directions box is one step per line).
fn box_lines(content: &str) -> (&str, Vec<&str>) {
    let mut lines = content.lines().map(str::trim).filter(|l| !l.is_empty());
    let first = lines.next().unwrap_or_default();
    (first, lines.collect())
}

/// What a version's support prints under a question of a 差異化學習單, as (bold label, text)
/// lines: the form's heading with its first line, the other lines, then the word bank.
fn support_lines(s: &SheetItemSupport) -> Vec<(String, String)> {
    let words = s.word_bank.join("  /  ");
    let mut out: Vec<(String, String)> = Vec::new();
    if s.kind == SupportKind::WordBank {
        out.push((format!("{}: ", s.heading), words));
        out.extend(s.lines.iter().map(|l| (String::new(), l.clone())));
    } else {
        let mut lines = s.lines.iter();
        out.push((
            format!("{}: ", s.heading),
            lines.next().cloned().unwrap_or_default(),
        ));
        out.extend(lines.map(|l| (String::new(), l.clone())));
        if !words.is_empty() {
            out.push(("Word bank: ".to_string(), words));
        }
    }
    out
}

// ================================================================ DOCX

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' | '\n' => out.push(' '),
            c if (c as u32) < 0x20 => {}
            c => out.push(c),
        }
    }
    out
}

#[derive(Clone, Copy, Default)]
struct RunStyle {
    bold: bool,
    italic: bool,
    /// hex colour without '#'
    color: Option<&'static str>,
    /// half-points
    size: Option<u32>,
}

fn run(text: &str, s: RunStyle) -> String {
    let mut props = String::new();
    if s.bold {
        props.push_str("<w:b/>");
    }
    if s.italic {
        props.push_str("<w:i/>");
    }
    if let Some(c) = s.color {
        props.push_str(&format!("<w:color w:val=\"{c}\"/>"));
    }
    if let Some(sz) = s.size {
        props.push_str(&format!("<w:sz w:val=\"{sz}\"/><w:szCs w:val=\"{sz}\"/>"));
    }
    format!(
        "<w:r><w:rPr>{props}</w:rPr><w:t xml:space=\"preserve\">{}</w:t></w:r>",
        esc(text)
    )
}

#[derive(Default)]
struct ParaStyle {
    style: Option<&'static str>,
    /// twips
    indent: u32,
    hanging: u32,
    before: u32,
    after: u32,
    keep_next: bool,
    border_bottom: bool,
    centre: bool,
}

fn para(runs: &[String], p: &ParaStyle) -> String {
    let mut props = String::new();
    if let Some(s) = p.style {
        props.push_str(&format!("<w:pStyle w:val=\"{s}\"/>"));
    }
    if p.keep_next {
        props.push_str("<w:keepNext/>");
    }
    if p.border_bottom {
        props.push_str(
            "<w:pBdr><w:bottom w:val=\"single\" w:sz=\"4\" w:space=\"1\" w:color=\"8C8C8C\"/>\
             <w:between w:val=\"single\" w:sz=\"4\" w:space=\"1\" w:color=\"8C8C8C\"/></w:pBdr>",
        );
    }
    props.push_str(&format!(
        "<w:spacing w:before=\"{}\" w:after=\"{}\"/>",
        p.before, p.after
    ));
    if p.indent > 0 || p.hanging > 0 {
        props.push_str(&format!(
            "<w:ind w:left=\"{}\" w:hanging=\"{}\"/>",
            p.indent, p.hanging
        ));
    }
    if p.centre {
        props.push_str("<w:jc w:val=\"center\"/>");
    }
    format!("<w:p><w:pPr>{props}</w:pPr>{}</w:p>", runs.join(""))
}

fn table(cols: &[u32], rows: &[Vec<String>], height: Option<u32>, header: bool) -> String {
    let grid: String = cols
        .iter()
        .map(|w| format!("<w:gridCol w:w=\"{w}\"/>"))
        .collect();
    let border = |side: &str| {
        format!("<w:{side} w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"8C8C8C\"/>")
    };
    let borders: String = ["top", "left", "bottom", "right", "insideH", "insideV"]
        .iter()
        .map(|s| border(s))
        .collect();
    let mut out = format!(
        "<w:tbl><w:tblPr><w:tblW w:w=\"{}\" w:type=\"dxa\"/><w:tblBorders>{borders}</w:tblBorders>\
         <w:tblCellMar><w:left w:w=\"100\" w:type=\"dxa\"/><w:right w:w=\"100\" w:type=\"dxa\"/></w:tblCellMar>\
         </w:tblPr><w:tblGrid>{grid}</w:tblGrid>",
        cols.iter().sum::<u32>()
    );
    for (ri, row) in rows.iter().enumerate() {
        out.push_str("<w:tr>");
        if let Some(h) = height {
            out.push_str(&format!("<w:trPr><w:trHeight w:val=\"{h}\"/></w:trPr>"));
        }
        for (ci, cell) in row.iter().enumerate() {
            let w = cols.get(ci).copied().unwrap_or(2000);
            let style = RunStyle {
                bold: header && ri == 0,
                ..RunStyle::default()
            };
            out.push_str(&format!(
                "<w:tc><w:tcPr><w:tcW w:w=\"{w}\" w:type=\"dxa\"/></w:tcPr>{}</w:tc>",
                para(&[run(cell, style)], &ParaStyle::default())
            ));
        }
        out.push_str("</w:tr>");
    }
    out.push_str("</w:tbl>");
    // Word needs a paragraph between two tables and after the last one.
    out.push_str(&para(&[], &ParaStyle::default()));
    out
}

/// Full text width of the A4 page with 2 cm margins, in twips.
const DOCX_WIDTH: u32 = 9638;
const TEACHER_INK: &str = "7A5200";
const MUTED_INK: &str = "595959";

/// The Bloom tag printed after a question in the teacher version.
fn bloom_tag(it: &SheetItem) -> Option<String> {
    it.bloom.flatten().map(|b| b.tag())
}

fn docx_item(out: &mut String, it: &SheetItem, teacher: bool) {
    let mut runs = vec![
        run(
            &format!("{}.\t", it.no),
            RunStyle {
                bold: true,
                ..RunStyle::default()
            },
        ),
        run(&it.prompt, RunStyle::default()),
    ];
    if let Some(tag) = bloom_tag(it).filter(|_| teacher) {
        runs.push(run(
            &format!(" {tag}"),
            RunStyle {
                color: Some(TEACHER_INK),
                size: Some(20),
                ..RunStyle::default()
            },
        ));
    }
    out.push_str(&para(
        &runs,
        &ParaStyle {
            indent: 400,
            hanging: 400,
            before: 120,
            after: 40,
            ..ParaStyle::default()
        },
    ));
    for (i, o) in it.options.iter().enumerate() {
        out.push_str(&para(
            &[run(&printed_option(o, i), RunStyle::default())],
            &ParaStyle {
                indent: 700,
                ..ParaStyle::default()
            },
        ));
    }
    for (label, text) in it.support.iter().flat_map(support_lines) {
        out.push_str(&para(
            &[
                run(
                    &label,
                    RunStyle {
                        bold: true,
                        ..RunStyle::default()
                    },
                ),
                run(&text, RunStyle::default()),
            ],
            &ParaStyle {
                indent: if label.is_empty() { 700 } else { 400 },
                before: 40,
                ..ParaStyle::default()
            },
        ));
    }
    if let Some(h) = &it.hint {
        out.push_str(&para(
            &[
                run(
                    "Hint: ",
                    RunStyle {
                        bold: true,
                        color: Some(MUTED_INK),
                        ..RunStyle::default()
                    },
                ),
                run(
                    h,
                    RunStyle {
                        italic: true,
                        color: Some(MUTED_INK),
                        ..RunStyle::default()
                    },
                ),
            ],
            &ParaStyle {
                indent: 400,
                before: 40,
                ..ParaStyle::default()
            },
        ));
    }
    if let Some(f) = &it.frame {
        out.push_str(&para(
            &[
                run(
                    "Sentence frame: ",
                    RunStyle {
                        bold: true,
                        ..RunStyle::default()
                    },
                ),
                run(f, RunStyle::default()),
            ],
            &ParaStyle {
                indent: 400,
                before: 40,
                ..ParaStyle::default()
            },
        ));
    }
    match answer_space(it) {
        AnswerSpace::Table(t) => {
            let n = t.columns.len().max(1);
            let width = (DOCX_WIDTH - 400) / n as u32;
            let mut rows: Vec<Vec<String>> = Vec::new();
            if t.has_headings() {
                rows.push(t.columns.clone());
            }
            rows.extend((0..usize::from(t.rows)).map(|r| {
                let mut row = vec![String::new(); n];
                // The row labels fill the first column.
                if let Some(label) = t.row_labels.get(r) {
                    row[0] = label.clone();
                }
                row
            }));
            out.push_str(&table(&vec![width; n], &rows, Some(520), t.has_headings()));
        }
        AnswerSpace::Organizer(o) => organizer::docx_organizer(out, o),
        AnswerSpace::Box => out.push_str(&table(
            &[DOCX_WIDTH - 400],
            &[vec![String::new()]],
            Some(2000),
            false,
        )),
        AnswerSpace::Lines(lines) => {
            for _ in 0..lines {
                out.push_str(&para(
                    &[run(" ", RunStyle::default())],
                    &ParaStyle {
                        indent: 400,
                        before: 160,
                        border_bottom: true,
                        ..ParaStyle::default()
                    },
                ));
            }
        }
    }
    if teacher {
        let gold = RunStyle {
            color: Some(TEACHER_INK),
            ..RunStyle::default()
        };
        let gold_bold = RunStyle { bold: true, ..gold };
        if let Some(a) = &it.answer {
            let a = printed_answer(it.answer_format.as_deref(), a, &it.options);
            out.push_str(&para(
                &[run("Answer: ", gold_bold), run(&a, gold)],
                &ParaStyle {
                    indent: 400,
                    before: 60,
                    ..ParaStyle::default()
                },
            ));
        }
        if let Some(sc) = &it.scoring {
            out.push_str(&para(
                &[run("Scoring: ", gold_bold), run(sc, gold)],
                &ParaStyle {
                    indent: 400,
                    ..ParaStyle::default()
                },
            ));
        }
        if let Some(e) = &it.evidence {
            out.push_str(&para(
                &[
                    run("Evidence: ", gold_bold),
                    run(
                        &format!("“{e}”"),
                        RunStyle {
                            italic: true,
                            ..gold
                        },
                    ),
                ],
                &ParaStyle {
                    indent: 400,
                    ..ParaStyle::default()
                },
            ));
        }
    }
}

/// The pictures one DOCX holds: `word/media/imageN.jpeg`, each with its relationship.
struct Media<'a> {
    bank: &'a ImageBank,
    /// (image id, relationship id, file name) in the order first used.
    files: Vec<(i64, String, String)>,
}

impl<'a> Media<'a> {
    fn new(bank: &'a ImageBank) -> Self {
        Self {
            bank,
            files: Vec::new(),
        }
    }

    /// The relationship id of a picture, adding it on first use.
    fn rel(&mut self, id: i64) -> String {
        if let Some((_, rel, _)) = self.files.iter().find(|(i, _, _)| *i == id) {
            return rel.clone();
        }
        let n = self.files.len() + 1;
        let rel = format!("rIdImg{n}");
        self.files.push((id, rel.clone(), format!("image{n}.jpeg")));
        rel
    }
}

/// English Metric Units per inch.
const EMU_PER_INCH: u64 = 914_400;
/// A picture is at most this wide and tall on the page.
const DOCX_IMAGE_MAX_W: u64 = 42 * EMU_PER_INCH / 10;
const DOCX_IMAGE_MAX_H: u64 = 3 * EMU_PER_INCH;

/// The printed size of a picture in EMU: as large as fits the box, aspect kept.
fn docx_extent(width: u32, height: u32) -> (u64, u64) {
    let (w, h) = (u64::from(width.max(1)), u64::from(height.max(1)));
    let by_w = (DOCX_IMAGE_MAX_W, DOCX_IMAGE_MAX_W * h / w);
    if by_w.1 <= DOCX_IMAGE_MAX_H {
        by_w
    } else {
        (DOCX_IMAGE_MAX_H * w / h, DOCX_IMAGE_MAX_H)
    }
}

/// A centred paragraph holding one inline picture.
fn docx_drawing(rel: &str, n: usize, alt: &str, cx: u64, cy: u64) -> String {
    let alt = esc(alt);
    format!(
        "<w:p><w:pPr><w:keepNext/><w:spacing w:before=\"80\" w:after=\"40\"/><w:jc w:val=\"center\"/></w:pPr>\
<w:r><w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\">\
<wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:docPr id=\"{n}\" name=\"Picture {n}\" descr=\"{alt}\"/>\
<wp:cNvGraphicFramePr><a:graphicFrameLocks xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" noChangeAspect=\"1\"/></wp:cNvGraphicFramePr>\
<a:graphic xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\">\
<a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">\
<pic:pic xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">\
<pic:nvPicPr><pic:cNvPr id=\"{n}\" name=\"image{n}.jpeg\" descr=\"{alt}\"/><pic:cNvPicPr/></pic:nvPicPr>\
<pic:blipFill><a:blip r:embed=\"{rel}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>\
<pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm>\
<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic>\
</wp:inline></w:drawing></w:r></w:p>"
    )
}

/// Width and height of the empty box of a slot without a picture (twips: about 10 × 4.6 cm).
const DOCX_BOX_W: u32 = 5800;
const DOCX_BOX_H: u32 = 2600;

/// A centred one-cell table with a dashed border and a fixed size: the empty box where a
/// picture goes, with `text` (the purpose, teacher version) inside.
fn docx_box(text: &str) -> String {
    let border = |side: &str| {
        format!("<w:{side} w:val=\"dashed\" w:sz=\"6\" w:space=\"0\" w:color=\"8C8C8C\"/>")
    };
    let borders: String = ["top", "left", "bottom", "right"]
        .iter()
        .map(|s| border(s))
        .collect();
    let inner = para(
        &[run(
            text,
            RunStyle {
                color: Some(MUTED_INK),
                size: Some(20),
                ..RunStyle::default()
            },
        )],
        &ParaStyle::default(),
    );
    format!(
        "<w:tbl><w:tblPr><w:tblW w:w=\"{DOCX_BOX_W}\" w:type=\"dxa\"/><w:jc w:val=\"center\"/>\
         <w:tblBorders>{borders}</w:tblBorders><w:tblLayout w:type=\"fixed\"/></w:tblPr>\
         <w:tblGrid><w:gridCol w:w=\"{DOCX_BOX_W}\"/></w:tblGrid>\
         <w:tr><w:trPr><w:trHeight w:val=\"{DOCX_BOX_H}\" w:hRule=\"exact\"/></w:trPr>\
         <w:tc><w:tcPr><w:tcW w:w=\"{DOCX_BOX_W}\" w:type=\"dxa\"/></w:tcPr>{inner}</w:tc></w:tr></w:tbl>{}",
        // Word needs a paragraph after a table.
        para(&[], &ParaStyle::default())
    )
}

/// A centred paragraph of one run.
fn centred(text: &str, style: RunStyle) -> String {
    format!(
        "<w:p><w:pPr><w:spacing w:before=\"0\" w:after=\"60\"/><w:jc w:val=\"center\"/></w:pPr>{}</w:p>",
        run(text, style)
    )
}

/// One picture of a block: the picture (centred) or an empty box, then its caption, and in the
/// teacher version a label under a generated picture.
fn docx_image(out: &mut String, img: &SheetImage, teacher: bool, media: &mut Media<'_>) {
    let picture = img
        .image_id
        .and_then(|id| media.bank.get(&id).map(|p| (id, p.width, p.height)));
    match picture {
        Some((id, w, h)) => {
            let rel = media.rel(id);
            let n = media.files.len();
            let (cx, cy) = docx_extent(w, h);
            out.push_str(&docx_drawing(&rel, n, &img.alt, cx, cy));
        }
        // An empty box about the size of a picture, for the teacher to fill.
        None => out.push_str(&docx_box(&placeholder_text(img))),
    }
    if !img.caption.trim().is_empty() {
        out.push_str(&centred(
            img.caption.trim(),
            RunStyle {
                italic: true,
                size: Some(20),
                ..RunStyle::default()
            },
        ));
    }
    if teacher && img.ai_generated && picture.is_some() {
        out.push_str(&centred(
            AI_LABEL,
            RunStyle {
                color: Some(TEACHER_INK),
                size: Some(16),
                ..RunStyle::default()
            },
        ));
    }
}

/// The paragraphs of one sheet: title, the teacher banner (`banner` replaces the usual one),
/// the name line, objectives and blocks.
/// The word as the table prints it: the teacher version adds the word-list level.
fn word_cell(v: &SheetWord, teacher: bool) -> String {
    match (teacher, v.level) {
        (true, Some(l)) if !v.task_word => format!("{} (Level {l})", v.word),
        _ => v.word.clone(),
    }
}

/// The two groups of Key Vocabulary: the key words of the text (Content) and the task words of
/// the instructions (Academic); a group without words is left out.
fn vocab_groups(words: &[SheetWord]) -> Vec<(&'static str, Vec<&SheetWord>)> {
    [("Content", false), ("Academic", true)]
        .into_iter()
        .map(|(title, task)| {
            (
                title,
                words
                    .iter()
                    .filter(|w| w.task_word == task)
                    .collect::<Vec<_>>(),
            )
        })
        .filter(|(_, g)| !g.is_empty())
        .collect()
}

/// One group as a `Word | POS | Meaning` table of `width` twips.
fn docx_vocab_table(group: &[&SheetWord], teacher: bool, width: u32) -> String {
    let mut rows = vec![vec![
        "Word".to_string(),
        "POS".to_string(),
        "Meaning".to_string(),
    ]];
    for v in group {
        rows.push(vec![
            word_cell(v, teacher),
            v.pos.clone(),
            v.definition.clone(),
        ]);
    }
    let (word, pos) = (width * 28 / 100, width * 15 / 100);
    table(&[word, pos, width - word - pos], &rows, None, true)
}

fn docx_group_title(title: &str) -> String {
    para(
        &[run(
            title,
            RunStyle {
                bold: true,
                ..RunStyle::default()
            },
        )],
        &ParaStyle {
            before: 40,
            after: 40,
            keep_next: true,
            ..ParaStyle::default()
        },
    )
}

/// Key Vocabulary: inside its block, or once after the last block (字詞表放在學習單最後). Content
/// and Academic sit side by side in a borderless two-cell layout table (each cell holds its own
/// `Word | POS | Meaning` table); with only one group it is one table of the full width.
fn docx_words(body: &mut String, words: &[SheetWord], teacher: bool) {
    if words.is_empty() {
        return;
    }
    body.push_str(&para(
        &[run(
            "Key Vocabulary",
            RunStyle {
                bold: true,
                ..RunStyle::default()
            },
        )],
        &ParaStyle {
            before: 120,
            keep_next: true,
            ..ParaStyle::default()
        },
    ));
    let groups = vocab_groups(words);
    if let [(title, group)] = groups.as_slice() {
        body.push_str(&docx_group_title(title));
        body.push_str(&docx_vocab_table(group, teacher, DOCX_WIDTH));
        return;
    }
    const GAP: u32 = 240;
    let cell = DOCX_WIDTH / 2;
    let mut out = format!(
        "<w:tbl><w:tblPr><w:tblW w:w=\"{DOCX_WIDTH}\" w:type=\"dxa\"/><w:tblLayout w:type=\"fixed\"/>\
         <w:tblCellMar><w:left w:w=\"0\" w:type=\"dxa\"/><w:right w:w=\"0\" w:type=\"dxa\"/></w:tblCellMar>\
         </w:tblPr><w:tblGrid><w:gridCol w:w=\"{cell}\"/><w:gridCol w:w=\"{}\"/></w:tblGrid><w:tr>",
        DOCX_WIDTH - cell
    );
    for (i, (title, group)) in groups.iter().enumerate() {
        let w = if i == 0 { cell } else { DOCX_WIDTH - cell };
        // The gap is taken from the cell's inner side, so the two tables never touch.
        let (left, right) = if i == 0 { (0, GAP / 2) } else { (GAP / 2, 0) };
        out.push_str(&format!(
            "<w:tc><w:tcPr><w:tcW w:w=\"{w}\" w:type=\"dxa\"/><w:tcMar><w:left w:w=\"{left}\" w:type=\"dxa\"/>\
             <w:right w:w=\"{right}\" w:type=\"dxa\"/></w:tcMar></w:tcPr>{}{}</w:tc>",
            docx_group_title(title),
            docx_vocab_table(group, teacher, w - GAP / 2),
        ));
    }
    out.push_str("</w:tr></w:tbl>");
    // Word needs a paragraph after a table.
    out.push_str(&para(&[], &ParaStyle::default()));
    body.push_str(&out);
}

fn sheet_body(sheet: &Sheet, banner: Option<&str>, media: &mut Media<'_>) -> String {
    let teacher = sheet.audience == Audience::Teacher;
    let mut body = String::new();
    body.push_str(&para(
        &[run(&sheet.title, RunStyle::default())],
        &ParaStyle {
            style: Some("Title"),
            after: 80,
            ..ParaStyle::default()
        },
    ));
    if teacher {
        body.push_str(&para(
            &[run(
                &banner.map_or_else(|| teacher_banner(sheet), str::to_string),
                RunStyle {
                    color: Some(TEACHER_INK),
                    size: Some(18),
                    ..RunStyle::default()
                },
            )],
            &ParaStyle {
                after: 60,
                ..ParaStyle::default()
            },
        ));
        if let Some(note) = &sheet.tiering_note {
            body.push_str(&para(
                &[run(
                    note,
                    RunStyle {
                        color: Some(TEACHER_INK),
                        size: Some(18),
                        ..RunStyle::default()
                    },
                )],
                &ParaStyle {
                    after: 60,
                    ..ParaStyle::default()
                },
            ));
        }
    }
    body.push_str(&para(
        &[run(
            &meta_line(sheet),
            RunStyle {
                color: Some(MUTED_INK),
                size: Some(20),
                ..RunStyle::default()
            },
        )],
        &ParaStyle {
            after: 200,
            border_bottom: true,
            ..ParaStyle::default()
        },
    ));
    if !sheet.objectives.is_empty() {
        body.push_str(&para(
            &[run("Today I can …", RunStyle::default())],
            &ParaStyle {
                style: Some("Heading2"),
                keep_next: true,
                ..ParaStyle::default()
            },
        ));
        for (i, o) in sheet.objectives.iter().enumerate() {
            let o = printed_objective(o);
            body.push_str(&para(
                &[run(&format!("{}.\t{o}", i + 1), RunStyle::default())],
                &ParaStyle {
                    indent: 400,
                    hanging: 400,
                    after: 40,
                    ..ParaStyle::default()
                },
            ));
        }
    }
    for b in &sheet.blocks {
        body.push_str(&para(
            &[run(&heading(b), RunStyle::default())],
            &ParaStyle {
                style: Some("Heading2"),
                keep_next: true,
                ..ParaStyle::default()
            },
        ));
        if let Some(summary) = &b.bloom_summary {
            body.push_str(&para(
                &[run(
                    summary,
                    RunStyle {
                        color: Some(TEACHER_INK),
                        size: Some(20),
                        ..RunStyle::default()
                    },
                )],
                &ParaStyle {
                    after: 60,
                    keep_next: true,
                    ..ParaStyle::default()
                },
            ));
        }
        if !b.instructions.is_empty() {
            body.push_str(&para(
                &[run(
                    &b.instructions,
                    RunStyle {
                        italic: true,
                        ..RunStyle::default()
                    },
                )],
                &ParaStyle {
                    after: 60,
                    ..ParaStyle::default()
                },
            ));
        }
        for img in &b.images {
            docx_image(&mut body, img, teacher, media);
        }
        if !b.steps_lead.is_empty() {
            body.push_str(&para(
                &[run(&b.steps_lead, RunStyle::default())],
                &ParaStyle {
                    after: 20,
                    ..ParaStyle::default()
                },
            ));
        }
        for (i, s) in b.steps.iter().enumerate() {
            let text = if s.to_lowercase().starts_with("step") {
                s.clone()
            } else {
                format!("Step {}: {s}", i + 1)
            };
            body.push_str(&para(
                &[run(
                    &text,
                    RunStyle {
                        color: Some(MUTED_INK),
                        ..RunStyle::default()
                    },
                )],
                &ParaStyle {
                    indent: 200,
                    after: 20,
                    ..ParaStyle::default()
                },
            ));
        }
        docx_words(&mut body, &b.vocabulary, teacher);
        if !b.original_text.is_empty() {
            body.push_str(&para(
                &[run(
                    ORIGINAL_TEXT,
                    RunStyle {
                        bold: true,
                        ..RunStyle::default()
                    },
                )],
                &ParaStyle {
                    before: 120,
                    keep_next: true,
                    ..ParaStyle::default()
                },
            ));
            for p in &b.original_text {
                body.push_str(&para(
                    &[run(p, RunStyle::default())],
                    &ParaStyle {
                        indent: 200,
                        after: 60,
                        ..ParaStyle::default()
                    },
                ));
            }
        }
        for s in &b.supports {
            let (first, rest) = box_lines(&s.content);
            body.push_str(&para(
                &[
                    run(
                        &format!("{}: ", s.heading),
                        RunStyle {
                            bold: true,
                            ..RunStyle::default()
                        },
                    ),
                    run(first, RunStyle::default()),
                ],
                &ParaStyle {
                    before: 80,
                    after: 40,
                    ..ParaStyle::default()
                },
            ));
            for line in rest {
                body.push_str(&para(
                    &[run(line, RunStyle::default())],
                    &ParaStyle {
                        indent: 200,
                        after: 40,
                        ..ParaStyle::default()
                    },
                ));
            }
        }
        for it in &b.items {
            docx_item(&mut body, it, teacher);
        }
        if !b.glosses.is_empty() {
            body.push_str(&para(
                &[run(
                    GLOSSARY,
                    RunStyle {
                        bold: true,
                        ..RunStyle::default()
                    },
                )],
                &ParaStyle {
                    before: 160,
                    keep_next: true,
                    ..ParaStyle::default()
                },
            ));
            let mut rows = vec![vec!["Word".to_string(), "Meaning".to_string()]];
            for g in &b.glosses {
                rows.push(vec![g.word.clone(), g.text()]);
            }
            body.push_str(&table(&[2600, DOCX_WIDTH - 2600], &rows, None, true));
        }
        if let Some(n) = &b.teacher_notes {
            body.push_str(&para(
                &[
                    run(
                        "Teacher notes: ",
                        RunStyle {
                            bold: true,
                            color: Some(TEACHER_INK),
                            ..RunStyle::default()
                        },
                    ),
                    run(
                        n,
                        RunStyle {
                            color: Some(TEACHER_INK),
                            ..RunStyle::default()
                        },
                    ),
                ],
                &ParaStyle {
                    before: 80,
                    ..ParaStyle::default()
                },
            ));
        }
    }
    docx_words(&mut body, &sheet.end_words, teacher);
    body
}

/// A whole `word/document.xml` around `body` (A4, 2 cm margins).
fn document(body: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\"><w:body>{body}\
<w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/>\
<w:pgMar w:top=\"1134\" w:right=\"1134\" w:bottom=\"1134\" w:left=\"1134\" w:header=\"567\" w:footer=\"567\" w:gutter=\"0\"/>\
</w:sectPr></w:body></w:document>"
    )
}

const STYLES_XML: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">\
<w:docDefaults><w:rPrDefault><w:rPr>\
<w:rFonts w:ascii=\"Times New Roman\" w:hAnsi=\"Times New Roman\" w:eastAsia=\"PMingLiU\" w:cs=\"Times New Roman\"/>\
<w:sz w:val=\"24\"/><w:szCs w:val=\"24\"/><w:lang w:val=\"en-US\" w:eastAsia=\"zh-TW\"/></w:rPr></w:rPrDefault>\
<w:pPrDefault><w:pPr><w:spacing w:after=\"80\" w:line=\"276\" w:lineRule=\"auto\"/></w:pPr></w:pPrDefault></w:docDefaults>\
<w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/><w:qFormat/></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Title\"><w:name w:val=\"Title\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/>\
<w:rPr><w:b/><w:sz w:val=\"40\"/><w:szCs w:val=\"40\"/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Heading1\"><w:name w:val=\"heading 1\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/>\
<w:pPr><w:keepNext/><w:spacing w:before=\"0\" w:after=\"160\"/><w:outlineLvl w:val=\"0\"/></w:pPr>\
<w:rPr><w:b/><w:sz w:val=\"32\"/><w:szCs w:val=\"32\"/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Heading2\"><w:name w:val=\"heading 2\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/>\
<w:pPr><w:keepNext/><w:spacing w:before=\"280\" w:after=\"80\"/><w:outlineLvl w:val=\"1\"/></w:pPr>\
<w:rPr><w:b/><w:sz w:val=\"26\"/><w:szCs w:val=\"26\"/></w:rPr></w:style>\
</w:styles>";

const CONTENT_TYPES: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
<Default Extension=\"xml\" ContentType=\"application/xml\"/>\
<Default Extension=\"jpeg\" ContentType=\"image/jpeg\"/>\
<Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>\
<Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/>\
<Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/>\
</Types>";

const ROOT_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/>\
<Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/>\
</Relationships>";

/// `word/_rels/document.xml.rels`: the styles and every picture.
fn document_rels(media: &Media<'_>) -> String {
    let mut rels = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/>",
    );
    for (_, rel, file) in &media.files {
        rels.push_str(&format!(
            "<Relationship Id=\"{rel}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"media/{file}\"/>"
        ));
    }
    rels.push_str("</Relationships>");
    rels
}

fn core_xml(title: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" \
xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:title>{}</dc:title><dc:creator>Manna</dc:creator></cp:coreProperties>",
        esc(title)
    )
}

/// The DOCX package of a `word/document.xml` and its pictures.
fn package(title: &str, document_xml: &str, media: &Media<'_>) -> anyhow::Result<Vec<u8>> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let parts: [(&str, String); 5] = [
        ("[Content_Types].xml", CONTENT_TYPES.to_string()),
        ("_rels/.rels", ROOT_RELS.to_string()),
        ("word/_rels/document.xml.rels", document_rels(media)),
        ("word/styles.xml", STYLES_XML.to_string()),
        ("docProps/core.xml", core_xml(title)),
    ];
    for (name, xml) in parts {
        zip.start_file(name, options)?;
        zip.write_all(xml.as_bytes())?;
    }
    zip.start_file("word/document.xml", options)?;
    zip.write_all(document_xml.as_bytes())?;
    // JPEG is compressed already.
    let stored =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (id, _, file) in &media.files {
        if let Some(p) = media.bank.get(id) {
            zip.start_file(format!("word/media/{file}"), stored)?;
            zip.write_all(&p.bytes)?;
        }
    }
    Ok(zip.finish()?.into_inner())
}

pub fn docx(sheet: &Sheet) -> anyhow::Result<Vec<u8>> {
    docx_with_images(sheet, &ImageBank::new())
}

/// The DOCX of a sheet with its pictures (a picture missing from `images` prints as a box).
pub fn docx_with_images(sheet: &Sheet, images: &ImageBank) -> anyhow::Result<Vec<u8>> {
    let mut media = Media::new(images);
    let body = sheet_body(sheet, None, &mut media);
    package(&sheet.title, &document(&body), &media)
}

// ---------------------------------------------------------------- several parts in one DOCX

/// A paragraph of a free-form part (for example a lesson plan) of a combined document.
#[derive(Debug, Clone, PartialEq)]
pub enum Line {
    /// A section heading (Heading 2).
    Heading(String),
    /// A bold lead-in followed by text: 「卡住了怎麼辦：…」.
    Labelled(String, String),
    Text(String),
    /// A numbered line with a hanging indent.
    Numbered(usize, String),
    /// Indented italic text (classroom English).
    Script(String),
    /// Small grey text.
    Muted(String),
    /// Small text in the teacher colour (Bloom levels).
    Teacher(String),
}

/// One part of a combined document; every part after the first starts on a new page under its
/// own Heading 1.
#[derive(Debug, Clone)]
pub enum Part<'a> {
    Lines {
        heading: String,
        lines: Vec<Line>,
    },
    /// A sheet as [`docx`] prints it; `banner` replaces the teacher version's usual banner.
    Sheet {
        heading: String,
        sheet: &'a Sheet,
        banner: Option<String>,
        /// Lines printed between the heading and the sheet.
        note: Vec<Line>,
    },
}

fn line_xml(l: &Line) -> String {
    let small = |text: &str, color: &'static str| {
        para(
            &[run(
                text,
                RunStyle {
                    color: Some(color),
                    size: Some(20),
                    ..RunStyle::default()
                },
            )],
            &ParaStyle {
                after: 60,
                ..ParaStyle::default()
            },
        )
    };
    match l {
        Line::Heading(t) => para(
            &[run(t, RunStyle::default())],
            &ParaStyle {
                style: Some("Heading2"),
                keep_next: true,
                ..ParaStyle::default()
            },
        ),
        Line::Labelled(label, t) => para(
            &[
                run(
                    &format!("{label}："),
                    RunStyle {
                        bold: true,
                        ..RunStyle::default()
                    },
                ),
                run(t, RunStyle::default()),
            ],
            &ParaStyle {
                before: 40,
                after: 40,
                ..ParaStyle::default()
            },
        ),
        Line::Text(t) => para(
            &[run(t, RunStyle::default())],
            &ParaStyle {
                after: 60,
                ..ParaStyle::default()
            },
        ),
        Line::Numbered(n, t) => para(
            &[run(&format!("{n}.\t{t}"), RunStyle::default())],
            &ParaStyle {
                indent: 400,
                hanging: 400,
                after: 40,
                ..ParaStyle::default()
            },
        ),
        Line::Script(t) => para(
            &[run(
                t,
                RunStyle {
                    italic: true,
                    ..RunStyle::default()
                },
            )],
            &ParaStyle {
                indent: 600,
                after: 20,
                ..ParaStyle::default()
            },
        ),
        Line::Muted(t) => small(t, MUTED_INK),
        Line::Teacher(t) => small(t, TEACHER_INK),
    }
}

const PAGE_BREAK: &str = "<w:p><w:r><w:br w:type=\"page\"/></w:r></w:p>";

/// One DOCX of several parts (a 教學套件: the lesson plan, then the student version of each
/// level, then the teacher version), each starting on its own page.
pub fn combined_docx(title: &str, parts: &[Part<'_>]) -> anyhow::Result<Vec<u8>> {
    // A 教學套件's worksheet is tiered practice, which has no picture slots.
    let bank = ImageBank::new();
    let mut media = Media::new(&bank);
    let mut body = String::new();
    body.push_str(&para(
        &[run(title, RunStyle::default())],
        &ParaStyle {
            style: Some("Title"),
            after: 200,
            ..ParaStyle::default()
        },
    ));
    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            body.push_str(PAGE_BREAK);
        }
        let heading = match part {
            Part::Lines { heading, .. } | Part::Sheet { heading, .. } => heading,
        };
        body.push_str(&para(
            &[run(heading, RunStyle::default())],
            &ParaStyle {
                style: Some("Heading1"),
                keep_next: true,
                ..ParaStyle::default()
            },
        ));
        match part {
            Part::Lines { lines, .. } => {
                for l in lines {
                    body.push_str(&line_xml(l));
                }
            }
            Part::Sheet {
                sheet,
                banner,
                note,
                ..
            } => {
                for l in note {
                    body.push_str(&line_xml(l));
                }
                body.push_str(&sheet_body(sheet, banner.as_deref(), &mut media));
            }
        }
    }
    package(title, &document(&body), &media)
}

// ================================================================ PDF

use krilla::color::rgb;
use krilla::geom::{PathBuilder, Point, Rect, Size, Transform};
use krilla::num::NormalizedF32;
use krilla::page::PageSettings;
use krilla::paint::{Fill, Stroke};
use krilla::text::{Font, TextDirection};
use krilla::Document;

static LIBERTINUS_REGULAR: &[u8] = include_bytes!("../../assets/fonts/LibertinusSerif-Regular.otf");
static LIBERTINUS_BOLD: &[u8] = include_bytes!("../../assets/fonts/LibertinusSerif-Bold.otf");
static LIBERTINUS_ITALIC: &[u8] = include_bytes!("../../assets/fonts/LibertinusSerif-Italic.otf");

/// Where a CJK font is looked for when `WORKSHEET_CJK_FONT` is not set.
const CJK_FONT_PATHS: [&str; 6] = [
    "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/opentype/noto/NotoSerifCJK-Regular.ttc",
    "/System/Library/Fonts/STHeiti Light.ttc",
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
    "/Library/Fonts/Arial Unicode.ttf",
];

/// The CJK font file, read once. `None`: there is none on this machine.
fn cjk_font_data() -> Option<&'static [u8]> {
    static DATA: OnceLock<Option<&'static [u8]>> = OnceLock::new();
    *DATA.get_or_init(|| {
        let configured = std::env::var("WORKSHEET_CJK_FONT").ok().filter(|p| !p.trim().is_empty());
        let candidates = configured.into_iter().chain(CJK_FONT_PATHS.iter().map(|p| p.to_string()));
        for path in candidates {
            if let Ok(bytes) = std::fs::read(&path) {
                if ttf_parser::Face::parse(&bytes, 0).is_ok() {
                    tracing::info!(path, "worksheet PDF export: CJK font loaded");
                    return Some(&*Box::leak(bytes.into_boxed_slice()));
                }
            }
        }
        tracing::warn!("worksheet PDF export: no CJK font found; sheets with Chinese text cannot be exported as PDF");
        None
    })
}

/// Whether PDFs with Chinese text can be made here.
pub fn pdf_supports_cjk() -> bool {
    cjk_font_data().is_some()
}

struct Face {
    parsed: ttf_parser::Face<'static>,
    font: Font,
    upem: f32,
}

impl Face {
    fn new(data: &'static [u8]) -> anyhow::Result<Self> {
        let parsed = ttf_parser::Face::parse(data, 0).map_err(|e| anyhow::anyhow!("font: {e}"))?;
        let font = Font::new(data.into(), 0).ok_or_else(|| anyhow::anyhow!("font not usable"))?;
        let upem = f32::from(parsed.units_per_em());
        Ok(Self { parsed, font, upem })
    }

    fn has(&self, c: char) -> bool {
        self.parsed.glyph_index(c).is_some()
    }

    fn advance(&self, c: char, size: f32) -> f32 {
        let gid = self
            .parsed
            .glyph_index(c)
            .or_else(|| self.parsed.glyph_index('?'));
        let units = gid
            .and_then(|g| self.parsed.glyph_hor_advance(g))
            .unwrap_or(500);
        f32::from(units) / self.upem * size
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Weight {
    Regular,
    Bold,
    Italic,
}

struct Fonts {
    regular: Face,
    bold: Face,
    italic: Face,
    cjk: Option<Face>,
}

impl Fonts {
    fn face(&self, w: Weight) -> &Face {
        match w {
            Weight::Regular => &self.regular,
            Weight::Bold => &self.bold,
            Weight::Italic => &self.italic,
        }
    }

    /// The face a character is drawn with: the Latin face when it has the glyph, otherwise the
    /// CJK face. Returns the id used in [`Op::Text`] (0–2 Latin, 3 CJK).
    fn pick(&self, w: Weight, c: char) -> (u8, &Face) {
        let latin = self.face(w);
        if latin.has(c) || c.is_whitespace() {
            return (w as u8, latin);
        }
        match &self.cjk {
            Some(cjk) if cjk.has(c) => (3, cjk),
            _ => (w as u8, latin),
        }
    }

    fn by_id(&self, id: u8) -> &Face {
        match id {
            0 => &self.regular,
            1 => &self.bold,
            2 => &self.italic,
            _ => self.cjk.as_ref().unwrap_or(&self.regular),
        }
    }

    fn width(&self, w: Weight, text: &str, size: f32) -> f32 {
        text.chars()
            .map(|c| self.pick(w, c).1.advance(c, size))
            .sum()
    }
}

type Rgb = (u8, u8, u8);
const INK: Rgb = (28, 28, 28);
const MUTED: Rgb = (89, 89, 89);
const RULE: Rgb = (140, 140, 140);
const GOLD: Rgb = (122, 82, 0);

enum Op {
    Text {
        x: f32,
        y: f32,
        face: u8,
        size: f32,
        color: Rgb,
        text: String,
    },
    Line {
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        width: f32,
        color: Rgb,
    },
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    },
    /// An ellipse outline (a circle of a Venn diagram), centred at (cx, cy).
    Ellipse {
        cx: f32,
        cy: f32,
        rx: f32,
        ry: f32,
    },
    /// A stored picture (by `image_id`), its top-left corner at (x, y).
    Image {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        id: i64,
    },
}

const PAGE_W: f32 = 595.28;
const PAGE_H: f32 = 841.89;
const MARGIN: f32 = 56.7;
const CONTENT_W: f32 = PAGE_W - 2.0 * MARGIN;

#[derive(Clone, Copy)]
struct Style {
    weight: Weight,
    size: f32,
    color: Rgb,
    indent: f32,
    before: f32,
    after: f32,
}

impl Style {
    fn body() -> Self {
        Self {
            weight: Weight::Regular,
            size: 11.5,
            color: INK,
            indent: 0.0,
            before: 0.0,
            after: 3.0,
        }
    }
}

struct Layout<'a> {
    fonts: &'a Fonts,
    pages: Vec<Vec<Op>>,
    y: f32,
    bank: &'a ImageBank,
}

/// A picture is at most this wide (share of the text width) and tall (points) on the page.
const PDF_IMAGE_SHARE: f32 = 0.62;
const PDF_IMAGE_MAX_H: f32 = 210.0;
/// The empty box of a slot without a picture.
const PDF_BOX_H: f32 = 110.0;

impl<'a> Layout<'a> {
    fn new(fonts: &'a Fonts, bank: &'a ImageBank) -> Self {
        Self {
            fonts,
            pages: vec![Vec::new()],
            y: MARGIN,
            bank,
        }
    }

    /// One picture of a block, centred: the picture or an empty box (the purpose inside in the
    /// teacher version), its caption, and in the teacher version a label under a generated one.
    fn picture(&mut self, img: &SheetImage, teacher: bool) {
        let max_w = CONTENT_W * PDF_IMAGE_SHARE;
        let picture = img
            .image_id
            .and_then(|id| self.bank.get(&id).map(|p| (id, p.width, p.height)));
        let caption_h = if img.caption.trim().is_empty() {
            0.0
        } else {
            16.0
        };
        self.y += 6.0;
        match picture {
            Some((id, pw, ph)) => {
                let (pw, ph) = (pw.max(1) as f32, ph.max(1) as f32);
                let mut w = max_w;
                let mut h = w * ph / pw;
                if h > PDF_IMAGE_MAX_H {
                    h = PDF_IMAGE_MAX_H;
                    w = h * pw / ph;
                }
                self.ensure(h + caption_h + 8.0);
                let x = MARGIN + (CONTENT_W - w) / 2.0;
                let y = self.y;
                self.ops().push(Op::Image { x, y, w, h, id });
                self.y += h + 4.0;
            }
            None => {
                self.ensure(PDF_BOX_H + caption_h + 8.0);
                let x = MARGIN + (CONTENT_W - max_w) / 2.0;
                let top = self.y;
                self.ops().push(Op::Rect {
                    x,
                    y: top,
                    w: max_w,
                    h: PDF_BOX_H,
                });
                let text = placeholder_text(img);
                if !text.is_empty() {
                    let style = Style {
                        size: 10.0,
                        color: MUTED,
                        ..Style::body()
                    };
                    self.y = top + 8.0;
                    for line in self
                        .wrap(&text, style.weight, style.size, max_w - 16.0)
                        .into_iter()
                        .take(5)
                    {
                        self.draw_line(&line, x + 8.0, style);
                        self.y += Self::line_height(style.size);
                    }
                }
                self.y = top + PDF_BOX_H + 4.0;
            }
        }
        let centred = |l: &mut Self, text: &str, style: Style| {
            for line in l.wrap(text, style.weight, style.size, CONTENT_W) {
                let w = l.fonts.width(style.weight, &line, style.size);
                l.ensure(Self::line_height(style.size));
                l.draw_line(&line, MARGIN + (CONTENT_W - w) / 2.0, style);
                l.y += Self::line_height(style.size);
            }
        };
        if !img.caption.trim().is_empty() {
            let style = Style {
                weight: Weight::Italic,
                size: 10.5,
                ..Style::body()
            };
            centred(self, img.caption.trim(), style);
        }
        if teacher && img.ai_generated && picture.is_some() {
            let style = Style {
                size: 8.5,
                color: GOLD,
                ..Style::body()
            };
            centred(self, AI_LABEL, style);
        }
        self.y += 4.0;
    }

    fn ops(&mut self) -> &mut Vec<Op> {
        self.pages.last_mut().expect("a page")
    }

    fn new_page(&mut self) {
        self.pages.push(Vec::new());
        self.y = MARGIN;
    }

    /// Starts a new page unless `h` more points fit on this one.
    fn ensure(&mut self, h: f32) {
        if self.y + h > PAGE_H - MARGIN - 14.0 && self.y > MARGIN + 1.0 {
            self.new_page();
        }
    }

    /// Breaks `text` into lines of at most `width`: at spaces, and around Chinese characters.
    fn wrap(&self, text: &str, weight: Weight, size: f32, width: f32) -> Vec<String> {
        let mut lines = Vec::new();
        for raw in text.split('\n') {
            // (token, whether a space came before it)
            let mut tokens: Vec<(String, bool)> = Vec::new();
            let mut current = String::new();
            let mut current_space = false;
            let mut space = false;
            for c in raw.chars() {
                if c.is_whitespace() {
                    if !current.is_empty() {
                        tokens.push((std::mem::take(&mut current), current_space));
                    }
                    space = true;
                } else if is_cjk(c) {
                    if !current.is_empty() {
                        tokens.push((std::mem::take(&mut current), current_space));
                    }
                    tokens.push((c.to_string(), space));
                    space = false;
                } else {
                    if current.is_empty() {
                        current_space = space;
                        space = false;
                    }
                    current.push(c);
                }
            }
            if !current.is_empty() {
                tokens.push((current, current_space));
            }
            let space_w = self.fonts.width(weight, " ", size);
            let mut line = String::new();
            let mut line_w = 0.0;
            for (token, space_before) in tokens {
                let w = self.fonts.width(weight, &token, size);
                let gap = if line.is_empty() || !space_before {
                    0.0
                } else {
                    space_w
                };
                if !line.is_empty() && line_w + gap + w > width {
                    lines.push(std::mem::take(&mut line));
                    line_w = 0.0;
                }
                if line.is_empty() && w > width {
                    // A single token longer than the line: break it by characters.
                    for c in token.chars() {
                        let cw = self.fonts.width(weight, &c.to_string(), size);
                        if !line.is_empty() && line_w + cw > width {
                            lines.push(std::mem::take(&mut line));
                            line_w = 0.0;
                        }
                        line.push(c);
                        line_w += cw;
                    }
                    continue;
                }
                if !line.is_empty() && space_before {
                    line.push(' ');
                    line_w += space_w;
                }
                line.push_str(&token);
                line_w += w;
            }
            lines.push(line);
        }
        lines
    }

    /// Draws one line at the current position, split into runs by font.
    fn draw_line(&mut self, text: &str, x: f32, style: Style) {
        let baseline = self.y + style.size * 0.95;
        let mut runs: Vec<(u8, String)> = Vec::new();
        for c in text.chars() {
            let (id, _) = self.fonts.pick(style.weight, c);
            match runs.last_mut() {
                Some((last, s)) if *last == id => s.push(c),
                _ => runs.push((id, c.to_string())),
            }
        }
        let mut cx = x;
        for (id, s) in runs {
            let w: f32 = s
                .chars()
                .map(|c| self.fonts.by_id(id).advance(c, style.size))
                .sum();
            self.ops().push(Op::Text {
                x: cx,
                y: baseline,
                face: id,
                size: style.size,
                color: style.color,
                text: s,
            });
            cx += w;
        }
    }

    fn line_height(size: f32) -> f32 {
        size * 1.38
    }

    fn paragraph(&mut self, text: &str, style: Style) {
        if text.trim().is_empty() {
            return;
        }
        self.y += style.before;
        let lines = self.wrap(text, style.weight, style.size, CONTENT_W - style.indent);
        for line in lines {
            self.ensure(Self::line_height(style.size));
            self.draw_line(&line, MARGIN + style.indent, style);
            self.y += Self::line_height(style.size);
        }
        self.y += style.after;
    }

    /// A label in another weight followed by text, wrapped together (e.g. "Answer: …").
    fn labelled(&mut self, label: &str, text: &str, style: Style, label_weight: Weight) {
        self.y += style.before;
        let label_w = self.fonts.width(label_weight, label, style.size);
        let width = CONTENT_W - style.indent;
        let mut lines = self.wrap(text, style.weight, style.size, width - label_w);
        if lines.is_empty() {
            lines.push(String::new());
        }
        let first = lines.remove(0);
        self.ensure(Self::line_height(style.size));
        self.draw_line(
            label,
            MARGIN + style.indent,
            Style {
                weight: label_weight,
                ..style
            },
        );
        self.draw_line(&first, MARGIN + style.indent + label_w, style);
        self.y += Self::line_height(style.size);
        let rest = lines.join(" ");
        if !rest.is_empty() {
            for line in self.wrap(&rest, style.weight, style.size, width) {
                self.ensure(Self::line_height(style.size));
                self.draw_line(&line, MARGIN + style.indent, style);
                self.y += Self::line_height(style.size);
            }
        }
        self.y += style.after;
    }

    fn rule(&mut self, x1: f32, x2: f32, width: f32, color: Rgb) {
        let y = self.y;
        self.ops().push(Op::Line {
            x1,
            y1: y,
            x2,
            y2: y,
            width,
            color,
        });
    }

    fn answer_lines(&mut self, n: u8, indent: f32) {
        for _ in 0..n {
            self.ensure(22.0);
            self.y += 20.0;
            self.rule(MARGIN + indent, PAGE_W - MARGIN, 0.6, RULE);
            self.y += 2.0;
        }
        if n > 0 {
            self.y += 4.0;
        }
    }

    /// A 表格整理 table: the headings in bold in the first row (when there are any), then
    /// `rows` empty rows of `row_h`.
    fn answer_table(&mut self, indent: f32, t: &SheetTable, row_h: f32) {
        let cols = t.columns.len().max(1);
        if !t.has_headings() {
            self.grid(indent, cols, t.rows.into(), row_h, &t.row_labels);
            return;
        }
        let size = 10.0;
        let pad = 3.0;
        let cell_w = (CONTENT_W - indent) / cols as f32;
        let heads: Vec<Vec<String>> = t
            .columns
            .iter()
            .map(|c| self.wrap(c, Weight::Bold, size, cell_w - 2.0 * pad))
            .collect();
        let head_lines = heads.iter().map(Vec::len).max().unwrap_or(1).max(1);
        let head_h = head_lines as f32 * Self::line_height(size) + 2.0 * pad;
        // The headings stay with their rows.
        self.ensure(head_h + row_h * f32::from(t.rows) + 8.0);
        let x = MARGIN + indent;
        let top = self.y + 4.0;
        let style = Style {
            weight: Weight::Bold,
            size,
            ..Style::body()
        };
        for (ci, lines) in heads.iter().enumerate() {
            self.y = top + pad;
            for line in lines {
                self.draw_line(line, x + cell_w * ci as f32 + pad, style);
                self.y += Self::line_height(size);
            }
        }
        self.y = top;
        let h = head_h;
        self.ops().push(Op::Rect {
            x,
            y: top,
            w: cell_w * cols as f32,
            h,
        });
        for c in 1..cols {
            let cx = x + cell_w * c as f32;
            self.ops().push(Op::Line {
                x1: cx,
                y1: top,
                x2: cx,
                y2: top + h,
                width: 0.6,
                color: RULE,
            });
        }
        // The empty rows right under the headings (the grid adds its own gap above).
        self.y = top + h - 4.0;
        self.grid(indent, cols, t.rows.into(), row_h, &t.row_labels);
    }

    /// The Key Vocabulary groups side by side (stacked widths when there is one: the full
    /// width), each a `Word | POS | Meaning` table. Rows of both columns that fit the page are
    /// drawn together; what does not fit goes on the next page under repeated headings.
    fn vocab_columns(&mut self, groups: &[(&str, Vec<&SheetWord>)], teacher: bool) {
        const GAP: f32 = 18.0;
        const SIZE: f32 = 10.0;
        const PAD: f32 = 3.0;
        const PAD_V: f32 = 2.0;
        struct Row {
            cells: [Vec<String>; 3],
            h: f32,
        }
        let n = groups.len();
        if n == 0 {
            return;
        }
        let cw = (CONTENT_W - GAP * (n - 1) as f32) / n as f32;
        let widths = [cw * 0.29, cw * 0.15, cw * 0.56];
        let lh = Self::line_height(SIZE);
        let title_h = lh + 3.0;
        let head_h = lh + 2.0 * PAD_V;
        let layouts: Vec<Vec<Row>> = groups
            .iter()
            .map(|(_, group)| {
                group
                    .iter()
                    .map(|w| {
                        let cells = [
                            self.wrap(
                                &word_cell(w, teacher),
                                Weight::Bold,
                                SIZE,
                                widths[0] - 2.0 * PAD,
                            ),
                            self.wrap(&w.pos, Weight::Regular, SIZE, widths[1] - 2.0 * PAD),
                            self.wrap(&w.definition, Weight::Regular, SIZE, widths[2] - 2.0 * PAD),
                        ];
                        let lines = cells.iter().map(Vec::len).max().unwrap_or(1).max(1);
                        Row {
                            cells,
                            h: lines as f32 * lh + 2.0 * PAD_V,
                        }
                    })
                    .collect()
            })
            .collect();
        let mut next = vec![0usize; n];
        while (0..n).any(|c| next[c] < layouts[c].len()) {
            let first_h: f32 = layouts
                .iter()
                .filter_map(|rows| rows.first().map(|r| r.h))
                .fold(0.0, f32::max);
            // The headings stay with at least one row.
            self.ensure(title_h + head_h + first_h + 8.0);
            let avail = PAGE_H - MARGIN - 14.0 - self.y - 4.0;
            let mut take = vec![0usize; n];
            for c in 0..n {
                let mut used = title_h + head_h;
                while next[c] + take[c] < layouts[c].len() {
                    let h = layouts[c][next[c] + take[c]].h;
                    if used + h > avail && take[c] > 0 {
                        break;
                    }
                    used += h;
                    take[c] += 1;
                }
            }
            let top = self.y + 4.0;
            let mut bottom = top;
            for c in 0..n {
                if take[c] == 0 {
                    continue;
                }
                let x = MARGIN + c as f32 * (cw + GAP);
                let cell_x = [x, x + widths[0], x + widths[0] + widths[1]];
                let bold = Style {
                    weight: Weight::Bold,
                    size: SIZE,
                    ..Style::body()
                };
                let regular = Style {
                    size: SIZE,
                    ..Style::body()
                };
                self.y = top;
                self.draw_line(
                    groups[c].0,
                    x,
                    Style {
                        size: SIZE + 0.5,
                        ..bold
                    },
                );
                let mut y = top + title_h;
                self.y = y;
                self.rule(x, x + cw, 0.8, INK);
                for (ci, label) in ["Word", "POS", "Meaning"].into_iter().enumerate() {
                    self.y = y + PAD_V;
                    self.draw_line(label, cell_x[ci] + PAD, bold);
                }
                y += head_h;
                self.y = y;
                self.rule(x, x + cw, 0.8, INK);
                for row in &layouts[c][next[c]..next[c] + take[c]] {
                    for (ci, lines) in row.cells.iter().enumerate() {
                        self.y = y + PAD_V;
                        for line in lines {
                            self.draw_line(
                                line,
                                cell_x[ci] + PAD,
                                if ci == 0 { bold } else { regular },
                            );
                            self.y += lh;
                        }
                    }
                    y += row.h;
                    self.y = y;
                    self.rule(x, x + cw, 0.4, RULE);
                }
                bottom = bottom.max(y);
            }
            for c in 0..n {
                next[c] += take[c];
            }
            self.y = bottom + 8.0;
            if (0..n).any(|c| next[c] < layouts[c].len()) {
                self.new_page();
            }
        }
    }

    /// An empty grid of `rows` rows; `labels` (one per row, or none) are written in its first
    /// column.
    fn grid(&mut self, indent: f32, cols: usize, rows: usize, row_h: f32, labels: &[String]) {
        let h = row_h * rows as f32;
        self.ensure(h + 8.0);
        self.y += 4.0;
        let x = MARGIN + indent;
        let w = CONTENT_W - indent;
        let top = self.y;
        self.ops().push(Op::Rect { x, y: top, w, h });
        for r in 1..rows {
            let y = top + row_h * r as f32;
            self.ops().push(Op::Line {
                x1: x,
                y1: y,
                x2: x + w,
                y2: y,
                width: 0.6,
                color: RULE,
            });
        }
        for c in 1..cols {
            let cx = x + w * c as f32 / cols as f32;
            self.ops().push(Op::Line {
                x1: cx,
                y1: top,
                x2: cx,
                y2: top + h,
                width: 0.6,
                color: RULE,
            });
        }
        let size = 10.0;
        let pad = 3.0;
        let cell_w = w / cols as f32;
        let style = Style {
            weight: Weight::Bold,
            size,
            ..Style::body()
        };
        for (r, label) in labels.iter().enumerate().take(rows) {
            let max_lines = ((row_h - pad) / Self::line_height(size)).floor().max(1.0) as usize;
            let mut y = top + row_h * r as f32 + pad;
            for l in self
                .wrap(label, Weight::Bold, size, cell_w - 2.0 * pad)
                .into_iter()
                .take(max_lines)
            {
                self.y = y;
                self.draw_line(&l, x + pad, style);
                y += Self::line_height(size);
            }
        }
        self.y = top + h + 8.0;
    }
}

fn pdf_item(l: &mut Layout<'_>, it: &SheetItem, teacher: bool) {
    let body = Style::body();
    let indent = 20.0;
    // Keep the number with the first line of the question (and an organizer's question with
    // the start of its organizer).
    l.y += 6.0;
    let room = if it.organizer.is_some() { 90.0 } else { 0.0 };
    l.ensure(Layout::line_height(body.size) * 2.0 + room);
    let no = format!("{}.", it.no);
    l.draw_line(
        &no,
        MARGIN,
        Style {
            weight: Weight::Bold,
            ..body
        },
    );
    let lines = l.wrap(&it.prompt, Weight::Regular, body.size, CONTENT_W - indent);
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            l.ensure(Layout::line_height(body.size));
        }
        l.draw_line(line, MARGIN + indent, body);
        l.y += Layout::line_height(body.size);
    }
    if let Some(tag) = bloom_tag(it).filter(|_| teacher) {
        l.paragraph(
            &tag,
            Style {
                color: GOLD,
                size: 9.5,
                indent,
                after: 1.0,
                ..body
            },
        );
    }
    for (i, o) in it.options.iter().enumerate() {
        l.paragraph(
            &printed_option(o, i),
            Style {
                indent: indent + 14.0,
                after: 1.0,
                ..body
            },
        );
    }
    for (label, text) in it.support.iter().flat_map(support_lines) {
        l.labelled(
            &label,
            &text,
            Style {
                size: 10.5,
                indent: if label.is_empty() {
                    indent + 14.0
                } else {
                    indent
                },
                before: 2.0,
                after: 1.0,
                ..body
            },
            Weight::Bold,
        );
    }
    if let Some(h) = &it.hint {
        l.labelled(
            "Hint: ",
            h,
            Style {
                weight: Weight::Italic,
                color: MUTED,
                size: 10.5,
                indent,
                before: 2.0,
                after: 1.0,
            },
            Weight::Bold,
        );
    }
    if let Some(f) = &it.frame {
        l.labelled(
            "Sentence frame: ",
            f,
            Style {
                size: 10.5,
                indent,
                before: 2.0,
                after: 1.0,
                ..body
            },
            Weight::Bold,
        );
    }
    match answer_space(it) {
        AnswerSpace::Table(t) => l.answer_table(indent, &t, 26.0),
        AnswerSpace::Organizer(o) => l.organizer(o, indent),
        AnswerSpace::Box => l.grid(indent, 1, 1, 96.0, &[]),
        AnswerSpace::Lines(n) => l.answer_lines(n, indent),
    }
    if teacher {
        let gold = Style {
            color: GOLD,
            indent,
            size: 10.5,
            before: 2.0,
            after: 1.0,
            ..body
        };
        if let Some(a) = &it.answer {
            let a = printed_answer(it.answer_format.as_deref(), a, &it.options);
            l.labelled("Answer: ", &a, gold, Weight::Bold);
        }
        if let Some(sc) = &it.scoring {
            l.labelled("Scoring: ", sc, gold, Weight::Bold);
        }
        if let Some(e) = &it.evidence {
            l.labelled(
                "Evidence: ",
                &format!("“{e}”"),
                Style {
                    weight: Weight::Italic,
                    ..gold
                },
                Weight::Bold,
            );
        }
    }
}

/// Key Vocabulary: inside its block, or once after the last block (字詞表放在學習單最後). Content and
/// Academic side by side, each a `Word | POS | Meaning` table (one group: the full width).
fn pdf_words(l: &mut Layout<'_>, words: &[SheetWord], teacher: bool, body: Style) {
    if words.is_empty() {
        return;
    }
    l.paragraph(
        "Key Vocabulary",
        Style {
            weight: Weight::Bold,
            before: 6.0,
            after: 2.0,
            ..body
        },
    );
    l.vocab_columns(&vocab_groups(words), teacher);
}

fn layout_sheet(l: &mut Layout<'_>, sheet: &Sheet) {
    let teacher = sheet.audience == Audience::Teacher;
    let body = Style::body();
    l.paragraph(
        &sheet.title,
        Style {
            weight: Weight::Bold,
            size: 19.0,
            after: 4.0,
            ..body
        },
    );
    if teacher {
        l.paragraph(
            &teacher_banner(sheet),
            Style {
                size: 9.0,
                color: GOLD,
                after: 2.0,
                ..body
            },
        );
        if let Some(note) = &sheet.tiering_note {
            l.paragraph(
                note,
                Style {
                    size: 9.0,
                    color: GOLD,
                    after: 2.0,
                    ..body
                },
            );
        }
    }
    l.paragraph(
        &meta_line(sheet),
        Style {
            size: 10.0,
            color: MUTED,
            before: 4.0,
            after: 6.0,
            ..body
        },
    );
    l.rule(MARGIN, PAGE_W - MARGIN, 1.2, INK);
    l.y += 8.0;
    let heading_style = Style {
        weight: Weight::Bold,
        size: 13.0,
        before: 12.0,
        after: 3.0,
        ..body
    };
    if !sheet.objectives.is_empty() {
        l.ensure(60.0);
        l.paragraph("Today I can …", heading_style);
        for (i, o) in sheet.objectives.iter().enumerate() {
            l.labelled(
                &format!("{}. ", i + 1),
                printed_objective(o),
                Style {
                    indent: 8.0,
                    ..body
                },
                Weight::Regular,
            );
        }
    }
    for b in &sheet.blocks {
        // Keep a heading with what follows it.
        l.ensure(80.0);
        l.paragraph(&heading(b), heading_style);
        if let Some(summary) = &b.bloom_summary {
            l.paragraph(
                summary,
                Style {
                    color: GOLD,
                    size: 9.5,
                    after: 3.0,
                    ..body
                },
            );
        }
        if !b.instructions.is_empty() {
            l.paragraph(
                &b.instructions,
                Style {
                    weight: Weight::Italic,
                    after: 3.0,
                    ..body
                },
            );
        }
        for img in &b.images {
            l.picture(img, teacher);
        }
        if !b.steps_lead.is_empty() {
            l.paragraph(&b.steps_lead, Style { after: 1.0, ..body });
        }
        for (i, s) in b.steps.iter().enumerate() {
            let text = if s.to_lowercase().starts_with("step") {
                s.clone()
            } else {
                format!("Step {}: {s}", i + 1)
            };
            l.paragraph(
                &text,
                Style {
                    color: MUTED,
                    size: 10.5,
                    indent: 8.0,
                    after: 1.0,
                    ..body
                },
            );
        }
        pdf_words(l, &b.vocabulary, teacher, body);
        if !b.original_text.is_empty() {
            l.paragraph(
                ORIGINAL_TEXT,
                Style {
                    weight: Weight::Bold,
                    before: 6.0,
                    after: 2.0,
                    ..body
                },
            );
            for p in &b.original_text {
                l.paragraph(
                    p,
                    Style {
                        indent: 8.0,
                        after: 3.0,
                        ..body
                    },
                );
            }
        }
        for s in &b.supports {
            let (first, rest) = box_lines(&s.content);
            l.labelled(
                &format!("{}: ", s.heading),
                first,
                Style {
                    before: 4.0,
                    after: 2.0,
                    ..body
                },
                Weight::Bold,
            );
            for line in rest {
                l.paragraph(
                    line,
                    Style {
                        indent: 8.0,
                        after: 2.0,
                        ..body
                    },
                );
            }
        }
        if b.kind == BlockKind::ExitTicket && b.items.is_empty() {
            l.answer_lines(2, 0.0);
        }
        for it in &b.items {
            pdf_item(l, it, teacher);
        }
        if !b.glosses.is_empty() {
            l.ensure(40.0);
            l.paragraph(
                GLOSSARY,
                Style {
                    weight: Weight::Bold,
                    before: 8.0,
                    after: 2.0,
                    ..body
                },
            );
            for g in &b.glosses {
                l.labelled(
                    &format!("{} ", g.word),
                    &g.text(),
                    Style {
                        indent: 8.0,
                        after: 1.0,
                        ..body
                    },
                    Weight::Bold,
                );
            }
        }
        if let Some(n) = &b.teacher_notes {
            l.labelled(
                "Teacher notes: ",
                n,
                Style {
                    color: GOLD,
                    size: 10.5,
                    before: 6.0,
                    ..body
                },
                Weight::Bold,
            );
        }
    }
    pdf_words(l, &sheet.end_words, teacher, body);
}

fn sheet_has_cjk(sheet: &Sheet) -> bool {
    serde_json::to_string(sheet).map(|s| s.chars().any(is_cjk)).unwrap_or(false)
        // the serialised form also holds the scaffold kinds (Chinese labels that are not printed)
        && printed_text(sheet).chars().any(is_cjk)
}

fn printed_text(sheet: &Sheet) -> String {
    let mut out = vec![sheet.title.clone(), meta_line(sheet)];
    out.extend(sheet.tiering_note.iter().cloned());
    out.extend(sheet.objectives.iter().cloned());
    out.extend(
        sheet
            .end_words
            .iter()
            .flat_map(|v| [v.word.clone(), v.definition.clone()]),
    );
    for b in &sheet.blocks {
        out.extend([
            b.title.clone(),
            b.instructions.clone(),
            b.steps_lead.clone(),
        ]);
        out.extend(b.steps.iter().cloned());
        out.extend(
            b.vocabulary
                .iter()
                .flat_map(|v| [v.word.clone(), v.definition.clone()]),
        );
        out.extend(b.original_text.iter().cloned());
        out.extend(b.supports.iter().map(|s| s.content.clone()));
        out.extend(b.bloom_summary.iter().cloned());
        for img in &b.images {
            out.push(img.caption.clone());
            out.push(placeholder_text(img));
        }
        for it in &b.items {
            out.push(it.prompt.clone());
            out.extend(bloom_tag(it));
            out.extend(it.hint.iter().cloned());
            out.extend(it.frame.iter().cloned());
            if let Some(s) = &it.support {
                out.extend(s.lines.iter().cloned());
                out.extend(s.word_bank.iter().cloned());
            }
            out.extend(it.scoring.iter().cloned());
            if let Some(o) = &it.organizer {
                out.extend(o.texts().into_iter().map(str::to_string));
            }
            out.extend(it.options.iter().cloned());
            out.extend(
                it.table
                    .iter()
                    .flat_map(|t| t.columns.iter().chain(&t.row_labels).cloned()),
            );
            out.extend(it.answer.iter().cloned());
            out.extend(it.evidence.iter().cloned());
        }
        out.extend(b.teacher_notes.iter().cloned());
        for g in &b.glosses {
            out.push(g.word.clone());
            out.push(g.text());
        }
    }
    out.join("\n")
}

pub fn pdf(sheet: &Sheet) -> Result<Vec<u8>, ExportError> {
    pdf_with_images(sheet, &ImageBank::new())
}

/// The PDF of a sheet with its pictures (a picture missing from `images`, or one that cannot be
/// read, prints as a box).
pub fn pdf_with_images(sheet: &Sheet, images: &ImageBank) -> Result<Vec<u8>, ExportError> {
    let cjk = cjk_font_data();
    if cjk.is_none() && sheet_has_cjk(sheet) {
        return Err(ExportError::NoCjkFont);
    }
    let fonts = Fonts {
        regular: Face::new(LIBERTINUS_REGULAR)?,
        bold: Face::new(LIBERTINUS_BOLD)?,
        italic: Face::new(LIBERTINUS_ITALIC)?,
        cjk: match cjk {
            Some(data) => Some(Face::new(data)?),
            None => None,
        },
    };
    let mut layout = Layout::new(&fonts, images);
    layout_sheet(&mut layout, sheet);
    let total = layout.pages.len();
    let footer = Style {
        size: 9.0,
        color: MUTED,
        ..Style::body()
    };
    for (i, page) in layout.pages.iter_mut().enumerate() {
        let text = match footer_label(sheet) {
            Some(label) => format!("{label} · {}/{}", i + 1, total),
            None => format!("{}/{}", i + 1, total),
        };
        let w = fonts.width(Weight::Regular, &text, footer.size);
        page.push(Op::Text {
            x: PAGE_W - MARGIN - w,
            y: PAGE_H - MARGIN + 18.0,
            face: Weight::Regular as u8,
            size: footer.size,
            color: footer.color,
            text,
        });
    }

    let mut document = Document::new();
    document.set_metadata(
        krilla::metadata::Metadata::new()
            .title(sheet.title.clone())
            .language("en".to_string())
            .creator("Manna".to_string()),
    );
    let fill = |c: Rgb| Fill {
        paint: rgb::Color::new(c.0, c.1, c.2).into(),
        opacity: NormalizedF32::ONE,
        rule: Default::default(),
    };
    let stroke = |c: Rgb, width: f32| Stroke {
        paint: rgb::Color::new(c.0, c.1, c.2).into(),
        width,
        ..Default::default()
    };
    // Every picture is embedded once, however often it is drawn.
    let mut embedded: HashMap<i64, Option<krilla::image::Image>> = HashMap::new();
    for ops in &layout.pages {
        let settings =
            PageSettings::from_wh(PAGE_W, PAGE_H).ok_or_else(|| anyhow::anyhow!("page size"))?;
        let mut page = document.start_page_with(settings);
        let mut surface = page.surface();
        for op in ops {
            match op {
                Op::Text {
                    x,
                    y,
                    face,
                    size,
                    color,
                    text,
                } => {
                    surface.set_stroke(None);
                    surface.set_fill(Some(fill(*color)));
                    surface.draw_text(
                        Point::from_xy(*x, *y),
                        fonts.by_id(*face).font.clone(),
                        *size,
                        text,
                        false,
                        TextDirection::Auto,
                    );
                }
                Op::Line {
                    x1,
                    y1,
                    x2,
                    y2,
                    width,
                    color,
                } => {
                    let mut pb = PathBuilder::new();
                    pb.move_to(*x1, *y1);
                    pb.line_to(*x2, *y2);
                    if let Some(path) = pb.finish() {
                        surface.set_fill(None);
                        surface.set_stroke(Some(stroke(*color, *width)));
                        surface.draw_path(&path);
                    }
                }
                Op::Rect { x, y, w, h } => {
                    let mut pb = PathBuilder::new();
                    if let Some(rect) = Rect::from_xywh(*x, *y, *w, *h) {
                        pb.push_rect(rect);
                    }
                    if let Some(path) = pb.finish() {
                        surface.set_fill(None);
                        surface.set_stroke(Some(stroke(RULE, 0.6)));
                        surface.draw_path(&path);
                    }
                }
                Op::Ellipse { cx, cy, rx, ry } => {
                    // Four cubic Béziers (κ ≈ 0.5523 of the radius to the control points).
                    let (k, cx, cy, rx, ry) = (0.552_284_8_f32, *cx, *cy, *rx, *ry);
                    let mut pb = PathBuilder::new();
                    pb.move_to(cx + rx, cy);
                    pb.cubic_to(cx + rx, cy + k * ry, cx + k * rx, cy + ry, cx, cy + ry);
                    pb.cubic_to(cx - k * rx, cy + ry, cx - rx, cy + k * ry, cx - rx, cy);
                    pb.cubic_to(cx - rx, cy - k * ry, cx - k * rx, cy - ry, cx, cy - ry);
                    pb.cubic_to(cx + k * rx, cy - ry, cx + rx, cy - k * ry, cx + rx, cy);
                    pb.close();
                    if let Some(path) = pb.finish() {
                        surface.set_fill(None);
                        surface.set_stroke(Some(stroke(INK, 0.9)));
                        surface.draw_path(&path);
                    }
                }
                Op::Image { x, y, w, h, id } => {
                    let image = embedded.entry(*id).or_insert_with(|| {
                        images.get(id).and_then(|p| {
                            krilla::image::Image::from_jpeg(p.bytes.clone().into(), true).ok()
                        })
                    });
                    if let (Some(image), Some(size)) = (image.clone(), Size::from_wh(*w, *h)) {
                        surface.push_transform(&Transform::from_translate(*x, *y));
                        surface.draw_image(image, size);
                        surface.pop();
                    }
                }
            }
        }
        surface.finish();
        page.finish();
    }
    document
        .finish()
        .map_err(|e| ExportError::Other(anyhow::anyhow!("PDF: {e:?}")))
}

/// `Content-Disposition` for a download, with the UTF-8 name (RFC 5987) and an ASCII fallback.
pub fn content_disposition(filename: &str) -> String {
    let ascii: String = filename
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let mut encoded = String::new();
    for b in filename.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(*b, b'.' | b'-' | b'_' | b'~') {
            encoded.push(*b as char);
        } else {
            encoded.push_str(&format!("%{b:02X}"));
        }
    }
    format!("attachment; filename=\"{ascii}\"; filename*=UTF-8''{encoded}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worksheet::assemble::{SheetSupport, SheetWord};
    use crate::worksheet::{Level, ScaffoldType, SupportKind};

    fn sheet(audience: Audience) -> Sheet {
        let teacher = audience == Audience::Teacher;
        Sheet {
            title: "The Umbrella Stand: Worksheet".into(),
            level: Level::Standard,
            audience,
            version_code: None,
            tiering_note: None,
            objectives: vec!["I can explain why Mrs. Lin started the stand.".into()],
            blocks: vec![SheetBlock {
                id: "b1".into(),
                kind: BlockKind::Activity,
                label: "A".into(),
                title: "Paragraph 1: Find".into(),
                instructions: "Read paragraph 1 again, then answer.".into(),
                steps: vec!["Read on your own.".into(), "Check with a partner.".into()],
                steps_lead: crate::worksheet::assemble::STEPS_LEAD.into(),
                vocabulary: vec![SheetWord { word: "stand".into(), level: Some(1), pos: "n.".into(), definition: "a small table".into(), task_word: false }],
                supports: vec![SheetSupport { words: Vec::new(), kind_type: ScaffoldType::Verbal, kind: "句型框架".into(), heading: "Sentence frames".into(), content: "She started it because ___.".into() }],
                items: (1..=30)
                    .map(|no| SheetItem {
                        id: format!("b1-i{no}"),
                        no,
                        answer_format: Some(if no % 3 == 0 { FMT_TABLE.to_string() } else { "簡答題".to_string() }),
                        prompt: format!("Question {no}: why did the neighbours laugh at the idea of a free umbrella stand outside the station on rainy mornings?"),
                        options: if no == 2 { vec!["Yes".into(), "No".into()] } else { vec![] },
                        lines: 3,
                        table: (no % 3 == 0).then(|| SheetTable {
                            columns: match no {
                                3 => vec!["Who?".into(), "What happened?".into(), "Why?".into()],
                                6 => vec![String::new(), "Path A".into(), "Path B".into()],
                                _ => vec![String::new(); 2],
                            },
                            rows: 3,
                            row_labels: if no == 6 { vec!["Claim".into(), "Supporting detail".into(), String::new()] } else { Vec::new() },
                        }),
                        answer: teacher.then(|| "Because they thought every umbrella would be gone.".to_string()),
                        evidence: teacher.then(|| "They were sure every umbrella would be gone.".to_string()),
                        bloom: None,
                        hint: None,
                        frame: None,
                        support: None,
                        scoring: None,
                        organizer: None,
                    })
                    .collect(),
                teacher_notes: teacher.then(|| "先找出關鍵句再推論。".to_string()),
                bloom_summary: None,
                images: Vec::new(),
                original_text: vec![],
                glosses: vec![],
            }],
            end_words: Vec::new(),
        }
    }

    #[test]
    fn docx_is_a_zip_with_the_document_and_no_answers_for_students() {
        let bytes = docx(&sheet(Audience::Student)).unwrap();
        assert_eq!(&bytes[..2], b"PK");
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("word/document.xml").unwrap(), &mut xml)
            .unwrap();
        assert!(xml.contains("Question 1: why"));
        assert!(!xml.contains("Answer:") && !xml.contains("every umbrella would be gone."));
        let teacher = docx(&sheet(Audience::Teacher)).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(teacher)).unwrap();
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("word/document.xml").unwrap(), &mut xml)
            .unwrap();
        assert!(xml.contains("Answer: ") && xml.contains("先找出關鍵句"));
    }

    #[test]
    fn a_table_prints_its_headings_over_empty_rows() {
        let xml = document(docx(&sheet(Audience::Student)).unwrap());
        // Question 3 names its columns: one heading row in bold, then three empty rows.
        for h in ["Who?", "What happened?", "Why?"] {
            let cell = format!("<w:rPr><w:b/></w:rPr><w:t xml:space=\"preserve\">{h}</w:t>");
            assert!(xml.contains(&cell), "{h}");
        }
        // Question 6 has row labels: a first column for them (empty heading) and two headings.
        for h in ["Path A", "Path B"] {
            let cell = format!("<w:rPr><w:b/></w:rPr><w:t xml:space=\"preserve\">{h}</w:t>");
            assert!(xml.contains(&cell), "{h}");
        }
        for label in ["Claim", "Supporting detail"] {
            assert!(xml.contains(&format!(">{label}<")), "{label}");
        }
        let three = (DOCX_WIDTH - 400) / 3;
        assert_eq!(
            xml.matches(&format!("<w:gridCol w:w=\"{three}\"/>"))
                .count(),
            6,
            "question 3 and question 6"
        );
        // A table without headings prints only its empty rows, in two columns.
        let two = (DOCX_WIDTH - 400) / 2;
        assert!(xml.matches(&format!("<w:gridCol w:w=\"{two}\"/>")).count() >= 2);
        // Choices still print for a question that has them.
        assert!(xml.contains("(A) Yes"));
        // The PDF draws it too (English only: no CJK font needed).
        let pdf = pdf(&sheet(Audience::Student)).unwrap();
        assert!(pdf.starts_with(b"%PDF-"));
    }

    #[test]
    fn a_box_prints_one_line_per_line_under_its_own_heading() {
        let mut s = sheet(Audience::Student);
        s.blocks[0].supports = vec![SheetSupport {
            words: Vec::new(),
            kind_type: ScaffoldType::Procedural,
            kind: "清楚的分步指示及完成順序".into(),
            heading: "Directions".into(),
            content: "Underline the topic sentence.\nCircle two details.".into(),
        }];
        let xml = document(docx(&s).unwrap());
        assert!(xml.contains(">Directions: <"));
        // Two paragraphs, not one line run together.
        assert!(xml.contains(">Underline the topic sentence.<"));
        assert!(xml.contains(">Circle two details.<"));
        assert!(!xml.contains("Underline the topic sentence. Circle"));
        assert!(pdf(&s).unwrap().starts_with(b"%PDF-"));
    }

    #[test]
    fn objectives_print_without_their_own_i_can() {
        let mut s = sheet(Audience::Student);
        s.objectives = vec![
            "I can identify the key elements of the story.".into(),
            "Retell the story in order.".into(),
        ];
        let xml = document(docx(&s).unwrap());
        // 「Today I can …」 then the line without its own "I can"; another line as it is.
        assert!(xml.contains("Today I can …"));
        assert!(xml.contains(">1. identify the key elements of the story.<"));
        assert!(!xml.contains("I can identify"));
        assert!(xml.contains(">2. Retell the story in order.<"));
        assert!(pdf(&s).unwrap().starts_with(b"%PDF-"));
    }

    #[test]
    fn choices_print_lettered_once_and_the_key_names_the_letter() {
        let mut s = sheet(Audience::Teacher);
        let it = &mut s.blocks[0].items[0];
        it.answer_format = Some("選擇題".into());
        it.options = vec![
            "A. Paris".into(),
            "B. London".into(),
            "Rome".into(),
            "Tokyo".into(),
        ];
        it.answer = Some("London".into());
        let xml = document(docx(&s).unwrap());
        for choice in ["(A) Paris", "(B) London", "(C) Rome", "(D) Tokyo"] {
            assert!(xml.contains(&format!(">{choice}<")), "{choice}");
        }
        assert!(!xml.contains("(A) A. Paris") && !xml.contains("(B) B. London"));
        assert!(xml.contains(">Answer: <") && xml.contains(">(B) London<"));
        // The PDF prints through the same helpers.
        assert!(pdf(&s).unwrap().starts_with(b"%PDF-"));
    }

    fn document(bytes: Vec<u8>) -> String {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("word/document.xml").unwrap(), &mut xml)
            .unwrap();
        xml
    }

    #[test]
    fn bloom_tags_print_in_the_teacher_version_and_hints_in_basic() {
        use crate::worksheet::{Block, Bloom, Content, Item};
        let content = Content {
            blocks: vec![Block {
                id: "b1".into(),
                title: "Part A · Look Closer".into(),
                bloom: Some(Bloom::Analyze),
                items: vec![Item {
                    id: "i1".into(),
                    answer_format: Some("簡答題".into()),
                    prompt: "Why does the writer compare trust to an umbrella?".into(),
                    answer: "Both protect people.".into(),
                    levels: Level::ALL.to_vec(),
                    bloom: Some(Bloom::Analyze),
                    hint: "Think about what an umbrella does.".into(),
                    ..Item::default()
                }],
                ..Block::default()
            }],
            levels: Level::ALL.to_vec(),
            ..Content::default()
        };
        let sheet =
            |level, audience| crate::worksheet::assemble::project("T", &content, level, audience);
        let teacher = document(docx(&sheet(Level::Standard, Audience::Teacher)).unwrap());
        assert!(teacher.contains("〔Bloom：分析 Analyze〕"));
        assert!(teacher.contains("Bloom：分析 1"), "block summary line");
        assert!(
            teacher.contains(">Part A · Look Closer<"),
            "no second letter"
        );
        assert!(!teacher.contains("Think about what an umbrella does."));
        for level in Level::ALL {
            let student = document(docx(&sheet(level, Audience::Student)).unwrap());
            assert!(!student.contains("Bloom"), "{level:?}");
            assert_eq!(
                student.contains("Hint: ")
                    && student.contains("Think about what an umbrella does."),
                level == Level::Basic,
                "{level:?}"
            );
        }
        let basic_teacher = document(docx(&sheet(Level::Basic, Audience::Teacher)).unwrap());
        assert!(basic_teacher.contains("Think about what an umbrella does."));
        // PDF: the student version has no Chinese, so no CJK font is needed for it.
        let pdf_bytes = pdf(&sheet(Level::Basic, Audience::Student)).unwrap();
        assert!(pdf_bytes.starts_with(b"%PDF-"));
        assert!(sheet_has_cjk(&sheet(Level::Standard, Audience::Teacher)));
        assert!(!sheet_has_cjk(&sheet(Level::Basic, Audience::Student)));
    }

    #[test]
    fn a_question_prints_its_own_sentence_frame_under_it() {
        use crate::worksheet::{Block, Content, Item};
        let frame = "Hello, everyone. My answer is ___ because ___. Thank you.";
        let content = Content {
            blocks: vec![Block {
                id: "b1".into(),
                title: "Look Closer".into(),
                items: vec![Item {
                    id: "i1".into(),
                    answer_format: Some("簡答題".into()),
                    prompt: "Why does the writer compare trust to an umbrella?".into(),
                    levels: Level::ALL.to_vec(),
                    frame: frame.into(),
                    ..Item::default()
                }],
                ..Block::default()
            }],
            levels: Level::ALL.to_vec(),
            ..Content::default()
        };
        for audience in [Audience::Student, Audience::Teacher] {
            let sheet =
                crate::worksheet::assemble::project("T", &content, Level::Standard, audience);
            let xml = document(docx(&sheet).unwrap());
            let at = xml.find("Why does the writer").unwrap();
            let label = xml.find("Sentence frame: ").unwrap();
            assert!(at < label, "the frame is under the question");
            assert!(xml.contains("My answer is ___ because ___."));
            assert!(printed_text(&sheet).contains(frame));
            assert!(pdf(&sheet).unwrap().starts_with(b"%PDF-"));
        }
    }

    #[test]
    fn student_sheets_carry_a_neutral_code_and_teacher_sheets_the_level() {
        use crate::worksheet::{Block, Bloom, Content, Item, ItemTier};
        let tier = |level, kind, support: &[&str], bank: &[&str], answer: &str| ItemTier {
            level,
            support_kind: kind,
            model_sentence: "The neighbors thought the umbrellas would be gone.".into(),
            support: support.iter().map(|s| s.to_string()).collect(),
            word_bank: bank.iter().map(|s| s.to_string()).collect(),
            answer: answer.into(),
            scoring: "片語即可".into(),
        };
        let content = Content {
            blocks: vec![Block {
                id: "b1".into(),
                title: "Understand the Text".into(),
                bloom: Some(Bloom::Understand),
                items: vec![Item {
                    id: "i1".into(),
                    answer_format: Some("簡答題".into()),
                    prompt: "What did the neighbors think?".into(),
                    levels: Level::ALL.to_vec(),
                    tiers: vec![
                        tier(
                            Level::Basic,
                            SupportKind::SentenceFrame,
                            &["The neighbors thought the ___ would be ___."],
                            &["umbrellas", "gone"],
                            "umbrellas, gone",
                        ),
                        tier(
                            Level::Standard,
                            SupportKind::WordBank,
                            &[],
                            &["think", "gone"],
                            "They thought the umbrellas would be gone.",
                        ),
                        tier(
                            Level::Advanced,
                            SupportKind::None,
                            &[],
                            &[],
                            "They were sure every umbrella would be gone.",
                        ),
                    ],
                    ..Item::default()
                }],
                ..Block::default()
            }],
            levels: Level::ALL.to_vec(),
            ..Content::default()
        };
        let sheet =
            |level, audience| crate::worksheet::assemble::project("T", &content, level, audience);
        for (level, code) in [
            (Level::Basic, "A"),
            (Level::Standard, "B"),
            (Level::Advanced, "C"),
        ] {
            let s = sheet(level, Audience::Student);
            assert_eq!(
                footer_label(&s).as_deref(),
                Some(format!("Version {code}").as_str())
            );
            let xml = document(docx(&s).unwrap());
            assert!(xml.contains(&format!("Version {code}")), "{level:?}");
            for name in [
                "Basic", "Standard", "Advanced", "基礎", "一般", "進階", "Scoring",
            ] {
                assert!(!xml.contains(name), "{name} on the {level:?} student sheet");
            }
            assert!(pdf(&s).unwrap().starts_with(b"%PDF-"));
        }
        let basic = document(docx(&sheet(Level::Basic, Audience::Student)).unwrap());
        assert!(basic.contains(">Sentence frame: <") && basic.contains("would be ___."));
        assert!(
            basic.contains(">gone  /  umbrellas<"),
            "word bank, alphabetical"
        );
        let standard = document(docx(&sheet(Level::Standard, Audience::Student)).unwrap());
        assert!(standard.contains(">Word bank: <") && standard.contains(">gone  /  think<"));
        let advanced = document(docx(&sheet(Level::Advanced, Audience::Student)).unwrap());
        assert!(!advanced.contains("Word bank") && !advanced.contains("Sentence frame"));
        // The teacher keeps the level's name, with the code its students see.
        let t = sheet(Level::Basic, Audience::Teacher);
        assert_eq!(footer_label(&t).as_deref(), Some("Basic (Version A)"));
        let xml = document(docx(&t).unwrap());
        assert!(xml.contains("Basic version (students' sheet: Version A)"));
        assert!(xml.contains(">Answer: <") && xml.contains(">umbrellas, gone<"));
        assert!(xml.contains(">Scoring: <") && xml.contains("片語即可"));
        assert!(xml.contains("差異化學習單：三個版本"), "分層說明");
        // One version: nothing to tell apart, and still no level name for students.
        let mut single = content.clone();
        single.levels = vec![Level::Standard];
        let s =
            crate::worksheet::assemble::project("T", &single, Level::Standard, Audience::Student);
        assert_eq!(footer_label(&s), None);
        assert!(!document(docx(&s).unwrap()).contains("Version"));
    }

    #[test]
    fn a_glossary_prints_at_the_end_of_its_block_in_docx_and_pdf() {
        use crate::worksheet::assemble::SheetGloss;
        let mut s = sheet(Audience::Student);
        s.blocks[0].glosses = vec![SheetGloss {
            word: "abandon".into(),
            pos: "v.".into(),
            meaning_zh: "拋棄".into(),
            definition_en: "to leave and not come back".into(),
        }];
        let xml = document(docx(&s).unwrap());
        let glossary = xml.find(GLOSSARY).expect("the Glossary heading");
        assert!(
            xml.find("Question 30").unwrap() < glossary,
            "after the questions"
        );
        assert!(xml.contains("(v.) 拋棄 — to leave and not come back"));
        // The PDF prints the same entries: they are part of what needs the CJK font.
        assert!(printed_text(&s).contains("abandon\n(v.) 拋棄 — to leave and not come back"));
        assert!(sheet_has_cjk(&s));
        if pdf_supports_cjk() {
            assert!(pdf(&s).unwrap().starts_with(b"%PDF-"));
        } else {
            assert!(matches!(pdf(&s), Err(ExportError::NoCjkFont)));
        }
        s.blocks[0].glosses.clear();
        assert!(!document(docx(&s).unwrap()).contains(GLOSSARY));
    }

    #[test]
    fn the_original_text_prints_before_a_simplified_one() {
        let mut s = sheet(Audience::Student);
        s.blocks[0].original_text = vec!["Mrs. Lin put an umbrella stand outside her shop.".into()];
        let xml = document(docx(&s).unwrap());
        let original = xml.find(ORIGINAL_TEXT).unwrap();
        assert!(
            original
                < xml
                    .find("Mrs. Lin put an umbrella stand outside her shop.")
                    .unwrap()
        );
        assert!(original < xml.find("Sentence frames").unwrap());
        assert!(pdf(&s).unwrap().starts_with(b"%PDF-"));
    }

    #[test]
    fn a_word_bank_that_starts_with_its_label_prints_the_label_once() {
        use crate::worksheet::{Block, Content, Item, Origin, Scaffold, KIND_WORD_LIST};
        let content = Content {
            blocks: vec![Block {
                id: "b1".into(),
                title: "Paragraph 1: Find".into(),
                items: vec![Item {
                    id: "i1".into(),
                    answer_format: Some("簡答題".into()),
                    prompt: "Why did she start the stand?".into(),
                    levels: Level::ALL.to_vec(),
                    ..Item::default()
                }],
                scaffolds: vec![Scaffold {
                    id: "b1-sv1".into(),
                    kind_type: ScaffoldType::Verbal,
                    kind: KIND_WORD_LIST.into(),
                    content: "Word Bank: reason, result, however".into(),
                    steps: vec![],
                    levels: Level::ALL.to_vec(),
                    auto: false,
                    origin: Origin::Ai,
                    heading: None,
                    bank: None,
                }],
                ..Block::default()
            }],
            levels: Level::ALL.to_vec(),
            ..Content::default()
        };
        let sheet =
            crate::worksheet::assemble::project("T", &content, Level::Standard, Audience::Student);
        let xml = document(docx(&sheet).unwrap()).to_lowercase();
        assert_eq!(xml.matches("word bank").count(), 1, "{xml}");
        assert!(xml.contains(">reason, result, however<"));
        assert!(pdf(&sheet).unwrap().starts_with(b"%PDF-"));
    }

    #[test]
    fn pdf_of_an_english_sheet_needs_no_cjk_font_and_breaks_pages() {
        let bytes = pdf(&sheet(Audience::Student)).unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.matches("/Type /Page").count() >= 3 || text.contains("/Count"));
        match pdf(&sheet(Audience::Teacher)) {
            Ok(b) => assert!(b.starts_with(b"%PDF-")),
            Err(ExportError::NoCjkFont) => assert!(!pdf_supports_cjk()),
            Err(e) => panic!("{e}"),
        }
    }

    #[test]
    fn wrapping_keeps_every_word_and_breaks_chinese() {
        let fonts = Fonts {
            regular: Face::new(LIBERTINUS_REGULAR).unwrap(),
            bold: Face::new(LIBERTINUS_BOLD).unwrap(),
            italic: Face::new(LIBERTINUS_ITALIC).unwrap(),
            cjk: None,
        };
        let bank = ImageBank::new();
        let l = Layout::new(&fonts, &bank);
        let text = "one two three four five six seven eight nine ten eleven twelve";
        let lines = l.wrap(text, Weight::Regular, 12.0, 120.0);
        assert!(lines.len() > 1);
        assert_eq!(lines.join(" "), text);
        let zh = l.wrap(
            "先找出關鍵句再推論先找出關鍵句再推論先找出關鍵句再推論",
            Weight::Regular,
            12.0,
            60.0,
        );
        assert!(zh.len() > 1);
        assert_eq!(
            zh.concat(),
            "先找出關鍵句再推論先找出關鍵句再推論先找出關鍵句再推論"
        );
    }

    /// A JPEG of `w`×`h`, as the server stores pictures.
    fn jpeg(w: u32, h: u32) -> Vec<u8> {
        let png = {
            let image = image::RgbImage::from_pixel(w, h, image::Rgb([200, 120, 40]));
            let mut out = std::io::Cursor::new(Vec::new());
            image::DynamicImage::ImageRgb8(image)
                .write_to(&mut out, image::ImageFormat::Png)
                .unwrap();
            out.into_inner()
        };
        crate::worksheet::images::normalize(&png).unwrap().bytes
    }

    fn picture(image_id: Option<i64>, teacher: bool) -> SheetImage {
        SheetImage {
            slot_id: format!("b1-p{}", image_id.unwrap_or(9)),
            status: if image_id.is_some() { "done" } else { "failed" }.into(),
            image_id,
            width: image_id.map(|_| 300),
            height: image_id.map(|_| 200),
            alt: "A girl & an umbrella".into(),
            caption: "Look at the umbrella.".into(),
            ai_generated: image_id.is_some(),
            purpose: teacher.then(|| "幫助學生想像情境".to_string()),
            message: None,
        }
    }

    fn with_pictures(audience: Audience) -> Sheet {
        let teacher = audience == Audience::Teacher;
        let mut s = sheet(audience);
        s.blocks[0].images = vec![picture(Some(7), teacher), picture(None, teacher)];
        s
    }

    #[test]
    fn docx_embeds_pictures_and_prints_boxes_for_the_missing_ones() {
        let bank = ImageBank::from([(
            7,
            ExportImage {
                bytes: jpeg(300, 200),
                width: 300,
                height: 200,
            },
        )]);
        let bytes = docx_with_images(&with_pictures(Audience::Teacher), &bank).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes.clone())).unwrap();
        let mut media = Vec::new();
        std::io::Read::read_to_end(
            &mut zip.by_name("word/media/image1.jpeg").unwrap(),
            &mut media,
        )
        .unwrap();
        assert_eq!(media, bank[&7].bytes);
        let mut rels = String::new();
        std::io::Read::read_to_string(
            &mut zip.by_name("word/_rels/document.xml.rels").unwrap(),
            &mut rels,
        )
        .unwrap();
        assert!(rels.contains("Id=\"rIdImg1\"") && rels.contains("Target=\"media/image1.jpeg\""));
        let mut types = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("[Content_Types].xml").unwrap(), &mut types)
            .unwrap();
        assert!(types.contains("Extension=\"jpeg\""));
        let xml = document(bytes);
        assert!(xml.contains("xmlns:wp="));
        assert!(xml.contains("<a:blip r:embed=\"rIdImg1\"/>"));
        // 4.2 in wide, aspect kept: 3:2.
        assert!(
            xml.contains("<wp:extent cx=\"3840480\" cy=\"2560320\"/>"),
            "{xml}"
        );
        assert!(xml.contains("descr=\"A girl &amp; an umbrella\""));
        assert!(xml.contains("Look at the umbrella."));
        assert!(xml.contains(AI_LABEL));
        // The picture that is not there: an empty box with the purpose (teacher version).
        assert!(xml.contains("請放一張圖：幫助學生想像情境"));
        assert!(xml.contains("<w:trHeight w:val=\"2600\" w:hRule=\"exact\"/>"));
        assert!(xml.contains("w:val=\"dashed\""));

        // Student version: the picture and its caption, an empty box, no purpose, no label.
        let student = document(docx_with_images(&with_pictures(Audience::Student), &bank).unwrap());
        assert!(student.contains("rIdImg1") && student.contains("Look at the umbrella."));
        assert!(!student.contains("請放一張圖") && !student.contains(AI_LABEL));
        // Without the bytes, both print as boxes and nothing is embedded.
        let none = docx_with_images(&with_pictures(Audience::Student), &ImageBank::new()).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(none)).unwrap();
        assert!(zip.by_name("word/media/image1.jpeg").is_err());
        // A sheet without pictures is what it was.
        assert!(!document(docx(&sheet(Audience::Student)).unwrap()).contains("w:drawing"));
    }

    #[test]
    fn pdf_embeds_pictures() {
        let bank = ImageBank::from([(
            7,
            ExportImage {
                bytes: jpeg(300, 200),
                width: 300,
                height: 200,
            },
        )]);
        let student = with_pictures(Audience::Student);
        let with = pdf_with_images(&student, &bank).unwrap();
        let without = pdf_with_images(&student, &ImageBank::new()).unwrap();
        assert!(with.starts_with(b"%PDF-"));
        let has_image = |pdf: &[u8]| {
            pdf.windows(13)
                .any(|w| w == b"/Subtype /Ima" || w == b"/Subtype/Imag")
        };
        assert!(has_image(&with), "an image XObject");
        assert!(!has_image(&without));
        assert!(with.len() > without.len());
        // The teacher version prints the purpose (Chinese): it needs the CJK font.
        match pdf_with_images(&with_pictures(Audience::Teacher), &bank) {
            Ok(b) => assert!(b.starts_with(b"%PDF-")),
            Err(ExportError::NoCjkFont) => assert!(!pdf_supports_cjk()),
            Err(e) => panic!("{e}"),
        }
    }

    #[test]
    fn disposition_has_an_ascii_and_a_utf8_name() {
        let d = content_disposition("雨傘 學習單_一般版.pdf");
        assert!(d.starts_with("attachment; filename=\""));
        assert!(d.contains("filename*=UTF-8''%E9%9B%A8"));
    }

    // ------------------------------------------------------------ Key Vocabulary

    fn vocab_content(
        lang: crate::worksheet::VocabLang,
        with_academic: bool,
        with_content: bool,
    ) -> crate::worksheet::Content {
        use crate::worksheet::{Block, BlockKind, Content, Vocab};
        let mut vocabulary = Vec::new();
        if with_content {
            vocabulary.push(Vocab {
                word: "umbrella".into(),
                level: Some(2),
                pos: "n.".into(),
                definition: "a thing that keeps rain off you".into(),
                meaning_zh: "雨傘".into(),
                ..Vocab::default()
            });
            vocabulary.push(Vocab {
                word: "trust".into(),
                level: Some(3),
                pos: "v.".into(),
                definition: "to believe someone is honest".into(),
                // no Chinese yet: the English one is printed in its place
                ..Vocab::default()
            });
        }
        if with_academic {
            let t = crate::worksheet::task_words::lookup("compare").unwrap();
            vocabulary.push(Vocab {
                word: t.word.into(),
                pos: t.pos.into(),
                definition: t.definition_en.into(),
                meaning_zh: t.meaning_zh.into(),
                task_word: true,
                ..Vocab::default()
            });
        }
        Content {
            blocks: vec![Block {
                id: "warmup".into(),
                kind: BlockKind::Warmup,
                title: "Warm-up".into(),
                vocabulary,
                ..Block::default()
            }],
            levels: Level::ALL.to_vec(),
            vocabulary_language: lang,
            ..Content::default()
        }
    }

    fn vocab_sheet(content: &crate::worksheet::Content, audience: Audience) -> Sheet {
        crate::worksheet::assemble::project("T", content, Level::Standard, audience)
    }

    /// Every drawn piece of text of the PDF layout: (page, x, y, text).
    fn pdf_texts(sheet: &Sheet) -> Vec<(usize, f32, f32, String)> {
        let fonts = Fonts {
            regular: Face::new(LIBERTINUS_REGULAR).unwrap(),
            bold: Face::new(LIBERTINUS_BOLD).unwrap(),
            italic: Face::new(LIBERTINUS_ITALIC).unwrap(),
            cjk: cjk_font_data().map(|d| Face::new(d).unwrap()),
        };
        let bank = ImageBank::new();
        let mut layout = Layout::new(&fonts, &bank);
        layout_sheet(&mut layout, sheet);
        let mut out = Vec::new();
        for (page, ops) in layout.pages.iter().enumerate() {
            for op in ops {
                if let Op::Text { x, y, text, .. } = op {
                    out.push((page, *x, *y, text.clone()));
                }
            }
        }
        out
    }

    #[test]
    fn key_vocabulary_prints_two_columns_with_pos_in_english_by_default() {
        use crate::worksheet::VocabLang;
        for audience in [Audience::Student, Audience::Teacher] {
            let s = vocab_sheet(&vocab_content(VocabLang::En, true, true), audience);
            let xml = document(docx(&s).unwrap());
            assert!(xml.contains(">Key Vocabulary<"));
            assert!(!xml.contains("Key words"));
            for heading in ["Content", "Academic", "Word", "POS", "Meaning"] {
                assert!(xml.contains(&format!(">{heading}<")), "{heading}");
            }
            // side by side: one borderless layout table of two cells holding the two tables
            let layout = xml
                .find("<w:tblLayout w:type=\"fixed\"/>")
                .expect("layout table");
            let content = xml.find(">Content<").unwrap();
            let academic = xml.find(">Academic<").unwrap();
            assert!(layout < content && content < academic);
            assert_eq!(
                xml.matches("<w:tbl>").count(),
                3,
                "the layout table and the two tables in it"
            );
            // the part of speech and the English meaning of both groups
            assert!(xml.contains(">n.<") && xml.contains(">v.<"));
            assert!(xml.contains("a thing that keeps rain off you"));
            assert!(xml.contains("to look at how things are the same and different"));
            // no Chinese in the English default
            assert!(!xml.contains("雨傘") && !xml.contains("比較"));
            let teacher = audience == Audience::Teacher;
            assert_eq!(xml.contains("umbrella (Level 2)"), teacher);
            assert!(pdf(&s).unwrap().starts_with(b"%PDF-"));
        }
    }

    #[test]
    fn key_vocabulary_prints_the_chosen_language_and_keeps_english_for_a_word_without_chinese() {
        use crate::worksheet::VocabLang;
        let content = vocab_content(VocabLang::Zh, true, true);
        let s = vocab_sheet(&content, Audience::Student);
        let xml = document(docx(&s).unwrap());
        assert!(xml.contains("雨傘") && xml.contains("比較（找出相同與不同）"));
        assert!(!xml.contains("a thing that keeps rain off you"));
        assert!(!xml.contains("to look at how things are the same"));
        // "trust" has no Chinese meaning yet: its English one stays
        assert!(xml.contains("to believe someone is honest"));
        assert!(printed_text(&s).contains("雨傘"));
        if pdf_supports_cjk() {
            assert!(pdf(&s).unwrap().starts_with(b"%PDF-"));
        } else {
            assert!(matches!(pdf(&s), Err(ExportError::NoCjkFont)));
        }
        // switching back loses nothing: the same content, the other switch
        let en = vocab_sheet(
            &crate::worksheet::Content {
                vocabulary_language: VocabLang::En,
                ..content
            },
            Audience::Student,
        );
        assert!(document(docx(&en).unwrap()).contains("a thing that keeps rain off you"));
    }

    #[test]
    fn key_vocabulary_is_one_column_when_one_group_is_empty() {
        use crate::worksheet::VocabLang;
        for (academic, content, shown, hidden) in [
            (true, false, "Academic", "Content"),
            (false, true, "Content", "Academic"),
        ] {
            let s = vocab_sheet(
                &vocab_content(VocabLang::En, academic, content),
                Audience::Student,
            );
            let xml = document(docx(&s).unwrap());
            assert!(xml.contains(&format!(">{shown}<")));
            assert!(!xml.contains(&format!(">{hidden}<")));
            assert!(
                !xml.contains("<w:tblLayout"),
                "no layout table for one column"
            );
            let texts = pdf_texts(&s);
            assert!(texts.iter().any(|(_, _, _, t)| t == shown));
            assert!(!texts.iter().any(|(_, _, _, t)| t == hidden));
            // the one column starts at the left margin
            assert!(texts
                .iter()
                .any(|(_, x, _, t)| t == shown && (*x - MARGIN).abs() < 0.01));
        }
    }

    #[test]
    fn key_vocabulary_columns_are_side_by_side_in_the_pdf() {
        use crate::worksheet::VocabLang;
        let s = vocab_sheet(&vocab_content(VocabLang::En, true, true), Audience::Student);
        let texts = pdf_texts(&s);
        let find = |what: &str| {
            texts
                .iter()
                .find(|(_, _, _, t)| t == what)
                .unwrap_or_else(|| panic!("{what} not drawn"))
        };
        assert!(texts.iter().any(|(_, _, _, t)| t == "Key Vocabulary"));
        let (_, content_x, content_y, _) = find("Content");
        let (_, academic_x, academic_y, _) = find("Academic");
        assert_eq!(content_y, academic_y, "the headings are on one line");
        assert!(
            academic_x > &(content_x + CONTENT_W / 2.0 - 1.0),
            "Academic is the right column"
        );
        // every column has its own Word | POS | Meaning headings
        assert_eq!(texts.iter().filter(|(_, _, _, t)| t == "POS").count(), 2);
        assert_eq!(
            texts.iter().filter(|(_, _, _, t)| t == "Meaning").count(),
            2
        );
        // the rows: the word, its part of speech and its meaning on one line
        let (_, wx, wy, _) = find("umbrella");
        let (_, px, py, _) = find("n.");
        assert_eq!(wy, py);
        assert!(px > wx);
        assert!(texts
            .iter()
            .any(|(_, x, y, t)| t == "a thing that keeps rain off you"
                || (t.starts_with("a thing") && *y == *wy && *x > *px)));
    }

    #[test]
    fn a_long_key_vocabulary_table_continues_on_the_next_page_with_its_headings() {
        use crate::worksheet::{Block, BlockKind, Content, Vocab, VocabLang};
        let vocabulary = (1..=80)
            .map(|n| Vocab {
                word: format!("word{n}"),
                pos: "n.".into(),
                definition: "a longer meaning that needs more than one line of the column to fit"
                    .into(),
                ..Vocab::default()
            })
            .collect();
        let content = Content {
            blocks: vec![Block {
                id: "warmup".into(),
                kind: BlockKind::Warmup,
                vocabulary,
                ..Block::default()
            }],
            levels: Level::ALL.to_vec(),
            vocabulary_language: VocabLang::En,
            ..Content::default()
        };
        let s = vocab_sheet(&content, Audience::Student);
        let texts = pdf_texts(&s);
        let pages: std::collections::BTreeSet<usize> = texts
            .iter()
            .filter(|(_, _, _, t)| t == "POS")
            .map(|(p, ..)| *p)
            .collect();
        assert!(pages.len() >= 2, "headings repeat on each page: {pages:?}");
        for n in 1..=80 {
            assert!(
                texts.iter().any(|(_, _, _, t)| *t == format!("word{n}")),
                "word{n}"
            );
        }
    }

    #[test]
    fn a_word_list_that_repeats_the_key_vocabulary_is_not_printed() {
        use crate::worksheet::{Scaffold, ScaffoldType, VocabLang, KIND_WORD_LIST};
        let word_list = |content: &str| Scaffold {
            id: "warmup-sv1".into(),
            kind_type: ScaffoldType::Verbal,
            kind: KIND_WORD_LIST.into(),
            content: content.into(),
            levels: Level::ALL.to_vec(),
            ..Scaffold::default()
        };
        let mut c = vocab_content(VocabLang::En, true, true);
        c.blocks[0].scaffolds = vec![word_list("Umbrella, trust, compare")];
        for audience in [Audience::Student, Audience::Teacher] {
            let s = vocab_sheet(&c, audience);
            assert!(
                s.blocks[0].supports.is_empty(),
                "the repeated list is left out"
            );
            assert!(!document(docx(&s).unwrap()).contains("Word bank"));
            assert!(!pdf_texts(&s)
                .iter()
                .any(|(_, _, _, t)| t.contains("Word bank")));
        }
        // a list with a word the table does not have still prints
        c.blocks[0].scaffolds = vec![word_list("umbrella, trust, stand")];
        let s = vocab_sheet(&c, Audience::Student);
        assert_eq!(s.blocks[0].supports.len(), 1);
        assert!(document(docx(&s).unwrap()).contains("Word bank"));
        // other scaffolds stay
        let mut frames = word_list("I think ___ because ___.");
        frames.kind = "句型框架".into();
        c.blocks[0].scaffolds = vec![word_list("umbrella"), frames];
        let s = vocab_sheet(&c, Audience::Student);
        assert_eq!(s.blocks[0].supports.len(), 1);
        assert_eq!(s.blocks[0].supports[0].kind, "句型框架");
        // at the end of the sheet the words of every block count
        c.vocabulary_at_end = true;
        c.blocks[0].scaffolds = vec![word_list("umbrella, compare")];
        let s = vocab_sheet(&c, Audience::Student);
        assert!(s.blocks[0].supports.is_empty());
        assert_eq!(s.end_words.len(), 3);
        let xml = document(docx(&s).unwrap());
        assert!(xml.contains(">Key Vocabulary<") && !xml.contains("Word bank"));
    }
}
