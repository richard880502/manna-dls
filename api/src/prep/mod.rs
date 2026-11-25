//! 備課對話 (prep sessions): a conversation-first entry point (docs/API.md「Phase 2.6：備課對話」).
//!
//! A session holds the lesson's context (a library material or only a topic, an optional class
//! and template, the teacher's 出題選項 [`SessionOptions`]); every request is one turn, run in
//! the background ([`run`]). The kinds: `worksheet` / `assessment` / `comprehension` / `tiered`
//! make an ordinary worksheet and start its generation; `activity` plans a lesson (one model
//! call); `kit` (教學套件) does both, an activity plan and a tiered worksheet; `resources` lists
//! curriculum items and the teacher's own materials (no model); `ask` answers a question (one
//! call); `revise` applies a typed instruction to the session's latest worksheet (one call,
//! every change checked like a template block).
//!
//! Every string and array the model writes has a cap in its JSON schema: the gateway enforces
//! `maxLength` / `maxItems` while decoding, which is what stops a runaway.

pub mod fake;
pub mod images;
pub mod kit;
pub mod run;
pub mod selfcheck;
pub mod store;

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};

use crate::checks::contains_cjk;
use crate::sop::checks::clean_markup;
use crate::worksheet::bloom::{Bloom, Output};
use crate::worksheet::template::{enum_of, obj, s_max, strings_max};
use crate::worksheet::{Settings, SettingsPatch, ANSWER_FORMATS};

pub const KIND_WORKSHEET: &str = "worksheet";
pub const KIND_ASSESSMENT: &str = "assessment";
pub const KIND_ACTIVITY: &str = "activity";
pub const KIND_RESOURCES: &str = "resources";
pub const KIND_ASK: &str = "ask";
pub const KIND_REVISE: &str = "revise";
pub const KIND_COMPREHENSION: &str = "comprehension";
pub const KIND_TIERED: &str = "tiered";
pub const KIND_KIT: &str = "kit";
pub const KINDS: [&str; 9] = [
    KIND_WORKSHEET,
    KIND_ASSESSMENT,
    KIND_ACTIVITY,
    KIND_RESOURCES,
    KIND_ASK,
    KIND_REVISE,
    KIND_COMPREHENSION,
    KIND_TIERED,
    KIND_KIT,
];
/// The next-step chips, in the order they are offered (all of them need material).
/// `assessment` and `resources` stay valid kinds but are no longer suggested.
pub const NEXT_KINDS: [&str; 5] = [
    KIND_COMPREHENSION,
    KIND_TIERED,
    KIND_ACTIVITY,
    KIND_KIT,
    KIND_WORKSHEET,
];

pub const STATUS_RUNNING: &str = "running";
pub const STATUS_DONE: &str = "done";
pub const STATUS_FAILED: &str = "failed";

pub const KEY_ACTIVITY: &str = "prep_activity";
pub const KEY_ASK: &str = "prep_ask";
pub const KEY_REVISE: &str = "prep_revise";

pub const MAX_TITLE_CHARS: usize = 200;
pub const MAX_TOPIC_CHARS: usize = 200;
pub const MAX_REQUEST_CHARS: usize = 2000;
/// A session's default title from the first request: at most this many characters.
pub const AUTO_TITLE_CHARS: usize = 40;

/// The three phases of a lesson plan (fixed labels).
pub const PHASE_WARMUP: &str = "引起動機";
pub const PHASE_DEVELOP: &str = "發展活動";
pub const PHASE_WRAP_UP: &str = "統整與評量";
pub const PHASES: [&str; 3] = [PHASE_WARMUP, PHASE_DEVELOP, PHASE_WRAP_UP];
/// The 引起動機 options, as the page and the kit name them.
pub const OPTION_NAMES_ZH: [&str; 3] = ["選項一", "選項二", "選項三"];

/// How the students work in a step (SIOP A2: at least two of them in a lesson; fixed labels).
pub const GROUP_CLASS: &str = "全班";
pub const GROUP_PAIR: &str = "兩人一組";
pub const GROUP_SMALL: &str = "小組";
pub const GROUP_SOLO: &str = "個人";
pub const GROUPINGS: [&str; 4] = [GROUP_CLASS, GROUP_PAIR, GROUP_SMALL, GROUP_SOLO];
/// A group of three or more lists at least this many member roles (主持、記錄、計時、報告…).
pub const SMALL_GROUP_ROLES_MIN: usize = 3;

/// The language skills a step uses (SIOP A3: all four in a lesson), in this order.
pub const SKILLS: [&str; 4] = ["聽", "說", "讀", "寫"];
/// The Bloom levels a 高層次問題 may be tagged with.
pub const HIGHER_ORDER: [Bloom; 3] = [Bloom::Analyze, Bloom::Evaluate, Bloom::Create];

/// The chip label a turn of `kind` stands for, used as its request when the teacher typed none.
pub fn chip_label(kind: &str) -> &'static str {
    match kind {
        KIND_WORKSHEET => "做學習單",
        KIND_ASSESSMENT => "設計評量",
        KIND_ACTIVITY => "規劃教學活動",
        KIND_RESOURCES => "找教學資源",
        KIND_ASK => "直接問 AI",
        KIND_COMPREHENSION => "閱讀理解題",
        KIND_TIERED => "差異化學習單",
        KIND_KIT => "教學套件",
        _ => "修改學習單",
    }
}

/// Labels the pages send as a request when the teacher typed nothing: the server's chip labels
/// and the home page's chips and tool names. Such a request is no 「教師的要求」. 差異化學習單 was
/// called 分層練習 before: a page loaded before the rename still sends the old name.
const BARE_LABELS: [&str; 14] = [
    "做學習單",
    "設計評量",
    "規劃教學活動",
    "找教學資源",
    "直接問 AI",
    "修改學習單",
    "閱讀理解題",
    "差異化學習單",
    "分層練習",
    "課堂活動",
    "教學套件",
    "學習單",
    "完整學習單",
    "照我的學習單做",
];

/// Whether a request is only a chip or tool label.
pub fn is_bare_label(request: &str) -> bool {
    BARE_LABELS.contains(&request.trim())
}

/// Kinds whose turn makes a worksheet (a kit makes one beside its activity plan).
pub fn is_worksheet_kind(kind: &str) -> bool {
    matches!(
        kind,
        KIND_WORKSHEET | KIND_ASSESSMENT | KIND_COMPREHENSION | KIND_TIERED | KIND_KIT
    )
}

/// What the worksheet of a turn of `kind` makes: `None` for the full worksheet (`worksheet`,
/// `assessment`).
pub fn output_of(kind: &str) -> Option<Output> {
    match kind {
        KIND_COMPREHENSION => Some(Output::Comprehension),
        KIND_TIERED | KIND_KIT => Some(Output::Tiered),
        _ => None,
    }
}

// ---------------------------------------------------------------- session options

/// A list that may also be sent as `null` (taken as empty).
fn null_as_empty<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(d)?.unwrap_or_default())
}

/// 出題選項 of a session (`POST` / `PATCH /api/prep-sessions`, echoed as `PrepSession.options`):
/// passed into every worksheet the session makes, as the worksheet settings of the same names.
/// Absent keys are not set (and not serialised): all six levels, the output's default count,
/// the formats that suit each level, no objective.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SessionOptions {
    #[serde(
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "null_as_empty"
    )]
    pub bloom: Vec<Bloom>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub question_count: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bloom_counts: Option<BTreeMap<Bloom, i32>>,
    #[serde(
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "null_as_empty"
    )]
    pub answer_formats: Vec<String>,
    /// 內容目標 (≤300, one objective per line). A session stored before the two fields existed
    /// has one `objective`: it is read as the 內容目標.
    #[serde(skip_serializing_if = "Option::is_none", alias = "objective")]
    pub content_objective: Option<String>,
    /// 語言目標 (≤300, one objective per line).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language_objective: Option<String>,
    /// 只選要用的段落: the ids of the session material's paragraphs every turn works with
    /// (empty = all of them). The material itself is never changed by it.
    #[serde(
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "null_as_empty"
    )]
    pub paragraph_ids: Vec<String>,
}

impl SessionOptions {
    /// Checked with the worksheet settings' own rules (fixed labels, 3–30 questions, 0–12 per
    /// level and 3–30 in all, objectives of at most 300 characters) and put in their order:
    /// levels in Bloom order without duplicates, formats in label order, every objective line
    /// trimmed (empty is none).
    pub fn normalised(&self) -> Result<SessionOptions, String> {
        let s = Settings {
            answer_formats: self.answer_formats.clone(),
            bloom: self.bloom.clone(),
            question_count: self.question_count,
            bloom_counts: self.bloom_counts.clone(),
            content_objective: self.content_objective.clone(),
            language_objective: self.language_objective.clone(),
            ..Settings::default()
        }
        .normalised()?;
        Ok(SessionOptions {
            bloom: s.bloom,
            question_count: s.question_count,
            bloom_counts: s.bloom_counts,
            answer_formats: s.answer_formats,
            content_objective: s.content_objective,
            language_objective: s.language_objective,
            paragraph_ids: crate::worksheet::material::clean_selection(&self.paragraph_ids)?
                .unwrap_or_default(),
        })
    }

    /// The teacher's 內容目標 and 語言目標, one line per objective.
    pub fn teacher_objectives(&self) -> crate::objectives::TeacherObjectives {
        crate::objectives::TeacherObjectives::from_fields(
            self.content_objective.as_deref(),
            self.language_objective.as_deref(),
        )
    }

    /// The partial settings of a worksheet the session makes: `output` (none for the full
    /// worksheet) and the options that are set. An assessment keeps its own answer formats
    /// (`with_formats = false`). `None` when there is nothing to set.
    pub fn settings_patch(
        &self,
        output: Option<Output>,
        with_formats: bool,
    ) -> Option<SettingsPatch> {
        let patch = SettingsPatch {
            output,
            bloom: (!self.bloom.is_empty()).then(|| self.bloom.clone()),
            question_count: self.question_count.map(Some),
            bloom_counts: self.bloom_counts.clone().map(Some),
            answer_formats: (with_formats && !self.answer_formats.is_empty())
                .then(|| self.answer_formats.clone()),
            content_objective: self.content_objective.clone().map(Some),
            language_objective: self.language_objective.clone().map(Some),
            ..SettingsPatch::default()
        };
        (patch != SettingsPatch::default()).then_some(patch)
    }
}

// ---------------------------------------------------------------- 教學套件

/// Why a 教學套件 turn failed, from what went wrong with each part; `None` when both parts
/// succeeded.
pub fn kit_error(activity: Option<&str>, worksheet: Option<&str>) -> Option<String> {
    match (activity, worksheet) {
        (None, None) => None,
        (Some(a), None) => Some(format!("教學套件的課堂活動沒有完成：{a}")),
        (None, Some(w)) => Some(format!("教學套件的差異化學習單沒有完成：{w}")),
        (Some(a), Some(w)) => Some(format!("教學套件沒有完成。課堂活動：{a}差異化學習單：{w}")),
    }
}

// ---------------------------------------------------------------- caps

pub const PLAN_TITLE_CHARS: usize = 100;
/// Goals of each group (內容目標, 語言目標): 1–2 each (SIOP R1: one as a rule, at most two;
/// `prep_activity` version 4; version 3 asked for 1–3).
pub const GOAL_GROUP_MIN: usize = 1;
pub const GOAL_GROUP_MAX: usize = 2;
/// A goal the teacher gave or adopted is kept as written, up to the length of the objective
/// field (`Settings.objective`).
pub const TEACHER_GOAL_CHARS: usize = 300;
/// 「學生要用［語言目標］，才能［內容目標］」: one sentence.
pub const RELATION_CHARS: usize = 200;
pub const KEY_WORDS_MAX: usize = 8;
pub const KEY_WORD_CHARS: usize = 40;
pub const KEY_WORD_ZH_CHARS: usize = 80;
pub const MATERIALS_MAX: usize = 5;
pub const MATERIAL_CHARS: usize = 60;
pub const QUESTIONS_MAX: usize = 4;
pub const QUESTION_CHARS: usize = 200;
pub const BACKGROUND_CHARS: usize = 200;
pub const REVIEW_CHARS: usize = 200;
pub const ROLES_MAX: usize = 6;
pub const ROLE_CHARS: usize = 20;
pub const ROLE_TASK_CHARS: usize = 100;
pub const MONITORING_CHARS: usize = 150;
pub const SCAFFOLDING_CHARS: usize = 150;
/// A goal in Chinese, for the teacher: one sentence.
pub const GOAL_ZH_CHARS: usize = 150;
/// The same goal in English, for the students: one sentence.
pub const GOAL_EN_CHARS: usize = 200;
pub const GOAL_CODES_MAX: usize = 3;
pub const CODE_CHARS: usize = 60;
pub const STEPS_MIN: usize = 3;
pub const STEPS_MAX: usize = 6;
pub const STEP_TITLE_CHARS: usize = 60;
pub const IDEA_CHARS: usize = 300;
pub const OPTIONS_MAX: usize = 3;
pub const OPTION_SCRIPT_LINES: usize = 4;
pub const STEP_SCRIPT_LINES: usize = 6;
pub const SCRIPT_LINE_CHARS: usize = 200;
pub const STUCK_TIP_CHARS: usize = 200;
/// Bloom levels of one step.
pub const STEP_BLOOM_MAX: usize = 3;
pub const MINUTES_MIN: i64 = 1;
pub const MINUTES_MAX: i64 = 50;

pub const ANSWER_CHARS: usize = 2500;
pub const ANSWER_PARAGRAPHS: usize = 10;
pub const ANSWER_PARAGRAPH_CHARS: usize = 400;
pub const FOLLOW_UPS_MAX: usize = 3;
pub const FOLLOW_UP_CHARS: usize = 40;

pub const REPLY_CHARS: usize = 300;
pub const CHANGES_MAX: usize = 6;
/// Items of one revised block (the `item_count` its changes are checked against).
pub const CHANGE_ITEMS_MAX: usize = 12;

/// Curriculum candidates put in front of the model (activity) and listed (resources).
pub const CURRICULUM_TOP: usize = 8;
/// Curriculum candidates for a direct question.
pub const ASK_CURRICULUM_TOP: usize = 5;
/// The teacher's own materials listed by 找教學資源.
pub const MATERIALS_TOP: usize = 5;

// ---------------------------------------------------------------- results

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CurriculumRef {
    pub code: String,
    pub text: String,
}

/// The two groups of an activity plan's goals, as SIOP separates them (the SOP's
/// `sop_objectives` uses the same split).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum GoalKind {
    /// 內容目標: what students will learn or do with the content.
    Content,
    /// 語言目標: the English functions, structures or vocabulary they use to do it.
    Language,
}

impl GoalKind {
    /// In the order they are written and shown.
    pub const ALL: [GoalKind; 2] = [GoalKind::Content, GoalKind::Language];

    pub fn label(self) -> &'static str {
        match self {
            GoalKind::Content => "內容目標",
            GoalKind::Language => "語言目標",
        }
    }
}

/// The four kinds of 語言目標 SIOP lists (academic vocabulary; language skills and functions;
/// language structures or grammar; language learning strategies). Manna drafts a strategy goal
/// first (SIOP R2, a team decision), so the strategy comes first here, in the schema and in a
/// plan's 語言目標.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum LanguageCategory {
    /// 學習策略 (rereading when lost, predicting then checking, previewing headings…).
    Strategy,
    /// 語言功能 (describe, compare, predict, explain causes…).
    Function,
    /// 句型文法.
    Structure,
    /// 學術字彙.
    Vocabulary,
}

impl LanguageCategory {
    /// In the order the model is offered them (the strategy first).
    pub const ALL: [LanguageCategory; 4] = [
        LanguageCategory::Strategy,
        LanguageCategory::Function,
        LanguageCategory::Structure,
        LanguageCategory::Vocabulary,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LanguageCategory::Strategy => "學習策略",
            LanguageCategory::Function => "語言功能",
            LanguageCategory::Structure => "句型文法",
            LanguageCategory::Vocabulary => "學術字彙",
        }
    }

    pub fn from_label(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.label() == s.trim())
    }
}

/// A goal of an activity plan. Since version 3 of `prep_activity` every goal has a `kind` and
/// is written twice: `zh` for the teacher, `en` for the students. A plan made with an earlier
/// version has one Chinese line per goal, stored as `text`: it is read into `zh`, without a
/// `kind` and with an empty `en` (the stored JSON itself is never rewritten).
///
/// Since version 4 a 語言目標 also says which kind it is (`category`), which 內容目標 it serves
/// (`supports`, 1-based among the plan's 內容目標) and how, in one sentence (`relation`:
/// 「學生要用［語言目標］，才能［內容目標］」); a goal the teacher gave or adopted is kept word for
/// word and marked `from_teacher`. Plans made before have none of these.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Goal {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<GoalKind>,
    #[serde(alias = "text")]
    pub zh: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub en: String,
    pub curriculum_codes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<LanguageCategory>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports: Option<usize>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub relation: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub from_teacher: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl Goal {
    /// A goal of `kind` with its two lines and codes (none of the version 4 additions).
    pub fn new(kind: GoalKind, zh: &str, en: &str, curriculum_codes: Vec<String>) -> Self {
        Goal {
            kind: Some(kind),
            zh: zh.to_string(),
            en: en.to_string(),
            curriculum_codes,
            category: None,
            supports: None,
            relation: String::new(),
            from_teacher: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StepOption {
    pub title: String,
    pub idea: String,
    pub script: Vec<String>,
    pub minutes: i64,
}

/// What one member of a pair or a small group does (「夥伴 A」「主持」…).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Role {
    pub role: String,
    pub task: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Step {
    pub phase: String,
    pub title: String,
    pub minutes: i64,
    pub idea: String,
    pub options: Vec<StepOption>,
    pub script: Vec<String>,
    pub stuck_tip: String,
    /// The Bloom levels the step works on (≤3, in Bloom order). Plans made with version 1 of
    /// `prep_activity` have none.
    #[serde(default)]
    pub bloom: Vec<Bloom>,
    /// Version 4 (SIOP A2, A3, A6); plans made before read as empty. One of [`GROUPINGS`].
    #[serde(default)]
    pub grouping: String,
    /// 兩人一組: what each partner does (2); 小組: the member roles (≥3); otherwise none.
    #[serde(default)]
    pub roles: Vec<Role>,
    /// How the teacher monitors (always written for a 小組).
    #[serde(default)]
    pub monitoring: String,
    /// 聽／說／讀／寫, in that order.
    #[serde(default)]
    pub skills: Vec<String>,
    /// 鷹架方式 (modelling, sentence frames, a graphic organiser…).
    #[serde(default)]
    pub scaffolding: String,
}

/// A 關鍵字彙 of the lesson: the word as in the text, what it means or how it is used.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KeyWord {
    pub word: String,
    pub zh: String,
}

/// A 高層次問題 (English), tagged analyze / evaluate / create.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HigherQuestion {
    pub question: String,
    pub bloom: Bloom,
}

/// 規劃教學活動: what the teacher gets (`result` of an `activity` turn).
///
/// The fields follow the SIOP lesson plan (A6): 目標, 關鍵字彙, 教材, 高層次問題, 背景連結, the
/// steps (each with its 分組方式, 語言技能 and 鷹架), 複習與評量. Everything after `goals` but
/// `steps` and `curriculum` is new in version 4 of `prep_activity`; a plan made before reads
/// them as empty.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActivityPlan {
    pub title: String,
    /// The 內容目標 first, then the 語言目標 (version 3); one list without kinds before.
    pub goals: Vec<Goal>,
    #[serde(default)]
    pub key_vocabulary: Vec<KeyWord>,
    /// 教材與教具.
    #[serde(default)]
    pub materials: Vec<String>,
    #[serde(default)]
    pub higher_order_questions: Vec<HigherQuestion>,
    /// 背景連結: the students' experience or prior knowledge the warm-up connects to (A1).
    #[serde(default)]
    pub background: String,
    pub steps: Vec<Step>,
    /// 複習: how the key words and concepts are reviewed.
    #[serde(default)]
    pub review: String,
    /// 評量: how the teacher checks every goal.
    #[serde(default)]
    pub assessment: String,
    /// The candidates actually cited, text from the knowledge base.
    pub curriculum: Vec<CurriculumRef>,
}

// ---------------------------------------------------------------- the teacher's objectives

/// The objectives the teacher gave or adopted for the lesson. They reach `prep_activity` as the
/// variables `{{teacher_content_objective}}`, `{{teacher_language_objective}}` and the combined
/// line `{{teacher_objective}}`; a plan uses the first two word for word as its goals. Empty is
/// none: the model drafts its own goals.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TeacherObjectives {
    pub content: String,
    pub language: String,
    pub combined: String,
}

impl TeacherObjectives {
    /// From a session's 出題選項: the teacher's 內容目標 and 語言目標 fields (two objectives in
    /// one field are joined with 「；」), and the combined line older prompt versions read.
    pub fn of(options: &SessionOptions) -> Self {
        let fields = options.teacher_objectives();
        TeacherObjectives {
            content: fields.content_text(),
            language: fields.language_text(),
            combined: if fields.is_empty() {
                String::new()
            } else {
                fields.combined()
            },
        }
    }

    /// The objective of a goal group ("" when the teacher gave none).
    pub fn of_kind(&self, kind: GoalKind) -> &str {
        match kind {
            GoalKind::Content => self.content.trim(),
            GoalKind::Language => self.language.trim(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResourceItem {
    pub code: String,
    pub text: String,
    pub school_track: Option<String>,
    pub similarity: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MaterialRef {
    pub id: i64,
    pub title: String,
}

/// 找教學資源 (`result` of a `resources` turn).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Resources {
    pub curriculum: Vec<ResourceItem>,
    pub materials: Vec<MaterialRef>,
    pub note: String,
}

/// 直接問 AI (`result` of an `ask` turn, and the model's output).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Answer {
    pub answer: String,
    pub follow_ups: Vec<String>,
}

/// What the model writes for `prep_ask`: the answer as separate paragraphs or points. gemma
/// under a JSON schema tends to end a string where it wants a line break ("以下三個活動：" and
/// nothing after it), so every line is its own array item and the server joins them.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawAnswer {
    pub paragraphs: Vec<String>,
    pub follow_ups: Vec<String>,
}

/// A revision (`result` of a `revise` turn).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Revision {
    pub reply: String,
    pub changed_block_ids: Vec<String>,
    pub version_no: Option<i32>,
    /// What happened to the picture actions (docs/API.md「改圖片」); empty without any.
    #[serde(default)]
    pub pictures: Vec<crate::worksheet::images::actions::PictureOutcome>,
}

// ---------------------------------------------------------------- what the model writes

/// A goal as the model writes it; its group is the array it is in. A 語言目標 (version 4) also
/// has `supports` (`"C1"` / `"C2"`, the 內容目標 it serves), `category` (a
/// [`LanguageCategory`] label) and `relation`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawGoal {
    #[serde(default)]
    pub supports: String,
    #[serde(default)]
    pub category: String,
    pub zh: String,
    pub en: String,
    #[serde(default)]
    pub relation: String,
    pub curriculum_codes: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawRole {
    pub role: String,
    pub task: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawWord {
    pub word: String,
    pub zh: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawQuestion {
    pub question: String,
    pub bloom: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawOption {
    pub title: String,
    pub idea: String,
    pub script: Vec<String>,
    pub minutes: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawStep {
    pub phase: String,
    pub title: String,
    pub minutes: i64,
    pub idea: String,
    /// Version 4 of the prompt (SIOP A2, A3, A6).
    #[serde(default)]
    pub grouping: String,
    #[serde(default)]
    pub roles: Vec<RawRole>,
    #[serde(default)]
    pub monitoring: String,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub scaffolding: String,
    /// Version 2 of the prompt; an id outside the set is dropped by the server.
    #[serde(default)]
    pub bloom: Vec<String>,
    pub options: Vec<RawOption>,
    pub script: Vec<String>,
    pub stuck_tip: String,
}

/// What the model writes for `prep_activity` (version 4): the two goal groups are two arrays,
/// so the schema itself asks for both (1–2 each); the server puts them into `goals` with their
/// `kind`. The A6 fields follow, in the order of the SIOP lesson plan.
#[derive(Debug, Clone, Deserialize)]
pub struct RawPlan {
    pub title: String,
    pub content_goals: Vec<RawGoal>,
    pub language_goals: Vec<RawGoal>,
    #[serde(default)]
    pub key_vocabulary: Vec<RawWord>,
    #[serde(default)]
    pub materials: Vec<String>,
    #[serde(default)]
    pub higher_order_questions: Vec<RawQuestion>,
    #[serde(default)]
    pub background: String,
    pub steps: Vec<RawStep>,
    #[serde(default)]
    pub review: String,
    #[serde(default)]
    pub assessment: String,
}

/// The schema key of a goal group.
fn goals_key(kind: GoalKind) -> &'static str {
    match kind {
        GoalKind::Content => "content_goals",
        GoalKind::Language => "language_goals",
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawChange {
    pub block_id: String,
    pub title: String,
    pub instructions: String,
    pub items: Vec<crate::worksheet::template::RawOutItem>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawRevision {
    pub reply: String,
    pub changes: Vec<RawChange>,
    #[serde(default)]
    pub picture_actions: Vec<crate::worksheet::images::actions::RawPictureAction>,
}

// ---------------------------------------------------------------- schemas (properties in written order)

pub const ACTIVITY_SCHEMA_NAME: &str = "prep_activity";
pub const ASK_SCHEMA_NAME: &str = "prep_ask";
pub const REVISE_SCHEMA_NAME: &str = "prep_revise";

fn int() -> Value {
    json!({ "type": "integer" })
}

/// An array of strings from `allowed` (at most `max`); an empty `allowed` allows none.
fn codes_schema(allowed: &[String], max: usize) -> Value {
    if allowed.is_empty() {
        return json!({ "type": "array", "items": s_max(CODE_CHARS), "maxItems": 0 });
    }
    let labels: Vec<&str> = allowed.iter().map(String::as_str).collect();
    json!({ "type": "array", "items": enum_of(&labels), "maxItems": max })
}

/// The six Bloom ids, in Bloom order.
fn bloom_ids() -> Vec<&'static str> {
    Bloom::ALL.iter().map(|b| b.as_str()).collect()
}

/// An array of `items`, `min`–`max` of them.
fn array_of(items: Value, min: usize, max: usize) -> Value {
    json!({ "type": "array", "items": items, "minItems": min, "maxItems": max })
}

/// How many goals a group may have: exactly one when the teacher gave it (kept word for word),
/// otherwise 1–2.
fn group_max(teacher: &TeacherObjectives, kind: GoalKind) -> usize {
    if teacher.of_kind(kind).is_empty() {
        GOAL_GROUP_MAX
    } else {
        1
    }
}

/// The ids a 語言目標 may name as the 內容目標 it serves: `C1`, `C2`… up to `count`.
pub fn content_refs(count: usize) -> Vec<String> {
    (1..=count).map(|n| format!("C{n}")).collect()
}

/// `prep_activity` (version 4), properties in the order they are written:
///
/// `title`; `content_goals` (1–2: `zh`, `en`, `curriculum_codes`), then `language_goals` derived
/// from them (1–2: `supports`, `category`, `zh`, `en`, `relation`, `curriculum_codes`), a group
/// the teacher gave exactly one; `key_vocabulary`; `materials`; `higher_order_questions`;
/// `background`; `steps` (each: `phase`, `title`, `minutes`, `idea`, `grouping`, `roles`,
/// `monitoring`, `skills`, `scaffolding`, `bloom`, `options`, `script`, `stuck_tip`); `review`;
/// `assessment`. Curriculum codes only from `candidates` (enum), every label from its fixed set
/// (enum), every string and array capped.
pub fn activity_schema(candidates: &[String], teacher: &TeacherObjectives) -> Value {
    let option = obj(vec![
        ("title", s_max(STEP_TITLE_CHARS)),
        ("idea", s_max(IDEA_CHARS)),
        (
            "script",
            strings_max(OPTION_SCRIPT_LINES, SCRIPT_LINE_CHARS),
        ),
        ("minutes", int()),
    ]);
    let role = obj(vec![
        ("role", s_max(ROLE_CHARS)),
        ("task", s_max(ROLE_TASK_CHARS)),
    ]);
    let step = obj(vec![
        ("phase", enum_of(&PHASES)),
        ("title", s_max(STEP_TITLE_CHARS)),
        ("minutes", int()),
        ("idea", s_max(IDEA_CHARS)),
        ("grouping", enum_of(&GROUPINGS)),
        ("roles", array_of(role, 0, ROLES_MAX)),
        ("monitoring", s_max(MONITORING_CHARS)),
        ("skills", array_of(enum_of(&SKILLS), 1, SKILLS.len())),
        ("scaffolding", s_max(SCAFFOLDING_CHARS)),
        ("bloom", array_of(enum_of(&bloom_ids()), 1, STEP_BLOOM_MAX)),
        (
            "options",
            json!({ "type": "array", "items": option, "maxItems": OPTIONS_MAX }),
        ),
        ("script", strings_max(STEP_SCRIPT_LINES, SCRIPT_LINE_CHARS)),
        ("stuck_tip", s_max(STUCK_TIP_CHARS)),
    ]);
    let content_max = group_max(teacher, GoalKind::Content);
    let content_goal = obj(vec![
        ("zh", s_max(GOAL_ZH_CHARS)),
        ("en", s_max(GOAL_EN_CHARS)),
        ("curriculum_codes", codes_schema(candidates, GOAL_CODES_MAX)),
    ]);
    let refs = content_refs(content_max);
    let refs: Vec<&str> = refs.iter().map(String::as_str).collect();
    let categories: Vec<&str> = LanguageCategory::ALL.iter().map(|c| c.label()).collect();
    let language_goal = obj(vec![
        ("supports", enum_of(&refs)),
        ("category", enum_of(&categories)),
        ("zh", s_max(GOAL_ZH_CHARS)),
        ("en", s_max(GOAL_EN_CHARS)),
        ("relation", s_max(RELATION_CHARS)),
        ("curriculum_codes", codes_schema(candidates, GOAL_CODES_MAX)),
    ]);
    let higher: Vec<&str> = HIGHER_ORDER.iter().map(|b| b.as_str()).collect();
    obj(vec![
        ("title", s_max(PLAN_TITLE_CHARS)),
        (
            goals_key(GoalKind::Content),
            array_of(content_goal, GOAL_GROUP_MIN, content_max),
        ),
        (
            goals_key(GoalKind::Language),
            array_of(
                language_goal,
                GOAL_GROUP_MIN,
                group_max(teacher, GoalKind::Language),
            ),
        ),
        (
            "key_vocabulary",
            array_of(
                obj(vec![
                    ("word", s_max(KEY_WORD_CHARS)),
                    ("zh", s_max(KEY_WORD_ZH_CHARS)),
                ]),
                1,
                KEY_WORDS_MAX,
            ),
        ),
        (
            "materials",
            array_of(s_max(MATERIAL_CHARS), 1, MATERIALS_MAX),
        ),
        (
            "higher_order_questions",
            array_of(
                obj(vec![
                    ("question", s_max(QUESTION_CHARS)),
                    ("bloom", enum_of(&higher)),
                ]),
                1,
                QUESTIONS_MAX,
            ),
        ),
        ("background", s_max(BACKGROUND_CHARS)),
        ("steps", array_of(step, STEPS_MIN, STEPS_MAX)),
        ("review", s_max(REVIEW_CHARS)),
        ("assessment", s_max(REVIEW_CHARS)),
    ])
}

/// `prep_ask`.
pub fn ask_schema() -> Value {
    obj(vec![
        (
            "paragraphs",
            json!({
                "type": "array",
                "items": s_max(ANSWER_PARAGRAPH_CHARS),
                "minItems": 1,
                "maxItems": ANSWER_PARAGRAPHS,
            }),
        ),
        ("follow_ups", strings_max(FOLLOW_UPS_MAX, FOLLOW_UP_CHARS)),
    ])
}

/// `prep_revise`: `block_id` only from the worksheet's blocks; items capped like a template block.
pub fn revise_schema(block_ids: &[String], slot_ids: &[String], picture_max: usize) -> Value {
    use crate::worksheet::template::{
        ITEM_ANSWER_CHARS, ITEM_OPTIONS, ITEM_OPTION_CHARS, ITEM_PROMPT_CHARS,
        SECTION_INSTRUCTIONS_CHARS, SECTION_TITLE_CHARS,
    };
    let block_id = if block_ids.is_empty() {
        s_max(CODE_CHARS)
    } else {
        let ids: Vec<&str> = block_ids.iter().map(String::as_str).collect();
        enum_of(&ids)
    };
    let item = obj(vec![
        ("answer_format", enum_of(&ANSWER_FORMATS)),
        ("prompt", s_max(ITEM_PROMPT_CHARS)),
        ("options", strings_max(ITEM_OPTIONS, ITEM_OPTION_CHARS)),
        ("answer", s_max(ITEM_ANSWER_CHARS)),
        // 表格整理 only (null for the other formats): what the table compares.
        ("table", crate::worksheet::table::schema()),
    ]);
    let change = obj(vec![
        ("block_id", block_id),
        ("title", s_max(SECTION_TITLE_CHARS)),
        ("instructions", s_max(SECTION_INSTRUCTIONS_CHARS)),
        (
            "items",
            json!({ "type": "array", "items": item, "maxItems": CHANGE_ITEMS_MAX }),
        ),
    ]);
    let max_changes = if block_ids.is_empty() { 0 } else { CHANGES_MAX };
    obj(vec![
        ("reply", s_max(REPLY_CHARS)),
        (
            "changes",
            json!({ "type": "array", "items": change, "maxItems": max_changes }),
        ),
        (
            "picture_actions",
            crate::worksheet::images::actions::schema(block_ids, slot_ids, picture_max),
        ),
    ])
}

// ---------------------------------------------------------------- clean-up

/// Markup removed, then cut to `max` characters (a provider that does not enforce
/// `maxLength` cannot make the stored result longer than the contract says).
pub fn clean_text(s: &str, max: usize) -> String {
    let cleaned = clean_markup(s);
    if cleaned.chars().count() <= max {
        cleaned
    } else {
        cleaned
            .chars()
            .take(max)
            .collect::<String>()
            .trim_end()
            .to_string()
    }
}

fn clean_lines(lines: &[String], max_lines: usize, max_chars: usize) -> Vec<String> {
    lines
        .iter()
        .map(|l| clean_text(l, max_chars))
        .filter(|l| !l.is_empty())
        .take(max_lines)
        .collect()
}

fn clamp_minutes(m: i64) -> i64 {
    m.clamp(MINUTES_MIN, MINUTES_MAX)
}

/// A step's Bloom levels: ids outside the set and repeats dropped, the first three the model
/// wrote kept, then put in Bloom order.
pub fn step_bloom(raw: &[String]) -> Vec<Bloom> {
    let mut out: Vec<Bloom> = Vec::new();
    for b in raw.iter().filter_map(|s| Bloom::parse(s)) {
        if !out.contains(&b) && out.len() < STEP_BLOOM_MAX {
            out.push(b);
        }
    }
    out.sort();
    out
}

/// English for a student-facing goal: some Latin letters and no Chinese characters.
fn is_english(s: &str) -> bool {
    s.chars().any(|c| c.is_ascii_alphabetic()) && !contains_cjk(s)
}

/// Verbs that do not show what a student can do (SIOP R1: no learn / know / understand), as the
/// first verb of a goal. Matched after 「學生能（夠）」 or "Students will be able to".
const VAGUE_ZH: [&str; 13] = [
    "了解", "瞭解", "理解", "認識", "知道", "學會", "學習", "明白", "熟悉", "體會", "欣賞", "懂得",
    "懂",
];
const VAGUE_EN: [&str; 11] = [
    "learn",
    "know",
    "understand",
    "appreciate",
    "realize",
    "realise",
    "comprehend",
    "grasp",
    "be aware",
    "become aware",
    "be familiar",
];

/// The vague verb a goal starts with, if any (in either line).
pub fn vague_verb(zh: &str, en: &str) -> Option<&'static str> {
    let zh = zh.trim();
    let body = ["學生能夠", "學生能", "學生可以", "能夠", "能"]
        .iter()
        .find_map(|p| zh.strip_prefix(p))
        .unwrap_or(zh)
        .trim_start();
    if let Some(v) = VAGUE_ZH.iter().find(|v| body.starts_with(*v)) {
        return Some(v);
    }
    let en = en.trim().to_lowercase();
    let body = [
        "students will be able to",
        "students are able to",
        "students will",
        "students should",
        "students can",
        "i will be able to",
        "i will",
        "i can",
    ]
    .iter()
    .find_map(|p| en.strip_prefix(p))
    .unwrap_or(&en)
    .trim_start();
    VAGUE_EN.iter().copied().find(|v| {
        body.strip_prefix(v)
            .is_some_and(|rest| !rest.starts_with(|c: char| c.is_ascii_alphabetic()))
    })
}

/// Pieces two texts share that say they are about the same thing: Chinese bigrams (without
/// 「學生」「生能」, which every goal has) and English words of four letters or more (without the
/// words every goal line has).
fn shares_meaning(a: &str, b: &str) -> bool {
    const COMMON_EN: [&str; 8] = [
        "students", "will", "able", "with", "that", "this", "their", "from",
    ];
    let bigrams = |s: &str| -> HashSet<String> {
        let cjk: Vec<char> = s.chars().filter(|c| contains_cjk(&c.to_string())).collect();
        cjk.windows(2)
            .map(|w| w.iter().collect::<String>())
            .filter(|g| g != "學生" && g != "生能")
            .collect()
    };
    let words = |s: &str| -> HashSet<String> {
        s.split(|c: char| !c.is_ascii_alphabetic())
            .map(str::to_lowercase)
            .filter(|w| w.len() >= 4 && !COMMON_EN.contains(&w.as_str()))
            .collect()
    };
    !bigrams(a).is_disjoint(&bigrams(b)) || !words(a).is_disjoint(&words(b))
}

/// Whether `relation` states how a 語言目標 serves `content`, in the fixed form
/// 「學生要用［語言目標］，才能［內容目標］」: both halves written, and the half after 「才能」
/// about the same thing as the 內容目標 (they share a Chinese bigram or an English word).
pub fn relation_holds(relation: &str, content: &Goal) -> bool {
    let r = relation
        .trim()
        .trim_start_matches(['「', '“', '"'])
        .trim_start();
    let Some(rest) = r.strip_prefix("學生要用") else {
        return false;
    };
    let Some((means, goal)) = rest.split_once("才能") else {
        return false;
    };
    let means = means.trim().trim_end_matches(['，', ',', '、']).trim();
    let goal = goal
        .trim()
        .trim_end_matches(['。', '.', '」', '”', '"'])
        .trim();
    means.chars().count() >= 2
        && goal.chars().count() >= 2
        && (shares_meaning(goal, &content.zh) || shares_meaning(goal, &content.en))
}

fn clean_codes(raw: &[String], allowed: &HashSet<&str>) -> Vec<String> {
    let mut seen = HashSet::new();
    raw.iter()
        .map(|c| c.trim().to_string())
        .filter(|c| allowed.contains(c.as_str()) && seen.insert(c.clone()))
        .take(GOAL_CODES_MAX)
        .collect()
}

/// A goal the teacher gave, word for word: in `zh` when it has Chinese, otherwise in `en`; the
/// other line is the model's (when it is in the right language), the codes too.
fn teacher_goal(
    kind: GoalKind,
    text: &str,
    raw: Option<&RawGoal>,
    allowed: &HashSet<&str>,
) -> Goal {
    let text: String = text.trim().chars().take(TEACHER_GOAL_CHARS).collect();
    let model_zh = raw
        .map(|g| clean_text(&g.zh, GOAL_ZH_CHARS))
        .filter(|z| contains_cjk(z))
        .unwrap_or_default();
    let model_en = raw
        .map(|g| clean_text(&g.en, GOAL_EN_CHARS))
        .filter(|e| is_english(e))
        .unwrap_or_default();
    let (zh, en) = if contains_cjk(&text) {
        (text, model_en)
    } else {
        (model_zh, text)
    };
    Goal {
        from_teacher: true,
        ..Goal::new(
            kind,
            &zh,
            &en,
            raw.map(|g| clean_codes(&g.curriculum_codes, allowed))
                .unwrap_or_default(),
        )
    }
}

/// The 內容目標: the teacher's (one, word for word) or else the model's usable ones, at most two.
/// A goal whose `zh` has no Chinese, whose `en` is not English or that starts with a vague verb
/// is dropped. Also returns, for every goal the model wrote, where it ended up (its index among
/// the kept goals), so a 語言目標's `supports` can be followed.
fn clean_content_goals(
    raw: &[RawGoal],
    teacher: &str,
    allowed: &HashSet<&str>,
) -> (Vec<Goal>, Vec<Option<usize>>) {
    if !teacher.is_empty() {
        let goal = teacher_goal(GoalKind::Content, teacher, raw.first(), allowed);
        let mut map = vec![None; raw.len().max(1)];
        map[0] = Some(0);
        return (vec![goal], map);
    }
    let mut kept: Vec<Goal> = Vec::new();
    let mut map = Vec::with_capacity(raw.len());
    for g in raw {
        let zh = clean_text(&g.zh, GOAL_ZH_CHARS);
        let en = clean_text(&g.en, GOAL_EN_CHARS);
        let usable = contains_cjk(&zh)
            && is_english(&en)
            && vague_verb(&zh, &en).is_none()
            && kept.len() < GOAL_GROUP_MAX;
        if usable {
            map.push(Some(kept.len()));
            let codes = clean_codes(&g.curriculum_codes, allowed);
            kept.push(Goal::new(GoalKind::Content, &zh, &en, codes));
        } else {
            map.push(None);
        }
    }
    (kept, map)
}

/// The 內容目標 a 語言目標 names (`"C2"` → the second one the model wrote), as its index among
/// the kept 內容目標.
fn supported(supports: &str, map: &[Option<usize>]) -> Option<usize> {
    let n: usize = supports
        .trim()
        .trim_start_matches(['C', 'c'])
        .parse()
        .ok()?;
    map.get(n.checked_sub(1)?).copied().flatten()
}

/// The 語言目標: the teacher's (one, word for word, its relation kept when it holds), or else
/// the model's usable ones. A model goal is dropped when its lines are not Chinese / English,
/// when it starts with a vague verb, when the 內容目標 it names was not kept, or when its
/// `relation` does not hold ([`relation_holds`]): a 語言目標 that cannot say how it serves a
/// 內容目標 is not used, however well it is written. The kept goals are put strategy first
/// (stable), then at most two are kept.
fn clean_language_goals(
    raw: &[RawGoal],
    teacher: &str,
    allowed: &HashSet<&str>,
    content: &[Goal],
    map: &[Option<usize>],
) -> Vec<Goal> {
    let link = |g: &RawGoal| -> Option<(usize, String)> {
        let target = supported(&g.supports, map)?;
        let relation = clean_text(&g.relation, RELATION_CHARS);
        relation_holds(&relation, &content[target]).then_some((target, relation))
    };
    if !teacher.is_empty() {
        let first = raw.first();
        let mut goal = teacher_goal(GoalKind::Language, teacher, first, allowed);
        goal.category = first.and_then(|g| LanguageCategory::from_label(&g.category));
        if let Some((target, relation)) = first.and_then(link) {
            goal.supports = Some(target + 1);
            goal.relation = relation;
        }
        return vec![goal];
    }
    let mut kept: Vec<Goal> = raw
        .iter()
        .filter_map(|g| {
            let zh = clean_text(&g.zh, GOAL_ZH_CHARS);
            let en = clean_text(&g.en, GOAL_EN_CHARS);
            if !contains_cjk(&zh) || !is_english(&en) || vague_verb(&zh, &en).is_some() {
                return None;
            }
            let (target, relation) = link(g)?;
            Some(Goal {
                category: LanguageCategory::from_label(&g.category),
                supports: Some(target + 1),
                relation,
                ..Goal::new(
                    GoalKind::Language,
                    &zh,
                    &en,
                    clean_codes(&g.curriculum_codes, allowed),
                )
            })
        })
        .collect();
    kept.sort_by_key(|g| g.category != Some(LanguageCategory::Strategy));
    kept.truncate(GOAL_GROUP_MAX);
    kept
}

/// The roles of a step as its grouping wants them: a pair keeps two, a small group up to six,
/// the whole class and individual work none. A role without a task is dropped.
fn clean_roles(raw: &[RawRole], grouping: &str) -> Vec<Role> {
    let max = match grouping {
        GROUP_PAIR => 2,
        GROUP_SMALL => ROLES_MAX,
        _ => 0,
    };
    raw.iter()
        .map(|r| Role {
            role: clean_text(&r.role, ROLE_CHARS),
            task: clean_text(&r.task, ROLE_TASK_CHARS),
        })
        .filter(|r| !r.task.is_empty())
        .take(max)
        .collect()
}

/// 聽／說／讀／寫 the step names, once each, in that order.
fn clean_skills(raw: &[String]) -> Vec<String> {
    SKILLS
        .iter()
        .filter(|s| raw.iter().any(|r| r.trim() == **s))
        .map(|s| s.to_string())
        .collect()
}

fn clean_step(s: &RawStep) -> Step {
    let phase = s.phase.trim().to_string();
    let options = if phase == PHASE_WARMUP {
        s.options
            .iter()
            .map(|o| StepOption {
                title: clean_text(&o.title, STEP_TITLE_CHARS),
                idea: clean_text(&o.idea, IDEA_CHARS),
                script: clean_lines(&o.script, OPTION_SCRIPT_LINES, SCRIPT_LINE_CHARS),
                minutes: clamp_minutes(o.minutes),
            })
            .filter(|o| !o.title.is_empty() || !o.idea.is_empty())
            .take(OPTIONS_MAX)
            .collect()
    } else {
        Vec::new()
    };
    let grouping = GROUPINGS
        .iter()
        .find(|g| **g == s.grouping.trim())
        .map(|g| g.to_string())
        .unwrap_or_default();
    Step {
        title: clean_text(&s.title, STEP_TITLE_CHARS),
        minutes: clamp_minutes(s.minutes),
        idea: clean_text(&s.idea, IDEA_CHARS),
        options,
        script: clean_lines(&s.script, STEP_SCRIPT_LINES, SCRIPT_LINE_CHARS),
        stuck_tip: clean_text(&s.stuck_tip, STUCK_TIP_CHARS),
        bloom: step_bloom(&s.bloom),
        roles: clean_roles(&s.roles, &grouping),
        monitoring: clean_text(&s.monitoring, MONITORING_CHARS),
        skills: clean_skills(&s.skills),
        scaffolding: clean_text(&s.scaffolding, SCAFFOLDING_CHARS),
        grouping,
        phase,
    }
}

/// What the retry should fix in the steps (SIOP A2, A3): at least two groupings in the lesson; a
/// pair says what both partners do; a small group lists at least three member roles and how the
/// teacher monitors; all four skills are used somewhere.
fn step_problems(steps: &[Step]) -> Vec<String> {
    let mut problems = Vec::new();
    let used: Vec<&str> = GROUPINGS
        .iter()
        .copied()
        .filter(|g| steps.iter().any(|s| s.grouping == *g))
        .collect();
    if used.len() < 2 {
        problems.push(format!(
            "一節課至少要用兩種分組方式（grouping：全班、兩人一組、小組、個人），這次只有：{}。",
            if used.is_empty() {
                "（沒有）".to_string()
            } else {
                used.join("、")
            }
        ));
    }
    for (i, s) in steps.iter().enumerate() {
        let name = format!("第 {} 步「{}」", i + 1, s.title);
        if s.grouping == GROUP_PAIR && s.roles.len() != 2 {
            problems.push(format!(
                "{name}是兩人一組：roles 要寫 2 項，分別寫夥伴 A、夥伴 B 各做什麼。"
            ));
        }
        if s.grouping == GROUP_SMALL {
            if s.roles.len() < SMALL_GROUP_ROLES_MIN {
                problems.push(format!(
                    "{name}是小組：roles 要寫出每個成員的角色與責任，至少 {SMALL_GROUP_ROLES_MIN} 項（例如主持、記錄、計時、報告）。"
                ));
            }
            if s.monitoring.is_empty() {
                problems.push(format!(
                    "{name}是小組：monitoring 要寫教師怎麼巡視、什麼時候介入。"
                ));
            }
        }
    }
    let missing: Vec<&str> = SKILLS
        .iter()
        .copied()
        .filter(|k| !steps.iter().any(|s| s.skills.iter().any(|x| x == k)))
        .collect();
    if !missing.is_empty() {
        problems.push(format!(
            "整節課要用到聽、說、讀、寫四種技能（skills），這次沒有用到：{}。",
            missing.join("、")
        ));
    }
    problems
}

/// The checked plan and what the retry should still fix, or (`Err`) why the plan cannot be used
/// at all.
///
/// Goals ([`clean_content_goals`], [`clean_language_goals`]): the 內容目標, then the 語言目標
/// derived from them, at most two each; a group the teacher gave is that one goal, word for
/// word. A group left without a usable goal is a problem for the retry, and so is a set of
/// 語言目標 without a 學習策略 goal (when the teacher did not give the 語言目標), a teacher's
/// 語言目標 whose relation does not hold, a missing 背景連結, and the steps' groupings, roles
/// and skills ([`step_problems`]); the plan stays usable: when the retry does no better, the
/// teacher still gets it. No usable goal at all, or fewer than three usable steps, makes the
/// plan unusable. Codes outside `candidates` are dropped, a step whose phase is not one of the
/// three labels is dropped, minutes are clamped to 1–50, options are kept for 引起動機 only, a
/// step keeps at most three Bloom levels (in Bloom order), a whole-class or individual step
/// keeps no roles, and every text is cleaned and capped. The curriculum list is filled from
/// `candidates` (text from the knowledge base), in their order.
pub fn clean_plan(
    raw: &RawPlan,
    candidates: &[CurriculumRef],
    teacher: &TeacherObjectives,
) -> Result<(ActivityPlan, Vec<String>), String> {
    let allowed: HashSet<&str> = candidates.iter().map(|c| c.code.as_str()).collect();
    let (content, map) = clean_content_goals(
        &raw.content_goals,
        teacher.of_kind(GoalKind::Content),
        &allowed,
    );
    let language = clean_language_goals(
        &raw.language_goals,
        teacher.of_kind(GoalKind::Language),
        &allowed,
        &content,
        &map,
    );
    let mut missing: Vec<String> = Vec::new();
    if content.is_empty() {
        missing.push(format!(
            "content_goals 沒有可用的內容目標：要有 {GOAL_GROUP_MIN}–{GOAL_GROUP_MAX} 個，每個的 zh 用繁體中文、en 用英文，各寫一句，用看得到表現的動詞（不要用 learn、know、understand、了解、知道、學會）。"
        ));
    }
    if language.is_empty() {
        missing.push(format!(
            "language_goals 沒有可用的語言目標：要有 {GOAL_GROUP_MIN}–{GOAL_GROUP_MAX} 個，每個的 zh 用繁體中文、en 用英文，各寫一句；supports 寫它支持的內容目標，relation 寫成「學生要用……，才能……」，「才能」後面接那個內容目標。"
        ));
    }
    let goals: Vec<Goal> = content.into_iter().chain(language).collect();
    let steps: Vec<Step> = raw
        .steps
        .iter()
        .filter(|s| PHASES.contains(&s.phase.trim()))
        .map(clean_step)
        .filter(|s| !s.title.is_empty() || !s.idea.is_empty())
        .take(STEPS_MAX)
        .collect();
    let mut problems = Vec::new();
    if goals.is_empty() {
        problems.extend(missing.iter().cloned());
    }
    if steps.len() < STEPS_MIN {
        problems.push(format!(
            "steps 要有 {STEPS_MIN}–{STEPS_MAX} 個步驟（phase 只能是「引起動機」「發展活動」「統整與評量」），這次可用的只有 {} 個。",
            steps.len()
        ));
    }
    if !problems.is_empty() {
        return Err(problems.join(" "));
    }
    let language_goals: Vec<&Goal> = goals
        .iter()
        .filter(|g| g.kind == Some(GoalKind::Language))
        .collect();
    if teacher.of_kind(GoalKind::Language).is_empty()
        && !language_goals.is_empty()
        && !language_goals
            .iter()
            .any(|g| g.category == Some(LanguageCategory::Strategy))
    {
        missing.push(
            "語言目標以語言學習策略優先：第一個語言目標的 category 要是「學習策略」（例如讀不懂時重讀、先預測再確認、讀前先看標題）。"
                .to_string(),
        );
    }
    if let Some(g) = language_goals
        .iter()
        .find(|g| g.from_teacher && g.relation.is_empty())
    {
        missing.push(format!(
            "教師的語言目標「{}」要寫 supports 與 relation：「學生要用……，才能……」，「才能」後面接它支持的內容目標。",
            if g.zh.is_empty() { &g.en } else { &g.zh }
        ));
    }
    let background = clean_text(&raw.background, BACKGROUND_CHARS);
    if background.is_empty() {
        missing.push(
            "background 是空的：要寫出引起動機連結的學生生活經驗或舊知，以及怎麼連結。".to_string(),
        );
    }
    missing.extend(step_problems(&steps));
    let cited: HashSet<&str> = goals
        .iter()
        .flat_map(|g| g.curriculum_codes.iter().map(String::as_str))
        .collect();
    let curriculum = candidates
        .iter()
        .filter(|c| cited.contains(c.code.as_str()))
        .cloned()
        .collect();
    let mut seen_words = HashSet::new();
    let plan = ActivityPlan {
        title: clean_text(&raw.title, PLAN_TITLE_CHARS),
        goals,
        key_vocabulary: raw
            .key_vocabulary
            .iter()
            .map(|w| KeyWord {
                word: clean_text(&w.word, KEY_WORD_CHARS),
                zh: clean_text(&w.zh, KEY_WORD_ZH_CHARS),
            })
            .filter(|w| {
                w.word.chars().any(|c| c.is_ascii_alphabetic())
                    && seen_words.insert(w.word.to_lowercase())
            })
            .take(KEY_WORDS_MAX)
            .collect(),
        materials: clean_lines(&raw.materials, MATERIALS_MAX, MATERIAL_CHARS),
        higher_order_questions: raw
            .higher_order_questions
            .iter()
            .filter_map(|q| {
                let bloom = Bloom::parse(q.bloom.trim()).filter(|b| HIGHER_ORDER.contains(b))?;
                let question = clean_text(&q.question, QUESTION_CHARS);
                question
                    .chars()
                    .any(|c| c.is_ascii_alphabetic())
                    .then_some(HigherQuestion { question, bloom })
            })
            .take(QUESTIONS_MAX)
            .collect(),
        background,
        steps,
        review: clean_text(&raw.review, REVIEW_CHARS),
        assessment: clean_text(&raw.assessment, REVIEW_CHARS),
        curriculum,
    };
    Ok((plan, missing))
}

/// The checked answer: the paragraphs joined one per line, plain text, capped; follow-ups
/// cleaned, empty ones dropped.
pub fn clean_answer(raw: &RawAnswer) -> Result<Answer, String> {
    let joined = raw
        .paragraphs
        .iter()
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let answer = clean_text(&strip_markdown(&joined), ANSWER_CHARS);
    if answer.is_empty() {
        return Err("answer 是空的。".to_string());
    }
    Ok(Answer {
        answer,
        follow_ups: clean_lines(&raw.follow_ups, FOLLOW_UPS_MAX, FOLLOW_UP_CHARS),
    })
}

/// Markdown the prompt asks the model not to write: heading marks and bullet stars at the
/// start of a line (`clean_markup` already removes `**`).
fn strip_markdown(s: &str) -> String {
    s.lines()
        .map(|l| {
            let t = l.trim_start();
            let t = t.trim_start_matches('#').trim_start();
            match t.strip_prefix("* ") {
                Some(rest) => format!("・{rest}"),
                None => t.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The ideas of a plan's 引起動機 steps (title + idea), for a worksheet's warm-up brief.
pub fn warmup_ideas(plan: &ActivityPlan) -> Vec<String> {
    let mut out = Vec::new();
    for s in plan.steps.iter().filter(|s| s.phase == PHASE_WARMUP) {
        let ideas: Vec<(&str, &str)> = if s.options.is_empty() {
            vec![(s.title.as_str(), s.idea.as_str())]
        } else {
            s.options
                .iter()
                .map(|o| (o.title.as_str(), o.idea.as_str()))
                .collect()
        };
        for (title, idea) in ideas {
            let line = match (title.trim(), idea.trim()) {
                ("", i) => i.to_string(),
                (t, "") => t.to_string(),
                (t, i) => format!("{t}：{i}"),
            };
            if !line.is_empty() {
                out.push(line);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn role(role: &str, task: &str) -> RawRole {
        RawRole {
            role: role.into(),
            task: task.into(),
        }
    }

    fn raw_step(phase: &str, minutes: i64, options: usize) -> RawStep {
        RawStep {
            phase: phase.into(),
            title: format!("**{phase}**"),
            minutes,
            idea: "想法".into(),
            grouping: GROUP_CLASS.into(),
            roles: vec![],
            monitoring: String::new(),
            skills: SKILLS.iter().map(|s| s.to_string()).collect(),
            scaffolding: "示範".into(),
            bloom: vec![
                "evaluate".into(),
                "remember".into(),
                "Thinking".into(),
                "remember".into(),
                "apply".into(),
                "create".into(),
            ],
            options: (0..options)
                .map(|i| RawOption {
                    title: format!("選項{i}"),
                    idea: "做法".into(),
                    script: vec!["Look at this.".into(); 6],
                    minutes: 99,
                })
                .collect(),
            script: vec!["Hello.".into(); 9],
            stuck_tip: String::new(),
        }
    }

    fn candidates() -> Vec<CurriculumRef> {
        vec![
            CurriculumRef {
                code: "A-1".into(),
                text: "條文一".into(),
            },
            CurriculumRef {
                code: "B-2".into(),
                text: "條文二".into(),
            },
        ]
    }

    fn raw_goal(zh: &str, en: &str, codes: &[&str]) -> RawGoal {
        RawGoal {
            zh: zh.into(),
            en: en.into(),
            curriculum_codes: codes.iter().map(|c| c.to_string()).collect(),
            ..RawGoal::default()
        }
    }

    /// A 語言目標 serving `supports` (`"C1"`…), of `category`, with its relation.
    fn raw_language(supports: &str, category: &str, zh: &str, en: &str, relation: &str) -> RawGoal {
        RawGoal {
            supports: supports.into(),
            category: category.into(),
            relation: relation.into(),
            ..raw_goal(zh, en, &[])
        }
    }

    /// Steps that keep every A2/A3 rule: 兩人一組 (two partners), 小組 (four roles, monitored),
    /// 個人, and all four skills.
    fn steps() -> Vec<RawStep> {
        let mut warm = raw_step(PHASE_WARMUP, 5, 0);
        warm.grouping = GROUP_PAIR.into();
        warm.roles = vec![role("夥伴 A", "描述圖片"), role("夥伴 B", "猜主題")];
        let mut develop = raw_step(PHASE_DEVELOP, 30, 0);
        develop.grouping = GROUP_SMALL.into();
        develop.roles = vec![
            role("主持", "帶討論"),
            role("記錄", "寫重點"),
            role("計時", "提醒時間"),
            role("報告", "向全班說明"),
        ];
        develop.monitoring = "巡視各組".into();
        let mut wrap = raw_step(PHASE_WRAP_UP, 10, 0);
        wrap.grouping = GROUP_SOLO.into();
        vec![warm, develop, wrap]
    }

    fn content_main() -> RawGoal {
        raw_goal(
            "學生能說出各段的重點",
            "Students will be able to tell the main point of each paragraph.",
            &[],
        )
    }

    fn strategy() -> RawGoal {
        raw_language(
            "C1",
            "學習策略",
            "學生能在讀不懂時重讀並畫出關鍵句",
            "Students will be able to reread and underline the key sentence.",
            "學生要用重讀並畫出關鍵句的策略，才能說出各段的重點。",
        )
    }

    fn plan_with(content: Vec<RawGoal>, language: Vec<RawGoal>, steps: Vec<RawStep>) -> RawPlan {
        RawPlan {
            title: "計畫".into(),
            content_goals: content,
            language_goals: language,
            key_vocabulary: vec![],
            materials: vec![],
            higher_order_questions: vec![],
            background: "借東西的經驗".into(),
            steps,
            review: String::new(),
            assessment: String::new(),
        }
    }

    fn none() -> TeacherObjectives {
        TeacherObjectives::default()
    }

    #[test]
    fn plan_is_cleaned_and_codes_come_from_the_candidates() {
        let mut raw = plan_with(
            vec![raw_goal(
                "學生能說出主旨",
                "Students will be able to tell the main idea.",
                &["B-2", "MADE-UP", "B-2"],
            )],
            vec![raw_language(
                "C1",
                "學習策略",
                "學生能先看標題預測主旨",
                "Students will be able to predict the main idea from the title.",
                "學生要用先看標題再預測的策略，才能說出主旨",
            )],
            vec![
                raw_step(PHASE_WARMUP, 0, 5),
                raw_step("暖身", 5, 0),
                raw_step(PHASE_DEVELOP, 80, 2),
                raw_step(PHASE_WRAP_UP, 10, 0),
            ],
        );
        raw.steps[0].grouping = GROUP_PAIR.into();
        raw.steps[0].roles = vec![
            role("夥伴 A", "說"),
            role("夥伴 B", "聽"),
            role("多的", "x"),
        ];
        let (plan, problems) = clean_plan(&raw, &candidates(), &none()).unwrap();
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(plan.goals[0].curriculum_codes, ["B-2"]);
        assert_eq!(
            plan.curriculum,
            vec![CurriculumRef {
                code: "B-2".into(),
                text: "條文二".into()
            }]
        );
        let phases: Vec<&str> = plan.steps.iter().map(|s| s.phase.as_str()).collect();
        assert_eq!(phases, [PHASE_WARMUP, PHASE_DEVELOP, PHASE_WRAP_UP]);
        assert_eq!(plan.steps[0].minutes, 1);
        assert_eq!(plan.steps[1].minutes, 50);
        assert_eq!(plan.steps[0].options.len(), 3);
        assert_eq!(plan.steps[0].options[0].script.len(), 4);
        assert_eq!(plan.steps[0].options[0].minutes, 50);
        assert!(plan.steps[1].options.is_empty());
        assert_eq!(plan.steps[0].script.len(), 6);
        assert_eq!(plan.steps[0].title, PHASE_WARMUP);
        // A pair keeps two partners.
        assert_eq!(plan.steps[0].roles.len(), 2);
        // At most three levels, the first ones written, in Bloom order.
        assert_eq!(
            plan.steps[0].bloom,
            [Bloom::Remember, Bloom::Apply, Bloom::Evaluate]
        );
        let ideas = warmup_ideas(&plan);
        assert_eq!(ideas.len(), 3);
        assert_eq!(ideas[0], "選項0：做法");
    }

    #[test]
    fn a_plan_without_enough_steps_is_retried() {
        let raw = plan_with(vec![], vec![], vec![raw_step(PHASE_WARMUP, 5, 0)]);
        let err = clean_plan(&raw, &candidates(), &none()).unwrap_err();
        assert!(
            err.contains("content_goals")
                && err.contains("language_goals")
                && err.contains("steps"),
            "{err}"
        );
    }

    #[test]
    fn goals_come_in_two_groups_of_at_most_two_in_both_languages() {
        let raw = plan_with(
            vec![
                raw_goal(
                    "學生能說出**主旨**",
                    "Students will be able to tell the main idea.",
                    &["A-1"],
                ),
                // Not Chinese / not English: dropped.
                raw_goal("Students can retell.", "Students can retell.", &[]),
                raw_goal("學生能說出原因", "學生能說出原因", &[]),
                raw_goal("學生能說出原因", "", &[]),
                raw_goal(
                    "學生能比較兩個角色",
                    "Students will be able to compare the two characters.",
                    &[],
                ),
                // A third usable one: R1 keeps two.
                raw_goal(
                    "學生能評論結局",
                    "Students will be able to comment on the ending.",
                    &[],
                ),
            ],
            vec![raw_language(
                "C1",
                "學習策略",
                &format!("學生能{}", "重讀".repeat(100)),
                &format!("Students will be able to {}", "a".repeat(300)),
                "學生要用重讀的策略，才能說出主旨",
            )],
            steps(),
        );
        let mut raw = raw;
        raw.language_goals[0].curriculum_codes =
            vec!["B-2".into(), "A-1".into(), "MADE-UP".into(), "B-2".into()];
        let (plan, problems) = clean_plan(&raw, &candidates(), &none()).unwrap();
        assert!(problems.is_empty(), "{problems:?}");
        let kinds: Vec<Option<GoalKind>> = plan.goals.iter().map(|g| g.kind).collect();
        assert_eq!(
            kinds,
            [
                Some(GoalKind::Content),
                Some(GoalKind::Content),
                Some(GoalKind::Language),
            ],
            "at most two per group, the content goals first"
        );
        assert_eq!(plan.goals[0].zh, "學生能說出主旨");
        assert_eq!(
            plan.goals[0].en,
            "Students will be able to tell the main idea."
        );
        assert_eq!(plan.goals[1].zh, "學生能比較兩個角色");
        let language = &plan.goals[2];
        assert_eq!(language.zh.chars().count(), GOAL_ZH_CHARS);
        assert_eq!(language.en.chars().count(), GOAL_EN_CHARS);
        assert_eq!(language.curriculum_codes, ["B-2", "A-1"]);
        assert_eq!(language.category, Some(LanguageCategory::Strategy));
        assert_eq!(language.supports, Some(1));
        assert_eq!(language.relation, "學生要用重讀的策略，才能說出主旨");
        // Every cited code is listed, in the candidates' order.
        let listed: Vec<&str> = plan.curriculum.iter().map(|c| c.code.as_str()).collect();
        assert_eq!(listed, ["A-1", "B-2"]);
        // A 內容目標 is stored with its kind, in Chinese and English, nothing else.
        assert_eq!(
            json!(plan.goals[0]),
            json!({
                "kind": "content",
                "zh": "學生能說出主旨",
                "en": "Students will be able to tell the main idea.",
                "curriculum_codes": ["A-1"],
            })
        );
        assert_eq!(
            json!(plan.goals[2])["category"],
            "strategy",
            "a 語言目標 also has its category, supports and relation"
        );
    }

    #[test]
    fn vague_verbs_are_not_goals() {
        for (zh, en) in [
            (
                "學生能了解主角的心情",
                "Students will be able to describe the feeling.",
            ),
            (
                "學生能夠理解作者的觀點",
                "Students will be able to state the view.",
            ),
            ("學生能說出主旨", "Students will understand the main idea."),
            (
                "學生能說出主旨",
                "Students will be able to learn the words.",
            ),
            ("學生能說出主旨", "Students will be able to know the plot."),
        ] {
            assert!(vague_verb(zh, en).is_some(), "{zh} / {en}");
        }
        for (zh, en) in [
            (
                "學生能說明自己對文章的理解",
                "Students will be able to explain the text.",
            ),
            (
                "學生能辨識關鍵字",
                "Students will be able to identify the key words.",
            ),
            (
                "學生能說出主旨",
                "Students will be able to knowingly compare.",
            ),
        ] {
            assert_eq!(vague_verb(zh, en), None, "{zh} / {en}");
        }
        let raw = plan_with(
            vec![
                raw_goal(
                    "學生能了解主旨",
                    "Students will understand the main idea.",
                    &[],
                ),
                content_main(),
            ],
            vec![
                // Serves the vague goal, which is gone: dropped with it.
                raw_language(
                    "C1",
                    "學習策略",
                    "學生能重讀找出主旨句",
                    "Students will be able to reread to find the topic sentence.",
                    "學生要用重讀的策略，才能了解主旨",
                ),
                raw_language(
                    "C2",
                    "學習策略",
                    "學生能在讀不懂時重讀並畫出關鍵句",
                    "Students will be able to reread and underline the key sentence.",
                    "學生要用重讀並畫出關鍵句的策略，才能說出各段的重點。",
                ),
            ],
            steps(),
        );
        let (plan, problems) = clean_plan(&raw, &candidates(), &none()).unwrap();
        assert!(problems.is_empty(), "{problems:?}");
        let zh: Vec<&str> = plan.goals.iter().map(|g| g.zh.as_str()).collect();
        assert_eq!(
            zh,
            ["學生能說出各段的重點", "學生能在讀不懂時重讀並畫出關鍵句"]
        );
        assert_eq!(plan.goals[1].supports, Some(1), "renumbered among the kept");
    }

    #[test]
    fn a_language_goal_must_say_how_it_serves_a_content_goal() {
        let content = Goal::new(
            GoalKind::Content,
            "學生能說明作者用哪些例子支持論點",
            "Students will be able to explain which examples support the argument.",
            vec![],
        );
        for ok in [
            "學生要用 for example、such as、this shows that 指出例子並說明它支持的論點，才能說明作者用哪些例子支持論點",
            "「學生要用 for example 指出例子，才能說明作者的例子」",
            "學生要用 such as 舉例，才能 explain the examples in the argument",
        ] {
            assert!(relation_holds(ok, &content), "{ok}");
        }
        for bad in [
            "",
            "這個目標很重要。",
            "學生要用 for example 指出例子",
            "學生要用，才能說明作者用哪些例子支持論點",
            "學生要用 for example 指出例子，才能",
            // Says how, but about another goal.
            "學生要用 because 和 so，才能比較兩個角色的做法",
        ] {
            assert!(!relation_holds(bad, &content), "{bad}");
        }
        let raw = plan_with(
            vec![content_main()],
            vec![
                raw_language(
                    "C1",
                    "句型文法",
                    "學生能用 first、next 排出順序",
                    "Students will be able to use first and next.",
                    "這個目標很重要。",
                ),
                raw_language(
                    "C9",
                    "學習策略",
                    "學生能重讀",
                    "Students will be able to reread.",
                    "學生要用重讀，才能說出各段的重點",
                ),
            ],
            steps(),
        );
        let (plan, problems) = clean_plan(&raw, &candidates(), &none()).unwrap();
        assert_eq!(plan.goals.len(), 1, "both 語言目標 dropped");
        assert!(
            problems
                .iter()
                .any(|p| p.contains("language_goals 沒有可用的語言目標") && p.contains("relation")),
            "{problems:?}"
        );
    }

    #[test]
    fn language_goals_are_strategy_first() {
        let function = raw_language(
            "C1",
            "語言功能",
            "學生能用 because 說明原因",
            "Students will be able to use because to give reasons.",
            "學生要用 because 說明原因，才能說出各段的重點",
        );
        let structure = raw_language(
            "C1",
            "句型文法",
            "學生能用 not only … but also 寫句子",
            "Students will be able to write sentences with not only … but also.",
            "學生要用 not only … but also，才能說出各段的重點",
        );
        let raw = plan_with(
            vec![content_main()],
            vec![function.clone(), structure.clone(), strategy()],
            steps(),
        );
        let (plan, problems) = clean_plan(&raw, &candidates(), &none()).unwrap();
        assert!(problems.is_empty(), "{problems:?}");
        let categories: Vec<Option<LanguageCategory>> =
            plan.goals[1..].iter().map(|g| g.category).collect();
        assert_eq!(
            categories,
            [
                Some(LanguageCategory::Strategy),
                Some(LanguageCategory::Function)
            ],
            "the strategy first, then the first of the others; two at most"
        );
        // No strategy at all: retried, the plan stays usable.
        let raw = plan_with(vec![content_main()], vec![function, structure], steps());
        let (plan, problems) = clean_plan(&raw, &candidates(), &none()).unwrap();
        assert_eq!(plan.goals.len(), 3);
        assert!(
            problems.iter().any(|p| p.contains("學習策略")),
            "{problems:?}"
        );
    }

    #[test]
    fn the_teachers_objectives_are_the_goals_word_for_word() {
        let teacher = TeacherObjectives {
            content: "  學生能說明作者用哪些例子支持論點  ".into(),
            language: "Students will be able to use for example to point to examples.".into(),
            combined: "合併的一行".into(),
        };
        let raw = plan_with(
            vec![
                raw_goal(
                    "學生能說出作者的例子（模型改寫）",
                    "Students will be able to explain which examples support the argument.",
                    &["A-1", "MADE-UP"],
                ),
                raw_goal("學生能比較", "Students will be able to compare.", &[]),
            ],
            vec![RawGoal {
                supports: "C1".into(),
                category: "語言功能".into(),
                relation: "學生要用 for example 指出例子，才能說明作者用哪些例子支持論點".into(),
                ..raw_goal(
                    "學生能用 for example 指出例子",
                    "Students can point to examples (reworded).",
                    &[],
                )
            }],
            steps(),
        );
        let (plan, problems) = clean_plan(&raw, &candidates(), &teacher).unwrap();
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(plan.goals.len(), 2, "one goal per group the teacher gave");
        let content = &plan.goals[0];
        assert_eq!(content.zh, "學生能說明作者用哪些例子支持論點");
        assert_eq!(
            content.en, "Students will be able to explain which examples support the argument.",
            "the other line is the model's"
        );
        assert!(content.from_teacher);
        assert_eq!(content.curriculum_codes, ["A-1"]);
        let language = &plan.goals[1];
        assert_eq!(
            language.en, "Students will be able to use for example to point to examples.",
            "English from the teacher stays in en"
        );
        assert_eq!(language.zh, "學生能用 for example 指出例子");
        assert!(language.from_teacher);
        assert_eq!(language.supports, Some(1));
        assert_eq!(language.category, Some(LanguageCategory::Function));
        assert_eq!(json!(content)["from_teacher"], true);

        // Kept even when the model cannot relate it (or wrote nothing); the retry is asked for
        // the relation. No 學習策略 is asked for: the teacher chose the 語言目標.
        let raw = plan_with(
            vec![],
            vec![raw_language("C1", "學習策略", "x", "y", "沒有關係")],
            steps(),
        );
        let (plan, problems) = clean_plan(&raw, &candidates(), &teacher).unwrap();
        assert_eq!(plan.goals[0].zh, "學生能說明作者用哪些例子支持論點");
        assert_eq!(plan.goals[0].en, "");
        assert_eq!(plan.goals[1].zh, "");
        assert_eq!(plan.goals[1].relation, "");
        assert_eq!(plan.goals[1].supports, None);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("教師的語言目標"), "{problems:?}");

        // Only the content objective: the model derives the 語言目標 from it.
        let only_content = TeacherObjectives {
            language: String::new(),
            ..teacher
        };
        let raw = plan_with(
            vec![raw_goal(
                "學生能說出例子",
                "Students will be able to name examples.",
                &[],
            )],
            vec![raw_language(
                "C1",
                "學習策略",
                "學生能畫出例子的關鍵字",
                "Students will be able to underline the key words of examples.",
                "學生要用畫關鍵字的策略，才能說明作者用哪些例子支持論點",
            )],
            steps(),
        );
        let (plan, problems) = clean_plan(&raw, &candidates(), &only_content).unwrap();
        assert!(problems.is_empty(), "{problems:?}");
        assert!(plan.goals[0].from_teacher && !plan.goals[1].from_teacher);
        assert_eq!(plan.goals[1].supports, Some(1));
    }

    #[test]
    fn groupings_roles_and_skills_are_checked() {
        let (plan, problems) = clean_plan(
            &plan_with(vec![content_main()], vec![strategy()], steps()),
            &candidates(),
            &none(),
        )
        .unwrap();
        assert!(problems.is_empty(), "{problems:?}");
        let groupings: Vec<&str> = plan.steps.iter().map(|s| s.grouping.as_str()).collect();
        assert_eq!(groupings, [GROUP_PAIR, GROUP_SMALL, GROUP_SOLO]);
        assert_eq!(plan.steps[1].roles.len(), 4);
        assert_eq!(plan.steps[1].monitoring, "巡視各組");
        assert_eq!(plan.steps[0].skills, ["聽", "說", "讀", "寫"]);

        // One grouping only, a pair with one partner, a small group without roles or
        // monitoring, two skills: every rule broken is a problem; the plan stays usable.
        let mut flat = steps();
        flat[0].grouping = GROUP_PAIR.into();
        flat[0].roles = vec![role("夥伴 A", "說")];
        flat[1].grouping = GROUP_SMALL.into();
        flat[1].roles = vec![role("主持", "帶"), role("記錄", "")];
        flat[1].monitoring = String::new();
        flat[2].grouping = GROUP_CLASS.into();
        flat[2].roles = vec![role("不該有", "全班沒有角色")];
        for s in &mut flat {
            s.skills = vec!["讀".into(), "讀".into(), "看".into(), "說".into()];
        }
        let (plan, problems) = clean_plan(
            &plan_with(vec![content_main()], vec![strategy()], flat.clone()),
            &candidates(),
            &none(),
        )
        .unwrap();
        assert_eq!(plan.steps[0].skills, ["說", "讀"], "known, once, in order");
        assert!(
            plan.steps[2].roles.is_empty(),
            "the whole class has no roles"
        );
        assert_eq!(
            plan.steps[1].roles.len(),
            1,
            "a role without a task is dropped"
        );
        let all = problems.join("\n");
        for part in [
            "第 1 步「引起動機」是兩人一組",
            "第 2 步「發展活動」是小組：roles",
            "第 2 步「發展活動」是小組：monitoring",
            "沒有用到：聽、寫",
        ] {
            assert!(all.contains(part), "{part} missing from {all}");
        }
        assert!(!all.contains("兩種分組方式"), "three groupings here");
        for s in &mut flat {
            s.grouping = GROUP_CLASS.into();
        }
        let (_, problems) = clean_plan(
            &plan_with(vec![content_main()], vec![strategy()], flat),
            &candidates(),
            &none(),
        )
        .unwrap();
        assert!(
            problems
                .iter()
                .any(|p| p.contains("兩種分組方式") && p.contains("全班")),
            "{problems:?}"
        );
    }

    #[test]
    fn the_a6_fields_are_cleaned() {
        let mut raw = plan_with(vec![content_main()], vec![strategy()], steps());
        raw.key_vocabulary = (0..10)
            .map(|i| RawWord {
                word: format!("word{i}"),
                zh: "意思".into(),
            })
            .chain([
                RawWord {
                    word: "Word0".into(),
                    zh: "重複".into(),
                },
                RawWord {
                    word: "鄰居".into(),
                    zh: "不是英文".into(),
                },
            ])
            .collect();
        raw.key_vocabulary.rotate_right(2);
        raw.key_vocabulary.swap(0, 2);
        raw.materials = vec!["課文".into(), " ".into(), "**圖片**".into()];
        raw.higher_order_questions = vec![
            RawQuestion {
                question: "Who is he?".into(),
                bloom: "remember".into(),
            },
            RawQuestion {
                question: "Why did it work?".into(),
                bloom: "evaluate".into(),
            },
            RawQuestion {
                question: "為什麼？".into(),
                bloom: "analyze".into(),
            },
        ];
        raw.background = " ".into();
        raw.review = "字卡".into();
        raw.assessment = "x".repeat(300);
        let (plan, problems) = clean_plan(&raw, &candidates(), &none()).unwrap();
        let words: Vec<&str> = plan
            .key_vocabulary
            .iter()
            .map(|w| w.word.as_str())
            .collect();
        assert_eq!(words.len(), KEY_WORDS_MAX);
        assert_eq!(words[0], "word0", "Word0 repeats it; 鄰居 is not English");
        assert_eq!(plan.materials, ["課文", "圖片"]);
        assert_eq!(
            plan.higher_order_questions,
            [HigherQuestion {
                question: "Why did it work?".into(),
                bloom: Bloom::Evaluate,
            }]
        );
        assert_eq!(plan.background, "");
        assert!(
            problems.iter().any(|p| p.starts_with("background")),
            "A1: {problems:?}"
        );
        assert_eq!(plan.review, "字卡");
        assert_eq!(plan.assessment.chars().count(), REVIEW_CHARS);
    }

    #[test]
    fn a_missing_goal_group_is_retried_but_the_plan_stays_usable() {
        let raw = plan_with(
            vec![raw_goal(
                "學生能說出主旨",
                "Students will be able to tell the main idea.",
                &[],
            )],
            // Only English: no usable 語言目標.
            vec![raw_language(
                "C1",
                "學習策略",
                "Use because.",
                "Use because.",
                "",
            )],
            steps(),
        );
        let (plan, problems) = clean_plan(&raw, &candidates(), &none()).unwrap();
        assert_eq!(plan.goals.len(), 1);
        assert_eq!(problems.len(), 1);
        assert!(
            problems[0].contains("language_goals") && problems[0].contains("語言目標"),
            "{problems:?}"
        );
        // No usable goal in either group: nothing to show.
        let raw = RawPlan {
            content_goals: vec![],
            ..raw
        };
        assert!(clean_plan(&raw, &candidates(), &none()).is_err());
    }

    #[test]
    fn a_stored_plan_of_any_version_is_read() {
        // Version 1 of the prompt: goals with one Chinese `text`, steps without Bloom levels.
        let v1: ActivityPlan = serde_json::from_value(json!({
            "title": "舊的規劃",
            "goals": [{ "text": "學生能說出主旨", "curriculum_codes": ["A-1"] }],
            "steps": [{
                "phase": PHASE_WARMUP, "title": "看圖", "minutes": 5, "idea": "看圖猜主題",
                "options": [], "script": [], "stuck_tip": "",
            }],
            "curriculum": [{ "code": "A-1", "text": "條文一" }],
        }))
        .unwrap();
        assert_eq!(
            v1.goals,
            [Goal {
                kind: None,
                ..Goal::new(GoalKind::Content, "學生能說出主旨", "", vec!["A-1".into()])
            }]
        );
        assert!(v1.steps[0].bloom.is_empty());
        assert!(v1.steps[0].grouping.is_empty() && v1.steps[0].skills.is_empty());
        assert!(v1.key_vocabulary.is_empty() && v1.background.is_empty());
        // Version 3: both groups, in Chinese and English, nothing of version 4.
        let v3: ActivityPlan = serde_json::from_value(json!({
            "title": "新的規劃",
            "goals": [
                { "kind": "content", "zh": "學生能說出主旨",
                  "en": "Students will be able to tell the main idea.", "curriculum_codes": [] },
                { "kind": "language", "zh": "學生能用 because",
                  "en": "Students will be able to use because.", "curriculum_codes": ["A-1"] },
            ],
            "steps": [],
            "curriculum": [],
        }))
        .unwrap();
        assert_eq!(v3.goals[1].kind, Some(GoalKind::Language));
        assert_eq!(v3.goals[1].en, "Students will be able to use because.");
        assert_eq!(v3.goals[1].category, None);
        assert_eq!(v3.goals[1].relation, "");
        // Version 4 round-trips.
        let (v4, _) = clean_plan(
            &plan_with(vec![content_main()], vec![strategy()], steps()),
            &candidates(),
            &none(),
        )
        .unwrap();
        let back: ActivityPlan = serde_json::from_value(json!(v4)).unwrap();
        assert_eq!(back, v4);
        // A kind outside the two is not a plan.
        assert!(serde_json::from_value::<ActivityPlan>(json!({
            "title": "t",
            "goals": [{ "kind": "skill", "zh": "學生能", "curriculum_codes": [] }],
            "steps": [],
            "curriculum": [],
        }))
        .is_err());
    }

    #[test]
    fn the_activity_schema_is_bounded_and_in_order() {
        let schema = activity_schema(&["A-1".into()], &none());
        let props: Vec<&String> = schema["properties"].as_object().unwrap().keys().collect();
        assert_eq!(
            props,
            [
                "title",
                "content_goals",
                "language_goals",
                "key_vocabulary",
                "materials",
                "higher_order_questions",
                "background",
                "steps",
                "review",
                "assessment",
            ],
            "the gateway writes the fields in this order: content goals before language goals"
        );
        assert_eq!(schema["required"], json!(props));
        let content = &schema["properties"]["content_goals"];
        assert_eq!(
            (content["minItems"].clone(), content["maxItems"].clone()),
            (json!(1), json!(2))
        );
        let fields: Vec<&String> = content["items"]["properties"]
            .as_object()
            .unwrap()
            .keys()
            .collect();
        assert_eq!(fields, ["zh", "en", "curriculum_codes"]);
        let language = &schema["properties"]["language_goals"];
        assert_eq!(
            (language["minItems"].clone(), language["maxItems"].clone()),
            (json!(1), json!(2))
        );
        let goal = &language["items"];
        let fields: Vec<&String> = goal["properties"].as_object().unwrap().keys().collect();
        assert_eq!(
            fields,
            [
                "supports",
                "category",
                "zh",
                "en",
                "relation",
                "curriculum_codes"
            ]
        );
        assert_eq!(goal["properties"]["supports"]["enum"], json!(["C1", "C2"]));
        assert_eq!(
            goal["properties"]["category"]["enum"],
            json!(["學習策略", "語言功能", "句型文法", "學術字彙"]),
            "the strategy first"
        );
        assert_eq!(goal["properties"]["relation"]["maxLength"], RELATION_CHARS);
        assert_eq!(goal["properties"]["zh"]["maxLength"], GOAL_ZH_CHARS);
        assert_eq!(goal["properties"]["en"]["maxLength"], GOAL_EN_CHARS);
        assert_eq!(
            goal["properties"]["curriculum_codes"]["items"]["enum"],
            json!(["A-1"])
        );
        let step = &schema["properties"]["steps"]["items"];
        let fields: Vec<&String> = step["properties"].as_object().unwrap().keys().collect();
        assert_eq!(
            fields,
            [
                "phase",
                "title",
                "minutes",
                "idea",
                "grouping",
                "roles",
                "monitoring",
                "skills",
                "scaffolding",
                "bloom",
                "options",
                "script",
                "stuck_tip",
            ]
        );
        assert_eq!(
            step["properties"]["grouping"]["enum"],
            json!(["全班", "兩人一組", "小組", "個人"])
        );
        assert_eq!(step["properties"]["skills"]["items"]["enum"], json!(SKILLS));
        assert_eq!(step["properties"]["roles"]["maxItems"], ROLES_MAX);
        assert_eq!(
            schema["properties"]["higher_order_questions"]["items"]["properties"]["bloom"]["enum"],
            json!(["analyze", "evaluate", "create"])
        );
        // A group the teacher gave: exactly one goal (kept word for word), C1 only.
        let teacher = TeacherObjectives {
            content: "學生能說出主旨".into(),
            language: "學生能重讀".into(),
            combined: String::new(),
        };
        let schema = activity_schema(&[], &teacher);
        for key in ["content_goals", "language_goals"] {
            assert_eq!(schema["properties"][key]["maxItems"], 1, "{key}");
        }
        assert_eq!(
            schema["properties"]["language_goals"]["items"]["properties"]["supports"]["enum"],
            json!(["C1"])
        );
    }

    #[test]
    fn options_are_checked_with_the_settings_rules() {
        let o = SessionOptions {
            bloom: vec![Bloom::Evaluate, Bloom::Remember, Bloom::Evaluate],
            answer_formats: vec!["簡答題".into(), "選擇題".into()],
            content_objective: Some("  能說出主旨  ".into()),
            language_objective: Some("I can use because.\n I can retell it. ".into()),
            ..SessionOptions::default()
        }
        .normalised()
        .unwrap();
        assert_eq!(o.bloom, [Bloom::Remember, Bloom::Evaluate]);
        assert_eq!(o.answer_formats, ["選擇題", "簡答題"]);
        assert_eq!(o.content_objective.as_deref(), Some("能說出主旨"));
        assert_eq!(
            o.language_objective.as_deref(),
            Some("I can use because.\nI can retell it.")
        );
        let blank = SessionOptions {
            content_objective: Some(" ".into()),
            language_objective: Some("".into()),
            ..SessionOptions::default()
        };
        assert_eq!(blank.normalised().unwrap(), SessionOptions::default());
        for bad in [
            SessionOptions {
                question_count: Some(2),
                ..SessionOptions::default()
            },
            SessionOptions {
                bloom_counts: Some(BTreeMap::from([(Bloom::Apply, 13)])),
                ..SessionOptions::default()
            },
            SessionOptions {
                answer_formats: vec!["連連看".into()],
                ..SessionOptions::default()
            },
            SessionOptions {
                content_objective: Some("字".repeat(301)),
                ..SessionOptions::default()
            },
            SessionOptions {
                language_objective: Some("字".repeat(301)),
                ..SessionOptions::default()
            },
        ] {
            assert!(bad.normalised().is_err(), "{bad:?}");
        }
        // Serialised without the keys that are not set.
        assert_eq!(json!(SessionOptions::default()), json!({}));
        // A session stored with the single `objective`: read as the 內容目標.
        let old: SessionOptions =
            serde_json::from_value(json!({ "objective": "能說出主旨" })).unwrap();
        assert_eq!(old.content_objective.as_deref(), Some("能說出主旨"));
        assert_eq!(
            json!(old),
            json!({ "content_objective": "能說出主旨" }),
            "written back with the new name"
        );
        let parsed: SessionOptions =
            serde_json::from_value(json!({ "bloom": null, "question_count": null })).unwrap();
        assert_eq!(parsed, SessionOptions::default());
        assert!(
            serde_json::from_value::<SessionOptions>(json!({ "bloom": ["thinking"] })).is_err()
        );
    }

    #[test]
    fn options_become_a_settings_patch() {
        assert_eq!(SessionOptions::default().settings_patch(None, true), None);
        let o = SessionOptions {
            question_count: Some(6),
            answer_formats: vec!["簡答題".into()],
            content_objective: Some("C".into()),
            language_objective: Some("L".into()),
            ..SessionOptions::default()
        };
        let p = o.settings_patch(Some(Output::Tiered), true).unwrap();
        assert_eq!(p.output, Some(Output::Tiered));
        assert_eq!(p.question_count, Some(Some(6)));
        assert_eq!(p.answer_formats, Some(vec!["簡答題".to_string()]));
        assert_eq!(p.content_objective, Some(Some("C".to_string())));
        assert_eq!(p.language_objective, Some(Some("L".to_string())));
        assert_eq!(p.bloom, None, "not set: the worksheet's own");
        assert_eq!(p.bloom_counts, None);
        assert_eq!(o.settings_patch(None, false).unwrap().answer_formats, None);
    }

    #[test]
    fn labels_and_kit_errors() {
        assert!(is_bare_label(" 閱讀理解題 "));
        // 差異化學習單 and its old name, 分層練習 (a page loaded before the rename).
        assert!(is_bare_label("差異化學習單") && is_bare_label("分層練習"));
        assert_eq!(chip_label(KIND_TIERED), "差異化學習單");
        assert!(is_bare_label(chip_label(KIND_KIT)));
        assert!(!is_bare_label("閱讀理解題，8 題"));
        assert_eq!(output_of(KIND_KIT), Some(Output::Tiered));
        assert_eq!(output_of(KIND_WORKSHEET), None);
        assert!(is_worksheet_kind(KIND_KIT) && !is_worksheet_kind(KIND_ACTIVITY));
        assert_eq!(kit_error(None, None), None);
        assert_eq!(
            kit_error(Some("A。"), None).unwrap(),
            "教學套件的課堂活動沒有完成：A。"
        );
        assert_eq!(
            kit_error(Some("A。"), Some("W。")).unwrap(),
            "教學套件沒有完成。課堂活動：A。差異化學習單：W。"
        );
    }

    #[test]
    fn answers_are_plain_text() {
        let a = clean_answer(&RawAnswer {
            paragraphs: vec![
                "## 標題".into(),
                "* **重點**一".into(),
                " ".into(),
                "正文".into(),
            ],
            follow_ups: vec!["  ".into(), "下一步？".into()],
        })
        .unwrap();
        assert_eq!(a.answer, "標題\n・重點一\n正文");
        assert_eq!(a.follow_ups, ["下一步？"]);
        assert!(clean_answer(&RawAnswer {
            paragraphs: vec![" ".into()],
            follow_ups: vec![]
        })
        .is_err());
    }

    fn caps(v: &Value, path: &str, problems: &mut Vec<String>) {
        match v.get("type").and_then(Value::as_str) {
            Some("string") if v.get("enum").is_none() && v.get("maxLength").is_none() => {
                problems.push(format!("{path}: string without maxLength"))
            }
            Some("array") if v.get("maxItems").is_none() => {
                problems.push(format!("{path}: array without maxItems"))
            }
            _ => {}
        }
        if let Some(props) = v.get("properties").and_then(Value::as_object) {
            for (k, p) in props {
                caps(p, &format!("{path}.{k}"), problems);
            }
        }
        if let Some(items) = v.get("items") {
            caps(items, &format!("{path}[]"), problems);
        }
    }

    #[test]
    fn every_string_and_array_has_a_cap() {
        let mut problems = Vec::new();
        for (name, schema) in [
            ("activity", activity_schema(&["A-1".into()], &none())),
            ("activity_no_kb", activity_schema(&[], &none())),
            (
                "activity_teacher",
                activity_schema(
                    &[],
                    &TeacherObjectives {
                        content: "c".into(),
                        language: "l".into(),
                        combined: String::new(),
                    },
                ),
            ),
            ("selfcheck", selfcheck::schema(&selfcheck::judged_ids())),
            ("ask", ask_schema()),
            (
                "revise",
                revise_schema(&["b1".into()], &["b1-p1".into()], 3),
            ),
            ("revise_empty", revise_schema(&[], &[], 0)),
        ] {
            caps(&schema, name, &mut problems);
        }
        assert!(problems.is_empty(), "{problems:#?}");
    }
}
