//! Deterministic outputs of the fake provider for drafting objectives (tests, CI, local demos).
//!
//! Markers, written anywhere in the material (see `ai::fake` for the generic ones):
//!
//! | marker                 | effect                                                                |
//! |------------------------|-----------------------------------------------------------------------|
//! | `FAKE_OBJ_TWO`         | two 內容目標 and two 語言目標, the 學習策略 second (the server puts it first) |
//! | `FAKE_OBJ_VAGUE`       | draft: on the first attempt the 內容目標 says "understand" (dropped by rule, with its 語言目標: the call is retried) |
//! | `FAKE_OBJ_R3`          | judge: in round 1 every 語言目標 is `not_needed` (the server drafts round 2, which passes) |
//! | `FAKE_OBJ_R3_ALWAYS`   | judge: every 語言目標 is `no_relation`, in both rounds (only the 內容目標 are shown) |
//! | `FAKE_BLOCK`           | draft: Chinese in every students' line, both attempts (nothing usable) |
//!
//! A draft with `avoid` (「換一組」) writes another set; round 2 writes other wording, so that a
//! test can tell the rounds apart.

use serde_json::{json, Value};

fn title(ctx: &Value) -> String {
    let t = ctx["material_title"].as_str().unwrap_or_default().trim();
    if t.is_empty() {
        "the text".to_string()
    } else {
        format!("\"{t}\"")
    }
}

pub fn draft(ctx: &Value, attempt: i32, has: &dyn Fn(&str) -> bool) -> Value {
    let round = ctx["round"].as_i64().unwrap_or(1);
    let other = ctx["avoid"].as_array().is_some_and(|a| !a.is_empty());
    let t = title(ctx);
    let (c_en, c_zh) = if other {
        (
            format!("I can summarize {t} in three sentences."),
            "用三句話摘要這篇文章".to_string(),
        )
    } else if round > 1 {
        (
            format!("I can describe the main problem in {t} and how it is solved."),
            "描述文章提出的主要問題與解決方式".to_string(),
        )
    } else {
        (
            format!("I can explain the main point of {t} with two examples from the text."),
            "用文中兩個例子說明文章的主要論點".to_string(),
        )
    };
    let vague = has("FAKE_OBJ_VAGUE") && attempt == 1;
    let c_en = if vague {
        format!("I can understand {t}.")
    } else {
        c_en
    };
    let mut content = vec![json!({
        "zh": c_zh, "en": c_en, "check_zh": "Exit ticket：請學生用一句話寫出主要論點和一個例子。",
    })];
    let strategy = json!({
        "category": "strategy",
        "supports": "C1",
        "zh": if round > 1 { "先看標題預測內容，讀完再確認預測" } else { "讀到難句時重讀，並畫出 for example、such as 等提示詞" },
        "en": if round > 1 {
            "I can predict from the title and check my prediction after reading."
        } else {
            "I can reread hard sentences and underline signal words like for example and such as."
        },
        "relation_zh": "學生要用重讀和畫出提示詞的策略找出文中的例子，才能說明文章的主要論點。",
        "check_zh": "檢查學生在文中畫出的提示詞和例子。",
    });
    let mut language = vec![strategy];
    if has("FAKE_OBJ_TWO") {
        content.push(json!({
            "zh": "比較兩個段落提出的理由",
            "en": format!("I can compare the reasons in two paragraphs of {t}."),
            "check_zh": "請學生完成一個兩欄的比較表。",
        }));
        language.insert(
            0,
            json!({
                "category": "function",
                "supports": "C2",
                "zh": "用 both … but … 寫出比較句",
                "en": "I can use both and but to write a sentence that compares two reasons.",
                "relation_zh": "學生要用 both … but … 的比較句型，才能比較兩個段落提出的理由。",
                "check_zh": "請學生寫一個比較句。",
            }),
        );
    }
    if has("FAKE_BLOCK") {
        for c in content.iter_mut().chain(language.iter_mut()) {
            c["en"] = json!("我能說明文章的主旨。");
        }
    }
    json!({ "content_objectives": content, "language_objectives": language })
}

pub fn check(ctx: &Value, _attempt: i32, has: &dyn Fn(&str) -> bool) -> Value {
    let round = ctx["round"].as_i64().unwrap_or(1);
    let items: Vec<Value> = ctx["items"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|item| {
            let language = item["kind"] == "language";
            let problems: Vec<&str> = if language && has("FAKE_OBJ_R3_ALWAYS") {
                vec!["no_relation"]
            } else if language && has("FAKE_OBJ_R3") && round == 1 {
                vec!["not_needed"]
            } else {
                Vec::new()
            };
            json!({
                "id": item["id"],
                "reason": if problems.is_empty() { "對準本課核心，學生做得到也看得到。" } else { "這個語言目標對本課幫助不大。" },
                "problems": problems,
            })
        })
        .collect();
    json!({ "items": items })
}
