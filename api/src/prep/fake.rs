//! Deterministic outputs of the fake provider for the three 備課對話 calls (tests, CI, local
//! demos), derived from `fake_context`. The generic markers of `ai::fake` (`FAKE_TIMEOUT`,
//! `FAKE_ERROR`, `FAKE_REFUSAL`, `FAKE_RUNAWAY`, `FAKE_SLOW`) work as everywhere; written in the
//! teacher's request they reach the model through the user message.
//!
//! | marker                 | effect                                                               |
//! |------------------------|----------------------------------------------------------------------|
//! | `FAKE_BLOCK`           | activity / ask: unusable output in both calls (the turn fails)        |
//! | `FAKE_BLOCK_ONCE`      | activity / ask: the first call is unusable, the retry is fine         |
//! | `FAKE_PREP_NO_PLAN`    | activity: unusable output in both calls, without touching the worksheet calls (a kit whose plan fails) |
//! | `FAKE_PREP_BAD_REVISE` | revise: the change uses an answer format outside the fixed labels in both calls (it is dropped and the reply says so) |
//! | `FAKE_PREP_REVISE_ADD` | revise: changes the **last** block that has items and adds one new item at its end |
//!
//! The plan cites the first curriculum candidate plus a code that was never offered (the server
//! drops it), and its 發展活動 step asks for 80 minutes (clamped to 50). Every step carries Bloom
//! levels (version 2 of the prompt), out of order, repeated and one too many on 發展活動 (the
//! server keeps three, in Bloom order). The revision changes the first block that has items: its
//! first item's prompt gets " (revised)".

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

pub fn activity(ctx: &Value, attempt: i32, has: &dyn Fn(&str) -> bool) -> Value {
    if unusable(attempt, has) || has("FAKE_PREP_NO_PLAN") {
        return json!({ "title": "（測試）不完整的規劃", "goals": [], "steps": [] });
    }
    let title = ctx["material_title"].as_str().unwrap_or_default().trim();
    let title = if title.is_empty() { "這一課" } else { title };
    let mut codes: Vec<String> = strings(&ctx["candidates"]).into_iter().take(1).collect();
    codes.push("NOT-A-REAL-CODE".to_string());
    let script = |line: &str| vec![line.to_string()];
    json!({
        "title": format!("**{title}** 教學活動規劃（測試）"),
        "goals": [
            { "text": format!("學生能說出「{title}」各段的重點。"), "curriculum_codes": codes },
            { "text": "學生能用課文中的句型寫出兩句自己的句子。", "curriculum_codes": [] },
        ],
        "steps": [
            {
                "phase": "引起動機", "title": "看圖猜主題", "minutes": 8,
                "idea": "用一張和課文主題有關的圖片引起學生的興趣，讓學生先猜課文在講什麼。",
                "bloom": ["understand", "remember", "understand"],
                "options": [
                    { "title": "選項一：看圖片", "idea": "展示老師準備的圖片，請學生兩兩猜主題。",
                      "script": script("What do you see in this picture?"), "minutes": 5 },
                    { "title": "選項二：提問", "idea": "問學生一個和生活經驗有關的問題。",
                      "script": script("Have you ever helped a stranger?"), "minutes": 5 },
                    { "title": "選項三：小遊戲", "idea": "用三個關鍵字讓學生猜課文內容。",
                      "script": script("Guess the story from these three words."), "minutes": 6 },
                ],
                "script": ["Good morning, everyone.", "Let's look at this picture."],
                "stuck_tip": "先給學生兩個選項讓他們選，再請他們說明理由。",
            },
            {
                "phase": "發展活動", "title": "逐段閱讀與提問", "minutes": 80,
                "idea": "學生分組閱讀第 1 段到最後一段，每段找出一個重點並寫在小白板上。",
                "bloom": ["analyze", "understand", "apply", "evaluate"],
                "options": [
                    { "title": "不該出現的選項", "idea": "只有引起動機可以有選項。",
                      "script": [], "minutes": 5 },
                ],
                "script": ["Read paragraph 1 with your partner.", "What is the main idea?"],
                "stuck_tip": "",
            },
            {
                "phase": "統整與評量", "title": "Exit ticket", "minutes": 7,
                "idea": "學生寫下今天學到的一個重點與一個問題。",
                "bloom": ["create", "evaluate"],
                "options": [],
                "script": ["Write one thing you learned today."],
                "stuck_tip": "提供句型框架：Today I learned that ...",
            },
        ],
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
