//! Deterministic outputs of the fake provider for the 備課對話 calls and the SIOP self-check
//! (tests, CI, local demos), derived from `fake_context`. The generic markers of `ai::fake`
//! (`FAKE_TIMEOUT`, `FAKE_ERROR`, `FAKE_REFUSAL`, `FAKE_RUNAWAY`, `FAKE_SLOW`) work as
//! everywhere; written in the teacher's request they reach the model through the user message.
//!
//! | marker                 | effect                                                               |
//! |------------------------|----------------------------------------------------------------------|
//! | `FAKE_BLOCK`           | activity / ask: unusable output in both calls (the turn fails)        |
//! | `FAKE_BLOCK_ONCE`      | activity / ask: the first call is unusable, the retry is fine         |
//! | `FAKE_PREP_NO_PLAN`    | activity: unusable output in both calls, without touching the worksheet calls (a kit whose plan fails) |
//! | `FAKE_PREP_ONE_GROUP`  | activity: no 語言目標 in both calls (retried; the retry's plan is shown with its 內容目標 only) |
//! | `FAKE_PREP_ONE_GROUP_ONCE` | activity: no 語言目標 in the first call, the retry has both groups |
//! | `FAKE_PREP_FLAT`       | activity: every step 全班, no roles, only 讀 in both calls (retried; the retry's plan is shown as it is) |
//! | `FAKE_PREP_FLAT_ONCE`  | activity: the same in the first call only; the retry has three groupings and all four skills |
//! | `FAKE_PREP_BAD_REVISE` | revise: the change uses an answer format outside the fixed labels in both calls (it is dropped and the reply says so) |
//! | `FAKE_PREP_REVISE_ADD` | revise: changes the **last** block that has items and adds one new item at its end |
//! | `FAKE_SIOP_BLOCK`      | SIOP self-check: unusable output in both calls (the check fails)      |
//! | `FAKE_SIOP_PARTIAL_ONCE` | SIOP self-check: the first call leaves out the last item, the retry has them all |
//! | `FAKE_SIOP_FAIL_FIRST` | SIOP self-check: the first check of a turn times out; a later one (再試一次) works |
//!
//! The plan (version 4 of the prompt) has three 內容目標: the second starts with a vague verb
//! (了解; the server drops it). Of its five 語言目標 the server keeps two, the 學習策略 one first:
//! one serves the dropped 內容目標, one has Chinese where its English should be, one has a
//! relation that is not 「學生要用……，才能……」. The first 內容目標 cites the first curriculum
//! candidate plus a code that was never offered (the server drops it), and the 發展活動 step asks
//! for 80 minutes (clamped to 50). The steps are 兩人一組 (two partners), 小組 (four roles and
//! monitoring) and 個人 (a role the server drops), with all four skills between them. Every step
//! carries Bloom levels, out of order, repeated and one too many on 發展活動 (the server keeps
//! three, in Bloom order). When the teacher's 內容目標 or 語言目標 is given (`teacher_content`,
//! `teacher_language` in the context), the plan has one goal in that group, reworded with
//! 「（模型改寫）」 (the server keeps the teacher's words). The revision changes the first block
//! that has items: its first item's prompt gets " (revised)".
//!
//! The self-check judges every item it is given, in order: item `3` is 部分符合, `17` is 未見, the
//! rest 符合, each with a one-sentence reason that names the output kind.

use serde_json::{json, Value};

use crate::worksheet::FMT_SHORT;

fn unusable(attempt: i32, has: &dyn Fn(&str) -> bool) -> bool {
    // FAKE_BLOCK_ONCE contains FAKE_BLOCK, so test it first.
    if has("FAKE_BLOCK_ONCE") {
        attempt == 1
    } else {
        has("FAKE_BLOCK")
    }
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Whether a `_ONCE` marker or its always-on form applies to this attempt (the `_ONCE` form
/// contains the other, so it is tested first).
fn marked(has: &dyn Fn(&str) -> bool, marker: &str, attempt: i32) -> bool {
    if has(&format!("{marker}_ONCE")) {
        attempt == 1
    } else {
        has(marker)
    }
}

/// A step of the fake plan (version 4 fields included).
#[allow(clippy::too_many_arguments)]
fn step(
    phase: &str,
    title: &str,
    minutes: i64,
    idea: &str,
    grouping: &str,
    roles: Value,
    monitoring: &str,
    skills: Value,
    bloom: Value,
    options: Value,
    script: Value,
    stuck_tip: &str,
) -> Value {
    json!({
        "phase": phase, "title": title, "minutes": minutes, "idea": idea,
        "grouping": grouping, "roles": roles, "monitoring": monitoring, "skills": skills,
        "scaffolding": format!("{title}：教師先示範一次，再提供句型框架。"),
        "bloom": bloom, "options": options, "script": script, "stuck_tip": stuck_tip,
    })
}

pub fn activity(ctx: &Value, attempt: i32, has: &dyn Fn(&str) -> bool) -> Value {
    if unusable(attempt, has) || has("FAKE_PREP_NO_PLAN") {
        return json!({
            "title": "（測試）不完整的規劃",
            "content_goals": [],
            "language_goals": [],
            "steps": [],
        });
    }
    let title = ctx["material_title"].as_str().unwrap_or_default().trim();
    let title = if title.is_empty() { "這一課" } else { title };
    let mut codes: Vec<String> = strings(&ctx["candidates"]).into_iter().take(1).collect();
    codes.push("NOT-A-REAL-CODE".to_string());
    let script = |line: &str| vec![line.to_string()];
    let one_group = marked(has, "FAKE_PREP_ONE_GROUP", attempt);
    let flat = marked(has, "FAKE_PREP_FLAT", attempt);
    // The teacher's objectives (when given) come back reworded: the server keeps the teacher's.
    let teacher_content = ctx["teacher_content"].as_str().unwrap_or_default();
    let teacher_language = ctx["teacher_language"].as_str().unwrap_or_default();
    let content_goals = if teacher_content.is_empty() {
        json!([
            {
                "zh": format!("學生能說出「{title}」各段的重點。"),
                "en": "Students will be able to tell the main point of each paragraph of the text.",
                "curriculum_codes": codes,
            },
            // A vague verb: dropped (and the 語言目標 that serves it with it).
            {
                "zh": "學生能了解主角的心情。",
                "en": "Students will understand how the main character feels.",
                "curriculum_codes": [],
            },
            {
                "zh": "學生能根據課文說明主角做決定的原因。",
                "en": "Students will be able to explain, with the text, why the main character made a decision.",
                "curriculum_codes": [],
            },
        ])
    } else {
        json!([{
            "zh": format!("（模型改寫）{teacher_content}"),
            "en": "Students will be able to do what the teacher asked.",
            "curriculum_codes": codes,
        }])
    };
    let language_goals = if one_group {
        json!([])
    } else if !teacher_language.is_empty() {
        json!([{
            "supports": "C1", "category": "學習策略",
            "zh": format!("（模型改寫）{teacher_language}"),
            "en": "Students will be able to use the strategy the teacher asked for.",
            "relation": format!(
                "學生要用教師指定的策略，才能{}",
                if teacher_content.is_empty() {
                    "說出各段的重點"
                } else {
                    teacher_content.trim_start_matches("學生能")
                }
            ),
            "curriculum_codes": [],
        }])
    } else {
        json!([
            {
                "supports": "C3", "category": "語言功能",
                "zh": "學生能用 because 和 so 說明事情的原因與結果。",
                "en": "Students will be able to use because and so to explain causes and results.",
                "relation": "學生要用 because 和 so 說明因果，才能根據課文說明主角做決定的原因。",
                "curriculum_codes": [],
            },
            {
                "supports": "C2", "category": "語言功能",
                "zh": "學生能用 feel 和形容詞說出主角的感受。",
                "en": "Students will be able to use feel and adjectives to say how the character feels.",
                "relation": "學生要用 feel 和形容詞，才能說出主角的感受。",
                "curriculum_codes": [],
            },
            {
                "supports": "C1", "category": "學習策略",
                "zh": "學生能在讀不懂時重讀並畫出每段的關鍵句。",
                "en": "Students will be able to reread and underline the key sentence of each paragraph when they are lost.",
                "relation": "學生要用重讀並畫出關鍵句的策略，才能說出各段的重點。",
                "curriculum_codes": [],
            },
            {
                "supports": "C1", "category": "句型文法",
                "zh": "學生能說出三個描述心情的形容詞。",
                "en": "學生能說出三個形容詞。",
                "relation": "學生要用形容詞，才能說出各段的重點。",
                "curriculum_codes": [],
            },
            {
                "supports": "C1", "category": "學術字彙",
                "zh": "學生能用 first、next、finally 排出事件順序。",
                "en": "Students will be able to use first, next and finally to order events.",
                "relation": "這個目標很重要。",
                "curriculum_codes": [],
            },
        ])
    };
    let (grouping, develop_grouping, wrap_grouping) = if flat {
        ("全班", "全班", "全班")
    } else {
        ("兩人一組", "小組", "個人")
    };
    let pair_roles = if flat {
        json!([])
    } else {
        json!([
            { "role": "夥伴 A", "task": "描述圖片裡看到的東西。" },
            { "role": "夥伴 B", "task": "根據描述猜課文主題並說出理由。" },
        ])
    };
    let group_roles = if flat {
        json!([])
    } else {
        json!([
            { "role": "主持", "task": "帶大家逐段討論，確定每個人都發言。" },
            { "role": "記錄", "task": "把每段的重點寫在小白板上。" },
            { "role": "計時", "task": "提醒每段討論不超過 4 分鐘。" },
            { "role": "報告", "task": "代表小組向全班說明重點。" },
        ])
    };
    let skills = |list: &[&str]| -> Value {
        if flat {
            json!(["讀"])
        } else {
            json!(list)
        }
    };
    json!({
        "title": format!("**{title}** 教學活動規劃（測試）"),
        "content_goals": content_goals,
        "language_goals": language_goals,
        "key_vocabulary": [
            { "word": "trust", "zh": "信任；課文的核心概念。" },
            { "word": "neighbor", "zh": "鄰居。" },
            { "word": "Trust", "zh": "重複的字（伺服器拿掉）。" },
            { "word": "鄰居", "zh": "不是英文（伺服器拿掉）。" },
        ],
        "materials": ["課文第 1–4 段", "老師準備的圖片", " "],
        "higher_order_questions": [
            { "question": "Why do you think the plan worked in the end?", "bloom": "evaluate" },
            { "question": "What would you do if you were the main character?", "bloom": "create" },
            { "question": "Who is the main character?", "bloom": "remember" },
        ],
        "background": "請學生回想自己借東西給別人或向別人借東西的經驗，連結課文的信任主題。",
        "steps": [
            step(
                "引起動機", "看圖猜主題", 8,
                "請學生回想借東西的經驗，再看一張和課文主題有關的圖片，兩兩猜課文在講什麼。",
                grouping, pair_roles, "", skills(&["聽", "說"]),
                json!(["understand", "remember", "understand"]),
                json!([
                    { "title": "選項一：看圖片", "idea": "展示老師準備的圖片，請學生兩兩猜主題。",
                      "script": script("What do you see in this picture?"), "minutes": 5 },
                    { "title": "選項二：提問", "idea": "問學生一個和生活經驗有關的問題。",
                      "script": script("Have you ever helped a stranger?"), "minutes": 5 },
                    { "title": "選項三：小遊戲", "idea": "用三個關鍵字讓學生猜課文內容。",
                      "script": script("Guess the story from these three words."), "minutes": 6 },
                ]),
                json!(["Good morning, everyone.", "Let's look at this picture."]),
                "先給學生兩個選項讓他們選，再請他們說明理由。",
            ),
            step(
                "發展活動", "逐段閱讀與提問", 80,
                "學生分組閱讀第 1 段到最後一段，每段找出一個重點並寫在小白板上。",
                develop_grouping, group_roles, "老師巡視各組，聽記錄員的重點，遇到卡住的組先問一個引導問題。",
                skills(&["讀", "說"]),
                json!(["analyze", "understand", "apply", "evaluate"]),
                json!([
                    { "title": "不該出現的選項", "idea": "只有引起動機可以有選項。",
                      "script": [], "minutes": 5 },
                ]),
                json!(["Read paragraph 1 with your partner.", "What is the main idea?"]),
                "",
            ),
            step(
                "統整與評量", "Exit ticket", 7,
                "學生寫下今天學到的一個重點與一個問題。",
                wrap_grouping, json!([{ "role": "不該有", "task": "個人作業沒有角色。" }]), "",
                skills(&["寫"]),
                json!(["create", "evaluate"]),
                json!([]),
                json!(["Write one thing you learned today."]),
                "提供句型框架：Today I learned that ...",
            ),
        ],
        "review": "用關鍵字彙卡複習 trust、neighbor，請學生用它們各說一句話。",
        "assessment": "用 exit ticket 看學生能否說出各段重點，巡視時聽學生能否用 because 和 so 說明原因。",
    })
}

pub fn ask(ctx: &Value, attempt: i32, has: &dyn Fn(&str) -> bool) -> Value {
    if unusable(attempt, has) {
        return json!({ "paragraphs": [""], "follow_ups": [] });
    }
    let question: String = ctx["question"]
        .as_str()
        .unwrap_or_default()
        .chars()
        .take(60)
        .collect();
    let recent = ctx["recent"].as_array().map_or(0, Vec::len);
    let material = if ctx["has_material"] == true {
        "，並參考了你的教材"
    } else {
        ""
    };
    json!({
        "paragraphs": [
            "## 測試回答".to_string(),
            format!("**你問的是**：{question}"),
            format!("1. 這是測試用的回答{material}（參考了 {recent} 則先前的對話）。"),
            "2. 第二個重點。".to_string(),
        ],
        "follow_ups": ["幫我做一張學習單", "再給我三個暖身活動", "  "],
    })
}

pub fn revise(ctx: &Value, _attempt: i32, has: &dyn Fn(&str) -> bool) -> Value {
    let bad = has("FAKE_PREP_BAD_REVISE");
    let add = has("FAKE_PREP_REVISE_ADD");
    let blocks = ctx["blocks"].as_array().cloned().unwrap_or_default();
    let with_items = |b: &&Value| b["items"].as_array().is_some_and(|i| !i.is_empty());
    let found = if add {
        blocks.iter().rev().find(with_items)
    } else {
        blocks.iter().find(with_items)
    };
    let Some(block) = found else {
        return json!({ "reply": "這份學習單還沒有題目可以修改。（測試）", "changes": [] });
    };
    let label = block["label"].as_str().unwrap_or_default();
    let mut items: Vec<Value> = block["items"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .map(|(k, it)| {
            let format = if bad {
                "連連看".to_string()
            } else {
                it["answer_format"]
                    .as_str()
                    .unwrap_or(FMT_SHORT)
                    .to_string()
            };
            let prompt = it["prompt"].as_str().unwrap_or_default();
            json!({
                "answer_format": format,
                "prompt": if k == 0 { format!("{prompt} (revised)") } else { prompt.to_string() },
                "options": it["options"].clone(),
                "answer": it["answer"].clone(),
            })
        })
        .collect();
    if add {
        items.push(json!({
            "answer_format": FMT_SHORT,
            "prompt": "What would you add to the story? (added)",
            "options": [],
            "answer": "Answers will vary.",
        }));
    }
    json!({
        "reply": format!("已修改 {label} 區的第 1 題。（測試）"),
        "changes": [{
            "block_id": block["block_id"].clone(),
            "title": block["title"].clone(),
            "instructions": block["instructions"].clone(),
            "items": items,
        }],
    })
}

/// The SIOP self-check (`siop_selfcheck`): every item of `items`, in order.
pub fn selfcheck(ctx: &Value, attempt: i32, has: &dyn Fn(&str) -> bool) -> Value {
    if has("FAKE_SIOP_BLOCK") {
        return json!({ "items": [] });
    }
    let mut ids = strings(&ctx["items"]);
    if has("FAKE_SIOP_PARTIAL_ONCE") && attempt == 1 {
        ids.pop();
    }
    let output = ctx["output"].as_str().unwrap_or("這份產出");
    let items: Vec<Value> = ids
        .iter()
        .map(|id| {
            let rating = match id.as_str() {
                "3" => "部分符合",
                "17" => "未見",
                _ => "符合",
            };
            json!({
                "id": id,
                "reason": format!("（測試）{output}裡看得到第 {id} 項的做法。"),
                "rating": rating,
            })
        })
        .collect();
    json!({ "items": items })
}
