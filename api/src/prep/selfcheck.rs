//! SIOP 自我檢核 (docs/SIOP_DESIGN_RULES.md section 八; docs/API.md「SIOP 自我檢核」): after a
//! /create output is made (a worksheet turn of the four worksheet tools, 課堂活動, 教學套件), one
//! model call judges the SIOP observation items that can be seen on the page.
//!
//! * The items ([`ITEMS`]) follow the spec's three tables: the ones judged from the page, the
//!   ones judged only on their visible part ([`Scope::Partial`], the model is told which part),
//!   and the ones that need a classroom ([`Scope::Classroom`]: 10, 18, 25, 26), which are never
//!   sent to the model and always read 「需課堂觀察」.
//! * One call (`siop_selfcheck`) returns every sent item, in their order: `id`, a one-sentence
//!   `reason`, then the `rating` 符合／部分符合／未見 (the reason first: the model says what it
//!   sees before it rates). No total score. Like `worksheet_bloom_check`: bounded, ids from an
//!   enum, an item left out or written twice is a problem for the one retry.
//! * The model reads a compact text rendering of the output ([`render_plan`],
//!   [`render_worksheet`]), not its JSON. The worksheet is read generically: fields this module
//!   does not know are printed as `key：value`, so new worksheet fields reach the check too.
//! * A check is stored per turn (`siop_checks`, one row per run). The first run opens with the
//!   turn's closing (in the same transaction) and runs in the background; the page shows it
//!   with the turn and polls it while it runs. A check that fails or times out leaves the
//!   output as it is; the teacher can run it again (再試一次), and again after the worksheet
//!   was revised.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{PgConnection, PgPool};

use super::run::{self, Call, Checked, BASE_TEMPERATURE};
use super::store::{SessionRow, TurnRow};
use super::{
    clean_text, ActivityPlan, GoalKind, Step, KIND_ACTIVITY, KIND_COMPREHENSION, KIND_KIT,
    KIND_TIERED, KIND_WORKSHEET, OPTION_NAMES_ZH, STATUS_DONE, STATUS_FAILED, STATUS_RUNNING,
};
use crate::error::{AppError, AppResult};
use crate::routes::clip_chars;
use crate::worksheet::assemble::letter;
use crate::worksheet::store as ws_store;
use crate::worksheet::template::{enum_of, obj, s_max};
use crate::worksheet::{
    info_function_label, Block, BlockKind, Content, Level, Settings, EXIT_PURPOSES,
};
use crate::AppState;

pub const KEY: &str = "siop_selfcheck";
pub const SCHEMA_NAME: &str = "siop_selfcheck";
const SCAFFOLD: &str = "SC-SIOP-SELFCHECK";
pub const MAX_TOKENS: u32 = 3000;
/// One sentence for the teacher.
pub const REASON_CHARS: usize = 100;
/// How much of the rendered output the model reads.
pub const OUTPUT_CHARS: usize = 16000;

/// The turn kinds whose output is checked: the /create tools (照我的學習單做 and 完整學習單 are
/// `worksheet` turns).
pub const CHECKED_KINDS: [&str; 5] = [
    KIND_WORKSHEET,
    KIND_COMPREHENSION,
    KIND_TIERED,
    KIND_ACTIVITY,
    KIND_KIT,
];

pub fn is_checked(kind: &str) -> bool {
    CHECKED_KINDS.contains(&kind)
}

pub const MSG_NOT_CHECKED: &str = "這種產出沒有 SIOP 自我檢核。";
pub const MSG_NO_CHECK: &str = "這一輪還沒有 SIOP 自我檢核。";
pub const MSG_TURN_NOT_DONE: &str = "這一輪還沒有完成，沒有可以檢核的產出。";
pub const MSG_CHECK_RUNNING: &str = "SIOP 自我檢核還在進行，好了之後再試。";
const MSG_STALE: &str = "這次檢核中斷了（可能是系統剛更新）。請按「再試一次」。";
const MSG_NO_OUTPUT: &str = "這一輪沒有可以檢核的產出。";
const MSG_WORKSHEET_GONE: &str = "學習單已經刪除了，沒有內容可以檢核。";
const MSG_NO_ITEMS: &str = "學習單沒有題目，沒有內容可以檢核。";
const MSG_INTERNAL: &str = "檢核時發生錯誤。請按「再試一次」。";

// ---------------------------------------------------------------- the items

/// How much of an item the page can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// Judged from the page.
    Page,
    /// Judged only on the part the page shows (`note` says which).
    Partial,
    /// Needs a classroom: never sent to the model, shown as 「需課堂觀察」.
    Classroom,
}

/// One row of the spec's tables: an item of the SIOP observation protocol (two items merged
/// where the spec merges them), in our own words.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct SiopItem {
    /// The protocol's item number(s): `"7-8"`.
    pub id: &'static str,
    pub name: &'static str,
    /// What the check asks ("" for a classroom item).
    pub question: &'static str,
    pub scope: Scope,
    /// [`Scope::Partial`]: the part the page shows; [`Scope::Classroom`]: what a classroom
    /// shows; "" otherwise.
    pub note: &'static str,
}

const fn page(id: &'static str, name: &'static str, question: &'static str) -> SiopItem {
    SiopItem {
        id,
        name,
        question,
        scope: Scope::Page,
        note: "",
    }
}

const fn partial(
    id: &'static str,
    name: &'static str,
    question: &'static str,
    note: &'static str,
) -> SiopItem {
    SiopItem {
        id,
        name,
        question,
        scope: Scope::Partial,
        note,
    }
}

const fn classroom(id: &'static str, name: &'static str, note: &'static str) -> SiopItem {
    SiopItem {
        id,
        name,
        question: "",
        scope: Scope::Classroom,
        note,
    }
}

/// Every item, in the protocol's order.
pub const ITEMS: [SiopItem; 26] = [
    page(
        "1-2",
        "內容目標與語言目標",
        "是否寫出內容目標與語言目標（各 1–2 個、用看得到表現的動詞、學生看得懂）？語言目標是否用來支持內容目標？",
    ),
    page(
        "3",
        "概念適切",
        "內容是否符合學生的年級與程度，而且沒有因為改寫或簡化而刪掉核心概念？",
    ),
    page(
        "4",
        "補充教材",
        "是否有圖片、表格、圖表組織等輔助教材？",
    ),
    page(
        "5",
        "依程度調整",
        "是否依學生程度調整：各版本的目標與思考層次相同，只差在提示與支持的多寡（例如預填克漏字、字庫、句型框架、句首）？",
    ),
    page(
        "6",
        "有意義的活動",
        "任務是否連結學生的生活經驗或真實用途並練習語言，而不是與主題無關的機械練習？",
    ),
    page(
        "7-8",
        "連結經驗與舊知",
        "暖身或引起動機是否明確連結學生的生活經驗與先前學過的內容？",
    ),
    page(
        "9",
        "關鍵字彙",
        "是否列出關鍵字彙，並讓這些字在任務中重複出現？",
    ),
    classroom("10", "教師語速與用語", "要在課堂上聽教師說話才能判斷"),
    page("11", "任務說明", "每個任務是否有分步驟的書面作答指示？"),
    partial(
        "12",
        "讓概念清楚的技巧",
        "書面上是否有讓概念更清楚的視覺輔助或示範（例如圖片、圖表、範例）？",
        "只看得到書面的視覺輔助",
    ),
    page(
        "13",
        "學習策略",
        "是否安排學生實際使用學習策略（例如預測、摘要、重讀、自我提問）？",
    ),
    page(
        "14",
        "鷹架",
        "是否有鷹架（口語、程序或教學鷹架，例如示範、句型框架、圖表組織），並由示範、共同練習到獨立完成逐步撤除？",
    ),
    page(
        "15",
        "高層次思考",
        "是否有高層次問題（分析、評鑑、創造）？有多個版本時，每個版本都有？",
    ),
    page("16", "互動", "是否設計了學生之間討論或合作的任務？"),
    page(
        "17",
        "分組",
        "是否寫明分組方式，而且至少有兩種（例如全班、兩人一組、小組、個人）？",
    ),
    classroom("18", "等待時間", "要在課堂上看教師是否給學生足夠的時間回答"),
    page("19", "母語支持", "是否在需要的地方提供中文說明或提示？"),
    page(
        "20",
        "動手操作",
        "是否有讓學生動手操作的任務（例如分類、排序、製作、填寫圖表）？",
    ),
    page("21", "應用", "是否有讓學生應用新學的內容與語言的任務？"),
    page("22", "聽說讀寫", "聽、說、讀、寫四種技能是否都有安排？"),
    partial(
        "23-24",
        "教學流程支持目標",
        "教學流程（各步驟或各區塊）是否對應內容目標與語言目標？",
        "只看得到教學流程是否對應目標",
    ),
    classroom("25", "學生投入", "要在課堂上看學生參與的情形"),
    classroom("26", "節奏", "要在課堂上看教學節奏是否適合學生"),
    page("27-28", "複習", "是否安排複習關鍵字彙與核心概念？"),
    partial(
        "29",
        "回饋",
        "是否安排給學生回饋的時機（例如核對答案、同儕互評、教師巡視時回饋）？",
        "只看得到是否設計回饋的時機",
    ),
    page(
        "30",
        "評量",
        "結尾的評量（例如 exit ticket）是否對應所有內容目標與語言目標？",
    ),
];

/// The items sent to the model (every item but the classroom ones), in order.
pub fn judged_items() -> Vec<&'static SiopItem> {
    ITEMS
        .iter()
        .filter(|i| i.scope != Scope::Classroom)
        .collect()
}

pub fn judged_ids() -> Vec<String> {
    judged_items().iter().map(|i| i.id.to_string()).collect()
}

/// The items as the prompt lists them: 「12｜讓概念清楚的技巧（只看書面：…）：question」.
pub fn items_text() -> String {
    judged_items()
        .iter()
        .map(|i| match i.scope {
            Scope::Partial => format!(
                "{}｜{}（只看書面部分：{}）：{}",
                i.id, i.name, i.note, i.question
            ),
            _ => format!("{}｜{}：{}", i.id, i.name, i.question),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 符合／部分符合／未見.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rating {
    Met,
    Partial,
    NotSeen,
}

impl Rating {
    pub const ALL: [Rating; 3] = [Rating::Met, Rating::Partial, Rating::NotSeen];

    pub fn label(self) -> &'static str {
        match self {
            Rating::Met => "符合",
            Rating::Partial => "部分符合",
            Rating::NotSeen => "未見",
        }
    }

    pub fn from_label(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.label() == s.trim())
    }
}

// ---------------------------------------------------------------- what the model writes

#[derive(Debug, Clone, Deserialize)]
pub struct RawJudgement {
    pub id: String,
    pub reason: String,
    pub rating: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawSelfCheck {
    pub items: Vec<RawJudgement>,
}

/// One judged item, as stored (`siop_checks.items`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Judgement {
    pub id: String,
    pub rating: Rating,
    pub reason: String,
}

/// `siop_selfcheck`: one judgement per id, exactly `ids.len()` of them; `id` from the ids,
/// `reason` (≤100) before `rating` (the three labels).
pub fn schema(ids: &[String]) -> Value {
    let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
    let ratings: Vec<&str> = Rating::ALL.iter().map(|r| r.label()).collect();
    obj(vec![(
        "items",
        json!({
            "type": "array",
            "items": obj(vec![
                ("id", enum_of(&ids)),
                ("reason", s_max(REASON_CHARS)),
                ("rating", enum_of(&ratings)),
            ]),
            "minItems": ids.len(),
            "maxItems": ids.len(),
        }),
    )])
}

/// The judgements that can be used, in the items' order, and what the retry should fix: an id
/// that is not one of `ids` or a rating outside the three labels is dropped; a repeated id keeps
/// its first judgement; an item left out or without a reason is a problem. Nothing usable at
/// all: no output.
pub(super) fn evaluate(raw: &RawSelfCheck, ids: &[String]) -> Checked<Vec<Judgement>> {
    let mut out: Vec<Judgement> = Vec::new();
    let mut problems: Vec<String> = Vec::new();
    for j in &raw.items {
        let id = j.id.trim();
        if !ids.iter().any(|i| i == id) {
            problems.push(format!("「{}」不是要檢核的項目。", clip_chars(id, 20)));
            continue;
        }
        if out.iter().any(|o| o.id == id) {
            problems.push(format!("{id} 檢核了兩次，每一項只寫一筆。"));
            continue;
        }
        let Some(rating) = Rating::from_label(&j.rating) else {
            problems.push(format!(
                "{id} 的 rating「{}」不是「符合」「部分符合」「未見」之一。",
                clip_chars(&j.rating, 20)
            ));
            continue;
        };
        let reason = clean_text(&j.reason, REASON_CHARS);
        if reason.is_empty() {
            problems.push(format!("{id} 沒有寫 reason。"));
        }
        out.push(Judgement {
            id: id.to_string(),
            rating,
            reason,
        });
    }
    let missing: Vec<&str> = ids
        .iter()
        .filter(|i| !out.iter().any(|o| &o.id == *i))
        .map(String::as_str)
        .collect();
    if !missing.is_empty() {
        problems.push(format!(
            "這些項目沒有檢核：{}。每一項都要有一筆。",
            missing.join("、")
        ));
    }
    out.sort_by_key(|j| ids.iter().position(|i| *i == j.id));
    Checked {
        output: (!out.is_empty()).then_some(out),
        problems,
    }
}

// ---------------------------------------------------------------- the page as text

fn push_line(out: &mut Vec<String>, indent: usize, label: &str, text: &str) {
    let text = text.trim();
    if !text.is_empty() {
        out.push(format!("{}{label}{text}", "  ".repeat(indent)));
    }
}

fn joined(list: &[String], sep: &str) -> String {
    list.iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(sep)
}

fn step_text(no: usize, s: &Step, out: &mut Vec<String>) {
    let mut head = format!("第 {no} 步｜{}｜{}｜{} 分鐘", s.phase, s.title, s.minutes);
    if !s.grouping.is_empty() {
        head.push_str(&format!("｜分組：{}", s.grouping));
    }
    if !s.skills.is_empty() {
        head.push_str(&format!("｜技能：{}", s.skills.join("、")));
    }
    if !s.bloom.is_empty() {
        let levels: Vec<&str> = s.bloom.iter().map(|b| b.label()).collect();
        head.push_str(&format!("｜Bloom：{}", levels.join("、")));
    }
    out.push(format!("  {head}"));
    push_line(out, 2, "教學思路：", &s.idea);
    if !s.roles.is_empty() {
        let roles: Vec<String> = s
            .roles
            .iter()
            .map(|r| format!("{}：{}", r.role, r.task))
            .collect();
        push_line(out, 2, "角色：", &roles.join("；"));
    }
    push_line(out, 2, "教師巡視：", &s.monitoring);
    push_line(out, 2, "鷹架：", &s.scaffolding);
    for (k, o) in s.options.iter().enumerate() {
        let name = OPTION_NAMES_ZH.get(k).copied().unwrap_or("選項");
        push_line(
            out,
            2,
            &format!("{name}「{}」（{} 分鐘）：", o.title, o.minutes),
            &format!("{} {}", o.idea, joined(&o.script, "｜")),
        );
    }
    push_line(out, 2, "課堂用語：", &joined(&s.script, "｜"));
    push_line(out, 2, "卡住了怎麼辦：", &s.stuck_tip);
}

/// An activity plan as the self-check reads it (any version: what a plan does not have is left
/// out).
pub fn render_plan(plan: &ActivityPlan) -> String {
    let mut out = Vec::new();
    push_line(&mut out, 0, "標題：", &plan.title);
    if !plan.goals.is_empty() {
        out.push("學習目標：".to_string());
        let mut numbers: HashMap<Option<GoalKind>, usize> = HashMap::new();
        for g in &plan.goals {
            let n = numbers.entry(g.kind).or_insert(0);
            *n += 1;
            let label = match g.kind {
                Some(k) => format!("{}{}", k.label(), n),
                None => format!("目標{n}"),
            };
            let mut notes = Vec::new();
            if let Some(c) = g.category {
                notes.push(c.label().to_string());
            }
            if let Some(s) = g.supports {
                notes.push(format!("支持內容目標{s}"));
            }
            if g.from_teacher {
                notes.push("教師提供".to_string());
            }
            let notes = if notes.is_empty() {
                String::new()
            } else {
                format!("（{}）", notes.join("，"))
            };
            let lines = joined(&[g.zh.clone(), g.en.clone()], "／");
            out.push(format!("  {label}{notes}：{lines}"));
            push_line(&mut out, 2, "關係：", &g.relation);
        }
    }
    if !plan.key_vocabulary.is_empty() {
        let words: Vec<String> = plan
            .key_vocabulary
            .iter()
            .map(|w| format!("{}（{}）", w.word, w.zh))
            .collect();
        push_line(&mut out, 0, "關鍵字彙：", &words.join("、"));
    }
    push_line(&mut out, 0, "教材與教具：", &joined(&plan.materials, "、"));
    if !plan.higher_order_questions.is_empty() {
        out.push("高層次問題：".to_string());
        for q in &plan.higher_order_questions {
            out.push(format!("  - {}（{}）", q.question, q.bloom.label()));
        }
    }
    push_line(&mut out, 0, "背景連結：", &plan.background);
    let minutes: i64 = plan.steps.iter().map(|s| s.minutes).sum();
    out.push(format!(
        "教學步驟（共 {} 步，約 {minutes} 分鐘）：",
        plan.steps.len()
    ));
    for (i, s) in plan.steps.iter().enumerate() {
        step_text(i + 1, s, &mut out);
    }
    push_line(&mut out, 0, "複習：", &plan.review);
    push_line(&mut out, 0, "評量：", &plan.assessment);
    out.join("\n")
}

/// A field this module does not know, printed as 「key：value」 (strings clipped, lists joined,
/// anything else as compact JSON); empty values are left out.
fn extras(value: &Value, known: &[&str], indent: usize, out: &mut Vec<String>) {
    let Some(map) = value.as_object() else {
        return;
    };
    for (k, v) in map {
        if known.contains(&k.as_str()) {
            continue;
        }
        let text = match v {
            Value::Null | Value::Bool(false) => String::new(),
            Value::String(s) => clip_chars(s.trim(), 300),
            Value::Array(a) if a.is_empty() => String::new(),
            Value::Array(a) if a.iter().all(Value::is_string) => clip_chars(
                &a.iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("、"),
                300,
            ),
            Value::Object(o) if o.is_empty() => String::new(),
            other => clip_chars(&other.to_string(), 300),
        };
        push_line(out, indent, &format!("{k}："), &text);
    }
}

const CONTENT_KEYS: [&str; 6] = [
    "objectives",
    "show_objectives",
    "blocks",
    "levels",
    "chosen",
    "notices",
];
const OBJECTIVE_KEYS: [&str; 6] = [
    "id",
    "type",
    "text",
    "student_facing_en",
    "origin",
    "sop_id",
];
const BLOCK_KEYS: [&str; 18] = [
    "id",
    "kind",
    "enabled",
    "title",
    "instructions",
    "paragraph_ids",
    "info_function",
    "student_tasks",
    "objective_ids",
    "vocabulary",
    "items",
    "scaffolds",
    "teacher_notes",
    "origin",
    "message",
    "sop_block_id",
    "template_section_id",
    "bloom",
];
const ITEM_KEYS: [&str; 12] = [
    "id",
    "answer_format",
    "prompt",
    "options",
    "answer",
    "evidence",
    "purpose",
    "levels",
    "origin",
    "bloom",
    "hint",
    "paragraph_ids",
];
const SCAFFOLD_KEYS: [&str; 8] = [
    "id", "type", "kind", "content", "steps", "levels", "auto", "origin",
];
const VOCAB_KEYS: [&str; 5] = ["word", "level", "definition", "context", "paragraph_id"];

fn levels_text(levels: &[Level]) -> String {
    levels
        .iter()
        .map(|l| l.label())
        .collect::<Vec<_>>()
        .join("、")
}

fn block_text(no: usize, b: &Block, several: bool, out: &mut Vec<String>) {
    let kind = match b.kind {
        BlockKind::Warmup => "暖身",
        BlockKind::Activity => "活動",
        BlockKind::ExitTicket => "Exit ticket",
    };
    let mut head = format!("區塊 {}（{kind}）", letter(no));
    if let Some(bloom) = b.bloom {
        head.push_str(&format!("｜Bloom：{}", bloom.label()));
    }
    out.push(format!("{head}：{}", b.title.trim()));
    push_line(out, 1, "作答指示：", &b.instructions);
    if let Some(f) = b.info_function {
        push_line(out, 1, "資訊處理功能：", info_function_label(f));
    }
    push_line(out, 1, "學生任務：", &joined(&b.student_tasks, "、"));
    if !b.vocabulary.is_empty() {
        let words: Vec<String> = b
            .vocabulary
            .iter()
            .map(|v| {
                let mut line = v.word.trim().to_string();
                if !v.definition.trim().is_empty() {
                    line.push_str(&format!("（{}）", clip_chars(v.definition.trim(), 80)));
                }
                let mut rest = Vec::new();
                extras(&json!(v), &VOCAB_KEYS, 0, &mut rest);
                if !rest.is_empty() {
                    line.push_str(&format!("〔{}〕", rest.join("；")));
                }
                line
            })
            .collect();
        push_line(out, 1, "字彙：", &words.join("、"));
    }
    for s in &b.scaffolds {
        let mut text = clip_chars(s.content.trim(), 300);
        if !s.steps.is_empty() {
            text.push_str(&format!(" 步驟：{}", joined(&s.steps, "｜")));
        }
        if several && !s.levels.is_empty() {
            text.push_str(&format!("（版本：{}）", levels_text(&s.levels)));
        }
        push_line(out, 1, &format!("鷹架（{}）：", s.kind.trim()), &text);
        extras(&json!(s), &SCAFFOLD_KEYS, 2, out);
    }
    let mut n = 0;
    for it in &b.items {
        // An item in none of the versions is not on any sheet.
        if several && it.levels.is_empty() {
            continue;
        }
        n += 1;
        let mut tags = Vec::new();
        if let Some(f) = &it.answer_format {
            tags.push(f.clone());
        }
        if let Some(bloom) = it.bloom {
            tags.push(format!("Bloom：{}", bloom.label()));
        }
        if several {
            tags.push(format!("版本：{}", levels_text(&it.levels)));
        }
        if let Some(p) = it.purpose {
            if let Some((_, label)) = EXIT_PURPOSES.iter().find(|(k, _)| *k == p) {
                tags.push(label.to_string());
            }
        }
        let tags = if tags.is_empty() {
            String::new()
        } else {
            format!("（{}）", tags.join("｜"))
        };
        out.push(format!(
            "  第 {n} 題{tags}：{}",
            clip_chars(it.prompt.trim(), 400)
        ));
        if !it.options.is_empty() {
            let options: Vec<String> = it
                .options
                .iter()
                .enumerate()
                .map(|(i, o)| format!("({}) {}", letter(i), clip_chars(o.trim(), 100)))
                .collect();
            push_line(out, 2, "選項：", &options.join(" "));
        }
        push_line(out, 2, "參考答案：", &clip_chars(it.answer.trim(), 300));
        push_line(out, 2, "依據：", &clip_chars(it.evidence.trim(), 200));
        push_line(
            out,
            2,
            "提示（印在基礎版）：",
            &clip_chars(it.hint.trim(), 200),
        );
        extras(&json!(it), &ITEM_KEYS, 2, out);
    }
    push_line(
        out,
        1,
        "教師備註：",
        &clip_chars(b.teacher_notes.trim(), 300),
    );
    extras(&json!(b), &BLOCK_KEYS, 1, out);
}

/// A worksheet as the self-check reads it: the teacher's objective, the versions, the
/// objectives shown on the sheet, then every enabled block with its instructions, words,
/// scaffolds and questions (with the versions each is in, when there are several). Fields this
/// module does not know are printed generically.
pub fn render_worksheet(title: &str, content: &Content, settings: &Settings) -> String {
    let mut out = Vec::new();
    push_line(&mut out, 0, "標題：", title);
    push_line(
        &mut out,
        0,
        "教師寫的內容目標：",
        settings.content_objective.as_deref().unwrap_or_default(),
    );
    push_line(
        &mut out,
        0,
        "教師寫的語言目標：",
        settings.language_objective.as_deref().unwrap_or_default(),
    );
    let several = content.levels.len() > 1;
    if several {
        push_line(&mut out, 0, "版本：", &levels_text(&content.levels));
    }
    if !content.objectives.is_empty() {
        out.push(format!(
            "學習目標{}：",
            if content.show_objectives {
                ""
            } else {
                "（不印在學習單上）"
            }
        ));
        for o in &content.objectives {
            let lines = joined(&[o.text.clone(), o.student_facing_en.clone()], "／");
            out.push(format!("  - {}：{lines}", o.kind.label()));
            extras(&json!(o), &OBJECTIVE_KEYS, 2, &mut out);
        }
    }
    for (no, b) in content.blocks.iter().filter(|b| b.enabled).enumerate() {
        block_text(no, b, several, &mut out);
    }
    if !content.notices.is_empty() {
        push_line(&mut out, 0, "提醒：", &joined(&content.notices, "；"));
    }
    extras(&json!(content), &CONTENT_KEYS, 0, &mut out);
    out.join("\n")
}

/// What the output is, for the prompt.
fn output_label(kind: &str, has_template: bool) -> &'static str {
    match kind {
        KIND_ACTIVITY => "課堂活動（一節課的教學活動規劃）",
        KIND_KIT => "教學套件（一節課的教學活動規劃，加上一份分基礎版、一般版、進階版的學習單）",
        KIND_COMPREHENSION => "閱讀理解題學習單",
        KIND_TIERED => "分層學習單（同一篇文章的基礎版、一般版、進階版）",
        KIND_WORKSHEET if has_template => "完整學習單（照教師自己的學習單樣式）",
        _ => "完整學習單",
    }
}

// ---------------------------------------------------------------- storage

/// A check as the API returns it (`PrepTurn.siop_check`, `GET …/siop-check`): every item of
/// [`ITEMS`] in order, with its rating and reason once the check is done.
#[derive(Debug, Clone, Serialize)]
pub struct SiopCheck {
    pub id: i64,
    pub run_no: i32,
    /// `running` | `done` | `failed`
    pub status: String,
    pub error: Option<String>,
    /// The worksheet version judged (worksheet turns and kits, once the check read it).
    pub worksheet_version_no: Option<i32>,
    pub items: Vec<ShownItem>,
    pub interaction_id: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShownItem {
    #[serde(flatten)]
    pub item: SiopItem,
    /// `null`: a classroom item, or one this check did not judge.
    pub rating: Option<Rating>,
    pub reason: String,
}

#[derive(sqlx::FromRow)]
struct CheckRow {
    id: i64,
    turn_id: i64,
    run_no: i32,
    status: String,
    error: Option<String>,
    worksheet_version_no: Option<i32>,
    items: sqlx::types::Json<Vec<Judgement>>,
    interaction_id: Option<i64>,
    created_at: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>,
}

const CHECK_COLS: &str = "id, turn_id, run_no, status, error, worksheet_version_no, items, \
     interaction_id, created_at, finished_at";

impl From<CheckRow> for SiopCheck {
    fn from(r: CheckRow) -> Self {
        let judged = r.items.0;
        SiopCheck {
            id: r.id,
            run_no: r.run_no,
            status: r.status,
            error: r.error,
            worksheet_version_no: r.worksheet_version_no,
            items: ITEMS
                .iter()
                .map(|item| {
                    let j = judged.iter().find(|j| j.id == item.id);
                    ShownItem {
                        item: *item,
                        rating: j.map(|j| j.rating),
                        reason: j.map(|j| j.reason.clone()).unwrap_or_default(),
                    }
                })
                .collect(),
            interaction_id: r.interaction_id,
            created_at: r.created_at,
            finished_at: r.finished_at,
        }
    }
}

/// Opens the next run of a turn's check (`running`), inside the caller's transaction; `None`
/// while another run of it is running.
pub async fn open(conn: &mut PgConnection, turn_id: i64) -> sqlx::Result<Option<i64>> {
    sqlx::query_scalar(&format!(
        "INSERT INTO siop_checks (turn_id, run_no, status)
         SELECT $1, COALESCE(MAX(run_no), 0) + 1, '{STATUS_RUNNING}' FROM siop_checks
          WHERE turn_id = $1
         ON CONFLICT DO NOTHING
         RETURNING id"
    ))
    .bind(turn_id)
    .fetch_optional(conn)
    .await
}

/// Seconds after which a running check is reported as failed (its call and retry must have
/// timed out, or the server restarted).
pub fn stale_after(state: &AppState) -> f64 {
    (state.config.ai_timeout_secs * 3 + 60) as f64
}

/// Fails the running checks of a session's turns that outlived [`stale_after`].
pub async fn expire_stale(state: &AppState, session_id: i64) -> sqlx::Result<()> {
    sqlx::query(&format!(
        "UPDATE siop_checks c SET status = '{STATUS_FAILED}', error = $2, finished_at = now()
          FROM prep_turns t
         WHERE t.id = c.turn_id AND t.session_id = $1 AND c.status = '{STATUS_RUNNING}'
           AND c.created_at < now() - make_interval(secs => $3)"
    ))
    .bind(session_id)
    .bind(MSG_STALE)
    .bind(stale_after(state))
    .execute(&state.db)
    .await?;
    Ok(())
}

/// The latest check of every turn of a session that has one.
pub async fn latest_of_session(
    db: &PgPool,
    session_id: i64,
) -> sqlx::Result<HashMap<i64, SiopCheck>> {
    let rows: Vec<CheckRow> = sqlx::query_as(&format!(
        "SELECT DISTINCT ON (c.turn_id) {cols} FROM siop_checks c
           JOIN prep_turns t ON t.id = c.turn_id
          WHERE t.session_id = $1
          ORDER BY c.turn_id, c.run_no DESC",
        cols = CHECK_COLS
            .split(", ")
            .map(|c| format!("c.{}", c.trim()))
            .collect::<Vec<_>>()
            .join(", ")
    ))
    .bind(session_id)
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| (r.turn_id, SiopCheck::from(r)))
        .collect())
}

/// The latest check of a turn.
pub async fn latest(db: &PgPool, turn_id: i64) -> sqlx::Result<Option<SiopCheck>> {
    let row: Option<CheckRow> = sqlx::query_as(&format!(
        "SELECT {CHECK_COLS} FROM siop_checks WHERE turn_id = $1 ORDER BY run_no DESC LIMIT 1"
    ))
    .bind(turn_id)
    .fetch_optional(db)
    .await?;
    Ok(row.map(SiopCheck::from))
}

async fn set_version(
    db: &PgPool,
    check_id: i64,
    worksheet_id: i64,
    version_no: i32,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE siop_checks SET worksheet_id = $2, worksheet_version_no = $3 WHERE id = $1",
    )
    .bind(check_id)
    .bind(worksheet_id)
    .bind(version_no)
    .execute(db)
    .await?;
    Ok(())
}

type Outcome = Result<(Vec<Judgement>, Option<i64>), String>;

/// Closes a running check: `done` with its judgements, or `failed` with why.
async fn finish(db: &PgPool, check_id: i64, outcome: Outcome) -> sqlx::Result<()> {
    let (status, items, interaction_id, error) = match outcome {
        Ok((items, interaction_id)) => (STATUS_DONE, items, interaction_id, None),
        Err(message) => (STATUS_FAILED, Vec::new(), None, Some(message)),
    };
    sqlx::query(&format!(
        "UPDATE siop_checks
            SET status = $2, items = $3, interaction_id = $4, error = $5, finished_at = now()
          WHERE id = $1 AND status = '{STATUS_RUNNING}'"
    ))
    .bind(check_id)
    .bind(status)
    .bind(sqlx::types::Json(items))
    .bind(interaction_id)
    .bind(error)
    .execute(db)
    .await?;
    Ok(())
}

/// Starts another run of a turn's check (再試一次, or after the worksheet changed): the turn
/// must be a checked kind and `done`, with no run of its check running.
pub async fn restart(state: &AppState, turn: &TurnRow) -> AppResult<SiopCheck> {
    if !is_checked(&turn.kind) {
        return Err(AppError::bad_request(MSG_NOT_CHECKED));
    }
    if turn.status != STATUS_DONE {
        return Err(AppError::Conflict(MSG_TURN_NOT_DONE.to_string()));
    }
    let mut conn = state.db.acquire().await?;
    let Some(id) = open(&mut conn, turn.id).await? else {
        drop(conn);
        return Err(AppError::Conflict(MSG_CHECK_RUNNING.to_string()));
    };
    drop(conn);
    spawn(state, id);
    latest(&state.db, turn.id)
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("siop check {id} vanished")))
}

// ---------------------------------------------------------------- the run

/// Runs a check opened with [`open`] in the background; a failure or a panic closes it as
/// `failed` with a message the teacher can read.
pub fn spawn(state: &AppState, check_id: i64) {
    let state = state.clone();
    tokio::spawn(async move {
        let work = {
            let state = state.clone();
            tokio::spawn(async move { run(&state, check_id).await })
        };
        let outcome = match work.await {
            Ok(Ok(outcome)) => outcome,
            Ok(Err(e)) => {
                tracing::error!(check_id, error = %e, "siop self-check failed");
                Err(MSG_INTERNAL.to_string())
            }
            Err(e) => {
                tracing::error!(check_id, error = %e, "siop self-check panicked");
                Err(MSG_INTERNAL.to_string())
            }
        };
        if let Err(e) = finish(&state.db, check_id, outcome).await {
            tracing::error!(check_id, error = %e, "could not close the siop self-check");
        }
    });
}

/// What the check reads: the output as text, the passage it was made from, and the worksheet
/// version judged.
struct Page {
    text: String,
    passage: String,
    worksheet: Option<(i64, i32)>,
    has_template: bool,
}

async fn page_of(
    state: &AppState,
    turn: &TurnRow,
    session: &SessionRow,
) -> AppResult<Result<Page, String>> {
    let db = &state.db;
    let plan: Option<ActivityPlan> = matches!(turn.kind.as_str(), KIND_ACTIVITY | KIND_KIT)
        .then(|| {
            turn.result
                .clone()
                .and_then(|r| serde_json::from_value(r).ok())
        })
        .flatten();
    let worksheet = match turn.worksheet_id {
        Some(ws_id) if turn.kind != KIND_ACTIVITY => {
            match ws_store::owned(db, ws_id, session.teacher_id).await {
                Ok(ws) => Some((ws_store::latest(db, ws_id).await?, ws)),
                Err(AppError::NotFound(_)) => None,
                Err(e) => return Err(e),
            }
        }
        _ => None,
    };
    let mut parts = Vec::new();
    if let Some(p) = &plan {
        parts.push(if turn.kind == KIND_KIT {
            format!("【教學活動規劃】\n{}", render_plan(p))
        } else {
            render_plan(p)
        });
    }
    let mut judged = None;
    let mut passage = None;
    let mut has_template = false;
    if let Some((v, ws)) = &worksheet {
        let items: usize = v.content.blocks.iter().map(|b| b.items.len()).sum();
        if items == 0 && plan.is_none() {
            return Ok(Err(MSG_NO_ITEMS.to_string()));
        }
        let text = render_worksheet(&ws.title, &v.content, &ws.settings);
        parts.push(if turn.kind == KIND_KIT {
            format!("【學習單】\n{text}")
        } else {
            text
        });
        judged = Some((ws.id, v.version_no));
        passage = Some(ws.material.numbered());
        has_template = ws.template_id.is_some();
    }
    match turn.kind.as_str() {
        KIND_ACTIVITY | KIND_KIT if plan.is_none() && worksheet.is_none() => {
            return Ok(Err(MSG_NO_OUTPUT.to_string()))
        }
        KIND_ACTIVITY | KIND_KIT => {}
        _ if worksheet.is_none() => return Ok(Err(MSG_WORKSHEET_GONE.to_string())),
        _ => {}
    }
    let passage = match passage {
        Some(p) => p,
        None => match run::material(db, session).await? {
            Some(m) => run::passage(&m),
            None => "（教材已經刪除，只看產出本身。）".to_string(),
        },
    };
    Ok(Ok(Page {
        text: clip_chars(&parts.join("\n\n"), OUTPUT_CHARS),
        passage,
        worksheet: judged,
        has_template,
    }))
}

async fn run(state: &AppState, check_id: i64) -> AppResult<Outcome> {
    let db = &state.db;
    let (turn_id, run_no): (i64, i32) =
        sqlx::query_as("SELECT turn_id, run_no FROM siop_checks WHERE id = $1")
            .bind(check_id)
            .fetch_one(db)
            .await?;
    let turn: TurnRow = super::store::turn(db, turn_id).await?;
    let session = super::store::session_row(db, turn.session_id).await?;
    let page = match page_of(state, &turn, &session).await? {
        Ok(p) => p,
        Err(message) => return Ok(Err(message)),
    };
    if let Some((ws_id, version_no)) = page.worksheet {
        set_version(db, check_id, ws_id, version_no).await?;
    }
    let (profile, _) = run::class_profile(state, &session).await?;
    let ids = judged_ids();
    let output = output_label(&turn.kind, page.has_template);
    let vars: HashMap<&'static str, String> = HashMap::from([
        ("output_kind", output.to_string()),
        ("class_profile", crate::routes::or_blank(&profile)),
        ("passage_numbered", page.passage),
        ("output_text", page.text),
        ("item_count", ids.len().to_string()),
        ("items", items_text()),
    ]);
    let c = Call {
        key: KEY,
        scaffold: SCAFFOLD,
        schema_name: SCHEMA_NAME,
        schema: schema(&ids),
        vars,
        required: vec![("class_profile", "班級特性")],
        // The material, and the output made from it, are logged only as their length and hash.
        redact: vec![
            ("passage_numbered", "備課對話教材"),
            ("output_text", "檢核的產出"),
        ],
        fake_context: json!({
            "task": KEY,
            "items": ids,
            "output": output,
            "run_no": run_no,
        }),
        max_tokens: MAX_TOKENS,
        // Another run of the same check is a new request (the gateway caches identical ones).
        temperature: BASE_TEMPERATURE + 0.1 * (run_no - 1).clamp(0, 4) as f32,
        trace: json!({
            "session_id": session.id,
            "turn_id": turn.id,
            "kind": turn.kind,
            "check_id": check_id,
            "run_no": run_no,
            "worksheet": page.worksheet.map(|(id, v)| json!({ "id": id, "version_no": v })),
        }),
        progress: None,
    };
    let result = run::call(
        state,
        session.teacher_id,
        c,
        |raw: &RawSelfCheck| evaluate(raw, &ids),
        run::nothing,
    )
    .await?;
    Ok(result.map(|(items, id)| (items, Some(id))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prep::{
        Goal, HigherQuestion, KeyWord, LanguageCategory, Role, StepOption, PHASE_DEVELOP,
        PHASE_WARMUP,
    };
    use crate::worksheet::bloom::Bloom;
    use crate::worksheet::{Item, Scaffold, Vocab};

    #[test]
    fn the_items_follow_the_three_tables_of_the_spec() {
        let classroom: Vec<&str> = ITEMS
            .iter()
            .filter(|i| i.scope == Scope::Classroom)
            .map(|i| i.id)
            .collect();
        assert_eq!(classroom, ["10", "18", "25", "26"]);
        let partial: Vec<&str> = ITEMS
            .iter()
            .filter(|i| i.scope == Scope::Partial)
            .map(|i| i.id)
            .collect();
        assert_eq!(partial, ["12", "23-24", "29"]);
        // Every protocol item 1–30 appears once.
        let mut numbers: Vec<u32> = ITEMS
            .iter()
            .flat_map(|i| {
                let (a, b) = i.id.split_once('-').unwrap_or((i.id, i.id));
                a.parse::<u32>().unwrap()..=b.parse::<u32>().unwrap()
            })
            .collect();
        numbers.sort();
        assert_eq!(numbers, (1..=30).collect::<Vec<_>>());
        for i in &ITEMS {
            assert_eq!(
                i.question.is_empty(),
                i.scope == Scope::Classroom,
                "{}",
                i.id
            );
            assert_eq!(i.note.is_empty(), i.scope == Scope::Page, "{}", i.id);
        }
    }

    #[test]
    fn classroom_items_never_reach_the_model() {
        let ids = judged_ids();
        assert_eq!(ids.len(), 22);
        for id in ["10", "18", "25", "26"] {
            assert!(!ids.iter().any(|i| i == id), "{id}");
        }
        let text = items_text();
        assert_eq!(text.lines().count(), 22);
        for name in ["教師語速與用語", "等待時間", "學生投入", "節奏"] {
            assert!(!text.contains(name), "{name}");
        }
        assert!(text.contains("12｜讓概念清楚的技巧（只看書面部分：只看得到書面的視覺輔助）："));
        let s = schema(&ids);
        let enum_ids = &s["properties"]["items"]["items"]["properties"]["id"]["enum"];
        assert_eq!(enum_ids, &json!(ids));
        assert!(!enum_ids.as_array().unwrap().contains(&json!("10")));
    }

    #[test]
    fn the_schema_is_bounded_and_in_order() {
        let ids = judged_ids();
        let s = schema(&ids);
        let items = &s["properties"]["items"];
        assert_eq!(items["minItems"], 22);
        assert_eq!(items["maxItems"], 22);
        let fields: Vec<&String> = items["items"]["properties"]
            .as_object()
            .unwrap()
            .keys()
            .collect();
        assert_eq!(
            fields,
            ["id", "reason", "rating"],
            "the reason before the rating"
        );
        assert_eq!(
            items["items"]["properties"]["reason"]["maxLength"],
            REASON_CHARS
        );
        assert_eq!(
            items["items"]["properties"]["rating"]["enum"],
            json!(["符合", "部分符合", "未見"])
        );
    }

    fn raw(items: &[(&str, &str, &str)]) -> RawSelfCheck {
        RawSelfCheck {
            items: items
                .iter()
                .map(|(id, reason, rating)| RawJudgement {
                    id: id.to_string(),
                    reason: reason.to_string(),
                    rating: rating.to_string(),
                })
                .collect(),
        }
    }

    #[test]
    fn judgements_are_checked_and_put_in_order() {
        let ids: Vec<String> = ["1-2", "3", "4"].iter().map(|s| s.to_string()).collect();
        let c = evaluate(
            &raw(&[
                ("4", "**有圖表**組織。", "部分符合"),
                ("10", "不該檢核", "符合"),
                ("1-2", "兩種目標都有。", "符合"),
                ("1-2", "又一次", "未見"),
                ("3", "理由", "很好"),
            ]),
            &ids,
        );
        let out = c.output.unwrap();
        assert_eq!(
            out,
            [
                Judgement {
                    id: "1-2".into(),
                    rating: Rating::Met,
                    reason: "兩種目標都有。".into()
                },
                Judgement {
                    id: "4".into(),
                    rating: Rating::Partial,
                    reason: "有圖表組織。".into()
                },
            ]
        );
        let problems = c.problems.join("\n");
        assert!(problems.contains("「10」不是要檢核的項目"), "{problems}");
        assert!(problems.contains("1-2 檢核了兩次"), "{problems}");
        assert!(problems.contains("3 的 rating「很好」"), "{problems}");
        assert!(problems.contains("這些項目沒有檢核：3"), "{problems}");
        // Nothing usable.
        assert!(evaluate(&raw(&[]), &ids).output.is_none());
        // All of them: no problem.
        let all = evaluate(
            &raw(&[("1-2", "a", "符合"), ("3", "b", "未見"), ("4", "c", "符合")]),
            &ids,
        );
        assert!(all.problems.is_empty(), "{:?}", all.problems);
    }

    fn plan() -> ActivityPlan {
        ActivityPlan {
            title: "信任的雨傘".into(),
            goals: vec![
                Goal::new(
                    GoalKind::Content,
                    "學生能說出主旨",
                    "Students will be able to tell the main idea.",
                    vec![],
                ),
                Goal {
                    category: Some(LanguageCategory::Strategy),
                    supports: Some(1),
                    relation: "學生要用重讀的策略，才能說出主旨".into(),
                    from_teacher: true,
                    ..Goal::new(GoalKind::Language, "學生能重讀找出關鍵句", "", vec![])
                },
            ],
            key_vocabulary: vec![KeyWord {
                word: "trust".into(),
                zh: "信任".into(),
            }],
            materials: vec!["課文".into()],
            higher_order_questions: vec![HigherQuestion {
                question: "Why did it work?".into(),
                bloom: Bloom::Evaluate,
            }],
            background: "借東西的經驗".into(),
            steps: vec![
                Step {
                    phase: PHASE_WARMUP.into(),
                    title: "看圖".into(),
                    minutes: 5,
                    idea: "看圖猜主題".into(),
                    options: vec![StepOption {
                        title: "圖片".into(),
                        idea: "展示圖片".into(),
                        script: vec!["What do you see?".into()],
                        minutes: 5,
                    }],
                    script: vec!["Hello.".into()],
                    stuck_tip: String::new(),
                    bloom: vec![Bloom::Understand],
                    grouping: "兩人一組".into(),
                    roles: vec![Role {
                        role: "夥伴 A".into(),
                        task: "描述".into(),
                    }],
                    monitoring: String::new(),
                    skills: vec!["聽".into(), "說".into()],
                    scaffolding: "示範".into(),
                },
                Step {
                    phase: PHASE_DEVELOP.into(),
                    title: "讀".into(),
                    minutes: 30,
                    idea: "逐段讀".into(),
                    options: vec![],
                    script: vec![],
                    stuck_tip: "給提示".into(),
                    bloom: vec![],
                    grouping: String::new(),
                    roles: vec![],
                    monitoring: String::new(),
                    skills: vec![],
                    scaffolding: String::new(),
                },
            ],
            review: "字卡".into(),
            assessment: "exit ticket".into(),
            curriculum: vec![],
        }
    }

    #[test]
    fn a_plan_is_read_as_compact_text() {
        let text = render_plan(&plan());
        for line in [
            "標題：信任的雨傘",
            "  內容目標1：學生能說出主旨／Students will be able to tell the main idea.",
            "  語言目標1（學習策略，支持內容目標1，教師提供）：學生能重讀找出關鍵句",
            "    關係：學生要用重讀的策略，才能說出主旨",
            "關鍵字彙：trust（信任）",
            "  - Why did it work?（評鑑）",
            "背景連結：借東西的經驗",
            "教學步驟（共 2 步，約 35 分鐘）：",
            "  第 1 步｜引起動機｜看圖｜5 分鐘｜分組：兩人一組｜技能：聽、說｜Bloom：理解",
            "    角色：夥伴 A：描述",
            "    選項一「圖片」（5 分鐘）：展示圖片 What do you see?",
            "  第 2 步｜發展活動｜讀｜30 分鐘",
            "    卡住了怎麼辦：給提示",
            "評量：exit ticket",
        ] {
            assert!(text.lines().any(|l| l == line), "{line:?} missing:\n{text}");
        }
        assert!(!text.contains('{'), "not JSON");
        // A plan of version 1 (one list of goals, no SIOP fields) still reads.
        let old: ActivityPlan = serde_json::from_value(json!({
            "title": "舊的", "goals": [{ "text": "學生能說出主旨", "curriculum_codes": [] }],
            "steps": [], "curriculum": [],
        }))
        .unwrap();
        let text = render_plan(&old);
        assert!(text.contains("  目標1：學生能說出主旨"), "{text}");
        assert!(!text.contains("背景連結"));
    }

    #[test]
    fn a_worksheet_is_read_generically() {
        let content: Content = serde_json::from_value(json!({
            "levels": ["basic", "standard", "advanced"],
            "objectives": [{ "id": "O1", "type": "content", "text": "說出主旨",
                             "student_facing_en": "I can tell the main idea." }],
            "blocks": [
                { "id": "b1", "kind": "warmup", "title": "Before you read",
                  "instructions": "Look at the picture.",
                  "vocabulary": [{ "word": "trust", "definition": "信任" }],
                  "scaffolds": [{ "id": "s1", "type": "procedural", "kind": "清楚的分步指示及完成順序",
                                  "content": "", "steps": ["Read.", "Answer."] }],
                  "items": [{ "id": "i1", "answer_format": "選擇題", "prompt": "Who?",
                              "options": ["A", "B"], "answer": "A", "levels": ["basic"],
                              "bloom": "remember", "hint": "Look at line 1." }] },
                { "id": "b2", "kind": "activity", "enabled": false, "title": "Hidden" },
                { "id": "b3", "kind": "exit_ticket", "title": "Exit ticket",
                  "items": [{ "id": "x1", "prompt": "One thing you learned.", "purpose": "reflection",
                              "levels": ["basic", "standard", "advanced"] },
                            { "id": "gone", "prompt": "In no version.", "levels": [] }] },
            ],
            "notices": ["一題改標了層次。"],
        }))
        .unwrap();
        let settings = Settings {
            content_objective: Some("能說出主旨".into()),
            language_objective: Some("能用 because 說明原因".into()),
            ..Settings::default()
        };
        let text = render_worksheet("The Umbrella", &content, &settings);
        for line in [
            "教師寫的內容目標：能說出主旨",
            "教師寫的語言目標：能用 because 說明原因",
            "版本：基礎版、一般版、進階版",
            "  - 內容目標：說出主旨／I can tell the main idea.",
            "區塊 A（暖身）：Before you read",
            "  作答指示：Look at the picture.",
            "  字彙：trust（信任）",
            "  鷹架（清楚的分步指示及完成順序）：步驟：Read.｜Answer.",
            "  第 1 題（選擇題｜Bloom：記憶｜版本：基礎版）：Who?",
            "    選項：(A) A (B) B",
            "    提示（印在基礎版）：Look at line 1.",
            "區塊 B（Exit ticket）：Exit ticket",
            "  第 1 題（版本：基礎版、一般版、進階版｜學生反思）：One thing you learned.",
            "提醒：一題改標了層次。",
        ] {
            assert!(text.lines().any(|l| l == line), "{line:?} missing:\n{text}");
        }
        assert!(!text.contains("Hidden") && !text.contains("In no version"));
        // A field added to an item, a block or the content later is read too, as 「key：value」.
        let mut extra = Vec::new();
        extras(
            &json!({
                "id": "i1", "prompt": "Who?", "origin": "ai",
                "answers_by_level": { "basic": "A word.", "advanced": "Two sentences." },
                "labels": ["A", "B"], "note": "  ", "image": null,
            }),
            &ITEM_KEYS,
            2,
            &mut extra,
        );
        assert_eq!(
            extra,
            [
                r#"    answers_by_level：{"basic":"A word.","advanced":"Two sentences."}"#,
                "    labels：A、B",
            ]
        );
        // One version: no version tags.
        let one = Content {
            levels: vec![Level::Standard],
            blocks: vec![Block {
                items: vec![Item {
                    prompt: "Why?".into(),
                    levels: vec![Level::Standard],
                    ..Item::default()
                }],
                scaffolds: vec![Scaffold::default()],
                vocabulary: vec![Vocab::default()],
                ..Block::default()
            }],
            ..Content::default()
        };
        let text = render_worksheet("T", &one, &Settings::default());
        assert!(text.contains("  第 1 題：Why?"), "{text}");
        assert!(!text.contains("版本"));
    }
}
