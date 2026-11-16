//! 學習單產生器 (worksheet generator): the fixed option labels, the worksheet document and the
//! rules that turn the teacher's selections into what is generated. Contract: docs/API.md
//! 「Phase 2.3」; product rules: 《學習單產生器：教師選項與 SOP 整合規格》 §4–§9.
//!
//! The worksheet is one ordered list of blocks. The student and teacher versions of each level
//! (基礎版／一般版／進階版) are projections of it made in code ([`assemble::project`]): the three
//! levels share the objectives and the core items and differ only in which items and scaffolds
//! they show and how much room there is to answer (spec §5.4).

pub mod ai;
pub mod assemble;
pub mod bloom;
pub mod checks;
pub mod export;
pub mod fake;
pub mod material;
pub mod schemas;
pub mod store;
pub mod sync;
pub mod template;
pub mod vocab;

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::sop::{ExitKind, InfoFunction};
pub use bloom::{Bloom, Output};
pub use material::Material;

// ---------------------------------------------------------------- fixed labels (spec §5)
// These are display labels fixed by the spec: never rename, merge or split them.

pub const MATERIAL_TYPES: [&str; 5] =
    ["閱讀文本", "聽力素材", "影片素材", "圖像素材", "資料／數據"];

/// 學生任務, grouped only for display (spec §5.2, §6).
pub const STUDENT_TASK_GROUPS: [(&str, &[&str]); 5] = [
    (
        "語言與內容學習",
        &["理解內容", "字彙練習", "文法／句型練習"],
    ),
    (
        "思考與資訊處理",
        &[
            "比較／分類",
            "推論／預測",
            "評估／提出理由",
            "蒐集資料",
            "整理／分析資料",
        ],
    ),
    ("互動與合作", &["討論／分享", "小組共同完成"]),
    ("表達與成果", &["寫作", "製作成果", "成果發表"]),
    ("反思與評量", &["學習反思", "自我評量", "同儕互評"]),
];

pub fn student_tasks() -> Vec<&'static str> {
    STUDENT_TASK_GROUPS
        .iter()
        .flat_map(|(_, labels)| labels.iter().copied())
        .collect()
}

pub const ANSWER_FORMATS: [&str; 8] = [
    "填空題",
    "選擇題",
    "是非題",
    "配對題",
    "簡答題",
    "表格整理",
    "圖像組織",
    "段落寫作",
];
pub const FMT_BLANK: &str = "填空題";
pub const FMT_CHOICE: &str = "選擇題";
pub const FMT_TRUE_FALSE: &str = "是非題";
pub const FMT_MATCHING: &str = "配對題";
pub const FMT_SHORT: &str = "簡答題";
pub const FMT_TABLE: &str = "表格整理";
pub const FMT_ORGANIZER: &str = "圖像組織";
pub const FMT_PARAGRAPH: &str = "段落寫作";

pub const GEN_WARMUP: &str = "加入暖身活動";
pub const GEN_OBJECTIVES: &str = "顯示學習目標";
pub const GEN_GUIDING: &str = "提供引導問題";
pub const GEN_EXAMPLE: &str = "顯示作答示例";
pub const GEN_WORD_LIST: &str = "提供字詞表";
pub const GEN_FRAMES: &str = "提供句型框架";
/// 產生設定 without its seventh entry, 「產生基礎版／一般版／進階版」, which is the three
/// independently selectable versions in [`Settings::levels`].
pub const GENERATION_OPTIONS: [&str; 6] = [
    GEN_WARMUP,
    GEN_OBJECTIVES,
    GEN_GUIDING,
    GEN_EXAMPLE,
    GEN_WORD_LIST,
    GEN_FRAMES,
];

/// Labels the spec explicitly keeps out of the options (§5, §6, §10.2). Only used by tests and
/// documentation: they are rejected like any other string outside the sets.
pub const FORBIDDEN_LABELS: [&str; 7] = [
    "課綱議題",
    "SDGs",
    "SEL",
    "探究",
    "高層次思考",
    "專題任務",
    "合作活動",
];

// Scaffold kinds: the examples the spec lists for each of the three scaffold types (§3).
pub const KIND_GUIDING: &str = "引導問題";
pub const KIND_WORD_LIST: &str = "字詞表";
pub const KIND_FRAMES: &str = "句型框架";
pub const KIND_EXAMPLE: &str = "作答示例";
pub const VERBAL_KINDS: [&str; 8] = [
    "改述或重述",
    "思考放聲",
    "提示與追問",
    "透過上下文解釋詞義",
    KIND_GUIDING,
    KIND_WORD_LIST,
    KIND_FRAMES,
    KIND_EXAMPLE,
];
pub const INSTRUCTIONAL_KINDS: [&str; 4] = [
    "圖像組織",
    "部分完成的大綱或內容",
    "調整文本",
    "標示關鍵概念或字彙",
];
pub const KIND_STEPS: &str = "清楚的分步指示及完成順序";
pub const PROCEDURAL_KINDS: [&str; 4] = [
    KIND_STEPS,
    "個別或小組指導安排",
    "策略性配對或分組",
    "逐步減少支持",
];

pub const INFO_FUNCTIONS: [(InfoFunction, &str); 5] = [
    (InfoFunction::Discover, "發現資訊"),
    (InfoFunction::Organize, "整理資訊"),
    (InfoFunction::Analyze, "分析資訊"),
    (InfoFunction::Compare, "比較資訊"),
    (InfoFunction::Create, "創造資訊"),
];

pub fn info_function_label(f: InfoFunction) -> &'static str {
    INFO_FUNCTIONS
        .iter()
        .find(|(x, _)| *x == f)
        .map(|(_, l)| *l)
        .unwrap_or("")
}

pub fn info_function_from_label(label: &str) -> Option<InfoFunction> {
    INFO_FUNCTIONS
        .iter()
        .find(|(_, l)| *l == label)
        .map(|(f, _)| *f)
}

/// The first student-task label the spec relates to an information function (§8). A reference
/// relation for suggestions, never a restriction.
pub fn task_for_function(f: InfoFunction) -> &'static str {
    match f {
        InfoFunction::Discover => "理解內容",
        InfoFunction::Organize => "整理／分析資料",
        InfoFunction::Analyze => "推論／預測",
        InfoFunction::Compare => "比較／分類",
        InfoFunction::Create => "寫作",
    }
}

pub const EXIT_PURPOSES: [(ExitKind, &str); 3] = [
    (ExitKind::Formative, "形成性評量"),
    (ExitKind::Reflection, "學生反思"),
    (ExitKind::Questions, "蒐集問題"),
];

// ---------------------------------------------------------------- limits

pub const MAX_TITLE_CHARS: usize = 200;
pub const MAX_TEXT_CHARS: usize = 4_000;
pub const MAX_BLOCKS: usize = 60;
pub const MAX_ITEMS: usize = 60;
pub const MAX_LIST: usize = 60;
pub const MAX_ID_CHARS: usize = 40;
pub const MINUTES_RANGE: std::ops::RangeInclusive<i32> = 5..=240;
/// Above this the tasks are not attempted in one call (long arrays end early).
pub const MAX_AI_PARAGRAPHS: usize = 40;

// ---------------------------------------------------------------- enums

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Basic,
    Standard,
    Advanced,
}

impl Level {
    pub const ALL: [Level; 3] = [Level::Basic, Level::Standard, Level::Advanced];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Basic => "basic",
            Self::Standard => "standard",
            Self::Advanced => "advanced",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|l| l.as_str() == s)
    }

    /// The spec's label of the version.
    pub fn label(self) -> &'static str {
        match self {
            Self::Basic => "基礎版",
            Self::Standard => "一般版",
            Self::Advanced => "進階版",
        }
    }

    pub fn english(self) -> &'static str {
        match self {
            Self::Basic => "Basic",
            Self::Standard => "Standard",
            Self::Advanced => "Advanced",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Audience {
    Student,
    Teacher,
}

impl Audience {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "student" => Some(Self::Student),
            "teacher" => Some(Self::Teacher),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Student => "學生版",
            Self::Teacher => "教師參考版",
        }
    }
}

/// Who wrote something: the teacher, Manna, or Manna and then edited by the teacher.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    #[default]
    Teacher,
    Ai,
    AiEdited,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum BlockKind {
    Warmup,
    #[default]
    Activity,
    ExitTicket,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ScaffoldType {
    #[default]
    Verbal,
    Instructional,
    Procedural,
}

impl ScaffoldType {
    pub fn kinds(self) -> &'static [&'static str] {
        match self {
            Self::Verbal => &VERBAL_KINDS,
            Self::Instructional => &INSTRUCTIONAL_KINDS,
            Self::Procedural => &PROCEDURAL_KINDS,
        }
    }
}

// ---------------------------------------------------------------- settings

/// The teacher's selections in the four option blocks (all multi-select, all optional), the
/// selected versions and the optional 預計完成時間; what to make (`output`) and, for the outputs
/// made per Bloom level, the target levels and how many questions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Settings {
    pub material_types: Vec<String>,
    pub student_tasks: Vec<String>,
    pub answer_formats: Vec<String>,
    pub generation: Vec<String>,
    pub levels: Vec<Level>,
    pub minutes: Option<i32>,
    /// `worksheet` (the four-step pipeline), `comprehension` or `tiered`.
    pub output: Output,
    /// Target levels, in Bloom order; empty means all six.
    pub bloom: Vec<Bloom>,
    /// 3–30; `None` uses the output's default (8, tiered 12).
    pub question_count: Option<i32>,
    /// Explicit questions per level (each 0–12, 3–30 in all); overrides `bloom` and
    /// `question_count` when set.
    pub bloom_counts: Option<BTreeMap<Bloom, i32>>,
    /// The teacher's learning objective (≤300), sent to every prompt.
    pub objective: Option<String>,
}

/// A partial `Settings` (`POST /api/worksheets { settings }`, `PUT …/:id { settings }`): the
/// fields given replace the current ones, the others stay. `null` clears the optional ones.
#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct SettingsPatch {
    pub material_types: Option<Vec<String>>,
    pub student_tasks: Option<Vec<String>>,
    pub answer_formats: Option<Vec<String>>,
    pub generation: Option<Vec<String>>,
    pub levels: Option<Vec<Level>>,
    #[serde(deserialize_with = "crate::routes::double_option")]
    pub minutes: Option<Option<i32>>,
    pub output: Option<Output>,
    pub bloom: Option<Vec<Bloom>>,
    #[serde(deserialize_with = "crate::routes::double_option")]
    pub question_count: Option<Option<i32>>,
    #[serde(deserialize_with = "crate::routes::double_option")]
    pub bloom_counts: Option<Option<BTreeMap<Bloom, i32>>>,
    #[serde(deserialize_with = "crate::routes::double_option")]
    pub objective: Option<Option<String>>,
}

impl SettingsPatch {
    /// `base` with the given fields replaced (not yet validated: call `normalised`).
    pub fn apply(&self, base: &Settings) -> Settings {
        let mut s = base.clone();
        let take = |v: &Option<Vec<String>>, old: &mut Vec<String>| {
            if let Some(v) = v {
                *old = v.clone();
            }
        };
        take(&self.material_types, &mut s.material_types);
        take(&self.student_tasks, &mut s.student_tasks);
        take(&self.answer_formats, &mut s.answer_formats);
        take(&self.generation, &mut s.generation);
        if let Some(v) = &self.levels {
            s.levels = v.clone();
        }
        if let Some(v) = self.minutes {
            s.minutes = v;
        }
        if let Some(v) = self.output {
            s.output = v;
        }
        if let Some(v) = &self.bloom {
            s.bloom = v.clone();
        }
        if let Some(v) = self.question_count {
            s.question_count = v;
        }
        if let Some(v) = &self.bloom_counts {
            s.bloom_counts = v.clone();
        }
        if let Some(v) = &self.objective {
            s.objective = v.clone();
        }
        s
    }
}

/// Keeps `chosen` in the order of `allowed`, without duplicates; a value outside the set is an
/// error naming it.
fn normalise_labels(
    what: &str,
    chosen: &[String],
    allowed: &[&str],
) -> Result<Vec<String>, String> {
    if let Some(bad) = chosen.iter().find(|c| !allowed.contains(&c.as_str())) {
        return Err(format!(
            "「{}」不是「{what}」的選項。",
            crate::routes::clip_chars(bad, 40)
        ));
    }
    Ok(allowed
        .iter()
        .filter(|a| chosen.iter().any(|c| c == *a))
        .map(|a| a.to_string())
        .collect())
}

fn normalise_levels(levels: &[Level]) -> Vec<Level> {
    Level::ALL
        .into_iter()
        .filter(|l| levels.contains(l))
        .collect()
}

impl Settings {
    /// Validates against the spec's label sets and puts every list in the spec's order.
    pub fn normalised(&self) -> Result<Settings, String> {
        if let Some(m) = self.minutes {
            if !MINUTES_RANGE.contains(&m) {
                return Err(format!(
                    "預計完成時間需為 {} 到 {} 分鐘；也可以不填。",
                    MINUTES_RANGE.start(),
                    MINUTES_RANGE.end()
                ));
            }
        }
        if let Some(n) = self.question_count {
            if !bloom::QUESTION_COUNT_RANGE.contains(&n) {
                return Err(format!(
                    "題數需為 {} 到 {} 題；也可以不填。",
                    bloom::QUESTION_COUNT_RANGE.start(),
                    bloom::QUESTION_COUNT_RANGE.end()
                ));
            }
        }
        if let Some(counts) = &self.bloom_counts {
            bloom::check_counts(counts)?;
        }
        let objective = self
            .objective
            .as_deref()
            .map(str::trim)
            .filter(|o| !o.is_empty())
            .map(str::to_string);
        if objective
            .as_ref()
            .is_some_and(|o| o.chars().count() > bloom::MAX_OBJECTIVE_CHARS)
        {
            return Err(format!("學習目標最多 {} 字。", bloom::MAX_OBJECTIVE_CHARS));
        }
        Ok(Settings {
            material_types: normalise_labels("素材類型", &self.material_types, &MATERIAL_TYPES)?,
            student_tasks: normalise_labels("學生任務", &self.student_tasks, &student_tasks())?,
            answer_formats: normalise_labels("作答方式", &self.answer_formats, &ANSWER_FORMATS)?,
            generation: normalise_labels("產生設定", &self.generation, &GENERATION_OPTIONS)?,
            levels: normalise_levels(&self.levels),
            minutes: self.minutes,
            output: self.output,
            bloom: bloom::normalise_levels(&self.bloom),
            question_count: self.question_count,
            bloom_counts: self.bloom_counts.clone(),
            objective,
        })
    }

    /// The same settings with what says what to make (`output`, `bloom`, `question_count`,
    /// `bloom_counts`, `objective`) taken from `other`.
    pub fn with_focus_of(mut self, other: &Settings) -> Settings {
        self.output = other.output;
        self.bloom = other.bloom.clone();
        self.question_count = other.question_count;
        self.bloom_counts = other.bloom_counts.clone();
        self.objective = other.objective.clone();
        self
    }

    pub fn wants(&self, generation_option: &str) -> bool {
        self.generation.iter().any(|g| g == generation_option)
    }
}

// ---------------------------------------------------------------- brief (carried over from the SOP)

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct SeedTask {
    pub id: String,
    pub paragraph_ids: Vec<String>,
    pub function: Option<InfoFunction>,
    pub text: String,
    pub sop_block_id: Option<String>,
    pub sop_task_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct SeedExit {
    pub kind: ExitKind,
    pub text: String,
}

/// What a lesson's SOP put in front of the generator. Suggestions only: the teacher can edit or
/// remove every entry before generating.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Brief {
    pub tasks: Vec<SeedTask>,
    pub warmup: Vec<String>,
    pub vocabulary: Vec<String>,
    pub exit_ticket: Vec<SeedExit>,
}

impl Brief {
    pub fn validate(&self) -> Result<(), String> {
        if self.tasks.len() > MAX_LIST
            || self.warmup.len() > MAX_LIST
            || self.vocabulary.len() > MAX_LIST
            || self.exit_ticket.len() > MAX_LIST
        {
            return Err(format!("帶入的內容每一類最多 {MAX_LIST} 項。"));
        }
        let texts = self
            .tasks
            .iter()
            .map(|t| &t.text)
            .chain(&self.warmup)
            .chain(&self.vocabulary)
            .chain(self.exit_ticket.iter().map(|e| &e.text));
        for t in texts {
            if t.chars().count() > MAX_TEXT_CHARS {
                return Err(format!("帶入的內容每一項最多 {MAX_TEXT_CHARS} 字。"));
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------- document

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Objective {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: crate::sop::ObjectiveType,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub student_facing_en: String,
    #[serde(default)]
    pub origin: Origin,
    #[serde(default)]
    pub sop_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Vocab {
    pub word: String,
    /// Always looked up in `vocabulary_items` by the server; never taken from the model or from
    /// the client. `None`: not in the list, or no list imported.
    pub level: Option<u8>,
    pub definition: String,
    pub context: String,
    pub paragraph_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Item {
    pub id: String,
    pub answer_format: Option<String>,
    pub prompt: String,
    pub options: Vec<String>,
    /// Teacher-reference version only.
    pub answer: String,
    /// Teacher-reference version only.
    pub evidence: String,
    pub purpose: Option<ExitKind>,
    pub levels: Vec<Level>,
    pub origin: Origin,
    /// The question's Bloom level (`null`: not labelled). Teacher version only.
    pub bloom: Option<Bloom>,
    /// A hint or sentence starter, printed only in 基礎版.
    pub hint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Scaffold {
    pub id: String,
    #[serde(rename = "type")]
    pub kind_type: ScaffoldType,
    pub kind: String,
    pub content: String,
    pub steps: Vec<String>,
    pub levels: Vec<Level>,
    /// A procedural scaffold added by the system without the teacher asking (spec §3.3).
    pub auto: bool,
    pub origin: Origin,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Block {
    pub id: String,
    pub kind: BlockKind,
    pub enabled: bool,
    pub title: String,
    pub instructions: String,
    pub paragraph_ids: Vec<String>,
    pub info_function: Option<InfoFunction>,
    pub student_tasks: Vec<String>,
    pub objective_ids: Vec<String>,
    pub vocabulary: Vec<Vocab>,
    pub items: Vec<Item>,
    pub scaffolds: Vec<Scaffold>,
    /// Teacher-reference version only.
    pub teacher_notes: String,
    pub origin: Origin,
    /// Why this block has no generated content, when a step failed. Never blocks editing.
    pub message: Option<String>,
    pub sop_block_id: Option<String>,
    /// The template section this block was generated from (a worksheet made from a template):
    /// regenerating the block re-runs that section.
    pub template_section_id: Option<String>,
    /// The Bloom level this block was generated for (閱讀理解題／分層練習): regenerating the
    /// block re-runs that level.
    pub bloom: Option<Bloom>,
}

impl Default for Block {
    fn default() -> Self {
        Self {
            id: String::new(),
            kind: BlockKind::Activity,
            enabled: true,
            title: String::new(),
            instructions: String::new(),
            paragraph_ids: Vec::new(),
            info_function: None,
            student_tasks: Vec::new(),
            objective_ids: Vec::new(),
            vocabulary: Vec::new(),
            items: Vec::new(),
            scaffolds: Vec::new(),
            teacher_notes: String::new(),
            origin: Origin::Teacher,
            message: None,
            sop_block_id: None,
            template_section_id: None,
            bloom: None,
        }
    }
}

/// What the system actually used for a generation (spec §4.6: shown to the teacher).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Chosen {
    pub settings: Settings,
    /// Which parts the system decided: `material_types`, `student_tasks`, `answer_formats`,
    /// `generation`, `levels`, `minutes`.
    pub auto: Vec<String>,
    pub item_budget: usize,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Content {
    pub objectives: Vec<Objective>,
    pub show_objectives: bool,
    pub blocks: Vec<Block>,
    pub levels: Vec<Level>,
    pub chosen: Option<Chosen>,
    pub notices: Vec<String>,
}

impl Default for Content {
    fn default() -> Self {
        Self {
            objectives: Vec::new(),
            show_objectives: true,
            blocks: Vec::new(),
            levels: Vec::new(),
            chosen: None,
            notices: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------- validation of a saved document

fn check_text(what: &str, s: &str) -> Result<(), String> {
    if s.chars().count() > MAX_TEXT_CHARS {
        Err(format!("{what}最多 {MAX_TEXT_CHARS} 字。"))
    } else {
        Ok(())
    }
}

fn check_ids<'a>(what: &str, ids: impl Iterator<Item = &'a str>) -> Result<(), String> {
    let mut seen = HashSet::new();
    for id in ids {
        if id.trim().is_empty() || id.chars().count() > MAX_ID_CHARS {
            return Err(format!("{what}的 id 需為 1 到 {MAX_ID_CHARS} 個字元。"));
        }
        if !seen.insert(id) {
            return Err(format!("{what}的 id「{id}」重複。"));
        }
    }
    Ok(())
}

impl Content {
    /// Checks what a client sent and puts labels and levels in canonical order. Labels must come
    /// from the spec's sets here too, so an edit can never introduce another one.
    pub fn normalise(&mut self) -> Result<(), String> {
        if self.blocks.len() > MAX_BLOCKS {
            return Err(format!("一份學習單最多 {MAX_BLOCKS} 個區塊。"));
        }
        if self.objectives.len() > MAX_LIST {
            return Err(format!("學習目標最多 {MAX_LIST} 項。"));
        }
        self.levels = normalise_levels(&self.levels);
        check_ids("學習目標", self.objectives.iter().map(|o| o.id.as_str()))?;
        for o in &self.objectives {
            check_text("學習目標", &o.text)?;
            check_text("學習目標", &o.student_facing_en)?;
        }
        check_ids("區塊", self.blocks.iter().map(|b| b.id.as_str()))?;
        check_ids(
            "題目",
            self.blocks
                .iter()
                .flat_map(|b| b.items.iter().map(|i| i.id.as_str())),
        )?;
        let all_tasks = student_tasks();
        for b in &mut self.blocks {
            if b.items.len() > MAX_ITEMS
                || b.scaffolds.len() > MAX_LIST
                || b.vocabulary.len() > MAX_LIST
            {
                return Err("一個區塊裡的題目、鷹架或字詞太多了。".to_string());
            }
            check_text("區塊標題", &b.title)?;
            check_text("作答指示", &b.instructions)?;
            check_text("教師備註", &b.teacher_notes)?;
            if b.template_section_id
                .as_ref()
                .is_some_and(|id| id.chars().count() > MAX_ID_CHARS)
            {
                return Err(format!("版型區塊的 id 最多 {MAX_ID_CHARS} 個字元。"));
            }
            b.student_tasks = normalise_labels("學生任務", &b.student_tasks, &all_tasks)?;
            check_ids("鷹架", b.scaffolds.iter().map(|s| s.id.as_str()))?;
            for i in &mut b.items {
                check_text("題目", &i.prompt)?;
                check_text("參考答案", &i.answer)?;
                check_text("依據", &i.evidence)?;
                check_text("提示", &i.hint)?;
                if i.options.len() > 12 {
                    return Err("一題最多 12 個選項。".to_string());
                }
                for o in &i.options {
                    check_text("選項", o)?;
                }
                if let Some(f) = &i.answer_format {
                    if !ANSWER_FORMATS.contains(&f.as_str()) {
                        return Err(format!(
                            "「{}」不是「作答方式」的選項。",
                            crate::routes::clip_chars(f, 40)
                        ));
                    }
                }
                i.levels = normalise_levels(&i.levels);
            }
            for s in &mut b.scaffolds {
                check_text("鷹架", &s.content)?;
                if s.steps.len() > 12 {
                    return Err("一個程序鷹架最多 12 個步驟。".to_string());
                }
                for step in &s.steps {
                    check_text("步驟", step)?;
                }
                if !s.kind_type.kinds().contains(&s.kind.as_str()) {
                    return Err(format!(
                        "「{}」不是這一類鷹架的種類。",
                        crate::routes::clip_chars(&s.kind, 40)
                    ));
                }
                s.levels = normalise_levels(&s.levels);
            }
            for v in &b.vocabulary {
                if v.word.chars().count() > 80 {
                    return Err("字詞最多 80 個字元。".to_string());
                }
                check_text("字詞解釋", &v.definition)?;
                check_text("字詞例句", &v.context)?;
            }
        }
        if let Some(chosen) = &mut self.chosen {
            chosen.settings = chosen.settings.normalised()?;
        }
        if self.notices.len() > MAX_LIST {
            self.notices.truncate(MAX_LIST);
        }
        Ok(())
    }

    pub fn block(&self, id: &str) -> Option<&Block> {
        self.blocks.iter().find(|b| b.id == id)
    }
}

// ---------------------------------------------------------------- suggestions and system choices

/// Share of digit characters above which a pasted text is also suggested as 資料／數據.
const DATA_DIGIT_SHARE: f64 = 0.06;

fn looks_like_data(material: &Material) -> bool {
    let text = material.text();
    let total = text.chars().filter(|c| !c.is_whitespace()).count();
    let digits = text.chars().filter(char::is_ascii_digit).count();
    total >= 80 && digits as f64 / total as f64 >= DATA_DIGIT_SHARE
}

fn material_types_for(material: &Material) -> Vec<String> {
    if material.paragraphs.is_empty() {
        return Vec::new();
    }
    let mut types = vec![MATERIAL_TYPES[0].to_string()];
    if looks_like_data(material) {
        types.push(MATERIAL_TYPES[4].to_string());
    }
    types
}

/// The starting selections for a worksheet made directly from pasted material: the material
/// type judged from the text, plus the common settings. All of it can be unticked.
pub fn suggest_direct(material: &Material) -> Settings {
    Settings {
        material_types: material_types_for(material),
        student_tasks: vec!["理解內容".to_string()],
        answer_formats: Vec::new(),
        generation: vec![
            GEN_WARMUP.to_string(),
            GEN_WORD_LIST.to_string(),
            GEN_FRAMES.to_string(),
        ],
        levels: vec![Level::Standard],
        minutes: None,
        ..Settings::default()
    }
}

/// The starting selections for a worksheet made from a lesson: derived from what the SOP
/// already contains (spec §4.1, §7, §8).
pub fn suggest_from_sop(material: &Material, sop: &crate::sop::SopContent) -> Settings {
    let mut tasks: Vec<String> = Vec::new();
    let mut verbal = false;
    for t in sop.task_blocks.iter().flat_map(|b| &b.tasks) {
        if let Some(f) = t.function {
            tasks.push(task_for_function(f).to_string());
        }
        verbal |= !t.scaffolds.verbal.trim().is_empty();
    }
    let has_tasks = sop
        .task_blocks
        .iter()
        .flat_map(|b| &b.tasks)
        .any(|t| !t.text.trim().is_empty());
    if has_tasks && tasks.is_empty() {
        tasks.push("理解內容".to_string());
    }
    let mut generation: Vec<String> = Vec::new();
    let has_warmup = sop
        .warmup
        .background
        .iter()
        .any(|a| !a.text.trim().is_empty())
        || sop
            .warmup
            .vocabulary
            .iter()
            .any(|v| !v.word.trim().is_empty());
    if has_warmup {
        generation.push(GEN_WARMUP.to_string());
    }
    if sop.objectives.iter().any(|o| !o.text.trim().is_empty()) {
        generation.push(GEN_OBJECTIVES.to_string());
    }
    if verbal {
        generation.push(GEN_GUIDING.to_string());
        generation.push(GEN_FRAMES.to_string());
    }
    if sop
        .warmup
        .vocabulary
        .iter()
        .any(|v| !v.word.trim().is_empty())
    {
        generation.push(GEN_WORD_LIST.to_string());
    }
    let settings = Settings {
        material_types: material_types_for(material),
        student_tasks: tasks,
        answer_formats: Vec::new(),
        generation,
        levels: vec![Level::Standard],
        minutes: None,
        ..Settings::default()
    };
    settings.normalised().unwrap_or_default()
}

/// Core main-task items for the time available (spec §9.2): after five minutes for a warm-up
/// and three for the exit ticket, one item per 2.5 minutes. Without a time, two per paragraph.
pub fn item_budget(minutes: Option<i32>, paragraphs: usize, warmup: bool) -> usize {
    match minutes {
        Some(m) => {
            let main = (m - if warmup { 5 } else { 0 } - 3).max(5) as f64;
            ((main / 2.5).round() as usize).clamp(3, 24)
        }
        None => (paragraphs * 2).clamp(4, 14),
    }
}

/// Key words in the warm-up and items in the exit ticket also follow the time.
pub fn vocab_count(minutes: Option<i32>) -> usize {
    match minutes {
        Some(m) if m <= 20 => 4,
        Some(m) if m <= 35 => 6,
        _ => 8,
    }
}

pub fn exit_count(minutes: Option<i32>) -> usize {
    match minutes {
        Some(m) if m <= 20 => 2,
        _ => 3,
    }
}

const MIXED_ABILITY_HINTS: [&str; 6] = ["落差", "差異", "程度不一", "補救", "基礎較弱", "低成就"];

/// Fills in every option block the teacher left empty (spec §4.6) and says what was chosen.
/// `hard_share`: share of the material's listed words above the default target level, when a
/// word list is available.
pub fn resolve(
    settings: &Settings,
    material: &Material,
    has_objectives: bool,
    class_profile: &str,
    hard_share: Option<f64>,
) -> Chosen {
    let mut s = settings.clone();
    let mut auto = Vec::new();
    let mut reasons = Vec::new();
    if s.material_types.is_empty() {
        s.material_types = material_types_for(material);
        auto.push("material_types".to_string());
        reasons.push(format!(
            "素材類型：依教材內容判斷為「{}」。",
            s.material_types.join("、")
        ));
    }
    if s.student_tasks.is_empty() {
        auto.push("student_tasks".to_string());
        reasons.push("學生任務：由 Manna 依各段內容設計，實際用到的列在這裡。".to_string());
    }
    if s.answer_formats.is_empty() {
        auto.push("answer_formats".to_string());
        reasons.push("作答方式：由 Manna 依題目性質搭配，實際用到的列在這裡。".to_string());
    }
    if s.generation.is_empty() {
        s.generation = vec![GEN_WARMUP.to_string()];
        if has_objectives {
            s.generation.push(GEN_OBJECTIVES.to_string());
        }
        s.generation.push(GEN_WORD_LIST.to_string());
        s.generation.push(GEN_FRAMES.to_string());
        auto.push("generation".to_string());
        reasons.push(format!(
            "產生設定：使用常用的組合「{}」。",
            s.generation.join("、")
        ));
    }
    if s.levels.is_empty() {
        s.levels = vec![Level::Standard];
        let mixed = MIXED_ABILITY_HINTS
            .iter()
            .any(|h| class_profile.contains(h));
        let hard = hard_share.is_some_and(|h| h >= 0.08);
        if mixed || hard {
            s.levels.insert(0, Level::Basic);
        }
        auto.push("levels".to_string());
        reasons.push(if mixed {
            "版本：班級特性提到程度差異，所以產生基礎版與一般版。".to_string()
        } else if hard {
            "版本：教材中超出第 4 級的詞偏多，所以產生基礎版與一般版。".to_string()
        } else {
            "版本：預設產生一般版。".to_string()
        });
    }
    let warmup = s.wants(GEN_WARMUP);
    let budget = item_budget(s.minutes, material.paragraphs.len(), warmup);
    if s.minutes.is_none() {
        auto.push("minutes".to_string());
        reasons.push(format!(
            "份量：沒有填預計完成時間，依教材 {} 段安排約 {budget} 題核心題。",
            material.paragraphs.len()
        ));
    } else {
        reasons.push(format!(
            "份量：依預計 {} 分鐘安排約 {budget} 題核心題。",
            s.minutes.unwrap_or_default()
        ));
    }
    Chosen {
        settings: s,
        auto,
        item_budget: budget,
        reasons,
    }
}

// ---------------------------------------------------------------- titles and file names

/// The worksheet's printed title: the material's title when it has one.
pub fn auto_title(material: &Material) -> String {
    let t = material.title.trim();
    if t.is_empty() {
        "Worksheet".to_string()
    } else {
        format!("{t}: Worksheet")
    }
}

/// Characters a file name may not contain on common systems are replaced.
pub fn safe_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\n' | '\r' | '\t' => ' ',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    let cleaned = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let cleaned: String = cleaned.trim_matches('.').chars().take(80).collect();
    if cleaned.trim().is_empty() {
        "學習單".to_string()
    } else {
        cleaned.trim().to_string()
    }
}

pub fn auto_filename(material: &Material, lesson_title: Option<&str>) -> String {
    let base = [
        material.title.trim(),
        lesson_title.unwrap_or_default().trim(),
    ]
    .into_iter()
    .find(|s| !s.is_empty())
    .unwrap_or("");
    if base.is_empty() {
        "學習單".to_string()
    } else {
        safe_filename(&format!("{base} 學習單"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn label_sets_match_the_spec() {
        assert_eq!(MATERIAL_TYPES.len(), 5);
        assert_eq!(student_tasks().len(), 16);
        assert_eq!(STUDENT_TASK_GROUPS.len(), 5);
        assert_eq!(ANSWER_FORMATS.len(), 8);
        assert_eq!(GENERATION_OPTIONS.len(), 6);
        let all: Vec<&str> = MATERIAL_TYPES
            .iter()
            .copied()
            .chain(student_tasks())
            .chain(ANSWER_FORMATS)
            .chain(GENERATION_OPTIONS)
            .collect();
        for forbidden in FORBIDDEN_LABELS {
            assert!(
                !all.contains(&forbidden),
                "{forbidden} must not be an option"
            );
        }
    }

    #[test]
    fn settings_reject_labels_outside_the_sets_and_keep_spec_order() {
        let s = Settings {
            student_tasks: labels(&["寫作", "理解內容", "寫作"]),
            levels: vec![Level::Advanced, Level::Basic],
            ..Settings::default()
        };
        let n = s.normalised().unwrap();
        assert_eq!(n.student_tasks, labels(&["理解內容", "寫作"]));
        assert_eq!(n.levels, vec![Level::Basic, Level::Advanced]);
        for forbidden in FORBIDDEN_LABELS {
            let s = Settings {
                student_tasks: labels(&[forbidden]),
                ..Settings::default()
            };
            assert!(s.normalised().is_err(), "{forbidden}");
        }
        let s = Settings {
            minutes: Some(3),
            ..Settings::default()
        };
        assert!(s.normalised().is_err());
    }

    #[test]
    fn bloom_settings_are_validated_and_patched() {
        let s = Settings {
            output: Output::Comprehension,
            bloom: vec![Bloom::Create, Bloom::Remember, Bloom::Create],
            question_count: Some(10),
            objective: Some("  Students can explain the writer's view.  ".into()),
            ..Settings::default()
        };
        let n = s.normalised().unwrap();
        assert_eq!(n.bloom, vec![Bloom::Remember, Bloom::Create]);
        assert_eq!(
            n.objective.as_deref(),
            Some("Students can explain the writer's view.")
        );
        for bad in [2, 31] {
            let s = Settings {
                question_count: Some(bad),
                ..Settings::default()
            };
            assert!(s.normalised().unwrap_err().contains("題數"), "{bad}");
        }
        let s = Settings {
            objective: Some("o".repeat(301)),
            ..Settings::default()
        };
        assert!(s.normalised().unwrap_err().contains("學習目標"));
        let s = Settings {
            objective: Some("   ".into()),
            ..Settings::default()
        };
        assert_eq!(s.normalised().unwrap().objective, None);
        let s = Settings {
            bloom_counts: Some(BTreeMap::from([(Bloom::Apply, 1), (Bloom::Create, 1)])),
            ..Settings::default()
        };
        assert!(s.normalised().is_err(), "2 in all");

        // A patch replaces what it names; null clears an optional field.
        let patch: SettingsPatch = serde_json::from_value(serde_json::json!({
            "output": "tiered", "question_count": null, "bloom_counts": { "analyze": 4 },
        }))
        .unwrap();
        let p = patch.apply(&n);
        assert_eq!(p.output, Output::Tiered);
        assert_eq!(p.question_count, None);
        assert_eq!(p.bloom, n.bloom, "not named, kept");
        assert_eq!(p.objective, n.objective);
        assert_eq!(p.bloom_counts, Some(BTreeMap::from([(Bloom::Analyze, 4)])));
        let err =
            serde_json::from_value::<SettingsPatch>(serde_json::json!({ "bloom": ["think"] }))
                .unwrap_err();
        assert!(err.to_string().contains("Bloom"));
        assert!(
            serde_json::from_value::<SettingsPatch>(serde_json::json!({ "output": "quiz" }))
                .is_err()
        );
    }

    #[test]
    fn amount_follows_the_time() {
        assert!(item_budget(Some(10), 4, true) < item_budget(Some(50), 4, true));
        assert_eq!(item_budget(Some(10), 4, true), 3);
        assert_eq!(item_budget(None, 4, true), 8);
        assert_eq!(item_budget(None, 1, false), 4);
        assert!(vocab_count(Some(15)) < vocab_count(Some(50)));
        assert_eq!(exit_count(Some(15)), 2);
    }

    #[test]
    fn empty_selections_are_filled_in_and_reported() {
        let material = Material::pasted(None, 0, "T", "One paragraph here.\n\nAnother one.", "").0;
        let c = resolve(&Settings::default(), &material, false, "", None);
        assert_eq!(c.settings.material_types, labels(&["閱讀文本"]));
        assert!(c.settings.wants(GEN_WARMUP) && !c.settings.wants(GEN_OBJECTIVES));
        assert_eq!(c.settings.levels, vec![Level::Standard]);
        for part in [
            "material_types",
            "student_tasks",
            "answer_formats",
            "generation",
            "levels",
            "minutes",
        ] {
            assert!(c.auto.iter().any(|a| a == part), "{part}");
        }
        let c = resolve(&Settings::default(), &material, true, "程度落差大", None);
        assert_eq!(c.settings.levels, vec![Level::Basic, Level::Standard]);
        // What the teacher chose is never replaced.
        let own = Settings {
            levels: vec![Level::Advanced],
            generation: labels(&[GEN_FRAMES]),
            ..Settings::default()
        };
        let c = resolve(&own, &material, true, "程度落差大", None);
        assert_eq!(c.settings.levels, vec![Level::Advanced]);
        assert_eq!(c.settings.generation, labels(&[GEN_FRAMES]));
        assert!(!c.auto.iter().any(|a| a == "levels" || a == "generation"));
    }

    #[test]
    fn file_names_are_safe() {
        assert_eq!(safe_filename("a/b:c?.docx"), "a b c .docx");
        assert_eq!(safe_filename("  "), "學習單");
    }
}
