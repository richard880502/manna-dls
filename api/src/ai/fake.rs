//! Deterministic offline provider for tests and CI. No network.
//!
//! Valid outputs pass the automatic checks (plan suggestions cite the first retrieved items).
//! Markers anywhere in the request text simulate failures:
//!
//! | marker            | effect                                                          |
//! |-------------------|-----------------------------------------------------------------|
//! | `FAKE_TIMEOUT`    | provider timeout                                                |
//! | `FAKE_ERROR`      | HTTP 500                                                        |
//! | `FAKE_REFUSAL`    | model refusal                                                   |
//! | `FAKE_BLOCK`      | output that fails the checks (both attempts → `blocked`)        |
//! | `FAKE_BLOCK_ONCE` | first attempt fails the checks, the regeneration passes         |
//! | `FAKE_KB_EMPTY`   | plan suggestions: the model answers `kb_empty` (items irrelevant) |

use serde_json::{json, Value};

use super::{LlmRequest, LlmResponse, ProviderError};

fn has(req: &LlmRequest, marker: &str) -> bool {
    req.user.contains(marker) || req.fake_context.to_string().contains(marker)
}

fn excerpt(s: &str, n: usize) -> String {
    s.chars()
        .filter(|c| !matches!(c, '「' | '」' | '『' | '』' | '?' | '？'))
        .take(n)
        .collect::<String>()
        .trim()
        .to_string()
}

pub fn complete(req: &LlmRequest, model: &str) -> Result<LlmResponse, ProviderError> {
    if has(req, "FAKE_TIMEOUT") {
        return Err(ProviderError::Timeout);
    }
    if has(req, "FAKE_ERROR") {
        return Err(ProviderError::Http {
            status: 500,
            body: "fake provider error".into(),
        });
    }
    if has(req, "FAKE_REFUSAL") {
        return Err(ProviderError::Refusal("fake refusal".into()));
    }
    // FAKE_BLOCK_ONCE contains FAKE_BLOCK, so test it first.
    let invalid = if has(req, "FAKE_BLOCK_ONCE") {
        req.attempt == 1
    } else {
        has(req, "FAKE_BLOCK")
    };
    let ctx = &req.fake_context;
    let json = match ctx["task"].as_str() {
        Some("core_assist") => core_assist(ctx, invalid),
        Some("plan_suggest") => plan_suggest(ctx, invalid, has(req, "FAKE_KB_EMPTY")),
        other => {
            return Err(ProviderError::InvalidOutput(format!(
                "fake provider: unknown task {other:?}"
            )))
        }
    };
    Ok(LlmResponse {
        json,
        model_version: format!("{model}-fake-1"),
    })
}

fn core_assist(ctx: &Value, invalid: bool) -> Value {
    let level = ctx["level"].as_u64().unwrap_or(1);
    if invalid {
        // A pasteable answer: must be rejected by the checks.
        return json!({ "level": level, "text": "答案：學生要能正確使用這個句型。" });
    }
    let help = ctx["question_help"].as_str().unwrap_or_default();
    let big_idea = excerpt(ctx["big_idea"].as_str().unwrap_or_default(), 20);
    let answer = excerpt(ctx["teacher_answer"].as_str().unwrap_or_default(), 20);
    let text = match level {
        1 => format!(
            "這一題想了解的是：{help}。換句話說，請從你自己的教學經驗出發，想想這個學習重點對你的學生代表什麼。"
        ),
        2 => "可以從幾個面向想想：\n・學生在這個重點上已經有哪些經驗\n・這個重點會出現在哪些真實溝通情境\n・它和前後單元的關聯".to_string(),
        _ => {
            let anchor = if !answer.is_empty() {
                answer
            } else if !big_idea.is_empty() {
                big_idea
            } else {
                "這個學習重點".to_string()
            };
            format!("你提到「{anchor}」，在這個單元裡，你會用哪一個具體的課堂情境讓學生用到它？")
        }
    };
    json!({ "level": level, "text": text })
}

fn plan_suggest(ctx: &Value, invalid: bool, kb_empty: bool) -> Value {
    if kb_empty {
        return json!({ "kb_empty": true, "suggestions": [] });
    }
    if invalid {
        return json!({
            "kb_empty": false,
            "suggestions": [{
                "title": "未經檢索的建議",
                "body": "這是一個引用不存在條目的建議。",
                "activity_type": null,
                "icap_level": null,
                "citations": ["NOT-A-REAL-CODE"]
            }]
        });
    }
    let items = ctx["items"].as_array().cloned().unwrap_or_default();
    let suggestions: Vec<Value> = items
        .iter()
        .take(2)
        .map(|it| {
            let code = it["item_code"].as_str().unwrap_or_default();
            let title = it["title"].as_str().unwrap_or_default();
            json!({
                "title": format!("（測試建議）{title}"),
                "body": format!("參考條目 {code}「{title}」，安排一段兩兩練習：學生先觀察例句，再各自造句並互相檢查。這是測試用的示範建議。"),
                "activity_type": "配對討論",
                "icap_level": "interactive",
                "citations": [code]
            })
        })
        .collect();
    json!({ "kb_empty": suggestions.is_empty(), "suggestions": suggestions })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::schemas::{CoreAssistOutput, PlanSuggestOutput};
    use crate::checks;

    fn req(user: &str, ctx: Value, attempt: i32) -> LlmRequest {
        LlmRequest {
            system: String::new(),
            user: user.into(),
            schema_name: "x",
            schema: json!({}),
            fake_context: ctx,
            attempt,
        }
    }

    #[test]
    fn core_outputs_pass_checks_at_every_level() {
        for level in 1..=3u8 {
            let ctx = json!({"task": "core_assist", "level": level, "question_help": "說明",
                             "big_idea": "倒裝句", "teacher_answer": "學生會用 Not only？"});
            let r = complete(&req("", ctx, 1), "m").unwrap();
            let out: CoreAssistOutput = serde_json::from_value(r.json).unwrap();
            let results = checks::check_core_assist(&out, level);
            assert!(checks::all_passed(&results), "level {level}: {results:?}");
        }
    }

    #[test]
    fn plan_outputs_cite_first_items() {
        let ctx = json!({"task": "plan_suggest", "items": [
            {"item_code": "A-1", "title": "倒裝句"}, {"item_code": "B-2", "title": "子句"},
            {"item_code": "C-3", "title": "其他"}]});
        let r = complete(&req("", ctx, 1), "m").unwrap();
        let out: PlanSuggestOutput = serde_json::from_value(r.json).unwrap();
        assert_eq!(checks::cited_codes(&out), vec!["A-1", "B-2"]);
    }

    #[test]
    fn markers() {
        let ctx = json!({"task": "plan_suggest", "items": [{"item_code": "A-1", "title": "t"}]});
        assert_eq!(
            complete(&req("note FAKE_TIMEOUT", ctx.clone(), 1), "m"),
            Err(ProviderError::Timeout)
        );
        let blocked = complete(&req("FAKE_BLOCK", ctx.clone(), 2), "m").unwrap();
        assert_eq!(
            blocked.json["suggestions"][0]["citations"][0],
            "NOT-A-REAL-CODE"
        );
        let once1 = complete(&req("FAKE_BLOCK_ONCE", ctx.clone(), 1), "m").unwrap();
        let once2 = complete(&req("FAKE_BLOCK_ONCE", ctx.clone(), 2), "m").unwrap();
        assert_eq!(
            once1.json["suggestions"][0]["citations"][0],
            "NOT-A-REAL-CODE"
        );
        assert_eq!(once2.json["suggestions"][0]["citations"][0], "A-1");
        let empty = complete(&req("FAKE_KB_EMPTY", ctx, 1), "m").unwrap();
        assert_eq!(empty.json, json!({"kb_empty": true, "suggestions": []}));
    }
}
