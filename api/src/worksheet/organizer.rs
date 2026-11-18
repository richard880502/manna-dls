//! 圖像組織 (graphic organizers): a question whose answer format is 圖像組織 can carry a structured
//! organizer that the server draws on every printed form (editor preview, DOCX, PDF) instead of
//! the empty box older questions print. The model only chooses the type and writes the labels
//! and the answer key; the drawing is code. Contract: docs/API.md「圖像組織」.
//!
//! One shape covers the eight types: a title, a centre word (mind map only) and 2–6 sections,
//! each with a label (a column heading, a box title, a branch) and 1–6 cells. A cell has its
//! answer (teacher version only) and the text the student sheet prints in the versions it is
//! pre-filled in (`prefilled`: SIOP Q2 教學鷹架, T3: 基礎版 gets the most pre-filled cells). The
//! types fall into five layouts ([`Family`]), drawn the same way in all three outputs.
//!
//! Why these eight: they are the organizers SIOP lists for 教學鷹架 (Q2) and 應用活動 (A4) that a
//! reading worksheet of a Taiwanese high-school English class actually uses: comparing (T-chart,
//! Venn), ordering (sequence), relations (cause → effect, problem → solution), narrative
//! structure (story map), before/after reading (KWL) and vocabulary or idea webs (mind map).
//! Fishbone, flowcharts with branches, timelines with dates and matrices were left out: they
//! need free layout or more than two dimensions, and the 表格整理 format already covers tables.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::template::{enum_of, obj, s_max};
use super::Level;
use crate::routes::clip_chars;
use crate::sop::checks::{clean_markup, english_only};

// ---------------------------------------------------------------- the types

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum OrganizerKind {
    #[default]
    TChart,
    Venn,
    Sequence,
    CauseEffect,
    ProblemSolution,
    StoryMap,
    Kwl,
    MindMap,
}

/// How a type is laid out on paper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// Headed columns with rows of cells (T-chart, KWL).
    Columns,
    /// Boxes joined by arrows (sequence, cause → effect, problem → solution).
    Chain,
    /// Two overlapping circles: only A, both, only B.
    Venn,
    /// A centre with branches around it (mind map).
    Web,
    /// Labelled rows (story map).
    Rows,
}

impl OrganizerKind {
    pub const ALL: [OrganizerKind; 8] = [
        OrganizerKind::TChart,
        OrganizerKind::Venn,
        OrganizerKind::Sequence,
        OrganizerKind::CauseEffect,
        OrganizerKind::ProblemSolution,
        OrganizerKind::StoryMap,
        OrganizerKind::Kwl,
        OrganizerKind::MindMap,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::TChart => "t_chart",
            Self::Venn => "venn",
            Self::Sequence => "sequence",
            Self::CauseEffect => "cause_effect",
            Self::ProblemSolution => "problem_solution",
            Self::StoryMap => "story_map",
            Self::Kwl => "kwl",
            Self::MindMap => "mind_map",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim().to_ascii_lowercase().replace(['-', ' '], "_");
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }

    /// The teacher's name of the type.
    pub fn label(self) -> &'static str {
        match self {
            Self::TChart => "T 字表",
            Self::Venn => "文氏圖",
            Self::Sequence => "順序鏈",
            Self::CauseEffect => "因果鏈",
            Self::ProblemSolution => "問題與解決",
            Self::StoryMap => "故事地圖",
            Self::Kwl => "KWL 表",
            Self::MindMap => "心智圖",
        }
    }

    /// The English name (accessibility text, never a heading on paper).
    pub fn english(self) -> &'static str {
        match self {
            Self::TChart => "T-chart",
            Self::Venn => "Venn diagram",
            Self::Sequence => "Sequence chain",
            Self::CauseEffect => "Cause and effect chain",
            Self::ProblemSolution => "Problem and solution",
            Self::StoryMap => "Story map",
            Self::Kwl => "KWL chart",
            Self::MindMap => "Mind map",
        }
    }

    pub fn family(self) -> Family {
        match self {
            Self::TChart | Self::Kwl => Family::Columns,
            Self::Sequence | Self::CauseEffect | Self::ProblemSolution => Family::Chain,
            Self::Venn => Family::Venn,
            Self::MindMap => Family::Web,
            Self::StoryMap => Family::Rows,
        }
    }

    /// Fewest and most sections.
    pub fn sections(self) -> (usize, usize) {
        match self {
            Self::TChart => (2, 2),
            Self::Venn | Self::Kwl => (3, 3),
            Self::Sequence | Self::MindMap => (3, 6),
            Self::CauseEffect => (2, 6),
            Self::ProblemSolution => (2, 4),
            Self::StoryMap => (5, 5),
        }
    }

    /// Most cells in one section.
    pub fn max_cells(self) -> usize {
        match self.family() {
            Family::Columns => MAX_CELLS,
            Family::Venn => 5,
            Family::Rows => 4,
            Family::Web => 3,
            Family::Chain => 2,
        }
    }

    /// How many cells a section of a new organizer of this type has.
    fn new_cells(self, index: usize) -> usize {
        match self {
            Self::TChart | Self::Kwl | Self::Venn => 3,
            Self::StoryMap if index == 3 => 3,
            Self::MindMap => 2,
            _ => 1,
        }
    }

    /// The label a section at `index` of `count` gets when it has none.
    pub fn default_label(self, index: usize, count: usize) -> String {
        let pick = |list: &[&str]| list.get(index).copied().unwrap_or("").to_string();
        match self {
            Self::TChart => pick(&["Topic A", "Topic B"]),
            Self::Venn => pick(&["Topic A", "Both", "Topic B"]),
            Self::Kwl => pick(&["What I Know", "What I Want to Know", "What I Learned"]),
            Self::StoryMap => pick(&["Setting", "Characters", "Problem", "Events", "Solution"]),
            Self::Sequence if index == 0 => "First".to_string(),
            Self::Sequence if index + 1 == count => "Finally".to_string(),
            Self::Sequence => pick(&["", "Next", "Then", "After that", "Later"]),
            Self::CauseEffect if index == 0 => "Cause".to_string(),
            Self::CauseEffect => "Effect".to_string(),
            Self::ProblemSolution if index == 0 => "Problem".to_string(),
            Self::ProblemSolution if index + 1 == count && count >= 3 => "Result".to_string(),
            Self::ProblemSolution => "Solution".to_string(),
            Self::MindMap => format!("Idea {}", index + 1),
        }
    }
}

// ---------------------------------------------------------------- limits

/// Most sections of any organizer (boxes of a chain, branches of a mind map).
pub const MAX_SECTIONS: usize = 6;
/// Most cells of one section (rows of a column).
pub const MAX_CELLS: usize = 6;
/// What the model may write (the gateway enforces these while decoding).
pub const TITLE_CHARS: usize = 80;
pub const CENTER_CHARS: usize = 60;
pub const LABEL_CHARS: usize = 40;
pub const CELL_CHARS: usize = 120;
/// What a saved organizer may hold (the teacher may write more than the model).
pub const MAX_TITLE_CHARS: usize = 200;
pub const MAX_LABEL_CHARS: usize = 100;
pub const MAX_CELL_CHARS: usize = 300;

// ---------------------------------------------------------------- the stored organizer

/// One cell: its answer (teacher version only) and, in the versions of `prefilled`, the text
/// the student sheet prints in it as an example (otherwise it is empty for the student to fill).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct OrgCell {
    pub answer: String,
    pub given: String,
    pub prefilled: Vec<Level>,
}

impl OrgCell {
    fn blank() -> Self {
        Self::default()
    }

    /// Whether the student sheet of `level` prints this cell filled in.
    pub fn given_in(&self, level: Level) -> bool {
        self.prefilled.contains(&level) && !self.given.trim().is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct OrgSection {
    pub label: String,
    pub cells: Vec<OrgCell>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Organizer {
    #[serde(rename = "type")]
    pub kind: OrganizerKind,
    /// Printed above the organizer ("" for none).
    pub title: String,
    /// The centre word of a mind map ("" for the other types).
    pub center: String,
    pub sections: Vec<OrgSection>,
}

impl Organizer {
    /// A new organizer of a type: the fewest sections, default labels, empty cells (what the
    /// editor makes when a question is switched to 圖像組織).
    pub fn new(kind: OrganizerKind) -> Self {
        let (min, _) = kind.sections();
        Self {
            kind,
            title: String::new(),
            center: if kind == OrganizerKind::MindMap {
                "Main idea".to_string()
            } else {
                String::new()
            },
            sections: (0..min)
                .map(|i| OrgSection {
                    label: kind.default_label(i, min),
                    cells: vec![OrgCell::blank(); kind.new_cells(i)],
                })
                .collect(),
        }
    }

    fn cells(&self) -> impl Iterator<Item = &OrgCell> {
        self.sections.iter().flat_map(|s| s.cells.iter())
    }

    /// Every text of the organizer (labels, title, centre, answers and pre-filled text).
    pub fn texts(&self) -> impl Iterator<Item = &str> {
        [self.title.as_str(), self.center.as_str()]
            .into_iter()
            .chain(self.sections.iter().flat_map(|s| {
                std::iter::once(s.label.as_str()).chain(
                    s.cells
                        .iter()
                        .flat_map(|c| [c.answer.as_str(), c.given.as_str()]),
                )
            }))
    }

    /// Fits the organizer to its type: at most its sections (the last ones cut) and cells per
    /// section, at least its sections (empty ones added), every section with a label and a
    /// cell, the centre only on a mind map. Returns what it changed.
    pub fn fit(&mut self) -> Vec<String> {
        let mut notes = Vec::new();
        let kind = self.kind;
        let (min, max) = kind.sections();
        if self.sections.len() > max {
            notes.push(format!(
                "{}最多 {max} 格（sections），交出 {} 格",
                kind.label(),
                self.sections.len()
            ));
            self.sections.truncate(max);
        }
        if self.sections.len() < min {
            notes.push(format!(
                "{}至少要 {min} 格（sections），只交出 {} 格",
                kind.label(),
                self.sections.len()
            ));
            while self.sections.len() < min {
                self.sections.push(OrgSection::default());
            }
        }
        let count = self.sections.len();
        let max_cells = kind.max_cells();
        let mut cut = false;
        for (i, s) in self.sections.iter_mut().enumerate() {
            if s.label.trim().is_empty() {
                s.label = kind.default_label(i, count);
            }
            if s.cells.len() > max_cells {
                s.cells.truncate(max_cells);
                cut = true;
            }
            if s.cells.is_empty() {
                s.cells.push(OrgCell::blank());
            }
        }
        if cut {
            notes.push(format!(
                "{}每一格最多 {max_cells} 個空格（cells）",
                kind.label()
            ));
        }
        if kind == OrganizerKind::MindMap {
            if self.center.trim().is_empty() {
                self.center = if self.title.trim().is_empty() {
                    "Main idea".to_string()
                } else {
                    self.title.clone()
                };
            }
        } else {
            self.center.clear();
        }
        notes
    }

    /// Leaves at least one cell for the students in every version: when a version would print
    /// every cell pre-filled, the cell pre-filled in the fewest versions (the last of them) is
    /// left empty in all of them. Returns whether it changed anything.
    pub fn leave_blanks(&mut self) -> bool {
        let mut changed = false;
        for level in Level::ALL {
            let all_given = self.cells().count() > 0 && self.cells().all(|c| c.given_in(level));
            if !all_given {
                continue;
            }
            let fewest = self
                .cells()
                .map(|c| c.prefilled.len())
                .min()
                .unwrap_or_default();
            if let Some(cell) = self
                .sections
                .iter_mut()
                .flat_map(|s| s.cells.iter_mut())
                .rev()
                .find(|c| c.prefilled.len() == fewest)
            {
                cell.prefilled.clear();
                changed = true;
            }
        }
        changed
    }

    /// Checks a saved organizer (what a client sent): within the limits, levels in order.
    pub fn validate(&mut self) -> Result<(), String> {
        if self.sections.len() > MAX_SECTIONS {
            return Err(format!("一個圖像組織最多 {MAX_SECTIONS} 格。"));
        }
        let long = |s: &str, max: usize| s.chars().count() > max;
        if long(&self.title, MAX_TITLE_CHARS) || long(&self.center, MAX_LABEL_CHARS) {
            return Err(format!(
                "圖像組織的標題最多 {MAX_TITLE_CHARS} 字，中心詞最多 {MAX_LABEL_CHARS} 字。"
            ));
        }
        for s in &mut self.sections {
            if s.cells.len() > MAX_CELLS {
                return Err(format!("圖像組織的每一格最多 {MAX_CELLS} 個空格。"));
            }
            if long(&s.label, MAX_LABEL_CHARS) {
                return Err(format!("圖像組織的格子標題最多 {MAX_LABEL_CHARS} 字。"));
            }
            for c in &mut s.cells {
                if long(&c.answer, MAX_CELL_CHARS) || long(&c.given, MAX_CELL_CHARS) {
                    return Err(format!("圖像組織的每個空格最多 {MAX_CELL_CHARS} 字。"));
                }
                c.prefilled = Level::ALL
                    .into_iter()
                    .filter(|l| c.prefilled.contains(l))
                    .collect();
            }
        }
        Ok(())
    }

    /// What one version prints for one audience.
    pub fn project(&self, level: Level, teacher: bool) -> SheetOrganizer {
        SheetOrganizer {
            kind: self.kind,
            name: self.kind.english(),
            title: self.title.trim().to_string(),
            center: self.center.trim().to_string(),
            sections: self
                .sections
                .iter()
                .map(|s| SheetOrgSection {
                    label: s.label.trim().to_string(),
                    cells: s
                        .cells
                        .iter()
                        .map(|c| {
                            let given = c.given_in(level);
                            SheetOrgCell {
                                given: if given {
                                    c.given.trim().to_string()
                                } else {
                                    String::new()
                                },
                                answer: (teacher && !given && !c.answer.trim().is_empty())
                                    .then(|| c.answer.trim().to_string()),
                            }
                        })
                        .collect(),
                })
                .collect(),
        }
    }

    /// The organizer in one line for the SIOP self-check: type, title, centre, then every
    /// section with its cells (answers, and which versions print a cell pre-filled).
    pub fn summary(&self, several_levels: bool) -> String {
        let mut parts = vec![format!("{}（{}）", self.kind.label(), self.kind.as_str())];
        if !self.title.trim().is_empty() {
            parts.push(format!("標題：{}", clip_chars(self.title.trim(), 80)));
        }
        if !self.center.trim().is_empty() {
            parts.push(format!("中心：{}", clip_chars(self.center.trim(), 60)));
        }
        for s in &self.sections {
            let cells: Vec<String> = s
                .cells
                .iter()
                .map(|c| {
                    let text = if c.answer.trim().is_empty() {
                        "（空白）".to_string()
                    } else {
                        clip_chars(c.answer.trim(), 80)
                    };
                    let given: Vec<&str> = c
                        .prefilled
                        .iter()
                        .filter(|_| !c.given.trim().is_empty())
                        .map(|l| l.label())
                        .collect();
                    match (given.is_empty(), several_levels) {
                        (true, _) => text,
                        (false, true) => format!("{text}〔預填：{}〕", given.join("、")),
                        (false, false) => format!("{text}〔預填〕"),
                    }
                })
                .collect();
            parts.push(format!(
                "{}：{}",
                clip_chars(s.label.trim(), 40),
                cells.join("／")
            ));
        }
        parts.join("｜")
    }
}

// ---------------------------------------------------------------- the printed organizer

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SheetOrgCell {
    /// Pre-filled text the student sheet prints ("" for an empty cell).
    pub given: String,
    /// Teacher version only: the answer of a cell the students fill in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SheetOrgSection {
    pub label: String,
    pub cells: Vec<SheetOrgCell>,
}

/// An organizer as one version prints it for one audience.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SheetOrganizer {
    #[serde(rename = "type")]
    pub kind: OrganizerKind,
    /// "T-chart", "Venn diagram" … (accessibility text).
    pub name: &'static str,
    pub title: String,
    pub center: String,
    pub sections: Vec<SheetOrgSection>,
}

impl SheetOrganizer {
    /// Every printed text (for the vocabulary check and the PDF's font check).
    pub fn texts(&self) -> Vec<&str> {
        let mut out = vec![self.title.as_str(), self.center.as_str()];
        for s in &self.sections {
            out.push(&s.label);
            for c in &s.cells {
                out.push(&c.given);
                out.extend(c.answer.as_deref());
            }
        }
        out.into_iter().filter(|t| !t.is_empty()).collect()
    }

    /// The rows of a columns layout: the most cells of any section.
    pub fn rows(&self) -> usize {
        self.sections
            .iter()
            .map(|s| s.cells.len())
            .max()
            .unwrap_or(0)
            .max(1)
    }
}

/// The branches of a mind map above and below its centre, as indices into the sections: the
/// first half (rounded up) above, the rest below.
pub fn web_rows(count: usize) -> (Vec<usize>, Vec<usize>) {
    let top = count.div_ceil(2);
    ((0..top).collect(), (top..count).collect())
}

/// The column (0–2 of three) each of `n` boxes in a row of a mind map takes: one in the middle,
/// two at the sides, three across.
pub fn web_slots(n: usize) -> Vec<usize> {
    match n {
        0 => Vec::new(),
        1 => vec![1],
        2 => vec![0, 2],
        _ => (0..n.min(3)).collect(),
    }
}

/// Boxes per row of a chain.
pub const CHAIN_PER_ROW: usize = 3;

// ---------------------------------------------------------------- what the model writes

/// `prefill_in` of a 差異化學習單 cell: the versions whose student sheet prints it (T3: 基礎版
/// the most). Always a set that includes 基礎版, so less support never means more cells.
pub const PREFILL_IN: [&str; 4] = ["none", "basic", "basic_standard", "all"];

fn prefill_levels(prefill_in: &str) -> Vec<Level> {
    match prefill_in.trim() {
        "basic" => vec![Level::Basic],
        "basic_standard" => vec![Level::Basic, Level::Standard],
        "all" => Level::ALL.to_vec(),
        _ => Vec::new(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct RawOrgCell {
    pub answer: String,
    /// One version (閱讀理解題, 完整學習單, template): printed in the student sheet.
    pub prefilled: bool,
    /// 差異化學習單: one of [`PREFILL_IN`].
    pub prefill_in: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct RawOrgSection {
    pub label: String,
    pub cells: Vec<RawOrgCell>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct RawOrganizer {
    #[serde(rename = "type")]
    pub kind: String,
    pub title: String,
    pub center: String,
    pub sections: Vec<RawOrgSection>,
}

/// A cleaned organizer in the shape the model writes (one version: a cell is pre-filled or not),
/// for a step whose output is kept in its raw shape until the block is built.
impl From<&Organizer> for RawOrganizer {
    fn from(o: &Organizer) -> Self {
        Self {
            kind: o.kind.as_str().to_string(),
            title: o.title.clone(),
            center: o.center.clone(),
            sections: o
                .sections
                .iter()
                .map(|s| RawOrgSection {
                    label: s.label.clone(),
                    cells: s
                        .cells
                        .iter()
                        .map(|c| RawOrgCell {
                            answer: c.answer.clone(),
                            prefilled: !c.prefilled.is_empty() && !c.given.is_empty(),
                            prefill_in: String::new(),
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}

/// `organizer` of an item: the type first (it decides everything after it), the title and the
/// centre, then the sections with their cells (the answer before whether it is pre-filled).
/// Every string and array is capped. `null` for a question of another format.
pub fn schema(tiered: bool) -> Value {
    let kinds: Vec<&str> = OrganizerKind::ALL.iter().map(|k| k.as_str()).collect();
    let cell = if tiered {
        obj(vec![
            ("answer", s_max(CELL_CHARS)),
            ("prefill_in", enum_of(&PREFILL_IN)),
        ])
    } else {
        obj(vec![
            ("answer", s_max(CELL_CHARS)),
            ("prefilled", json!({ "type": "boolean" })),
        ])
    };
    let section = obj(vec![
        ("label", s_max(LABEL_CHARS)),
        (
            "cells",
            json!({ "type": "array", "items": cell, "minItems": 1, "maxItems": MAX_CELLS }),
        ),
    ]);
    let organizer = obj(vec![
        ("type", enum_of(&kinds)),
        ("title", s_max(TITLE_CHARS)),
        ("center", s_max(CENTER_CHARS)),
        (
            "sections",
            json!({ "type": "array", "items": section, "minItems": 2, "maxItems": MAX_SECTIONS }),
        ),
    ]);
    json!({ "anyOf": [organizer, { "type": "null" }] })
}

/// One line of model text: markup and line breaks gone, capped.
fn line(s: &str, max: usize) -> String {
    let flat = clean_markup(s)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    flat.chars().take(max).collect()
}

/// The type a model output without a valid one gets: a mind map when it wrote a centre, a
/// T-chart for two sections or fewer, a sequence otherwise (the type that holds what it wrote).
pub fn fallback_kind(raw: &RawOrganizer) -> OrganizerKind {
    if !raw.center.trim().is_empty() {
        OrganizerKind::MindMap
    } else if raw.sections.len() <= 2 {
        OrganizerKind::TChart
    } else {
        OrganizerKind::Sequence
    }
}

/// What the cleaning of a model's organizer changed: (what to tell the model, what the code did).
pub type Fixes = Vec<(String, &'static str)>;

/// A model's organizer, cleaned: an unknown type falls back ([`fallback_kind`]), texts are one
/// capped line each, the organizer is fitted to its type ([`Organizer::fit`]), a pre-filled
/// cell prints its answer, and every version leaves a cell for the students.
pub fn from_raw(raw: &RawOrganizer, tiered: bool) -> (Organizer, Fixes) {
    let mut fixes: Fixes = Vec::new();
    let kind = match OrganizerKind::parse(&raw.kind) {
        Some(k) => k,
        None => {
            let k = fallback_kind(raw);
            fixes.push((
                format!(
                    "organizer.type「{}」不是固定的類型；只能用：{}。",
                    clip_chars(&raw.kind, 30),
                    OrganizerKind::ALL
                        .iter()
                        .map(|k| k.as_str())
                        .collect::<Vec<_>>()
                        .join("、")
                ),
                match k {
                    OrganizerKind::MindMap => "改成心智圖",
                    OrganizerKind::TChart => "改成 T 字表",
                    _ => "改成順序鏈",
                },
            ));
            k
        }
    };
    let mut org = Organizer {
        kind,
        title: line(&raw.title, TITLE_CHARS),
        center: line(&raw.center, CENTER_CHARS),
        sections: raw
            .sections
            .iter()
            .map(|s| OrgSection {
                label: line(&s.label, LABEL_CHARS),
                cells: s
                    .cells
                    .iter()
                    .map(|c| {
                        let answer = line(&c.answer, CELL_CHARS);
                        let prefilled = if answer.is_empty() {
                            Vec::new()
                        } else if tiered {
                            prefill_levels(&c.prefill_in)
                        } else if c.prefilled {
                            Level::ALL.to_vec()
                        } else {
                            Vec::new()
                        };
                        OrgCell {
                            given: if prefilled.is_empty() {
                                String::new()
                            } else {
                                answer.clone()
                            },
                            answer,
                            prefilled,
                        }
                    })
                    .collect(),
            })
            .collect(),
    };
    for note in org.fit() {
        fixes.push((format!("{note}。"), "依類型調整格數"));
    }
    if org.leave_blanks() {
        fixes.push((
            "圖像組織每一格都預填了，學生沒有要寫的；至少要留一半的格子給學生寫。".to_string(),
            "留一格給學生寫",
        ));
    }
    (org, fixes)
}

/// Whether every student-facing text of the organizer is English (the 完整學習單's rule).
pub fn english(org: &Organizer) -> bool {
    org.texts().all(english_only)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(kind: &str, sections: &[(&str, &[(&str, bool)])]) -> RawOrganizer {
        RawOrganizer {
            kind: kind.into(),
            title: "Why the stand worked".into(),
            center: String::new(),
            sections: sections
                .iter()
                .map(|(label, cells)| RawOrgSection {
                    label: (*label).into(),
                    cells: cells
                        .iter()
                        .map(|(a, p)| RawOrgCell {
                            answer: (*a).into(),
                            prefilled: *p,
                            prefill_in: String::new(),
                        })
                        .collect(),
                })
                .collect(),
        }
    }

    #[test]
    fn every_type_parses_and_has_a_layout() {
        for k in OrganizerKind::ALL {
            assert_eq!(OrganizerKind::parse(k.as_str()), Some(k));
            let (min, max) = k.sections();
            assert!(min >= 2 && min <= max && max <= MAX_SECTIONS, "{k:?}");
            assert!(k.max_cells() <= MAX_CELLS);
            let new = Organizer::new(k);
            assert_eq!(new.sections.len(), min);
            assert!(new.sections.iter().all(|s| !s.label.is_empty()));
            let json = serde_json::to_value(&new).unwrap();
            assert_eq!(json["type"], k.as_str());
            assert_eq!(serde_json::from_value::<Organizer>(json).unwrap(), new);
        }
        assert_eq!(OrganizerKind::parse("T-Chart"), Some(OrganizerKind::TChart));
        assert_eq!(OrganizerKind::parse("fishbone"), None);
        assert_eq!(
            Organizer::new(OrganizerKind::Kwl).sections[2].label,
            "What I Learned"
        );
        assert_eq!(Organizer::new(OrganizerKind::MindMap).center, "Main idea");
    }

    #[test]
    fn the_schema_is_bounded_nullable_and_in_order() {
        let s = schema(false);
        let any = s["anyOf"].as_array().unwrap();
        assert_eq!(any[1], json!({ "type": "null" }));
        let o = &any[0];
        let keys: Vec<&str> = o["properties"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, ["type", "title", "center", "sections"]);
        assert_eq!(o["properties"]["type"]["enum"].as_array().unwrap().len(), 8);
        let sections = &o["properties"]["sections"];
        assert_eq!(sections["maxItems"], MAX_SECTIONS);
        let cells = &sections["items"]["properties"]["cells"];
        assert_eq!(cells["maxItems"], MAX_CELLS);
        let cell: Vec<&str> = cells["items"]["properties"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(cell, ["answer", "prefilled"]);
        assert_eq!(
            cells["items"]["properties"]["answer"]["maxLength"],
            CELL_CHARS
        );
        assert_eq!(
            sections["items"]["properties"]["label"]["maxLength"],
            LABEL_CHARS
        );
        let t = schema(true);
        let cell =
            &t["anyOf"][0]["properties"]["sections"]["items"]["properties"]["cells"]["items"];
        assert_eq!(cell["properties"]["prefill_in"]["enum"], json!(PREFILL_IN));
    }

    #[test]
    fn an_unknown_type_falls_back_to_one_that_holds_the_output() {
        let two = raw(
            "fishbone",
            &[("Before", &[("a", false)]), ("After", &[("b", false)])],
        );
        let (org, fixes) = from_raw(&two, false);
        assert_eq!(org.kind, OrganizerKind::TChart);
        assert!(fixes
            .iter()
            .any(|(p, r)| p.contains("fishbone") && *r == "改成 T 字表"));
        let four = raw(
            "timeline",
            &[
                ("1", &[("a", false)]),
                ("2", &[("b", false)]),
                ("3", &[("c", false)]),
                ("4", &[("d", false)]),
            ],
        );
        assert_eq!(from_raw(&four, false).0.kind, OrganizerKind::Sequence);
        let mut web = four.clone();
        web.center = "Rain".into();
        let (org, _) = from_raw(&web, false);
        assert_eq!(org.kind, OrganizerKind::MindMap);
        assert_eq!(org.center, "Rain");
    }

    #[test]
    fn too_many_boxes_are_cut_and_too_few_are_added() {
        let cells: &[(&str, bool)] = &[("x", false), ("y", false), ("z", false)];
        let many: Vec<(&str, &[(&str, bool)])> = (0..8).map(|_| ("Step", cells)).collect();
        let (org, fixes) = from_raw(&raw("sequence", &many), false);
        assert_eq!(org.sections.len(), 6);
        assert!(
            org.sections.iter().all(|s| s.cells.len() == 2),
            "a chain box holds two cells"
        );
        assert_eq!(fixes.len(), 2, "{fixes:?}");
        let (story, fixes) = from_raw(&raw("story_map", &[("", &[("Taipei", false)])]), false);
        assert_eq!(
            story
                .sections
                .iter()
                .map(|s| s.label.as_str())
                .collect::<Vec<_>>(),
            ["Setting", "Characters", "Problem", "Events", "Solution"]
        );
        assert!(story.sections.iter().all(|s| !s.cells.is_empty()));
        assert!(fixes.iter().any(|(p, _)| p.contains("至少要 5 格")));
        // Labels are one capped line; the centre only stays on a mind map.
        let mut long = raw(
            "t_chart",
            &[(
                "A\nvery **long** heading that goes on and on and on and on",
                &[("a", false)],
            )],
        );
        long.center = "dropped".into();
        let (org, _) = from_raw(&long, false);
        assert!(org.sections[0].label.chars().count() <= LABEL_CHARS);
        assert!(!org.sections[0].label.contains('\n') && !org.sections[0].label.contains("**"));
        assert!(org.center.is_empty());
    }

    #[test]
    fn pre_filled_cells_print_their_answer_and_leave_room_to_write() {
        let (org, fixes) = from_raw(
            &raw(
                "t_chart",
                &[
                    ("Then", &[("dry", true), ("", true)]),
                    ("Now", &[("wet", true)]),
                ],
            ),
            false,
        );
        assert_eq!(org.sections[0].cells[0].given, "dry");
        assert_eq!(org.sections[0].cells[0].prefilled, Level::ALL);
        assert!(
            org.sections[0].cells[1].prefilled.is_empty(),
            "an empty cell is never pre-filled"
        );
        assert!(fixes.is_empty(), "{fixes:?}");
        let (all, fixes) = from_raw(
            &raw("t_chart", &[("A", &[("x", true)]), ("B", &[("y", true)])]),
            false,
        );
        assert!(
            all.sections[1].cells[0].prefilled.is_empty(),
            "the last cell is left to write"
        );
        assert_eq!(fixes.len(), 1);
        // 差異化學習單: per version, always including 基礎版.
        let mut tiered = raw(
            "sequence",
            &[
                ("1", &[("a", false)]),
                ("2", &[("b", false)]),
                ("3", &[("c", false)]),
            ],
        );
        tiered.sections[0].cells[0].prefill_in = "all".into();
        tiered.sections[1].cells[0].prefill_in = "basic_standard".into();
        tiered.sections[2].cells[0].prefill_in = "basic".into();
        let (org, fixes) = from_raw(&tiered, true);
        assert_eq!(fixes.len(), 1, "基礎版 had every cell");
        let given = |l: Level| org.cells().filter(|c| c.given_in(l)).count();
        assert_eq!(
            (
                given(Level::Basic),
                given(Level::Standard),
                given(Level::Advanced)
            ),
            (2, 2, 1)
        );
    }

    #[test]
    fn students_see_given_cells_and_teachers_the_answers() {
        let (org, _) = from_raw(
            &raw(
                "t_chart",
                &[
                    ("Then", &[("dry", true), ("quiet", false)]),
                    ("Now", &[("wet", false)]),
                ],
            ),
            false,
        );
        let student = org.project(Level::Standard, false);
        assert_eq!(student.sections[0].cells[0].given, "dry");
        assert_eq!(student.sections[0].cells[1].given, "");
        assert!(student
            .sections
            .iter()
            .flat_map(|s| &s.cells)
            .all(|c| c.answer.is_none()));
        let teacher = org.project(Level::Standard, true);
        assert_eq!(
            teacher.sections[0].cells[1].answer.as_deref(),
            Some("quiet")
        );
        assert_eq!(
            teacher.sections[0].cells[0].answer, None,
            "a given cell needs no key"
        );
        assert!(!serde_json::to_string(&student).unwrap().contains("quiet"));
        assert_eq!(teacher.name, "T-chart");
    }

    #[test]
    fn a_saved_organizer_is_checked() {
        let mut org = Organizer::new(OrganizerKind::Sequence);
        org.sections[0].cells[0].prefilled = vec![Level::Advanced, Level::Basic, Level::Advanced];
        org.validate().unwrap();
        assert_eq!(
            org.sections[0].cells[0].prefilled,
            [Level::Basic, Level::Advanced]
        );
        org.sections = vec![OrgSection::default(); 7];
        assert!(org.validate().is_err());
        let mut long = Organizer::new(OrganizerKind::TChart);
        long.sections[0].cells[0].answer = "x".repeat(MAX_CELL_CHARS + 1);
        assert!(long.validate().is_err());
    }

    #[test]
    fn the_summary_names_the_type_and_every_label() {
        let (org, _) = from_raw(
            &raw(
                "cause_effect",
                &[
                    ("Cause", &[("Rain came", true)]),
                    ("Effect", &[("Umbrellas ran out", false)]),
                ],
            ),
            false,
        );
        let s = org.summary(true);
        assert!(s.starts_with("因果鏈（cause_effect）"), "{s}");
        assert!(
            s.contains("Cause：Rain came〔預填：基礎版、一般版、進階版〕"),
            "{s}"
        );
        assert!(s.contains("Effect：Umbrellas ran out"), "{s}");
        assert!(org.summary(false).contains("Rain came〔預填〕"));
    }
}
