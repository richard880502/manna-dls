//! 圖像組織 on paper: the DOCX and PDF drawing of an item's [`SheetOrganizer`], one layout per
//! [`Family`], the same arrangement as the editor's preview (web `OrganizerView`):
//!
//! * Columns (T-chart, KWL): headed columns, one row per cell.
//! * Rows (story map): a label column and a column to write in.
//! * Chain (sequence, cause → effect, problem → solution): numbered boxes, three to a row, with
//!   arrows between them.
//! * Venn: two overlapping circles in the PDF; a three-column table (only A | both | only B) in
//!   the DOCX, where Word shapes with text in them would not stay editable or in place.
//! * Web (mind map): the centre in the middle, the branches above and below it. The PDF joins
//!   them with lines; the DOCX is a 3 × 5 table whose connector rows hold arrows (↖ ↑ ↗ / ↙ ↓ ↘).
//!
//! A blank cell is room to write (a writing line, or the empty table cell); a pre-filled one
//! prints its text; in the teacher version a blank cell prints its answer in the teacher's ink.

use super::{
    para, run, Layout, Op, ParaStyle, RunStyle, Style, Weight, CONTENT_W, DOCX_WIDTH, GOLD, INK,
    MARGIN, RULE, TEACHER_INK,
};
use crate::worksheet::organizer::{
    web_rows, web_slots, Family, SheetOrgCell, SheetOrganizer, CHAIN_PER_ROW,
};

// ================================================================ DOCX

const BORDER_INK: &str = "595959";
const HEAD_SHADE: &str = "EDEDED";
const CENTRE_SHADE: &str = "F5EBD6";
/// Room to write in an empty cell of a columns or rows table (twips).
const DOCX_ROW_H: u32 = 620;
/// The DOCX organizer is indented like the item's text.
const DOCX_INDENT: u32 = 400;
const DOCX_ARROW_W: u32 = 520;

/// One cell of an organizer table.
struct Td {
    width: u32,
    body: String,
    borders: bool,
    shade: Option<&'static str>,
    centre: bool,
}

impl Td {
    fn boxed(width: u32, body: String) -> Self {
        Self {
            width,
            body,
            borders: true,
            shade: None,
            centre: false,
        }
    }

    fn open(width: u32, body: String) -> Self {
        Self {
            width,
            body,
            borders: false,
            shade: None,
            centre: true,
        }
    }
}

/// A fixed-layout table with per-cell borders; `rows` are (minimum height, cells).
fn org_table(rows: &[(Option<u32>, Vec<Td>)]) -> String {
    let cols: Vec<u32> = rows
        .iter()
        .max_by_key(|(_, cells)| cells.len())
        .map(|(_, cells)| cells.iter().map(|c| c.width).collect())
        .unwrap_or_default();
    let grid: String = cols
        .iter()
        .map(|w| format!("<w:gridCol w:w=\"{w}\"/>"))
        .collect();
    let mut out = format!(
        "<w:tbl><w:tblPr><w:tblW w:w=\"{}\" w:type=\"dxa\"/><w:tblInd w:w=\"{DOCX_INDENT}\" w:type=\"dxa\"/>\
         <w:tblLayout w:type=\"fixed\"/>\
         <w:tblCellMar><w:top w:w=\"60\" w:type=\"dxa\"/><w:left w:w=\"100\" w:type=\"dxa\"/>\
         <w:bottom w:w=\"60\" w:type=\"dxa\"/><w:right w:w=\"100\" w:type=\"dxa\"/></w:tblCellMar>\
         </w:tblPr><w:tblGrid>{grid}</w:tblGrid>",
        cols.iter().sum::<u32>()
    );
    for (height, cells) in rows {
        out.push_str("<w:tr><w:trPr><w:cantSplit/>");
        if let Some(h) = height {
            out.push_str(&format!("<w:trHeight w:val=\"{h}\"/>"));
        }
        out.push_str("</w:trPr>");
        for c in cells {
            let side = |s: &str| {
                if c.borders {
                    format!("<w:{s} w:val=\"single\" w:sz=\"8\" w:space=\"0\" w:color=\"{BORDER_INK}\"/>")
                } else {
                    format!("<w:{s} w:val=\"nil\"/>")
                }
            };
            let borders: String = ["top", "left", "bottom", "right"]
                .iter()
                .map(|s| side(s))
                .collect();
            let shade = c
                .shade
                .map(|f| format!("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{f}\"/>"))
                .unwrap_or_default();
            let valign = if c.centre {
                "<w:vAlign w:val=\"center\"/>"
            } else {
                ""
            };
            let body = if c.body.is_empty() {
                para(&[], &ParaStyle::default())
            } else {
                c.body.clone()
            };
            out.push_str(&format!(
                "<w:tc><w:tcPr><w:tcW w:w=\"{}\" w:type=\"dxa\"/><w:tcBorders>{borders}</w:tcBorders>{shade}{valign}</w:tcPr>{body}</w:tc>",
                c.width
            ));
        }
        out.push_str("</w:tr>");
    }
    out.push_str("</w:tbl>");
    // Word needs a paragraph after a table.
    out.push_str(&para(&[], &ParaStyle::default()));
    out
}

fn small(bold: bool) -> RunStyle {
    RunStyle {
        bold,
        size: Some(20),
        ..RunStyle::default()
    }
}

fn label_para(text: &str, centre: bool) -> String {
    para(
        &[run(text, small(true))],
        &ParaStyle {
            centre,
            after: 40,
            ..ParaStyle::default()
        },
    )
}

/// The cells of a box: pre-filled text, the teacher's answer, or a line to write on.
fn cells_xml(cells: &[SheetOrgCell], lines: bool) -> String {
    let mut out = String::new();
    for c in cells {
        if !c.given.is_empty() {
            out.push_str(&para(&[run(&c.given, small(false))], &ParaStyle::default()));
        } else if let Some(a) = &c.answer {
            out.push_str(&para(
                &[run(
                    a,
                    RunStyle {
                        italic: true,
                        color: Some(TEACHER_INK),
                        size: Some(20),
                        ..RunStyle::default()
                    },
                )],
                &ParaStyle::default(),
            ));
        } else if lines {
            out.push_str(&para(
                &[run(" ", small(false))],
                &ParaStyle {
                    before: 200,
                    border_bottom: true,
                    ..ParaStyle::default()
                },
            ));
        } else {
            out.push_str(&para(&[], &ParaStyle::default()));
        }
    }
    out
}

/// One numbered box of a chain: 「1. First」 and its cells.
fn chain_box(no: usize, label: &str, cells: &[SheetOrgCell]) -> String {
    let head = if label.is_empty() {
        format!("{no}.")
    } else {
        format!("{no}. {label}")
    };
    format!("{}{}", label_para(&head, false), cells_xml(cells, true))
}

fn arrow_para(arrow: &str) -> String {
    para(
        &[run(
            arrow,
            RunStyle {
                bold: true,
                color: Some(MUTED_HEX),
                size: Some(28),
                ..RunStyle::default()
            },
        )],
        &ParaStyle {
            centre: true,
            ..ParaStyle::default()
        },
    )
}

const MUTED_HEX: &str = "595959";

/// The organizer of an item, after its prompt (and support), in place of answer lines.
pub(super) fn docx_organizer(out: &mut String, o: &SheetOrganizer) {
    let width = DOCX_WIDTH - DOCX_INDENT;
    if !o.title.is_empty() {
        out.push_str(&para(
            &[run(
                &o.title,
                RunStyle {
                    bold: true,
                    ..RunStyle::default()
                },
            )],
            &ParaStyle {
                indent: DOCX_INDENT,
                before: 80,
                after: 60,
                keep_next: true,
                ..ParaStyle::default()
            },
        ));
    }
    let n = o.sections.len().max(1) as u32;
    let rows: Vec<(Option<u32>, Vec<Td>)> = match o.kind.family() {
        Family::Columns | Family::Venn => {
            let w = width / n;
            let head = o
                .sections
                .iter()
                .map(|s| Td {
                    shade: Some(HEAD_SHADE),
                    centre: true,
                    ..Td::boxed(w, label_para(&s.label, true))
                })
                .collect();
            let mut rows = vec![(None, head)];
            if o.kind.family() == Family::Venn {
                // Every region in one cell, its items one under the other.
                rows.push((
                    Some(DOCX_ROW_H * 3),
                    o.sections
                        .iter()
                        .map(|s| Td::boxed(w, cells_xml(&s.cells, true)))
                        .collect(),
                ));
            } else {
                for r in 0..o.rows() {
                    rows.push((
                        Some(DOCX_ROW_H),
                        o.sections
                            .iter()
                            .map(|s| {
                                Td::boxed(
                                    w,
                                    cells_xml(
                                        s.cells.get(r).map(std::slice::from_ref).unwrap_or(&[]),
                                        false,
                                    ),
                                )
                            })
                            .collect(),
                    ));
                }
            }
            rows
        }
        Family::Rows => {
            let label_w = width * 28 / 100;
            o.sections
                .iter()
                .map(|s| {
                    (
                        Some(DOCX_ROW_H),
                        vec![
                            Td {
                                shade: Some(HEAD_SHADE),
                                ..Td::boxed(label_w, label_para(&s.label, false))
                            },
                            Td::boxed(width - label_w, cells_xml(&s.cells, true)),
                        ],
                    )
                })
                .collect()
        }
        Family::Chain => {
            let box_w = (width - DOCX_ARROW_W * (CHAIN_PER_ROW as u32 - 1)) / CHAIN_PER_ROW as u32;
            o.sections
                .chunks(CHAIN_PER_ROW)
                .enumerate()
                .map(|(ri, chunk)| {
                    let mut cells = Vec::new();
                    for slot in 0..CHAIN_PER_ROW {
                        if slot > 0 {
                            let arrow = if slot < chunk.len() { "→" } else { "" };
                            cells.push(Td::open(DOCX_ARROW_W, arrow_para(arrow)));
                        }
                        match chunk.get(slot) {
                            Some(s) => cells.push(Td::boxed(
                                box_w,
                                chain_box(ri * CHAIN_PER_ROW + slot + 1, &s.label, &s.cells),
                            )),
                            None => cells.push(Td::open(box_w, String::new())),
                        }
                    }
                    (Some(DOCX_ROW_H * 2), cells)
                })
                .collect()
        }
        Family::Web => {
            let col_w = width / 3;
            let (top, bottom) = web_rows(o.sections.len());
            let branch_row = |indices: &[usize]| -> Vec<Td> {
                let slots = web_slots(indices.len());
                (0..3)
                    .map(|col| match slots.iter().position(|s| *s == col) {
                        Some(k) => {
                            let s = &o.sections[indices[k]];
                            Td::boxed(
                                col_w,
                                format!(
                                    "{}{}",
                                    label_para(&s.label, true),
                                    cells_xml(&s.cells, true)
                                ),
                            )
                        }
                        None => Td::open(col_w, String::new()),
                    })
                    .collect()
            };
            let connector = |indices: &[usize], arrows: [&str; 3]| -> Vec<Td> {
                let slots = web_slots(indices.len());
                (0..3)
                    .map(|col| {
                        let arrow = if slots.contains(&col) {
                            arrows[col]
                        } else {
                            ""
                        };
                        Td::open(col_w, arrow_para(arrow))
                    })
                    .collect()
            };
            let centre = vec![
                Td::open(col_w, String::new()),
                Td {
                    shade: Some(CENTRE_SHADE),
                    centre: true,
                    ..Td::boxed(col_w, label_para(&o.center, true))
                },
                Td::open(col_w, String::new()),
            ];
            let mut rows = vec![
                (Some(DOCX_ROW_H * 2), branch_row(&top)),
                (None, connector(&top, ["↖", "↑", "↗"])),
                (Some(DOCX_ROW_H), centre),
            ];
            if !bottom.is_empty() {
                rows.push((None, connector(&bottom, ["↙", "↓", "↘"])));
                rows.push((Some(DOCX_ROW_H * 2), branch_row(&bottom)));
            }
            rows
        }
    };
    out.push_str(&org_table(&rows));
}

// ================================================================ PDF

const ORG_SIZE: f32 = 10.0;
const ORG_PAD: f32 = 5.0;
/// Height of an empty cell to write in.
const WRITE_H: f32 = 18.0;
const ARROW_GAP: f32 = 22.0;
const WEB_GAP: f32 = 20.0;

fn label_style() -> Style {
    Style {
        weight: Weight::Bold,
        size: ORG_SIZE,
        ..Style::body()
    }
}

fn title_style() -> Style {
    Style {
        size: ORG_SIZE + 0.5,
        ..label_style()
    }
}

fn given_style() -> Style {
    Style {
        size: ORG_SIZE,
        ..Style::body()
    }
}

fn answer_style() -> Style {
    Style {
        weight: Weight::Italic,
        size: ORG_SIZE,
        color: GOLD,
        ..Style::body()
    }
}

impl Layout<'_> {
    fn org_lines(&self, text: &str, style: Style, width: f32) -> Vec<String> {
        if text.trim().is_empty() {
            return Vec::new();
        }
        self.wrap(text, style.weight, style.size, width.max(20.0))
    }

    /// Lines drawn at (x, y) as their tops; returns the height used.
    fn text_block(&mut self, lines: &[String], x: f32, y: f32, style: Style) -> f32 {
        let saved = self.y;
        self.y = y;
        for l in lines {
            self.draw_line(l, x, style);
            self.y += Self::line_height(style.size);
        }
        self.y = saved;
        lines.len() as f32 * Self::line_height(style.size)
    }

    /// The height of a box's cells at `width` (inner).
    fn cells_height(&self, cells: &[SheetOrgCell], width: f32, empty_h: f32) -> f32 {
        cells
            .iter()
            .map(|c| {
                if !c.given.is_empty() {
                    self.org_lines(&c.given, given_style(), width).len() as f32
                        * Self::line_height(ORG_SIZE)
                        + 2.0
                } else if let Some(a) = &c.answer {
                    self.org_lines(a, answer_style(), width).len() as f32
                        * Self::line_height(ORG_SIZE)
                        + 2.0
                } else {
                    empty_h
                }
            })
            .sum()
    }

    /// Draws cells from `y` down at `x` within `width`: pre-filled text, the teacher's answer, or
    /// (with `lines`) a rule to write on.
    fn draw_cells(
        &mut self,
        cells: &[SheetOrgCell],
        x: f32,
        mut y: f32,
        width: f32,
        lines: bool,
        empty_h: f32,
    ) {
        for c in cells {
            if !c.given.is_empty() {
                let l = self.org_lines(&c.given, given_style(), width);
                y += self.text_block(&l, x, y, given_style()) + 2.0;
            } else if let Some(a) = &c.answer {
                let l = self.org_lines(a, answer_style(), width);
                y += self.text_block(&l, x, y, answer_style()) + 2.0;
            } else {
                y += empty_h;
                if lines {
                    self.ops().push(Op::Line {
                        x1: x,
                        y1: y - 3.0,
                        x2: x + width,
                        y2: y - 3.0,
                        width: 0.5,
                        color: RULE,
                    });
                }
            }
        }
    }

    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.ops().push(Op::Rect { x, y, w, h });
    }

    fn line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, width: f32) {
        self.ops().push(Op::Line {
            x1,
            y1,
            x2,
            y2,
            width,
            color: INK,
        });
    }

    /// A box with a bold label (`centre`: centred) and its cells; returns nothing, `h` is given.
    #[allow(clippy::too_many_arguments)]
    fn org_box(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        label: &str,
        cells: &[SheetOrgCell],
        centre: bool,
    ) {
        self.rect(x, y, w, h);
        let inner = w - 2.0 * ORG_PAD;
        let lines = self.org_lines(label, label_style(), inner);
        let mut ty = y + ORG_PAD;
        for l in &lines {
            let lx = if centre {
                x + (w - self.fonts.width(Weight::Bold, l, ORG_SIZE)) / 2.0
            } else {
                x + ORG_PAD
            };
            ty += self.text_block(std::slice::from_ref(l), lx, ty, label_style());
        }
        self.draw_cells(cells, x + ORG_PAD, ty + 2.0, inner, true, WRITE_H);
    }

    fn box_height(&self, label: &str, cells: &[SheetOrgCell], w: f32) -> f32 {
        let inner = w - 2.0 * ORG_PAD;
        2.0 * ORG_PAD
            + self.org_lines(label, label_style(), inner).len() as f32 * Self::line_height(ORG_SIZE)
            + 2.0
            + self.cells_height(cells, inner, WRITE_H)
    }

    fn arrow(&mut self, x1: f32, y1: f32, x2: f32, y2: f32) {
        self.line(x1, y1, x2, y2, 0.9);
        let (dx, dy) = (x2 - x1, y2 - y1);
        let len = (dx * dx + dy * dy).sqrt().max(0.01);
        let (ux, uy) = (dx / len, dy / len);
        let (hx, hy) = (x2 - ux * 5.0, y2 - uy * 5.0);
        self.line(x2, y2, hx - uy * 3.0, hy + ux * 3.0, 0.9);
        self.line(x2, y2, hx + uy * 3.0, hy - ux * 3.0, 0.9);
    }

    /// The organizer of an item, `indent` from the margin, in place of answer lines. Its title
    /// is drawn with the first part of it (never alone at the foot of a page).
    pub(super) fn organizer(&mut self, o: &SheetOrganizer, indent: f32) {
        let x0 = MARGIN + indent;
        let width = CONTENT_W - indent;
        self.y += 4.0;
        let mut title = self.org_lines(&o.title, title_style(), width);
        let t = &mut title;
        match o.kind.family() {
            Family::Columns => self.pdf_columns(o, x0, width, t),
            Family::Rows => self.pdf_rows(o, x0, width, t),
            Family::Chain => self.pdf_chain(o, x0, width, t),
            Family::Venn => self.pdf_venn(o, x0, width, t),
            Family::Web => self.pdf_web(o, x0, width, t),
        }
        self.y += 8.0;
    }

    /// Makes room for the next `h` points of an organizer (a new page if needed), drawing the
    /// title first while it has not been drawn.
    fn unit(&mut self, h: f32, title: &mut Vec<String>, x0: f32) {
        let title_h = if title.is_empty() {
            0.0
        } else {
            title.len() as f32 * Self::line_height(title_style().size) + 3.0
        };
        self.ensure(h + title_h);
        if !title.is_empty() {
            let y = self.y;
            self.text_block(title, x0, y, title_style());
            self.y = y + title_h;
            title.clear();
        }
    }

    /// Headed columns, one row per cell; a page break may fall between rows.
    fn pdf_columns(&mut self, o: &SheetOrganizer, x0: f32, width: f32, title: &mut Vec<String>) {
        let n = o.sections.len().max(1);
        let w = width / n as f32;
        let inner = w - 2.0 * ORG_PAD;
        let head_h = o
            .sections
            .iter()
            .map(|s| self.org_lines(&s.label, label_style(), inner).len().max(1))
            .max()
            .unwrap_or(1) as f32
            * Self::line_height(ORG_SIZE)
            + 2.0 * ORG_PAD;
        self.unit(head_h + 30.0, title, x0);
        let y = self.y;
        for (i, s) in o.sections.iter().enumerate() {
            let x = x0 + w * i as f32;
            self.rect(x, y, w, head_h);
            // A second rule under the heading.
            self.line(x, y + head_h - 1.2, x + w, y + head_h - 1.2, 0.5);
            let lines = self.org_lines(&s.label, label_style(), inner);
            let mut ty = y + ORG_PAD;
            for l in &lines {
                let lx = x + (w - self.fonts.width(Weight::Bold, l, ORG_SIZE)) / 2.0;
                ty += self.text_block(std::slice::from_ref(l), lx, ty, label_style());
            }
        }
        self.y = y + head_h;
        for r in 0..o.rows() {
            let row_h = o
                .sections
                .iter()
                .map(|s| {
                    s.cells
                        .get(r)
                        .map(|c| self.cells_height(std::slice::from_ref(c), inner, 0.0))
                        .unwrap_or(0.0)
                })
                .fold(26.0_f32, |a, b| a.max(b + 2.0 * ORG_PAD));
            self.unit(row_h, title, x0);
            let y = self.y;
            for (i, s) in o.sections.iter().enumerate() {
                let x = x0 + w * i as f32;
                self.rect(x, y, w, row_h);
                if let Some(c) = s.cells.get(r) {
                    self.draw_cells(
                        std::slice::from_ref(c),
                        x + ORG_PAD,
                        y + ORG_PAD,
                        inner,
                        false,
                        0.0,
                    );
                }
            }
            self.y = y + row_h;
        }
    }

    /// A label column and a column to write in, one row per section.
    fn pdf_rows(&mut self, o: &SheetOrganizer, x0: f32, width: f32, title: &mut Vec<String>) {
        let label_w = width * 0.28;
        let text_w = width - label_w;
        for s in &o.sections {
            let label_h = self
                .org_lines(&s.label, label_style(), label_w - 2.0 * ORG_PAD)
                .len() as f32
                * Self::line_height(ORG_SIZE);
            let h = (self.cells_height(&s.cells, text_w - 2.0 * ORG_PAD, WRITE_H)).max(label_h)
                + 2.0 * ORG_PAD;
            self.unit(h, title, x0);
            let y = self.y;
            self.rect(x0, y, label_w, h);
            let lines = self.org_lines(&s.label, label_style(), label_w - 2.0 * ORG_PAD);
            self.text_block(&lines, x0 + ORG_PAD, y + ORG_PAD, label_style());
            self.rect(x0 + label_w, y, text_w, h);
            self.draw_cells(
                &s.cells,
                x0 + label_w + ORG_PAD,
                y + ORG_PAD,
                text_w - 2.0 * ORG_PAD,
                true,
                WRITE_H,
            );
            self.y = y + h;
        }
    }

    /// Numbered boxes, three to a row, arrows between them.
    fn pdf_chain(&mut self, o: &SheetOrganizer, x0: f32, width: f32, title: &mut Vec<String>) {
        let per = CHAIN_PER_ROW as f32;
        let box_w = (width - ARROW_GAP * (per - 1.0)) / per;
        for (ri, chunk) in o.sections.chunks(CHAIN_PER_ROW).enumerate() {
            let labels: Vec<String> = chunk
                .iter()
                .enumerate()
                .map(|(k, s)| {
                    let no = ri * CHAIN_PER_ROW + k + 1;
                    if s.label.is_empty() {
                        format!("{no}.")
                    } else {
                        format!("{no}. {}", s.label)
                    }
                })
                .collect();
            let h = chunk
                .iter()
                .zip(&labels)
                .map(|(s, label)| self.box_height(label, &s.cells, box_w))
                .fold(40.0_f32, f32::max);
            self.unit(h + 6.0, title, x0);
            let y = self.y;
            for (k, (s, label)) in chunk.iter().zip(&labels).enumerate() {
                let x = x0 + (box_w + ARROW_GAP) * k as f32;
                self.org_box(x, y, box_w, h, label, &s.cells, false);
                if k + 1 < chunk.len() {
                    let ay = y + h / 2.0;
                    self.arrow(x + box_w + 4.0, ay, x + box_w + ARROW_GAP - 4.0, ay);
                }
            }
            self.y = y + h + 8.0;
        }
    }

    /// Two overlapping circles: only A on the left, both in the overlap, only B on the right,
    /// the three labels above them.
    fn pdf_venn(&mut self, o: &SheetOrganizer, x0: f32, width: f32, title: &mut Vec<String>) {
        let empty = Vec::new();
        let region = |i: usize| o.sections.get(i).map_or(&empty, |s| &s.cells);
        let label = |i: usize| o.sections.get(i).map_or("", |s| s.label.as_str());
        // Text columns inside each region.
        let cols = [(0.06, 0.30), (0.41, 0.18), (0.64, 0.30)];
        let content_h = (0..3)
            .map(|i| self.cells_height(region(i), width * cols[i].1, WRITE_H))
            .fold(0.0_f32, f32::max);
        let h = (content_h + 44.0).max(120.0);
        let label_h = Self::line_height(ORG_SIZE) * 2.0;
        self.unit(label_h + h + 6.0, title, x0);
        let y = self.y;
        for (i, (cx, cw)) in cols.iter().enumerate() {
            let lines = self.org_lines(label(i), label_style(), width * cw);
            let lw = lines
                .first()
                .map(|l| self.fonts.width(Weight::Bold, l, ORG_SIZE))
                .unwrap_or(0.0);
            let lx = x0 + width * (cx + cw / 2.0) - lw / 2.0;
            let ly = if lines.len() > 1 {
                y
            } else {
                y + Self::line_height(ORG_SIZE)
            };
            self.text_block(&lines[..lines.len().min(2)], lx, ly, label_style());
        }
        let top = y + label_h + 2.0;
        let (rx, ry) = (width * 0.31, h / 2.0);
        self.ops().push(Op::Ellipse {
            cx: x0 + width * 0.31,
            cy: top + ry,
            rx,
            ry,
        });
        self.ops().push(Op::Ellipse {
            cx: x0 + width * 0.69,
            cy: top + ry,
            rx,
            ry,
        });
        for (i, (cx, cw)) in cols.iter().enumerate() {
            let cells = region(i);
            let ch = self.cells_height(cells, width * cw, WRITE_H);
            let cy = top + (h - ch) / 2.0;
            self.draw_cells(cells, x0 + width * cx, cy, width * cw, true, WRITE_H);
        }
        self.y = top + h;
    }

    /// The centre in the middle, the branches above and below, joined to it by lines.
    fn pdf_web(&mut self, o: &SheetOrganizer, x0: f32, width: f32, title: &mut Vec<String>) {
        let gap = 10.0;
        let col_w = (width - 2.0 * gap) / 3.0;
        let col_x = |col: usize| x0 + (col_w + gap) * col as f32;
        let (top, bottom) = web_rows(o.sections.len());
        let row_h = |l: &Self, indices: &[usize]| {
            indices
                .iter()
                .map(|i| l.box_height(&o.sections[*i].label, &o.sections[*i].cells, col_w))
                .fold(36.0_f32, f32::max)
        };
        let top_h = row_h(self, &top);
        let bottom_h = if bottom.is_empty() {
            0.0
        } else {
            row_h(self, &bottom)
        };
        let centre_lines = self.org_lines(&o.center, label_style(), col_w - 2.0 * ORG_PAD);
        let centre_h =
            (centre_lines.len() as f32 * Self::line_height(ORG_SIZE) + 2.0 * ORG_PAD).max(30.0);
        let total = top_h
            + WEB_GAP
            + centre_h
            + if bottom.is_empty() {
                0.0
            } else {
                WEB_GAP + bottom_h
            };
        self.unit(total + 6.0, title, x0);
        let y = self.y;
        let centre_y = y + top_h + WEB_GAP;
        let centre_x = col_x(1);
        // The centre: a double border.
        self.rect(centre_x, centre_y, col_w, centre_h);
        self.rect(centre_x + 2.0, centre_y + 2.0, col_w - 4.0, centre_h - 4.0);
        let mut ty =
            centre_y + (centre_h - centre_lines.len() as f32 * Self::line_height(ORG_SIZE)) / 2.0;
        for l in &centre_lines {
            let lx = centre_x + (col_w - self.fonts.width(Weight::Bold, l, ORG_SIZE)) / 2.0;
            ty += self.text_block(std::slice::from_ref(l), lx, ty, label_style());
        }
        let mid = centre_x + col_w / 2.0;
        for (k, col) in web_slots(top.len()).into_iter().enumerate() {
            let s = &o.sections[top[k]];
            let x = col_x(col);
            self.org_box(x, y, col_w, top_h, &s.label, &s.cells, true);
            self.line(mid, centre_y, x + col_w / 2.0, y + top_h, 0.8);
        }
        let bottom_y = centre_y + centre_h + WEB_GAP;
        for (k, col) in web_slots(bottom.len()).into_iter().enumerate() {
            let s = &o.sections[bottom[k]];
            let x = col_x(col);
            self.org_box(x, bottom_y, col_w, bottom_h, &s.label, &s.cells, true);
            self.line(mid, centre_y + centre_h, x + col_w / 2.0, bottom_y, 0.8);
        }
        self.y = y + total;
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::worksheet::assemble::project;
    use crate::worksheet::organizer::{Family, OrgCell, Organizer, OrganizerKind};
    use crate::worksheet::{Block, Content, Item, Level, FMT_ORGANIZER};

    /// A worksheet with one organizer question of `kind`: every section's first cell pre-filled
    /// with 「Given i」, the others with answers 「Key i.j」.
    fn content(kind: OrganizerKind, with_organizer: bool) -> Content {
        let mut org = Organizer::new(kind);
        org.title = "Organize the story".into();
        for (i, s) in org.sections.iter_mut().enumerate() {
            s.label = format!("Label {i}");
            if s.cells.len() < 2 && kind.max_cells() >= 2 {
                s.cells.push(OrgCell::default());
            }
            for (j, c) in s.cells.iter_mut().enumerate() {
                c.answer = format!("Key {i}.{j}");
                if j == 0 && i == 0 {
                    c.given = format!("Given {i}");
                    c.prefilled = Level::ALL.to_vec();
                }
            }
        }
        if kind == OrganizerKind::MindMap {
            org.center = "Umbrellas".into();
        }
        Content {
            levels: vec![Level::Standard],
            blocks: vec![Block {
                id: "b1".into(),
                title: "Organize".into(),
                items: vec![Item {
                    id: "i1".into(),
                    answer_format: Some(FMT_ORGANIZER.into()),
                    prompt: "Complete the organizer.".into(),
                    levels: vec![Level::Standard],
                    organizer: with_organizer.then_some(org),
                    ..Item::default()
                }],
                ..Block::default()
            }],
            ..Content::default()
        }
    }

    fn sheet(kind: OrganizerKind, audience: Audience) -> Sheet {
        project("Umbrellas", &content(kind, true), Level::Standard, audience)
    }

    fn document(bytes: Vec<u8>) -> String {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("word/document.xml").unwrap(), &mut xml)
            .unwrap();
        xml
    }

    fn fonts() -> Fonts {
        Fonts {
            regular: Face::new(LIBERTINUS_REGULAR).unwrap(),
            bold: Face::new(LIBERTINUS_BOLD).unwrap(),
            italic: Face::new(LIBERTINUS_ITALIC).unwrap(),
            cjk: None,
        }
    }

    /// The PDF drawing of a sheet: every text drawn, and how many boxes, lines and ellipses.
    fn drawn(sheet: &Sheet) -> (String, usize, usize, usize) {
        let fonts = fonts();
        let bank = ImageBank::new();
        let mut l = Layout::new(&fonts, &bank);
        layout_sheet(&mut l, sheet);
        let mut text = String::new();
        let (mut rects, mut lines, mut ellipses) = (0, 0, 0);
        for op in l.pages.iter().flatten() {
            match op {
                Op::Text { text: t, .. } => {
                    text.push_str(t);
                    text.push('\n');
                }
                Op::Rect { .. } => rects += 1,
                Op::Line { .. } => lines += 1,
                Op::Ellipse { .. } => ellipses += 1,
                Op::Image { .. } => {}
            }
        }
        (text, rects, lines, ellipses)
    }

    #[test]
    fn every_type_prints_its_structure_in_docx_and_pdf() {
        for kind in OrganizerKind::ALL {
            let student = sheet(kind, Audience::Student);
            let org = student.blocks[0].items[0].organizer.clone().unwrap();
            assert_eq!(
                student.blocks[0].items[0].lines, 0,
                "{kind:?}: no answer lines"
            );
            let xml = document(docx(&student).unwrap());
            for s in &org.sections {
                assert!(xml.contains(&s.label), "{kind:?} docx label {}", s.label);
            }
            assert!(xml.contains("Organize the story"), "{kind:?} title");
            assert!(xml.contains(">Given 0<"), "{kind:?}: the pre-filled cell");
            assert!(!xml.contains("Key "), "{kind:?}: no answers for students");
            assert!(xml.contains("<w:tblLayout w:type=\"fixed\"/>"), "{kind:?}");
            let (text, rects, lines, ellipses) = drawn(&student);
            for s in &org.sections {
                assert!(
                    text.contains(&s.label),
                    "{kind:?} pdf label {}:\n{text}",
                    s.label
                );
            }
            assert!(
                text.contains("Given 0") && !text.contains("Key "),
                "{kind:?}"
            );
            match kind.family() {
                Family::Venn => {
                    assert_eq!(ellipses, 2, "two circles");
                    assert!(xml.contains("Label 1"));
                }
                Family::Chain => {
                    assert_eq!(rects, org.sections.len(), "{kind:?}: one box each");
                    assert!(xml.contains(">→<"), "{kind:?}: arrows");
                    assert!(text.contains("1. Label 0") && text.contains("2. Label 1"));
                    assert!(lines >= 3 * (org.sections.len().min(3) - 1), "arrows drawn");
                }
                Family::Web => {
                    assert!(text.contains("Umbrellas") && xml.contains(">Umbrellas<"));
                    assert!(xml.contains("↖") || xml.contains("↑"));
                    assert!(lines >= org.sections.len(), "a line per branch");
                }
                Family::Columns => {
                    assert_eq!(
                        rects,
                        org.sections.len() * (1 + org.rows()),
                        "{kind:?}: a grid"
                    );
                }
                Family::Rows => assert_eq!(rects, org.sections.len() * 2),
            }
            assert!(pdf(&student).unwrap().starts_with(b"%PDF-"));
            // The teacher sees the answers of the cells students fill in, in the teacher's ink.
            let teacher = sheet(kind, Audience::Teacher);
            let xml = document(docx(&teacher).unwrap());
            assert!(
                xml.contains(">Key 0.1<") || xml.contains(">Key 1.0<"),
                "{kind:?}"
            );
            assert!(xml.contains(TEACHER_INK));
            let (text, ..) = drawn(&teacher);
            assert!(text.contains("Key 1.0"), "{kind:?}:\n{text}");
        }
    }

    #[test]
    fn an_organizer_question_without_one_still_prints_a_box() {
        let s = project(
            "Old",
            &content(OrganizerKind::TChart, false),
            Level::Standard,
            Audience::Student,
        );
        assert!(s.blocks[0].items[0].organizer.is_none());
        let xml = document(docx(&s).unwrap());
        assert!(
            xml.contains("<w:trHeight w:val=\"2000\"/>"),
            "the old empty box"
        );
        let (_, rects, ..) = drawn(&s);
        assert_eq!(rects, 1);
        // An organizer kept on a question switched to another format is not printed.
        let mut c = content(OrganizerKind::TChart, true);
        c.blocks[0].items[0].answer_format = Some("簡答題".into());
        let s = project("Switched", &c, Level::Standard, Audience::Student);
        assert!(s.blocks[0].items[0].organizer.is_none());
        assert!(s.blocks[0].items[0].lines > 0);
    }

    #[test]
    fn basic_gets_more_pre_filled_cells_than_advanced() {
        let mut c = content(OrganizerKind::Sequence, true);
        c.levels = Level::ALL.to_vec();
        c.blocks[0].items[0].levels = Level::ALL.to_vec();
        let org = c.blocks[0].items[0].organizer.as_mut().unwrap();
        org.sections[1].cells[0] = OrgCell {
            answer: "Rain".into(),
            given: "Rain".into(),
            prefilled: vec![Level::Basic],
        };
        let xml = |level| document(docx(&project("T", &c, level, Audience::Student)).unwrap());
        assert!(xml(Level::Basic).contains(">Rain<"));
        assert!(!xml(Level::Advanced).contains(">Rain<"));
        assert!(xml(Level::Advanced).contains(">Given 0<"));
    }
}
