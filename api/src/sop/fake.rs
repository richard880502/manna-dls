//! Deterministic outputs of the fake provider for the two SOP stages (tests, CI, local demos).
//!
//! Markers, written anywhere in the material (see `ai::fake` for the generic ones):
//!
//! | marker                   | effect                                                            |
//! |--------------------------|-------------------------------------------------------------------|
//! | `FAKE_SOP_STOP_EARLY`    | analysis: call 1 stops after two paragraphs; the repair call delivers the rest |
//! | `FAKE_SOP_STOP_ALWAYS`   | analysis: neither call delivers a paragraph after the second      |
//! | `FAKE_SOP_SPLICE`        | analysis: call 1 splices the first paragraph's evidence with "..."; the repair call quotes verbatim |
//! | `FAKE_SOP_SPLICE_ALWAYS` | analysis: both calls splice it                                    |
//! | `FAKE_SOP_INVENT`        | objectives: call 1 cites a code that was not offered; call 2 does not |
//! | `FAKE_SOP_INVENT_ALWAYS` | objectives: both calls cite it                                    |
//! | `FAKE_SOP_UNLINKED`      | objectives: nothing is linked                                     |
//! | `FAKE_SOP_ONE_TYPE`      | objectives: only content objectives                               |
//! | `FAKE_BLOCK`             | both stages: output that fails every check                        |
//! | `FAKE_SLOW`              | each call waits three seconds first (handled in `sop::ai`)        |

use serde_json::{json, Value};

pub const INVENTED_CODE: &str = "9-V-99";

/// The first sentence of `text` (through its closing mark), a verbatim piece of it.
fn first_sentence(text: &str) -> &str {
    for (i, c) in text.char_indices() {
        if matches!(c, '.' | '!' | '?') && i >= 12 {
            return text[..i + c.len_utf8()].trim();
        }
    }
    text.trim()
}

fn spliced(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let head = words.iter().take(3).copied().collect::<Vec<_>>().join(" ");
    let tail = words
        .iter()
        .rev()
        .take(3)
        .rev()
        .copied()
        .collect::<Vec<_>>()
        .join(" ");
    format!("{head} ... {tail}")
}

pub fn analysis(ctx: &Value, attempt: i32, has: &dyn Fn(&str) -> bool) -> Value {
    let invalid = has("FAKE_BLOCK");
    let stop_always = has("FAKE_SOP_STOP_ALWAYS");
    let stop_early = !stop_always && has("FAKE_SOP_STOP_EARLY") && attempt == 1;
    let splice_always = has("FAKE_SOP_SPLICE_ALWAYS");
    let splice = splice_always || (has("FAKE_SOP_SPLICE") && attempt == 1);
    let only: Option<Vec<i64>> = ctx["only_paragraphs"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_i64).collect());
    let all = ctx["paragraphs"].as_array().cloned().unwrap_or_default();
    let wanted: Vec<&Value> = all
        .iter()
        .filter(|p| {
            let no = p["no"].as_i64().unwrap_or(0);
            only.as_ref().is_none_or(|o| o.contains(&no))
                && !((stop_always || stop_early) && no > 2)
        })
        .collect();
    let paragraphs: Vec<Value> = wanted
        .iter()
        .map(|p| {
            let no = p["no"].as_i64().unwrap_or(0);
            let text = p["text"].as_str().unwrap_or_default();
            if invalid {
                return json!({
                    "paragraph_no": no, "function_zh": "Opening", "summary_zh": "A summary.",
                    "writing_features": [{
                        "feature_zh": "invented", "explanation_zh": "invented",
                        "evidence": "This sentence does not appear in the material."
                    }]
                });
            }
            let evidence = if splice && no == 1 {
                spliced(text)
            } else {
                first_sentence(text).to_string()
            };
            json!({
                "paragraph_no": no,
                "function_zh": format!("（測試）第 {no} 段在全文中的功能"),
                "summary_zh": format!("（測試）第 {no} 段的一句話摘要"),
                "writing_features": [{
                    "feature_zh": "（測試）主題句放在段首",
                    "evidence": evidence,
                    "explanation_zh": "（測試）這一句先交代這一段要談的事，這是測試用的示範說明。"
                }]
            })
        })
        .collect();
    let language_focus: Vec<Value> = if invalid {
        Vec::new()
    } else {
        wanted
            .iter()
            .take(1)
            .filter_map(|p| {
                let text = p["text"].as_str().unwrap_or_default();
                let word = text
                    .split(|c: char| !c.is_ascii_alphabetic())
                    .find(|w| w.len() >= 4)?;
                Some(json!({
                    "kind": "字彙", "item": word, "paragraph_no": p["no"],
                    "evidence": first_sentence(text),
                    "note_zh": "（測試）值得先教的字彙"
                }))
            })
            .collect()
    };
    json!({
        "text_type": "記敘文",
        "main_idea_zh": if invalid { "Main idea" } else { "（測試）全文主旨的示範說明" },
        "structure_zh": if invalid { "Structure" } else { "（測試）依段落順序鋪陳" },
        "paragraphs": paragraphs,
        "language_focus": language_focus
    })
}

pub fn objectives(ctx: &Value, attempt: i32, has: &dyn Fn(&str) -> bool) -> Value {
    if has("FAKE_BLOCK") {
        return json!({ "objectives": [{
            "objective_id": "O1", "siop_type": "內容目標",
            "objective_zh": "Students understand the text.",
            "student_facing_en": "Understand the text.", "reason_zh": "none",
            "paragraph_refs": [1], "link_status": "linked",
            "curriculum_links": [{ "item_code": INVENTED_CODE, "why_zh": "invented" }]
        }]});
    }
    let unlinked = has("FAKE_SOP_UNLINKED");
    let invent = has("FAKE_SOP_INVENT_ALWAYS") || (has("FAKE_SOP_INVENT") && attempt == 1);
    let one_type = has("FAKE_SOP_ONE_TYPE");
    let codes: Vec<&str> = ctx["items"]
        .as_array()
        .map(|a| a.iter().filter_map(|i| i["item_code"].as_str()).collect())
        .unwrap_or_default();
    let n = ctx["paragraphs"].as_i64().unwrap_or(1).max(1);
    let link = |code: Option<&&str>| -> Vec<Value> {
        match code {
            Some(code) if !unlinked => {
                vec![json!({ "item_code": code, "why_zh": "（測試）這個目標對應這一條學習表現。" })]
            }
            _ => Vec::new(),
        }
    };
    let mut first_links = link(codes.first());
    if invent {
        first_links.push(json!({ "item_code": INVENTED_CODE, "why_zh": "（測試）憑記憶寫的代碼" }));
    }
    let objective =
        |id: &str, kind: &str, zh: &str, en: &str, refs: Vec<i64>, links: Vec<Value>| {
            json!({
                "objective_id": id, "siop_type": kind, "objective_zh": zh, "student_facing_en": en,
                "reason_zh": "（測試）對應教材的轉折與這個班級的需求，這是測試用的示範理由。",
                "paragraph_refs": refs,
                "link_status": if links.is_empty() { "unlinked" } else { "linked" },
                "curriculum_links": links
            })
        };
    let language = if one_type {
        "內容目標"
    } else {
        "語言目標"
    };
    json!({ "objectives": [
        objective("O1", "內容目標",
                  "學生能說出文章中事件發生的原因，並用課文裡的句子支持自己的說法（測試建議）",
                  "I can explain why it happened and point to a sentence in the text.",
                  vec![1], first_links),
        objective("O2", language,
                  "學生能用 because 和 however 口頭說明預期與結果的差別（測試建議）",
                  "I can use because and however to talk about what changed.",
                  vec![n.min(2)], link(codes.get(1))),
        objective("O3", language,
                  "學生能從上下文推測兩個關鍵字的意思，並說出線索在哪裡（測試建議）",
                  "I can guess what a new word means from the sentences around it.",
                  vec![n], Vec::new()),
    ]})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evidence_is_verbatim_unless_spliced() {
        let text = "Mrs. Lin put an umbrella stand outside her shop. A sign said so.";
        assert_eq!(
            first_sentence(text),
            "Mrs. Lin put an umbrella stand outside her shop."
        );
        assert!(text.contains(first_sentence(text)));
        assert_eq!(spliced(text), "Mrs. Lin put ... sign said so.");
        assert_eq!(first_sentence("Short"), "Short");
    }

    #[test]
    fn stop_early_only_affects_the_first_call() {
        let ctx = json!({ "paragraphs": [
            {"no": 1, "text": "First paragraph here."}, {"no": 2, "text": "Second paragraph here."},
            {"no": 3, "text": "Third paragraph here."}] });
        let early = |m: &str| m == "FAKE_SOP_STOP_EARLY";
        assert_eq!(
            analysis(&ctx, 1, &early)["paragraphs"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        let mut repair = ctx.clone();
        repair["only_paragraphs"] = json!([3]);
        let out = analysis(&repair, 2, &early);
        assert_eq!(out["paragraphs"][0]["paragraph_no"], 3);
        let always = |m: &str| m == "FAKE_SOP_STOP_ALWAYS";
        assert!(analysis(&repair, 2, &always)["paragraphs"]
            .as_array()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn objectives_link_offered_codes_and_invent_on_request() {
        let ctx = json!({ "paragraphs": 4, "items": [
            {"item_code": "CUR-1", "title": "a"}, {"item_code": "CUR-2", "title": "b"}] });
        let none = |_: &str| false;
        let out = objectives(&ctx, 1, &none);
        assert_eq!(
            out["objectives"][0]["curriculum_links"][0]["item_code"],
            "CUR-1"
        );
        assert_eq!(out["objectives"][1]["siop_type"], "語言目標");
        assert_eq!(out["objectives"][2]["link_status"], "unlinked");
        let invent = |m: &str| m == "FAKE_SOP_INVENT";
        assert_eq!(
            objectives(&ctx, 1, &invent)["objectives"][0]["curriculum_links"][1]["item_code"],
            INVENTED_CODE
        );
        assert_eq!(
            objectives(&ctx, 2, &invent)["objectives"][0]["curriculum_links"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
}
