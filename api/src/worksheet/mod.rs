//! 學習單產生器 (worksheet generator): the fixed option labels, the worksheet document and the
//! rules that turn the teacher's selections into what is generated. Contract: docs/API.md
//! 「Phase 2.3」; product rules: 《學習單產生器：教師選項與 SOP 整合規格》 §4–§9.
//!
//! The worksheet is one ordered list of blocks. The student and teacher versions of each level
//! (基礎版／一般版／進階版) are projections of it made in code ([`assemble::project`]): the three
//! levels share the objectives and the core items and differ only in which items and scaffolds
//! they show and how much room there is to answer (spec §5.4). In a 差異化學習單 every question is
//! in all three and carries its own support and answer per version ([`Item::tiers`]).

pub mod ai;
pub mod assemble;
pub mod bloom;
pub mod checks;
pub mod comments;
pub mod export;
pub mod fake;
pub mod history;
pub mod images;
pub mod key_vocab;
pub mod material;
pub mod organizer;
pub mod revise;
pub mod schemas;
pub mod store;
pub mod sync;
pub mod table;
pub mod task_words;
pub mod template;
pub mod thumbnail;
pub mod tiered;
pub mod vocab;
pub mod vocab_actions;

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::sop::{ExitKind, InfoFunction};
pub use bloom::{Bloom, Output};
pub use material::Material;
pub use organizer::{Organizer, OrganizerKind};
pub use tiered::{ItemTier, SupportKind};

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
/// More paragraphs than this is longer than one lesson: the web app says so before generating
/// (a notice with 修改課文, 只選要用的段落 and 照樣產生). Nothing is refused or cut because of it.
pub const LONG_MATERIAL_PARAGRAPHS: usize = 40;

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
    /// 內容目標 (≤300; one objective per line), the teacher's own or a draft she adopted. A row
    /// stored before the two fields existed has one `objective`: it is read as the 內容目標.
    #[serde(alias = "objective")]
    pub content_objective: Option<String>,
    /// 語言目標 (≤300; one objective per line).
    pub language_objective: Option<String>,
    /// The sheet has no exit ticket (a lesson's SOP turned it off). False: it ends with one.
    pub skip_exit_ticket: bool,
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
    #[serde(deserialize_with = "crate::serde_support::double_option")]
    pub minutes: Option<Option<i32>>,
    pub output: Option<Output>,
    pub bloom: Option<Vec<Bloom>>,
    #[serde(deserialize_with = "crate::serde_support::double_option")]
    pub question_count: Option<Option<i32>>,
    #[serde(deserialize_with = "crate::serde_support::double_option")]
    pub bloom_counts: Option<Option<BTreeMap<Bloom, i32>>>,
    /// `objective` (the single field before 內容目標 and 語言目標) is the 內容目標.
    #[serde(
        alias = "objective",
        deserialize_with = "crate::serde_support::double_option"
    )]
    pub content_objective: Option<Option<String>>,
    #[serde(deserialize_with = "crate::serde_support::double_option")]
    pub language_objective: Option<Option<String>>,
    pub skip_exit_ticket: Option<bool>,
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
        if let Some(v) = &self.content_objective {
            s.content_objective = v.clone();
        }
        if let Some(v) = &self.language_objective {
            s.language_objective = v.clone();
        }
        if let Some(v) = self.skip_exit_ticket {
            s.skip_exit_ticket = v;
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
            crate::text::clip_chars(bad, 40)
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
        let content_objective =
            crate::objectives::normalised_field("內容目標", self.content_objective.as_deref())?;
        let language_objective =
            crate::objectives::normalised_field("語言目標", self.language_objective.as_deref())?;
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
            content_objective,
            language_objective,
            skip_exit_ticket: self.skip_exit_ticket,
        })
    }

    /// The same settings with what says what to make (`output`, `bloom`, `question_count`,
    /// `bloom_counts`, the two objectives) taken from `other`.
    pub fn with_focus_of(mut self, other: &Settings) -> Settings {
        self.output = other.output;
        self.bloom = other.bloom.clone();
        self.question_count = other.question_count;
        self.bloom_counts = other.bloom_counts.clone();
        self.content_objective = other.content_objective.clone();
        self.language_objective = other.language_objective.clone();
        self
    }

    /// The teacher's 內容目標 and 語言目標, one line per objective.
    pub fn teacher_objectives(&self) -> crate::objectives::TeacherObjectives {
        crate::objectives::TeacherObjectives::from_fields(
            self.content_objective.as_deref(),
            self.language_objective.as_deref(),
        )
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

/// Which language the Key Vocabulary table prints its meanings in (one switch per worksheet).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum VocabLang {
    /// English meanings (the default).
    #[default]
    En,
    /// Chinese meanings (a word without one keeps its English meaning).
    Zh,
}

impl VocabLang {
    pub fn is_en(&self) -> bool {
        *self == Self::En
    }
}

/// A row of the Key Vocabulary table. Both meanings are kept, so switching the table's language
/// loses neither: `definition` is the English one, `meaning_zh` the Traditional Chinese one.
#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct Vocab {
    pub word: String,
    /// Always looked up in `vocabulary_items` by the server; never taken from the model or from
    /// the client. `None`: not in the list, or no list imported.
    pub level: Option<u8>,
    /// The English meaning.
    pub definition: String,
    pub context: String,
    pub paragraph_id: Option<String>,
    /// A word of the instructions (compare, support your answer …) listed by the code with a
    /// fixed English definition and Chinese hint (SIOP V2, [`task_words`]); not a key word of
    /// the text.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub task_word: bool,
    /// Part of speech as the word is used here (`n.`, `v.`, `adj.` …, see [`key_vocab`]); empty
    /// when unknown (a worksheet made before this existed).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub pos: String,
    /// The short Traditional Chinese meaning; empty when none was written yet (the English one
    /// is printed instead).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub meaning_zh: String,
}

/// What is stored (and what the editor sends): [`Vocab`] before old content is read correctly.
#[derive(Deserialize, Default)]
#[serde(default)]
struct VocabWire {
    word: String,
    level: Option<u8>,
    definition: String,
    context: String,
    paragraph_id: Option<String>,
    task_word: bool,
    pos: String,
    meaning_zh: String,
}

impl From<VocabWire> for Vocab {
    /// Old content: before the two languages were kept apart, a task word's `definition` was
    /// its Chinese hint. It moves to `meaning_zh` and the fixed English definition takes its
    /// place, so every reader sees the new shape.
    fn from(w: VocabWire) -> Self {
        let mut v = Vocab {
            word: w.word,
            level: w.level,
            definition: w.definition,
            context: w.context,
            paragraph_id: w.paragraph_id,
            task_word: w.task_word,
            pos: w.pos,
            meaning_zh: w.meaning_zh,
        };
        if v.task_word {
            let fixed = task_words::lookup(&v.word);
            if v.meaning_zh.trim().is_empty() && key_vocab::has_han(&v.definition) {
                v.meaning_zh = std::mem::take(&mut v.definition);
            }
            if let Some(t) = fixed {
                if v.definition.trim().is_empty() {
                    v.definition = t.definition_en.to_string();
                }
                if v.pos.trim().is_empty() {
                    v.pos = t.pos.to_string();
                }
            }
        }
        v
    }
}

impl<'de> Deserialize<'de> for Vocab {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        VocabWire::deserialize(d).map(Vocab::from)
    }
}

impl Vocab {
    /// The meaning printed in `lang`: the Chinese one when asked for and written, else the
    /// English one; and the other way round when the English one is empty.
    pub fn meaning(&self, lang: VocabLang) -> &str {
        let (en, zh) = (self.definition.trim(), self.meaning_zh.trim());
        match lang {
            VocabLang::Zh if !zh.is_empty() => zh,
            _ if !en.is_empty() => en,
            _ => zh,
        }
    }

    /// Whether the Chinese meaning is still to be written.
    pub fn lacks_zh(&self) -> bool {
        self.meaning_zh.trim().is_empty()
    }
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
    /// A hint or sentence starter, printed only in 基礎版 (閱讀理解題, and 分層練習 made before
    /// the 差異化學習單 redesign).
    pub hint: String,
    /// 語言鷹架：給這一題的句型框架（SIOP Q2）, a short paragraph the student can say or write
    /// word for word with `___` for the blanks («Hello, everyone. My answer is ___ because ___.
    /// Thank you.»), printed under the question (`assemble::project`). Empty (every question
    /// made before, and every question that needs no spoken or written answer): none. Absent
    /// from the JSON when empty.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub frame: String,
    /// 差異化學習單: the question's support, answer and scoring note per version (T3–T6); empty
    /// for every other question, which prints `hint` and `answer` as before.
    pub tiers: Vec<ItemTier>,
    /// 表格整理: the table students fill in. `None` (every generated question): the table its
    /// prompt names ([`ItemTable::for_prompt`]). Kept when the format changes; printed only for
    /// 表格整理.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table: Option<ItemTable>,
    /// Choices set aside when the teacher made the question a 是非題 (True／False took their
    /// place): changing it back to 選擇題 or 配對題 brings them back. Never printed.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub kept_options: Vec<String>,
    /// 圖像組織: the organizer the sheet draws (printed only while `answer_format` is 圖像組織).
    /// A question without one (made before organizers, or by hand) prints an empty box.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub organizer: Option<Organizer>,
}

impl Item {
    /// The question's part for one version of a 差異化學習單, if it has one.
    pub fn tier(&self, level: Level) -> Option<&ItemTier> {
        self.tiers.iter().find(|t| t.level == level)
    }

    /// The table a 表格整理 question prints: its own, or the one its prompt names.
    ///
    /// A placeholder heading an older worksheet may carry (「Column 1」) never prints: the headings
    /// the prompt names take its place, else it prints empty ([`ItemTable::without_placeholders`]).
    pub fn table_or_default(&self) -> ItemTable {
        self.table
            .clone()
            .unwrap_or_else(|| ItemTable::for_prompt(&self.prompt))
            .without_placeholders(&self.prompt)
    }

    /// The organizer the sheet draws: the question's, while its format is 圖像組織.
    pub fn printed_organizer(&self) -> Option<&Organizer> {
        self.organizer
            .as_ref()
            .filter(|_| self.answer_format.as_deref() == Some(FMT_ORGANIZER))
    }
}

/// Whether a question of this format prints its `options` (choices to pick from): 選擇題, 是非題,
/// 配對題, and a question without a format (older data). Every other format keeps the options it
/// may have (the teacher can switch back) without printing them.
pub fn uses_choices(format: Option<&str>) -> bool {
    match format {
        None => true,
        Some(f) => [FMT_CHOICE, FMT_TRUE_FALSE, FMT_MATCHING].contains(&f),
    }
}

pub const MAX_TABLE_COLUMNS: usize = 6;
pub const MAX_TABLE_ROWS: u8 = 12;
/// Empty rows under the headings of a table that has no size of its own (what the sheet has
/// always printed).
pub const TABLE_ROWS: u8 = 3;
const TABLE_COLUMN_CHARS: usize = 40;

/// The answer table of a 表格整理 question: the column headings (printed as the first row when
/// any is filled in) and the number of empty rows for the answers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ItemTable {
    #[serde(default)]
    pub columns: Vec<String>,
    #[serde(default = "table_rows")]
    pub rows: u8,
    /// What each row asks the student to write (「Claim」, 「Supporting detail」): printed as a
    /// first column under the headings. Empty (older tables, a table the teacher cleared): the
    /// rows are free. Row `i` has `row_labels[i]`; rows past the end of the list have none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub row_labels: Vec<String>,
}

fn table_rows() -> u8 {
    TABLE_ROWS
}

impl ItemTable {
    /// The table with its placeholder headings (「Column 1」, 「Header」) replaced: all headings
    /// by the ones the question names, else only the placeholders by empty ones. Empty
    /// headings are the teacher's own choice and stay.
    pub fn without_placeholders(mut self, prompt: &str) -> Self {
        let is_bad = |c: &String| !c.trim().is_empty() && table::is_placeholder(c);
        if self.columns.iter().any(is_bad) {
            match columns_from_prompt(prompt).or_else(|| table::columns_named_in_prompt(prompt)) {
                Some(named) => self.columns = named,
                None => {
                    for c in self.columns.iter_mut().filter(|c| is_bad(c)) {
                        c.clear();
                    }
                }
            }
        }
        self
    }

    /// The table of a question that has none of its own. The generator names the columns in the
    /// prompt (「Complete the table: Who? / What happened? / Why?」); without such a list, two
    /// columns without headings (the grid the sheet has always printed).
    pub fn for_prompt(prompt: &str) -> Self {
        Self {
            columns: columns_from_prompt(prompt)
                .or_else(|| table::columns_named_in_prompt(prompt))
                .unwrap_or_else(|| vec![String::new(); 2]),
            rows: TABLE_ROWS,
            row_labels: Vec::new(),
        }
    }
}

/// The columns a prompt lists after its last colon, separated by 「/」 or 「|」: two to six short
/// headings, else `None`. A closing 「(Question 3)」 is not part of the list. The editor reads
/// prompts the same way (`columnsFromPrompt` in web/src/lib/worksheet.ts).
pub fn columns_from_prompt(prompt: &str) -> Option<Vec<String>> {
    let mut s = prompt.trim();
    if s.ends_with(')') {
        if let Some(i) = s.rfind('(') {
            s = s[..i].trim_end();
        }
    }
    let (_, list) = s.rsplit_once([':', '：'])?;
    let parts: Vec<String> = list
        .split(['/', '|', '／'])
        .map(|p| p.trim().trim_end_matches(['.', '。']).trim().to_string())
        .collect();
    let fits = (2..=MAX_TABLE_COLUMNS).contains(&parts.len())
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.chars().count() <= TABLE_COLUMN_CHARS
                && p.split_whitespace().count() <= 6
        });
    fits.then_some(parts)
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
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
    /// The heading printed over it, when it is not the kind's own English heading
    /// (`assemble::support_heading`): a block made from a template prints the sample's own
    /// headings ("Directions", "Sentence Frame for Sharing"). A procedural scaffold with a
    /// heading prints as a box of lines under it instead of the block's numbered steps. Absent
    /// from the JSON when there is none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heading: Option<String>,
    /// A 字詞表 (「Word bank」) scaffold made of plain words: the words with their part of speech
    /// and meanings, and which of them print their meaning. `content` is the same words as a
    /// comma-separated list (kept in step with `bank`, so every reader of `content` still
    /// works). `None`: not a word bank, or one with text that is not a plain list of words
    /// (「Linking words for …: …」), which prints as it is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bank: Option<WordBank>,
}

/// One word of a word bank.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct BankWord {
    pub word: String,
    /// Part of speech (one of `key_vocab::POS_SET`); empty when not known yet.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub pos: String,
    /// The English meaning.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub definition: String,
    /// The Traditional Chinese meaning.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub meaning_zh: String,
    /// The teacher ticked it: the sheet prints this word's meaning (in the bank's language).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub show_meaning: bool,
}

/// The words of a word bank and the language of the meanings that print.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct WordBank {
    #[serde(skip_serializing_if = "VocabLang::is_en")]
    pub lang: VocabLang,
    pub words: Vec<BankWord>,
}

/// Most words a word bank holds.
pub const MAX_BANK_WORDS: usize = 40;

/// The words of a word bank's text when it is a plain list (`reason, result, however`, with or
/// without a leading 「Word bank:」); `None` for any other text (a lead sentence, a colon in the
/// middle, long entries).
pub fn parse_word_list(text: &str) -> Option<Vec<String>> {
    let mut text = text.trim();
    if let Some(head) = text.get(..10) {
        if head.eq_ignore_ascii_case("word bank:") {
            text = text[10..].trim();
        }
    }
    if text.is_empty() || text.contains([':', '：']) {
        return None;
    }
    let mut words: Vec<String> = Vec::new();
    for raw in text.split([',', '，', '、', ';', '；', '\n', '/']) {
        let w = raw.trim().trim_end_matches('.').trim();
        if w.is_empty() {
            continue;
        }
        let plain = w
            .chars()
            .all(|c| c.is_alphabetic() && c.is_ascii() || matches!(c, ' ' | '-' | '\'' | '’'));
        if !plain || w.split_whitespace().count() > 4 || w.chars().count() > 40 {
            return None;
        }
        if !words.iter().any(|x| x.eq_ignore_ascii_case(w)) {
            words.push(w.to_string());
        }
    }
    // One long entry is a sentence, not a list.
    if words.len() == 1 && words[0].split_whitespace().count() > 2 {
        return None;
    }
    (!words.is_empty() && words.len() <= MAX_BANK_WORDS).then_some(words)
}

/// What is stored (and what the editor sends): [`Scaffold`] before its word bank is read.
#[derive(Deserialize)]
#[serde(default)]
struct ScaffoldWire {
    id: String,
    #[serde(rename = "type")]
    kind_type: ScaffoldType,
    kind: String,
    content: String,
    steps: Vec<String>,
    levels: Vec<Level>,
    auto: bool,
    origin: Origin,
    heading: Option<String>,
    bank: Option<WordBank>,
}

impl Default for ScaffoldWire {
    fn default() -> Self {
        let s = Scaffold::default();
        Self {
            id: s.id,
            kind_type: s.kind_type,
            kind: s.kind,
            content: s.content,
            steps: s.steps,
            levels: s.levels,
            auto: s.auto,
            origin: s.origin,
            heading: s.heading,
            bank: None,
        }
    }
}

impl From<ScaffoldWire> for Scaffold {
    /// A word bank written before words had their own entries (plain text) is read as its words.
    fn from(w: ScaffoldWire) -> Self {
        let mut s = Scaffold {
            id: w.id,
            kind_type: w.kind_type,
            kind: w.kind,
            content: w.content,
            steps: w.steps,
            levels: w.levels,
            auto: w.auto,
            origin: w.origin,
            heading: w.heading,
            bank: w.bank,
        };
        s.sync_bank();
        s
    }
}

impl<'de> Deserialize<'de> for Scaffold {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        ScaffoldWire::deserialize(d).map(Scaffold::from)
    }
}

/// Longest heading a scaffold may print over it.
pub const MAX_SCAFFOLD_HEADING_CHARS: usize = 80;

impl Scaffold {
    /// Whether this is a 字詞表 (「Word bank」) scaffold.
    pub fn is_word_bank(&self) -> bool {
        self.kind_type == ScaffoldType::Verbal && self.kind == KIND_WORD_LIST
    }

    /// Keeps `bank` and `content` in step. A word bank with words: the words are the truth
    /// (clean, no repeats, a valid part of speech) and `content` is their list. One without:
    /// its text is read as a plain list of words when it is one. Not a word bank: no bank.
    pub fn sync_bank(&mut self) {
        if !self.is_word_bank() {
            self.bank = None;
            return;
        }
        match self.bank.take() {
            Some(mut b) if !b.words.is_empty() => {
                let mut seen: Vec<String> = Vec::new();
                b.words.retain_mut(|w| {
                    w.word = w.word.trim().to_string();
                    w.pos = key_vocab::normalise_pos(&w.pos)
                        .unwrap_or_default()
                        .to_string();
                    let key = w.word.to_lowercase();
                    let keep = !key.is_empty() && !seen.contains(&key);
                    seen.push(key);
                    keep
                });
                self.content = b
                    .words
                    .iter()
                    .map(|w| w.word.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                self.bank = Some(b);
            }
            _ => {
                self.bank = parse_word_list(&self.content).map(|words| WordBank {
                    lang: VocabLang::En,
                    words: words
                        .into_iter()
                        .map(|word| BankWord {
                            word,
                            ..BankWord::default()
                        })
                        .collect(),
                });
                if let Some(b) = &self.bank {
                    self.content = b
                        .words
                        .iter()
                        .map(|w| w.word.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                }
            }
        }
    }

    /// A new text for the word bank (the teacher's or the model's edit): the words it lists
    /// keep what is known about them (part of speech, meanings, ticks); new words start empty.
    pub fn set_text(&mut self, text: &str) {
        // The same words as now (whatever the writing): nothing changes.
        if let (Some(b), Some(words)) = (&self.bank, parse_word_list(text)) {
            let same = words.len() == b.words.len()
                && words
                    .iter()
                    .zip(&b.words)
                    .all(|(w, o)| w.eq_ignore_ascii_case(o.word.trim()));
            if same {
                return;
            }
        }
        let old = self.bank.take();
        self.content = text.to_string();
        self.bank = None;
        if !self.is_word_bank() {
            return;
        }
        let Some(words) = parse_word_list(text) else {
            return;
        };
        let (lang, old_words) = old.map_or((VocabLang::En, Vec::new()), |b| (b.lang, b.words));
        self.bank = Some(WordBank {
            lang,
            words: words
                .into_iter()
                .map(|word| {
                    old_words
                        .iter()
                        .find(|o| o.word.eq_ignore_ascii_case(&word))
                        .cloned()
                        .map(|o| BankWord {
                            word: word.clone(),
                            ..o
                        })
                        .unwrap_or(BankWord {
                            word,
                            ..BankWord::default()
                        })
                })
                .collect(),
        });
        self.sync_bank();
    }

    /// The heading of its own, trimmed; `None` when it has none (or only blanks).
    pub fn own_heading(&self) -> Option<&str> {
        self.heading
            .as_deref()
            .map(str::trim)
            .filter(|h| !h.is_empty())
    }
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
    /// The Bloom level this block was generated for (閱讀理解題／差異化學習單): regenerating the
    /// block re-runs that level.
    pub bloom: Option<Bloom>,
    /// Pictures that belong in this block (written by the model that wrote the block, at most
    /// two; their pictures live in `worksheet_images`). Optional and kept by every edit that
    /// copies the block; absent from the JSON when empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub image_slots: Vec<images::ImageSlot>,
    /// Where an activity block of the 完整學習單 stands in 示範 → 共同練習 → 同儕練習 → 獨立應用
    /// (SIOP Q4). Blocks made before (and of the other outputs) have none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub practice: Option<Practice>,
    /// 詞彙級別檢查「提供字義」: meanings of words above the teacher's target level, printed in a
    /// Glossary box at the end of the block, each in the versions it lists. Kept by every edit
    /// that copies the block; absent from the JSON when empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub glosses: Vec<Gloss>,
    /// 字詞表 rows the teacher (or Manna, at her request) took out of this block, kept so 「恢復字詞表」
    /// can put them back: the levels and the 指示用語 hints are computed again, not remembered.
    /// Never printed; absent from the JSON when empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub vocabulary_removed: Vec<Vocab>,
}

/// One entry of a block's Glossary (詞彙級別檢查「提供字義」): a word above the target level
/// kept in the text, with its meaning. Not part of what the vocabulary check analyses.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Gloss {
    /// The dictionary form, as printed (e.g. `abandon`).
    pub word: String,
    /// Part of speech in this context, as the word list writes it (`v.`, `adj.`); may be "".
    pub pos: String,
    /// The Chinese meaning in this context (required).
    pub meaning_zh: String,
    /// A short English definition within the target level; may be "".
    pub definition_en: String,
    /// The versions that print it.
    pub levels: Vec<Level>,
    pub origin: Origin,
}

pub const MAX_GLOSSES: usize = 30;
pub const GLOSS_WORD_CHARS: usize = 60;
pub const GLOSS_POS_CHARS: usize = 20;
pub const GLOSS_MEANING_CHARS: usize = 60;
pub const GLOSS_DEFINITION_CHARS: usize = 200;

/// SIOP Q4: the order of the per-paragraph tasks of the 完整學習單, from the teacher showing
/// how to the students doing it alone (a student who is stuck goes back a step).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Practice {
    /// 示範: the teacher models the first item, thinking aloud.
    Model,
    /// 共同練習: the class does it with the teacher.
    Guided,
    /// 同儕練習: pairs or groups.
    Peer,
    /// 獨立應用: each student on their own.
    Independent,
}

impl Practice {
    pub const ALL: [Practice; 4] = [
        Practice::Model,
        Practice::Guided,
        Practice::Peer,
        Practice::Independent,
    ];

    /// The fixed label the model writes (`practice` in `worksheet_tasks`).
    pub fn label(self) -> &'static str {
        match self {
            Self::Model => "示範",
            Self::Guided => "共同練習",
            Self::Peer => "同儕練習",
            Self::Independent => "獨立應用",
        }
    }

    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.label() == label.trim())
    }

    /// What the teacher reads in the block's notes.
    pub fn note(self) -> &'static str {
        match self {
            Self::Model => "教學順序：示範——老師先放聲思考，做第一題給學生看，再讓學生接著做。",
            Self::Guided => "教學順序：共同練習——全班和老師一起做，老師邊問邊引導。",
            Self::Peer => "教學順序：同儕練習——兩人或小組合作完成，老師巡視。",
            Self::Independent => {
                "教學順序：獨立應用——學生自己完成；卡住時可以回到前一個區塊的做法。"
            }
        }
    }

    /// The stage of the block at `index` of `count` blocks in paragraph order: the first one
    /// models, the last one is independent, the ones between move forward (all four from four
    /// blocks on).
    pub fn for_position(index: usize, count: usize) -> Self {
        match count {
            0 | 1 => Self::Independent,
            2 => [Self::Model, Self::Independent][index.min(1)],
            3 => [Self::Model, Self::Peer, Self::Independent][index.min(2)],
            n => Self::ALL[(index.min(n - 1) * 4 / n).min(3)],
        }
    }

    /// Whether the stages of the blocks, in order, follow Q4: every block has one, they never
    /// go back, the first models and the last is independent (from two blocks on), and all
    /// four are there (from four blocks on).
    pub fn in_order(stages: &[Option<Practice>]) -> bool {
        let Some(stages) = stages.iter().copied().collect::<Option<Vec<Practice>>>() else {
            return false;
        };
        let n = stages.len();
        if n < 2 {
            return true;
        }
        let forward = stages.windows(2).all(|w| w[0] <= w[1]);
        let ends = stages[0] == Self::Model && stages[n - 1] == Self::Independent;
        let all = n < 4 || Self::ALL.iter().all(|p| stages.contains(p));
        forward && ends && all
    }
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
            image_slots: Vec::new(),
            practice: None,
            glosses: Vec::new(),
            vocabulary_removed: Vec::new(),
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
    /// The order the students' codes follow (T9): the first level here prints 「Version A」, the
    /// next 「Version B」…. A 差異化學習單 gets a random order when it is first generated (and keeps
    /// it), so the codes don't tell students which version has the most support. Empty (older
    /// worksheets): the order of `levels`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub student_order: Vec<Level>,
    /// 字詞表放在學習單最後: every block's word table is printed once, after the last block,
    /// instead of inside its block. Off (the tables stay in their blocks) when absent.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub vocabulary_at_end: bool,
    /// The language of the Key Vocabulary meanings, one switch for the worksheet (English when
    /// absent).
    #[serde(skip_serializing_if = "VocabLang::is_en")]
    pub vocabulary_language: VocabLang,
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
            student_order: Vec::new(),
            vocabulary_at_end: false,
            vocabulary_language: VocabLang::En,
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

/// A question's versions (差異化學習單): one per level, in level order, each within the limits.
fn check_tiers(tiers: &mut [ItemTier]) -> Result<(), String> {
    if tiers.len() > Level::ALL.len() {
        return Err("一題最多有三個版本。".to_string());
    }
    let mut seen = HashSet::new();
    for t in tiers.iter() {
        if !seen.insert(t.level) {
            return Err("一題的同一個版本只能有一份。".to_string());
        }
        if t.support.len() > 12 || t.word_bank.len() > 30 {
            return Err("一題的支持或字庫太多了。".to_string());
        }
        for line in t.support.iter().chain(&t.word_bank) {
            check_text("支持", line)?;
        }
        check_text("範句", &t.model_sentence)?;
        check_text("參考答案", &t.answer)?;
        check_text("評分說明", &t.scoring)?;
    }
    tiers.sort_by_key(|t| t.level);
    Ok(())
}

/// A block's Glossary: within the limits, every entry with a word and a meaning, one entry per
/// word, levels in canonical order (an entry printed in no version is dropped).
fn check_glosses(glosses: &mut Vec<Gloss>) -> Result<(), String> {
    if glosses.len() > MAX_GLOSSES {
        return Err(format!("一個區塊的字義註解最多 {MAX_GLOSSES} 個。"));
    }
    let mut seen = HashSet::new();
    for g in glosses.iter_mut() {
        g.word = g.word.trim().to_string();
        g.pos = g.pos.trim().to_string();
        g.meaning_zh = g.meaning_zh.trim().to_string();
        g.definition_en = g.definition_en.trim().to_string();
        if g.word.is_empty() || g.meaning_zh.is_empty() {
            return Err("字義註解要有字和中文意思。".to_string());
        }
        if g.word.chars().count() > GLOSS_WORD_CHARS
            || g.pos.chars().count() > GLOSS_POS_CHARS
            || g.meaning_zh.chars().count() > GLOSS_MEANING_CHARS
            || g.definition_en.chars().count() > GLOSS_DEFINITION_CHARS
        {
            return Err(format!(
                "字義註解太長了（中文意思最多 {GLOSS_MEANING_CHARS} 字，英文解釋最多 {GLOSS_DEFINITION_CHARS} 字）。"
            ));
        }
        if !seen.insert(g.word.to_lowercase()) {
            return Err(format!("字義註解「{}」重複。", g.word));
        }
        g.levels = normalise_levels(&g.levels);
    }
    glosses.retain(|g| !g.levels.is_empty());
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
        let mut slot_ids = HashSet::new();
        for b in &self.blocks {
            images::check_block_slots(b, &mut slot_ids)?;
        }
        let all_tasks = student_tasks();
        for b in &mut self.blocks {
            if b.items.len() > MAX_ITEMS
                || b.scaffolds.len() > MAX_LIST
                || b.vocabulary.len() > MAX_LIST
                || b.vocabulary_removed.len() > MAX_LIST
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
                check_text("句型框架", &i.frame)?;
                check_tiers(&mut i.tiers)?;
                if let Some(org) = &mut i.organizer {
                    org.validate()?;
                }
                if i.options.len() > 12 || i.kept_options.len() > 12 {
                    return Err("一題最多 12 個選項。".to_string());
                }
                for o in i.options.iter().chain(&i.kept_options) {
                    check_text("選項", o)?;
                }
                if let Some(t) = &i.table {
                    if t.columns.is_empty() || t.columns.len() > MAX_TABLE_COLUMNS {
                        return Err(format!("表格要有 1 到 {MAX_TABLE_COLUMNS} 欄。"));
                    }
                    if !(1..=MAX_TABLE_ROWS).contains(&t.rows) {
                        return Err(format!("表格要有 1 到 {MAX_TABLE_ROWS} 列。"));
                    }
                    if t.row_labels.len() > usize::from(MAX_TABLE_ROWS)
                        || t.row_labels.iter().any(|l| l.chars().count() > 200)
                    {
                        return Err("列標籤太多或太長了。".to_string());
                    }
                    if t.columns.iter().any(|c| c.chars().count() > 200) {
                        return Err("表格的欄名最多 200 字。".to_string());
                    }
                }
                if let Some(f) = &i.answer_format {
                    if !ANSWER_FORMATS.contains(&f.as_str()) {
                        return Err(format!(
                            "「{}」不是「作答方式」的選項。",
                            crate::text::clip_chars(f, 40)
                        ));
                    }
                }
                i.levels = normalise_levels(&i.levels);
            }
            for s in &mut b.scaffolds {
                check_text("鷹架", &s.content)?;
                if let Some(bank) = &mut s.bank {
                    bank.words.truncate(MAX_BANK_WORDS);
                    for w in &bank.words {
                        if w.word.chars().count() > 80 {
                            return Err("字詞最多 80 個字元。".to_string());
                        }
                        check_text("字詞解釋", &w.definition)?;
                        check_text("字詞中文解釋", &w.meaning_zh)?;
                    }
                }
                s.sync_bank();
                s.heading = s.own_heading().map(str::to_string);
                if s.heading
                    .as_deref()
                    .is_some_and(|h| h.chars().count() > MAX_SCAFFOLD_HEADING_CHARS)
                {
                    return Err(format!(
                        "鷹架的標題最多 {MAX_SCAFFOLD_HEADING_CHARS} 個字。"
                    ));
                }
                if s.steps.len() > 12 {
                    return Err("一個程序鷹架最多 12 個步驟。".to_string());
                }
                for step in &s.steps {
                    check_text("步驟", step)?;
                }
                if !s.kind_type.kinds().contains(&s.kind.as_str()) {
                    return Err(format!(
                        "「{}」不是這一類鷹架的種類。",
                        crate::text::clip_chars(&s.kind, 40)
                    ));
                }
                s.levels = normalise_levels(&s.levels);
            }
            for v in b.vocabulary.iter_mut().chain(&mut b.vocabulary_removed) {
                if v.word.chars().count() > 80 {
                    return Err("字詞最多 80 個字元。".to_string());
                }
                check_text("字詞解釋", &v.definition)?;
                check_text("字詞中文解釋", &v.meaning_zh)?;
                // A part of speech is one of the fixed set (or empty), whatever the editor sent.
                v.pos = key_vocab::normalise_pos(&v.pos)
                    .unwrap_or_default()
                    .to_string();
                check_text("字詞例句", &v.context)?;
            }
            check_glosses(&mut b.glosses)?;
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
    derive_from_sop(material, sop, true)
}

/// What the SOP's content alone suggests, ignoring what the teacher chose in the steps
/// (`SopContent.options` and the labels on the task blocks): the starred (✦) defaults.
pub fn defaults_from_sop(material: &Material, sop: &crate::sop::SopContent) -> Settings {
    derive_from_sop(material, sop, false)
}

fn derive_from_sop(material: &Material, sop: &crate::sop::SopContent, chosen: bool) -> Settings {
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
    let mut settings = Settings {
        material_types: material_types_for(material),
        student_tasks: tasks,
        answer_formats: Vec::new(),
        generation,
        levels: vec![Level::Standard],
        minutes: None,
        ..Settings::default()
    };
    if chosen {
        // The teacher's choices in the steps (spec §7) win where there are any.
        let o = &sop.options;
        let blocks = sop.task_blocks.iter();
        let picked_tasks: Vec<String> = blocks
            .clone()
            .flat_map(|b| b.student_tasks.iter().cloned())
            .collect();
        if !picked_tasks.is_empty() {
            settings.student_tasks = picked_tasks;
        }
        settings.answer_formats = blocks
            .flat_map(|b| {
                b.answer_formats
                    .iter()
                    .cloned()
                    .chain(b.tasks.iter().map(|t| t.answer_format.clone()))
            })
            .filter(|f| !f.is_empty())
            .collect();
        if let Some(v) = &o.material_types {
            settings.material_types = v.clone();
        }
        if let Some(v) = &o.generation {
            settings.generation = v.clone();
        }
        if let Some(v) = &o.levels {
            settings.levels = v.clone();
        }
        settings.minutes = o.minutes;
        settings.skip_exit_ticket = o.exit_ticket == Some(false);
    }
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
            content_objective: Some("  Students can explain the writer's view.  ".into()),
            language_objective: Some(" I can underline because. \n\n  I can retell it. ".into()),
            ..Settings::default()
        };
        let n = s.normalised().unwrap();
        assert_eq!(n.bloom, vec![Bloom::Remember, Bloom::Create]);
        assert_eq!(
            n.content_objective.as_deref(),
            Some("Students can explain the writer's view.")
        );
        // One objective per line, every line trimmed, blank lines dropped.
        assert_eq!(
            n.language_objective.as_deref(),
            Some("I can underline because.\nI can retell it.")
        );
        for bad in [2, 31] {
            let s = Settings {
                question_count: Some(bad),
                ..Settings::default()
            };
            assert!(s.normalised().unwrap_err().contains("題數"), "{bad}");
        }
        let s = Settings {
            content_objective: Some("o".repeat(301)),
            ..Settings::default()
        };
        assert!(s.normalised().unwrap_err().contains("內容目標"));
        let s = Settings {
            language_objective: Some("o".repeat(301)),
            ..Settings::default()
        };
        assert!(s.normalised().unwrap_err().contains("語言目標"));
        let s = Settings {
            content_objective: Some("   ".into()),
            language_objective: Some(" \n ".into()),
            ..Settings::default()
        };
        let blank = s.normalised().unwrap();
        assert_eq!(blank.content_objective, None);
        assert_eq!(blank.language_objective, None);
        // A row stored with the single field from before the two: read as the 內容目標, and
        // written back without it.
        let old: Settings =
            serde_json::from_value(serde_json::json!({ "objective": "能說出主旨" })).unwrap();
        assert_eq!(old.content_objective.as_deref(), Some("能說出主旨"));
        assert_eq!(old.language_objective, None);
        let written = serde_json::to_value(&old).unwrap();
        assert!(written.get("objective").is_none(), "{written}");
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
        assert_eq!(p.content_objective, n.content_objective);
        assert_eq!(p.language_objective, n.language_objective);
        // An old client's `objective` patches the 內容目標.
        let legacy: SettingsPatch =
            serde_json::from_value(serde_json::json!({ "objective": null })).unwrap();
        assert_eq!(legacy.apply(&n).content_objective, None);
        assert_eq!(legacy.apply(&n).language_objective, n.language_objective);
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
    fn practice_moves_from_modelling_to_independent_work() {
        use Practice::*;
        let at = |n: usize| {
            (0..n)
                .map(|i| Practice::for_position(i, n))
                .collect::<Vec<_>>()
        };
        assert_eq!(at(1), [Independent]);
        assert_eq!(at(2), [Model, Independent]);
        assert_eq!(at(3), [Model, Peer, Independent]);
        assert_eq!(at(4), [Model, Guided, Peer, Independent]);
        assert_eq!(at(6), [Model, Model, Guided, Peer, Peer, Independent]);
        for n in 1..=12 {
            let s: Vec<Option<Practice>> = at(n).into_iter().map(Some).collect();
            assert!(Practice::in_order(&s), "{n}");
        }
        assert!(!Practice::in_order(&[Some(Model), None]));
        assert!(!Practice::in_order(&[
            Some(Model),
            Some(Peer),
            Some(Guided),
            Some(Independent)
        ]));
        assert!(
            !Practice::in_order(&[Some(Guided), Some(Independent)]),
            "starts with 示範"
        );
        assert!(
            !Practice::in_order(&[Some(Model), Some(Peer)]),
            "ends with 獨立應用"
        );
        assert!(
            !Practice::in_order(&[Some(Model), Some(Model), Some(Peer), Some(Independent)]),
            "four blocks use all four"
        );
        assert!(Practice::in_order(&[
            Some(Model),
            Some(Guided),
            Some(Independent)
        ]));
        assert_eq!(Practice::from_label(" 同儕練習 "), Some(Peer));
        assert_eq!(serde_json::json!(Peer), "peer");
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

    // The editor reads prompts the same way: web/src/__tests__/editorControls.test.tsx has
    // these cases too.
    #[test]
    fn a_placeholder_heading_never_prints() {
        let t = |cols: &[&str]| ItemTable {
            columns: labels(cols),
            rows: 3,
            row_labels: Vec::new(),
        };
        let named = "Complete a T-chart with two columns: Pros and Cons. Add one detail.";
        // The question names the headings: they replace 「Column 1」.
        assert_eq!(
            t(&["Column 1", "Column 2"])
                .without_placeholders(named)
                .columns,
            ["Pros", "Cons"]
        );
        // It names none: the placeholder goes, a real heading stays, an empty one stays empty.
        assert_eq!(
            t(&["Who", "Column 2", ""])
                .without_placeholders("Complete the table.")
                .columns,
            ["Who", "", ""]
        );
        assert_eq!(
            t(&["", ""]).without_placeholders(named).columns,
            ["", ""],
            "empty headings are left to the teacher"
        );
        // An item without a table of its own takes the one its question names.
        let item = Item {
            prompt: named.into(),
            ..Item::default()
        };
        assert_eq!(item.table_or_default().columns, ["Pros", "Cons"]);
    }

    #[test]
    fn a_table_takes_its_columns_from_the_prompt() {
        let cols = |p: &str| columns_from_prompt(p);
        assert_eq!(
            cols("Complete the table for P1: Who? / What happened? / Why? (Question 2)"),
            Some(labels(&["Who?", "What happened?", "Why?"]))
        );
        assert_eq!(
            cols("Fill in the chart： Cause | Effect."),
            Some(labels(&["Cause", "Effect"]))
        );
        for none in [
            "Complete the table about his/her day.",
            "Complete the table: Cause",
            "Compare them: what the neighbors expected before the stand opened / what really happened after a month",
            "Complete the table: a / b / c / d / e / f / g",
            "Complete the table: Who? / / Why?",
            "",
        ] {
            assert_eq!(cols(none), None, "{none}");
        }
        assert_eq!(
            ItemTable::for_prompt("Complete the table."),
            ItemTable {
                columns: labels(&["", ""]),
                rows: TABLE_ROWS,
                row_labels: Vec::new(),
            }
        );
        assert!(uses_choices(None) && uses_choices(Some(FMT_TRUE_FALSE)));
        assert!(!uses_choices(Some(FMT_SHORT)) && !uses_choices(Some(FMT_BLANK)));
    }

    #[test]
    fn a_saved_table_and_kept_choices_are_checked() {
        let with = |table: Option<ItemTable>, kept: Vec<String>| Content {
            blocks: vec![Block {
                id: "b1".into(),
                items: vec![Item {
                    id: "i1".into(),
                    answer_format: Some(FMT_TABLE.into()),
                    table,
                    kept_options: kept,
                    ..Item::default()
                }],
                ..Block::default()
            }],
            ..Content::default()
        };
        let table = |n: usize, rows: u8| {
            Some(ItemTable {
                columns: vec!["Who?".to_string(); n],
                rows,
                row_labels: Vec::new(),
            })
        };
        assert!(with(table(3, 3), labels(&["A", "B"])).normalise().is_ok());
        assert!(with(None, Vec::new()).normalise().is_ok());
        for bad in [table(0, 3), table(7, 3), table(2, 0), table(2, 13)] {
            assert!(with(bad, Vec::new()).normalise().is_err());
        }
        assert!(with(None, vec!["x".to_string(); 13]).normalise().is_err());
        // Old rows (no `table`, no `kept_options`) read as before, and the new keys are only
        // written when they hold something.
        let old: Item = serde_json::from_str(r#"{"id":"i1","answer_format":"表格整理"}"#).unwrap();
        assert_eq!((old.table, old.kept_options.len()), (None, 0));
        let json = serde_json::to_string(&Item::default()).unwrap();
        assert!(!json.contains("table") && !json.contains("kept_options"));
        let t: ItemTable = serde_json::from_str(r#"{"columns":["A"]}"#).unwrap();
        assert_eq!(t.rows, TABLE_ROWS);
    }

    #[test]
    fn old_vocabulary_content_reads_in_the_new_shape() {
        use serde_json::json;
        // A task word's Chinese hint was its `definition`: it moves to `meaning_zh`.
        let v: Vocab = serde_json::from_value(json!({
            "word": "compare", "definition": "比較（找出相同與不同）", "task_word": true
        }))
        .unwrap();
        assert_eq!(v.meaning_zh, "比較（找出相同與不同）");
        assert_eq!(
            v.definition,
            "to look at how things are the same and different"
        );
        assert_eq!(v.pos, "v.");
        // New content is left as it is, and a key word of the text without a part of speech too.
        let v: Vocab = serde_json::from_value(json!({
            "word": "compare", "definition": "to check", "meaning_zh": "比較", "task_word": true, "pos": "v."
        }))
        .unwrap();
        assert_eq!(
            (v.definition.as_str(), v.meaning_zh.as_str()),
            ("to check", "比較")
        );
        let v: Vocab =
            serde_json::from_value(json!({ "word": "umbrella", "definition": "a thing for rain" }))
                .unwrap();
        assert!(v.pos.is_empty() && v.meaning_zh.is_empty());
        // Nothing new is written when it is empty.
        assert!(serde_json::to_value(&v).unwrap().get("pos").is_none());
        // The meaning printed: the language asked for, else the other one.
        let v = Vocab {
            definition: "a thing for rain".into(),
            meaning_zh: "雨傘".into(),
            ..Vocab::default()
        };
        assert_eq!(v.meaning(VocabLang::En), "a thing for rain");
        assert_eq!(v.meaning(VocabLang::Zh), "雨傘");
        let v = Vocab {
            meaning_zh: String::new(),
            ..v
        };
        assert_eq!(v.meaning(VocabLang::Zh), "a thing for rain");
    }

    #[test]
    fn a_plain_text_word_bank_is_read_as_its_words() {
        use serde_json::json;
        let s: Scaffold = serde_json::from_value(json!({
            "id": "s1", "type": "verbal", "kind": "字詞表", "content": "Word bank: plain, profound, in the end"
        }))
        .unwrap();
        let words: Vec<&str> = s
            .bank
            .as_ref()
            .unwrap()
            .words
            .iter()
            .map(|w| w.word.as_str())
            .collect();
        assert_eq!(words, ["plain", "profound", "in the end"]);
        assert_eq!(s.content, "plain, profound, in the end");
        // Not a list of words: kept as text, no bank.
        for text in [
            "Linking words for contrast: however, but",
            "Write what you think.",
            "",
        ] {
            let s: Scaffold = serde_json::from_value(json!({
                "id": "s1", "type": "verbal", "kind": "字詞表", "content": text
            }))
            .unwrap();
            assert!(s.bank.is_none(), "{text}");
        }
        // Another kind of scaffold never has one.
        let s: Scaffold = serde_json::from_value(json!({
            "id": "s1", "type": "verbal", "kind": "句型框架", "content": "plain, profound"
        }))
        .unwrap();
        assert!(s.bank.is_none());
        // With words stored, they are the truth: the text follows them; new text keeps what is known.
        let mut s: Scaffold = serde_json::from_value(json!({
            "id": "s1", "type": "verbal", "kind": "字詞表", "content": "old",
            "bank": { "lang": "zh", "words": [
                { "word": "plain", "pos": "adj", "meaning_zh": "明顯的", "show_meaning": true },
                { "word": "fork", "pos": "n." }
            ]}
        }))
        .unwrap();
        assert_eq!(s.content, "plain, fork");
        assert_eq!(
            s.bank.as_ref().unwrap().words[0].pos,
            "adj.",
            "the part of speech is normalised"
        );
        s.set_text("Plain, fork, hesitate");
        let bank = s.bank.as_ref().unwrap();
        assert_eq!(bank.lang, VocabLang::Zh);
        assert!(bank.words[0].show_meaning && bank.words[0].meaning_zh == "明顯的");
        assert_eq!(bank.words[2].word, "hesitate");
        assert!(bank.words[2].pos.is_empty());
        s.set_text("fork");
        assert_eq!(s.bank.as_ref().unwrap().words.len(), 1);
    }
}
