//! 備課對話 (prep sessions): a conversation-first entry point (docs/API.md「Phase 2.6：備課對話」).
//!
//! A session holds the lesson's context (a library material or only a topic, an optional class
//! and template); every request is one turn, run in the background ([`run`]). The kinds:
//! `worksheet` / `assessment` make an ordinary worksheet and start its generation; `activity`
//! plans a lesson (one model call); `resources` lists curriculum items and the teacher's own
//! materials (no model); `ask` answers a question (one call); `revise` applies a typed
//! instruction to the session's latest worksheet (one call, every change checked like a
//! template block).
//!
//! Every string and array the model writes has a cap in its JSON schema: the gateway enforces
//! `maxLength` / `maxItems` while decoding, which is what stops a runaway.

pub mod fake;
pub mod run;
pub mod store;

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::sop::checks::clean_markup;
use crate::worksheet::template::{enum_of, obj, s_max, strings_max};
use crate::worksheet::ANSWER_FORMATS;

pub const KIND_WORKSHEET: &str = "worksheet";
pub const KIND_ASSESSMENT: &str = "assessment";
pub const KIND_ACTIVITY: &str = "activity";
pub const KIND_RESOURCES: &str = "resources";
pub const KIND_ASK: &str = "ask";
pub const KIND_REVISE: &str = "revise";
pub const KINDS: [&str; 6] = [
    KIND_WORKSHEET,
    KIND_ASSESSMENT,
    KIND_ACTIVITY,
    KIND_RESOURCES,
    KIND_ASK,
    KIND_REVISE,
];
/// The next-step chips, in the order they are offered.
pub const NEXT_KINDS: [&str; 4] = [
    KIND_WORKSHEET,
    KIND_ACTIVITY,
    KIND_ASSESSMENT,
    KIND_RESOURCES,
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
        _ => "修改學習單",
    }
}

pub fn is_worksheet_kind(kind: &str) -> bool {
    kind == KIND_WORKSHEET || kind == KIND_ASSESSMENT
}

// ---------------------------------------------------------------- caps

pub const PLAN_TITLE_CHARS: usize = 100;
pub const GOALS_MAX: usize = 4;
pub const GOAL_TEXT_CHARS: usize = 150;
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Goal {
    pub text: String,
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
}

/// 規劃教學活動: what the teacher gets (`result` of an `activity` turn).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActivityPlan {
    pub title: String,
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

#[derive(Debug, Clone, Deserialize)]
pub struct RawGoal {
    pub text: String,
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
    pub options: Vec<RawOption>,
    pub script: Vec<String>,
    pub stuck_tip: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawPlan {
    pub title: String,
    pub goals: Vec<RawGoal>,
    pub steps: Vec<RawStep>,
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

/// `prep_activity`: curriculum codes only from `candidates` (enum), every string and array capped.
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
            "options",
            json!({ "type": "array", "items": option, "maxItems": OPTIONS_MAX }),
        ),
        ("script", strings_max(STEP_SCRIPT_LINES, SCRIPT_LINE_CHARS)),
        ("stuck_tip", s_max(STUCK_TIP_CHARS)),
    ]);
    let goal = obj(vec![
        ("text", s_max(GOAL_TEXT_CHARS)),
        ("curriculum_codes", codes_schema(candidates, GOAL_CODES_MAX)),
    ]);
    obj(vec![
        ("title", s_max(PLAN_TITLE_CHARS)),
        (
            "goals",
            json!({ "type": "array", "items": goal, "minItems": 1, "maxItems": GOALS_MAX }),
        ),
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

/// The checked plan, or what is wrong with it (for the retry). Codes outside `candidates` are
/// dropped, a step whose phase is not one of the three labels is dropped, minutes are clamped
/// to 1–50, options are kept for 引起動機 only, and every text is cleaned and capped. The
/// curriculum list is filled from `candidates` (text from the knowledge base), in their order.
pub fn clean_plan(raw: &RawPlan, candidates: &[CurriculumRef]) -> Result<ActivityPlan, String> {
    let allowed: HashSet<&str> = candidates.iter().map(|c| c.code.as_str()).collect();
    let goals: Vec<Goal> = raw
        .goals
        .iter()
        .map(|g| {
            let mut seen = HashSet::new();
            Goal {
                text: clean_text(&g.text, GOAL_TEXT_CHARS),
                curriculum_codes: g
                    .curriculum_codes
                    .iter()
                    .map(|c| c.trim().to_string())
                    .filter(|c| allowed.contains(c.as_str()) && seen.insert(c.clone()))
                    .take(GOAL_CODES_MAX)
                    .collect(),
            }
        })
        .filter(|g| !g.text.is_empty())
        .take(GOALS_MAX)
        .collect();
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
            }
        })
        .filter(|s| !s.title.is_empty() || !s.idea.is_empty())
        .take(STEPS_MAX)
        .collect();
    let mut problems = Vec::new();
    if goals.is_empty() {
        problems.push("goals 至少要有 1 個學習目標。".to_string());
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
    Ok(ActivityPlan {
        title: clean_text(&raw.title, PLAN_TITLE_CHARS),
        goals,
        steps,
        curriculum,
    })
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

    #[test]
    fn plan_is_cleaned_and_codes_come_from_the_candidates() {
        let raw = RawPlan {
            title: "計畫".into(),
            goals: vec![RawGoal {
                text: "學生能說出主旨".into(),
                curriculum_codes: vec!["B-2".into(), "MADE-UP".into(), "B-2".into()],
            }],
            steps: vec![
                raw_step(PHASE_WARMUP, 0, 5),
                raw_step("暖身", 5, 0),
                raw_step(PHASE_DEVELOP, 80, 2),
                raw_step(PHASE_WRAP_UP, 10, 0),
            ],
        };
        let plan = clean_plan(&raw, &candidates()).unwrap();
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
        let ideas = warmup_ideas(&plan);
        assert_eq!(ideas.len(), 3);
        assert_eq!(ideas[0], "選項0：做法");
    }

    #[test]
    fn a_plan_without_enough_steps_is_retried() {
        let raw = RawPlan {
            title: "t".into(),
            goals: vec![],
            steps: vec![raw_step(PHASE_WARMUP, 5, 0)],
        };
        let err = clean_plan(&raw, &candidates()).unwrap_err();
        assert!(err.contains("goals") && err.contains("steps"), "{err}");
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
