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
pub mod kit;
pub mod run;
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

/// The chip label a turn of `kind` stands for, used as its request when the teacher typed none.
pub fn chip_label(kind: &str) -> &'static str {
    match kind {
        KIND_WORKSHEET => "做學習單",
        KIND_ASSESSMENT => "設計評量",
        KIND_ACTIVITY => "規劃教學活動",
        KIND_RESOURCES => "找教學資源",
        KIND_ASK => "直接問 AI",
        KIND_COMPREHENSION => "閱讀理解題",
        KIND_TIERED => "分層練習",
        KIND_KIT => "教學套件",
        _ => "修改學習單",
    }
}

/// Labels the pages send as a request when the teacher typed nothing: the server's chip labels
/// and the home page's chips and tool names. Such a request is no 「教師的要求」.
const BARE_LABELS: [&str; 13] = [
    "做學習單",
    "設計評量",
    "規劃教學活動",
    "找教學資源",
    "直接問 AI",
    "修改學習單",
    "閱讀理解題",
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
        (None, Some(w)) => Some(format!("教學套件的分層學習單沒有完成：{w}")),
        (Some(a), Some(w)) => Some(format!("教學套件沒有完成。課堂活動：{a}分層學習單：{w}")),
    }
}

// ---------------------------------------------------------------- caps

pub const PLAN_TITLE_CHARS: usize = 100;
/// Goals of each group (內容目標, 語言目標): 1–3 each (`prep_activity` version 3).
pub const GOAL_GROUP_MIN: usize = 1;
pub const GOAL_GROUP_MAX: usize = 3;
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
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
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

/// A goal of an activity plan. Since version 3 of `prep_activity` every goal has a `kind` and
/// is written twice: `zh` for the teacher, `en` for the students. A plan made with an earlier
/// version has one Chinese line per goal, stored as `text`: it is read into `zh`, without a
/// `kind` and with an empty `en` (the stored JSON itself is never rewritten).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Goal {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<GoalKind>,
    #[serde(alias = "text")]
    pub zh: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub en: String,
    pub curriculum_codes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StepOption {
    pub title: String,
    pub idea: String,
    pub script: Vec<String>,
    pub minutes: i64,
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
}

/// 規劃教學活動: what the teacher gets (`result` of an `activity` turn).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActivityPlan {
    pub title: String,
    /// The 內容目標 first, then the 語言目標 (version 3); one list without kinds before.
    pub goals: Vec<Goal>,
    pub steps: Vec<Step>,
    /// The candidates actually cited, text from the knowledge base.
    pub curriculum: Vec<CurriculumRef>,
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
}

// ---------------------------------------------------------------- what the model writes

/// A goal as the model writes it; its group is the array it is in.
#[derive(Debug, Clone, Deserialize)]
pub struct RawGoal {
    pub zh: String,
    pub en: String,
    pub curriculum_codes: Vec<String>,
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
    /// Version 2 of the prompt; an id outside the set is dropped by the server.
    #[serde(default)]
    pub bloom: Vec<String>,
    pub options: Vec<RawOption>,
    pub script: Vec<String>,
    pub stuck_tip: String,
}

/// What the model writes for `prep_activity` (version 3): the two goal groups are two arrays,
/// so the schema itself asks for both (1–3 each); the server puts them into `goals` with their
/// `kind`.
#[derive(Debug, Clone, Deserialize)]
pub struct RawPlan {
    pub title: String,
    pub content_goals: Vec<RawGoal>,
    pub language_goals: Vec<RawGoal>,
    pub steps: Vec<RawStep>,
}

impl RawPlan {
    fn goals_of(&self, kind: GoalKind) -> &[RawGoal] {
        match kind {
            GoalKind::Content => &self.content_goals,
            GoalKind::Language => &self.language_goals,
        }
    }
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

/// `prep_activity`: the title, the 內容目標 (`content_goals`) and 語言目標 (`language_goals`),
/// 1–3 each, every goal in Chinese (`zh`) then English (`en`), then the steps. Curriculum codes
/// only from `candidates` (enum), Bloom levels only from the six ids (enum, 1–3 per step), every
/// string and array capped.
pub fn activity_schema(candidates: &[String]) -> Value {
    let option = obj(vec![
        ("title", s_max(STEP_TITLE_CHARS)),
        ("idea", s_max(IDEA_CHARS)),
        (
            "script",
            strings_max(OPTION_SCRIPT_LINES, SCRIPT_LINE_CHARS),
        ),
        ("minutes", int()),
    ]);
    let step = obj(vec![
        ("phase", enum_of(&PHASES)),
        ("title", s_max(STEP_TITLE_CHARS)),
        ("minutes", int()),
        ("idea", s_max(IDEA_CHARS)),
        (
            "bloom",
            json!({
                "type": "array",
                "items": enum_of(&bloom_ids()),
                "minItems": 1,
                "maxItems": STEP_BLOOM_MAX,
            }),
        ),
        (
            "options",
            json!({ "type": "array", "items": option, "maxItems": OPTIONS_MAX }),
        ),
        ("script", strings_max(STEP_SCRIPT_LINES, SCRIPT_LINE_CHARS)),
        ("stuck_tip", s_max(STUCK_TIP_CHARS)),
    ]);
    let goal = obj(vec![
        ("zh", s_max(GOAL_ZH_CHARS)),
        ("en", s_max(GOAL_EN_CHARS)),
        ("curriculum_codes", codes_schema(candidates, GOAL_CODES_MAX)),
    ]);
    let goals = json!({
        "type": "array",
        "items": goal,
        "minItems": GOAL_GROUP_MIN,
        "maxItems": GOAL_GROUP_MAX,
    });
    obj(vec![
        ("title", s_max(PLAN_TITLE_CHARS)),
        (goals_key(GoalKind::Content), goals.clone()),
        (goals_key(GoalKind::Language), goals),
        (
            "steps",
            json!({ "type": "array", "items": step, "minItems": STEPS_MIN, "maxItems": STEPS_MAX }),
        ),
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
pub fn revise_schema(block_ids: &[String]) -> Value {
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

/// The usable goals of one group, at most three: text cleaned and capped; a goal whose `zh` has
/// no Chinese or whose `en` is not English is dropped; codes outside `allowed` (and repeats)
/// dropped, at most three.
fn clean_goals(raw: &[RawGoal], kind: GoalKind, allowed: &HashSet<&str>) -> Vec<Goal> {
    raw.iter()
        .filter_map(|g| {
            let zh = clean_text(&g.zh, GOAL_ZH_CHARS);
            let en = clean_text(&g.en, GOAL_EN_CHARS);
            if !contains_cjk(&zh) || !is_english(&en) {
                return None;
            }
            let mut seen = HashSet::new();
            Some(Goal {
                kind: Some(kind),
                zh,
                en,
                curriculum_codes: g
                    .curriculum_codes
                    .iter()
                    .map(|c| c.trim().to_string())
                    .filter(|c| allowed.contains(c.as_str()) && seen.insert(c.clone()))
                    .take(GOAL_CODES_MAX)
                    .collect(),
            })
        })
        .take(GOAL_GROUP_MAX)
        .collect()
}

/// The checked plan and what the retry should still fix, or (`Err`) why the plan cannot be used
/// at all.
///
/// The goals are the 內容目標 then the 語言目標 ([`clean_goals`]). A group left without a usable
/// goal is a problem for the retry, but the plan stays usable: when the retry does no better,
/// the teacher still gets it. No usable goal at all, or fewer than three usable steps, makes the
/// plan unusable. Codes outside `candidates` are dropped, a step whose phase is not one of the
/// three labels is dropped, minutes are clamped to 1–50, options are kept for 引起動機 only, a
/// step keeps at most three Bloom levels (in Bloom order), and every text is cleaned and capped.
/// The curriculum list is filled from `candidates` (text from the knowledge base), in their
/// order.
pub fn clean_plan(
    raw: &RawPlan,
    candidates: &[CurriculumRef],
) -> Result<(ActivityPlan, Vec<String>), String> {
    let allowed: HashSet<&str> = candidates.iter().map(|c| c.code.as_str()).collect();
    let mut goals: Vec<Goal> = Vec::new();
    let mut missing: Vec<String> = Vec::new();
    for kind in GoalKind::ALL {
        let group = clean_goals(raw.goals_of(kind), kind, &allowed);
        if group.is_empty() {
            missing.push(format!(
                "{} 沒有可用的{}：要有 {GOAL_GROUP_MIN}–{GOAL_GROUP_MAX} 個，每個的 zh 用繁體中文、en 用英文，各寫一句。",
                goals_key(kind),
                kind.label()
            ));
        }
        goals.extend(group);
    }
    let steps: Vec<Step> = raw
        .steps
        .iter()
        .filter(|s| PHASES.contains(&s.phase.trim()))
        .map(|s| {
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
            Step {
                phase,
                title: clean_text(&s.title, STEP_TITLE_CHARS),
                minutes: clamp_minutes(s.minutes),
                idea: clean_text(&s.idea, IDEA_CHARS),
                options,
                script: clean_lines(&s.script, STEP_SCRIPT_LINES, SCRIPT_LINE_CHARS),
                stuck_tip: clean_text(&s.stuck_tip, STUCK_TIP_CHARS),
                bloom: step_bloom(&s.bloom),
            }
        })
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
    let cited: HashSet<&str> = goals
        .iter()
        .flat_map(|g| g.curriculum_codes.iter().map(String::as_str))
        .collect();
    let curriculum = candidates
        .iter()
        .filter(|c| cited.contains(c.code.as_str()))
        .cloned()
        .collect();
    let plan = ActivityPlan {
        title: clean_text(&raw.title, PLAN_TITLE_CHARS),
        goals,
        steps,
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

    fn raw_step(phase: &str, minutes: i64, options: usize) -> RawStep {
        RawStep {
            phase: phase.into(),
            title: format!("**{phase}**"),
            minutes,
            idea: "想法".into(),
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
        }
    }

    fn steps() -> Vec<RawStep> {
        vec![
            raw_step(PHASE_WARMUP, 5, 0),
            raw_step(PHASE_DEVELOP, 30, 0),
            raw_step(PHASE_WRAP_UP, 10, 0),
        ]
    }

    #[test]
    fn plan_is_cleaned_and_codes_come_from_the_candidates() {
        let raw = RawPlan {
            title: "計畫".into(),
            content_goals: vec![raw_goal(
                "學生能說出主旨",
                "Students will be able to tell the main idea.",
                &["B-2", "MADE-UP", "B-2"],
            )],
            language_goals: vec![raw_goal(
                "學生能用 because 說明原因",
                "Students will be able to use because.",
                &[],
            )],
            steps: vec![
                raw_step(PHASE_WARMUP, 0, 5),
                raw_step("暖身", 5, 0),
                raw_step(PHASE_DEVELOP, 80, 2),
                raw_step(PHASE_WRAP_UP, 10, 0),
            ],
        };
        let (plan, problems) = clean_plan(&raw, &candidates()).unwrap();
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
        let raw = RawPlan {
            title: "t".into(),
            content_goals: vec![],
            language_goals: vec![],
            steps: vec![raw_step(PHASE_WARMUP, 5, 0)],
        };
        let err = clean_plan(&raw, &candidates()).unwrap_err();
        assert!(
            err.contains("content_goals")
                && err.contains("language_goals")
                && err.contains("steps"),
            "{err}"
        );
    }

    #[test]
    fn goals_come_in_two_groups_in_both_languages() {
        let raw = RawPlan {
            title: "t".into(),
            content_goals: vec![
                raw_goal(
                    "學生能說出**主旨**",
                    "Students will be able to tell the main idea.",
                    &["A-1"],
                ),
                // Not Chinese / not English: dropped.
                raw_goal("Students can retell.", "Students can retell.", &[]),
                raw_goal("學生能說出原因", "學生能說出原因", &[]),
                raw_goal("學生能說出原因", "", &[]),
                raw_goal("學生能比較", "Students will be able to compare.", &[]),
                raw_goal("學生能評論", "Students will be able to comment.", &[]),
                raw_goal("學生能多寫一個", "One too many.", &[]),
            ],
            language_goals: vec![raw_goal(
                &"學".repeat(200),
                &"a".repeat(300),
                &["B-2", "A-1", "MADE-UP", "B-2"],
            )],
            steps: steps(),
        };
        let (plan, problems) = clean_plan(&raw, &candidates()).unwrap();
        assert!(problems.is_empty(), "{problems:?}");
        let kinds: Vec<Option<GoalKind>> = plan.goals.iter().map(|g| g.kind).collect();
        assert_eq!(
            kinds,
            [
                Some(GoalKind::Content),
                Some(GoalKind::Content),
                Some(GoalKind::Content),
                Some(GoalKind::Language),
            ],
            "at most three per group, the content goals first"
        );
        assert_eq!(plan.goals[0].zh, "學生能說出主旨");
        assert_eq!(
            plan.goals[0].en,
            "Students will be able to tell the main idea."
        );
        assert_eq!(plan.goals[1].zh, "學生能比較");
        let language = &plan.goals[3];
        assert_eq!(language.zh.chars().count(), GOAL_ZH_CHARS);
        assert_eq!(language.en.chars().count(), GOAL_EN_CHARS);
        assert_eq!(language.curriculum_codes, ["B-2", "A-1"]);
        // Every cited code is listed, in the candidates' order.
        let listed: Vec<&str> = plan.curriculum.iter().map(|c| c.code.as_str()).collect();
        assert_eq!(listed, ["A-1", "B-2"]);
        // Stored with the kind, in Chinese and English; no `text`.
        assert_eq!(
            json!(plan.goals[0]),
            json!({
                "kind": "content",
                "zh": "學生能說出主旨",
                "en": "Students will be able to tell the main idea.",
                "curriculum_codes": ["A-1"],
            })
        );
    }

    #[test]
    fn a_missing_goal_group_is_retried_but_the_plan_stays_usable() {
        let raw = RawPlan {
            title: "t".into(),
            content_goals: vec![raw_goal(
                "學生能說出主旨",
                "Students will be able to tell the main idea.",
                &[],
            )],
            // Only English: no usable 語言目標.
            language_goals: vec![raw_goal("Use because.", "Use because.", &[])],
            steps: steps(),
        };
        let (plan, problems) = clean_plan(&raw, &candidates()).unwrap();
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
        assert!(clean_plan(&raw, &candidates()).is_err());
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
                zh: "學生能說出主旨".into(),
                en: String::new(),
                curriculum_codes: vec!["A-1".into()],
            }]
        );
        assert!(v1.steps[0].bloom.is_empty());
        // Version 3: both groups, in Chinese and English.
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
    fn the_activity_schema_asks_for_both_goal_groups_in_order() {
        let schema = activity_schema(&["A-1".into()]);
        let props: Vec<&String> = schema["properties"].as_object().unwrap().keys().collect();
        assert_eq!(
            props,
            ["title", "content_goals", "language_goals", "steps"],
            "the gateway writes the fields in this order"
        );
        for key in ["content_goals", "language_goals"] {
            let group = &schema["properties"][key];
            assert_eq!(group["minItems"], 1, "{key}");
            assert_eq!(group["maxItems"], 3, "{key}");
            let goal = &group["items"];
            let fields: Vec<&String> = goal["properties"].as_object().unwrap().keys().collect();
            assert_eq!(fields, ["zh", "en", "curriculum_codes"], "{key}");
            assert_eq!(goal["required"], json!(["zh", "en", "curriculum_codes"]));
            assert_eq!(goal["properties"]["zh"]["maxLength"], GOAL_ZH_CHARS);
            assert_eq!(goal["properties"]["en"]["maxLength"], GOAL_EN_CHARS);
            assert_eq!(
                goal["properties"]["curriculum_codes"]["items"]["enum"],
                json!(["A-1"])
            );
        }
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
            "教學套件沒有完成。課堂活動：A。分層學習單：W。"
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
            ("activity", activity_schema(&["A-1".into()])),
            ("activity_no_kb", activity_schema(&[])),
            ("ask", ask_schema()),
            ("revise", revise_schema(&["b1".into()])),
            ("revise_empty", revise_schema(&[])),
        ] {
            caps(&schema, name, &mut problems);
        }
        assert!(problems.is_empty(), "{problems:#?}");
    }
}
