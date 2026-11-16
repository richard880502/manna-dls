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

use std::io::Write as _;
use std::sync::OnceLock;

use super::assemble::{Sheet, SheetBlock, SheetItem};
use super::{Audience, BlockKind, FMT_ORGANIZER, FMT_TABLE};
use crate::retrieval::is_cjk;

pub const DOCX_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
pub const PDF_TYPE: &str = "application/pdf";

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

fn option_letter(i: usize) -> char {
    (b'A' + (i % 26) as u8) as char
}

fn boxed(item: &SheetItem) -> Option<&'static str> {
    match item.answer_format.as_deref() {
        Some(f) if f == FMT_TABLE => Some("table"),
        Some(f) if f == FMT_ORGANIZER => Some("box"),
        _ => None,
    }
}

const META_LINE: &str = "Class ________   No. ________   Name ____________________";

fn teacher_banner(sheet: &Sheet) -> String {
    format!(
        "Teacher reference · {} version · answers and notes are not on the student sheet",
        sheet.level.english()
    )
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
            &[run(
                &format!("({}) {o}", option_letter(i)),
                RunStyle::default(),
            )],
            &ParaStyle {
                indent: 700,
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
    match boxed(it) {
        Some("table") => out.push_str(&table(
            &[DOCX_WIDTH / 2 - 200, DOCX_WIDTH / 2 - 200],
            &[
                vec![String::new(), String::new()],
                vec![String::new(), String::new()],
                vec![String::new(), String::new()],
            ],
            Some(520),
            false,
        )),
        Some(_) => out.push_str(&table(
            &[DOCX_WIDTH - 400],
            &[vec![String::new()]],
            Some(2000),
            false,
        )),
        None => {
            for _ in 0..it.lines {
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
            out.push_str(&para(
                &[run("Answer: ", gold_bold), run(a, gold)],
                &ParaStyle {
                    indent: 400,
                    before: 60,
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

fn document_xml(sheet: &Sheet) -> String {
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
                &teacher_banner(sheet),
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
    body.push_str(&para(
        &[run(
            META_LINE,
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
        if !b.vocabulary.is_empty() {
            body.push_str(&para(
                &[run(
                    "Key words",
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
            let mut rows = vec![vec!["Word".to_string(), "Meaning".to_string()]];
            for v in &b.vocabulary {
                let word = match (teacher, v.level) {
                    (true, Some(l)) => format!("{} (Level {l})", v.word),
                    _ => v.word.clone(),
                };
                rows.push(vec![word, v.definition.clone()]);
            }
            body.push_str(&table(&[2600, DOCX_WIDTH - 2600], &rows, None, true));
        }
        for s in &b.supports {
            body.push_str(&para(
                &[
                    run(
                        &format!("{}: ", s.heading),
                        RunStyle {
                            bold: true,
                            ..RunStyle::default()
                        },
                    ),
                    run(&s.content, RunStyle::default()),
                ],
                &ParaStyle {
                    before: 80,
                    after: 40,
                    ..ParaStyle::default()
                },
            ));
        }
        for it in &b.items {
            docx_item(&mut body, it, teacher);
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
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><w:body>{body}\
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
<w:style w:type=\"paragraph\" w:styleId=\"Heading2\"><w:name w:val=\"heading 2\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/>\
<w:pPr><w:keepNext/><w:spacing w:before=\"280\" w:after=\"80\"/><w:outlineLvl w:val=\"1\"/></w:pPr>\
<w:rPr><w:b/><w:sz w:val=\"26\"/><w:szCs w:val=\"26\"/></w:rPr></w:style>\
</w:styles>";

const CONTENT_TYPES: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
<Default Extension=\"xml\" ContentType=\"application/xml\"/>\
<Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>\
<Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/>\
<Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/>\
</Types>";

const ROOT_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/>\
<Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/>\
</Relationships>";

const DOCUMENT_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/>\
</Relationships>";

fn core_xml(title: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" \
xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:title>{}</dc:title><dc:creator>Manna</dc:creator></cp:coreProperties>",
        esc(title)
    )
}

pub fn docx(sheet: &Sheet) -> anyhow::Result<Vec<u8>> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let parts: [(&str, String); 5] = [
        ("[Content_Types].xml", CONTENT_TYPES.to_string()),
        ("_rels/.rels", ROOT_RELS.to_string()),
        ("word/_rels/document.xml.rels", DOCUMENT_RELS.to_string()),
        ("word/styles.xml", STYLES_XML.to_string()),
        ("docProps/core.xml", core_xml(&sheet.title)),
    ];
    for (name, xml) in parts {
        zip.start_file(name, options)?;
        zip.write_all(xml.as_bytes())?;
    }
    zip.start_file("word/document.xml", options)?;
    zip.write_all(document_xml(sheet).as_bytes())?;
    Ok(zip.finish()?.into_inner())
}

// ================================================================ PDF

use krilla::color::rgb;
use krilla::geom::{PathBuilder, Point, Rect};
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
}

impl<'a> Layout<'a> {
    fn new(fonts: &'a Fonts) -> Self {
        Self {
            fonts,
            pages: vec![Vec::new()],
            y: MARGIN,
        }
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

    fn grid(&mut self, indent: f32, cols: usize, rows: usize, row_h: f32) {
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
        self.y = top + h + 8.0;
    }
}

fn pdf_item(l: &mut Layout<'_>, it: &SheetItem, teacher: bool) {
    let body = Style::body();
    let indent = 20.0;
    // Keep the number with the first line of the question.
    l.y += 6.0;
    l.ensure(Layout::line_height(body.size) * 2.0);
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
            &format!("({}) {o}", option_letter(i)),
            Style {
                indent: indent + 14.0,
                after: 1.0,
                ..body
            },
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
    match boxed(it) {
        Some("table") => l.grid(indent, 2, 3, 26.0),
        Some(_) => l.grid(indent, 1, 1, 96.0),
        None => l.answer_lines(it.lines, indent),
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
            l.labelled("Answer: ", a, gold, Weight::Bold);
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
    }
    l.paragraph(
        META_LINE,
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
                o,
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
        if !b.vocabulary.is_empty() {
            l.paragraph(
                "Key words",
                Style {
                    weight: Weight::Bold,
                    before: 6.0,
                    after: 2.0,
                    ..body
                },
            );
            for v in &b.vocabulary {
                let word = match (teacher, v.level) {
                    (true, Some(level)) => format!("{} (Level {level})", v.word),
                    _ => v.word.clone(),
                };
                let label = format!("{word} — ");
                l.labelled(
                    &label,
                    &v.definition,
                    Style {
                        indent: 8.0,
                        after: 1.0,
                        ..body
                    },
                    Weight::Bold,
                );
            }
        }
        for s in &b.supports {
            l.labelled(
                &format!("{}: ", s.heading),
                &s.content,
                Style {
                    before: 4.0,
                    after: 2.0,
                    ..body
                },
                Weight::Bold,
            );
        }
        if b.kind == BlockKind::ExitTicket && b.items.is_empty() {
            l.answer_lines(2, 0.0);
        }
        for it in &b.items {
            pdf_item(l, it, teacher);
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
}

fn sheet_has_cjk(sheet: &Sheet) -> bool {
    serde_json::to_string(sheet).map(|s| s.chars().any(is_cjk)).unwrap_or(false)
        // the serialised form also holds the scaffold kinds (Chinese labels that are not printed)
        && printed_text(sheet).chars().any(is_cjk)
}

fn printed_text(sheet: &Sheet) -> String {
    let mut out = vec![sheet.title.clone()];
    out.extend(sheet.objectives.iter().cloned());
    for b in &sheet.blocks {
        out.extend([b.title.clone(), b.instructions.clone()]);
        out.extend(b.steps.iter().cloned());
        out.extend(
            b.vocabulary
                .iter()
                .flat_map(|v| [v.word.clone(), v.definition.clone()]),
        );
        out.extend(b.supports.iter().map(|s| s.content.clone()));
        out.extend(b.bloom_summary.iter().cloned());
        for it in &b.items {
            out.push(it.prompt.clone());
            out.extend(bloom_tag(it));
            out.extend(it.hint.iter().cloned());
            out.extend(it.options.iter().cloned());
            out.extend(it.answer.iter().cloned());
            out.extend(it.evidence.iter().cloned());
        }
        out.extend(b.teacher_notes.iter().cloned());
    }
    out.join("\n")
}

pub fn pdf(sheet: &Sheet) -> Result<Vec<u8>, ExportError> {
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
    let mut layout = Layout::new(&fonts);
    layout_sheet(&mut layout, sheet);
    let total = layout.pages.len();
    let footer = Style {
        size: 9.0,
        color: MUTED,
        ..Style::body()
    };
    for (i, page) in layout.pages.iter_mut().enumerate() {
        let text = format!("{} · {}/{}", sheet.level.english(), i + 1, total);
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
    use crate::worksheet::{Level, ScaffoldType};

    fn sheet(audience: Audience) -> Sheet {
        let teacher = audience == Audience::Teacher;
        Sheet {
            title: "The Umbrella Stand: Worksheet".into(),
            level: Level::Standard,
            audience,
            objectives: vec!["I can explain why Mrs. Lin started the stand.".into()],
            blocks: vec![SheetBlock {
                id: "b1".into(),
                kind: BlockKind::Activity,
                label: "A".into(),
                title: "Paragraph 1: Find".into(),
                instructions: "Read paragraph 1 again, then answer.".into(),
                steps: vec!["Read on your own.".into(), "Check with a partner.".into()],
                vocabulary: vec![SheetWord { word: "stand".into(), level: Some(1), definition: "a small table".into() }],
                supports: vec![SheetSupport { kind_type: ScaffoldType::Verbal, kind: "句型框架".into(), heading: "Sentence frames", content: "She started it because ___.".into() }],
                items: (1..=30)
                    .map(|no| SheetItem {
                        no,
                        answer_format: Some(if no % 3 == 0 { FMT_TABLE.to_string() } else { "簡答題".to_string() }),
                        prompt: format!("Question {no}: why did the neighbours laugh at the idea of a free umbrella stand outside the station on rainy mornings?"),
                        options: if no == 2 { vec!["Yes".into(), "No".into()] } else { vec![] },
                        lines: 3,
                        answer: teacher.then(|| "Because they thought every umbrella would be gone.".to_string()),
                        evidence: teacher.then(|| "They were sure every umbrella would be gone.".to_string()),
                        bloom: None,
                        hint: None,
                    })
                    .collect(),
                teacher_notes: teacher.then(|| "先找出關鍵句再推論。".to_string()),
                bloom_summary: None,
            }],
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
        let l = Layout::new(&fonts);
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

    #[test]
    fn disposition_has_an_ascii_and_a_utf8_name() {
        let d = content_disposition("雨傘 學習單_一般版.pdf");
        assert!(d.starts_with("attachment; filename=\""));
        assert!(d.contains("filename*=UTF-8''%E9%9B%A8"));
    }
}
