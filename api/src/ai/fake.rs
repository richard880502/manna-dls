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
//! | `FAKE_RUNAWAY`    | first attempt runs to `max_tokens` in whitespace (with the partial output); the regeneration is normal |
//! | `FAKE_BLOCK`      | output that fails the checks (both attempts → `blocked`)        |
//! | `FAKE_BLOCK_ONCE` | first attempt fails the checks, the regeneration passes         |
//! | `FAKE_KB_EMPTY`   | plan suggestions: the model answers `kb_empty` (items irrelevant) |
//! | `FAKE_NO_DIFF`    | plan suggestions: differentiation fields left null (fails the check when the teacher's P3 includes differentiation) |
//!
//! The lesson SOP stages (`sop_passage_analysis`, `sop_objectives`, `sop_paragraph_goals`,
//! `sop_objective_english`; steps 5–7 use the worksheet's fakes) add their own markers:
//! see `crate::sop::fake`; the worksheet steps (`worksheet_*`) see `crate::worksheet::fake`;
//! the 備課對話 calls (`prep_*`) see `crate::prep::fake`; drafting objectives
//! (`objective_draft`, `objective_check`) see `crate::objectives::fake`.
//!
//! Task-specific behaviour read from `fake_context`:
//! * core assist level 2 with `goal_emphasis: true` names the goal-related dimensions
//!   (選文理由, 語言以外的內容目標, 課綱及考點);
//! * plan suggestions with `differentiation: true` fill `duration_min`, `support_note` and
//!   `extension_note`;
//! * material focuses quote up to three sentences of `material_text` verbatim (fewer when the
//!   material is short; none for an empty material); `FAKE_BLOCK` yields an English big idea
//!   with an excerpt that is not in the material.

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
    // The SIOP self-check of a turn times out the first time it runs (see `prep::fake`).
    if req.fake_context["task"] == "siop_selfcheck"
        && req.fake_context["run_no"] == 1
        && has(req, "FAKE_SIOP_FAIL_FIRST")
    {
        return Err(ProviderError::Timeout);
    }
    if has(req, "FAKE_RUNAWAY") && req.attempt == 1 {
        // What gemma does when constrained decoding leaves it nothing it wants to write: an
        // unfinished object, then whitespace until max_tokens.
        return Err(ProviderError::MaxTokens {
            partial: format!(
                "{{\n  \"{}\": [\n    {{\n      \"title_en\": \"Before you read\"\n    {}",
                req.schema_name,
                "    \n".repeat(1500)
            ),
        });
    }
    // FAKE_BLOCK_ONCE contains FAKE_BLOCK, so test it first.
    let invalid = if has(req, "FAKE_BLOCK_ONCE") {
        req.attempt == 1
    } else {
        has(req, "FAKE_BLOCK")
    };
    let ctx = &req.fake_context;
    let json = match ctx["task"].as_str() {
        Some("teacher_memory_extract") => crate::memory::fake_extract(ctx),
        Some("core_assist") => core_assist(ctx, invalid),
        Some("plan_suggest") => plan_suggest(
            ctx,
            invalid,
            has(req, "FAKE_KB_EMPTY"),
            has(req, "FAKE_NO_DIFF"),
        ),
        Some("material_focuses") => material_focuses(ctx, invalid),
        // The lesson SOP stages have their own markers (see `sop::fake`).
        Some("sop_passage_analysis") => {
            crate::sop::fake::analysis(ctx, req.attempt, &|m| has(req, m))
        }
        Some("sop_objectives") => crate::sop::fake::objectives(ctx, req.attempt, &|m| has(req, m)),
        Some("sop_paragraph_goals") => crate::sop::fake::goals(ctx, req.attempt, &|m| has(req, m)),
        Some("sop_objective_english") => {
            crate::sop::fake::english(ctx, req.attempt, &|m| has(req, m))
        }
        // The worksheet steps (see `worksheet::fake`).
        Some("worksheet_tasks") => {
            crate::worksheet::fake::tasks(ctx, req.attempt, &|m| has(req, m))
        }
        Some("worksheet_warmup") => {
            crate::worksheet::fake::warmup(ctx, req.attempt, &|m| has(req, m))
        }
        Some("worksheet_scaffolds") => {
            crate::worksheet::fake::scaffolds(ctx, req.attempt, &|m| has(req, m))
        }
        Some("worksheet_exit_ticket") => {
            crate::worksheet::fake::exit_ticket(ctx, req.attempt, &|m| has(req, m))
        }
        // Worksheet templates (see `worksheet::fake`).
        Some("worksheet_template_outline") => {
            crate::worksheet::fake::template_outline(ctx, req.attempt, &|m| has(req, m))
        }
        Some("worksheet_template_details") => {
            crate::worksheet::fake::template_details(ctx, req.attempt, &|m| has(req, m))
        }
        Some("worksheet_template_section") => {
            crate::worksheet::fake::template_section(ctx, req.attempt, &|m| has(req, m))
        }
        // 閱讀理解題／分層練習 (see `worksheet::fake`).
        Some("worksheet_bloom_level") => {
            crate::worksheet::fake::bloom_level(ctx, req.attempt, &|m| has(req, m))
        }
        // 差異化學習單 (see `worksheet::fake::tiered_level`).
        Some("worksheet_tiered_level") => {
            crate::worksheet::fake::tiered_level(ctx, req.attempt, &|m| has(req, m))
        }
        Some("worksheet_bloom_check") => {
            crate::worksheet::fake::bloom_check(ctx, req.attempt, &|m| has(req, m))
        }
        // 詞彙級別檢查：字義與換字 (see `worksheet::vocab_actions::fake`).
        Some("worksheet_vocab_gloss") => {
            crate::worksheet::vocab_actions::fake::gloss(ctx, req.attempt, &|m| has(req, m))
        }
        Some("worksheet_vocab_replace") => {
            crate::worksheet::vocab_actions::fake::replace(ctx, req.attempt, &|m| has(req, m))
        }
        // 逐區修改 (see `worksheet::fake`).
        Some("worksheet_block_revise") => {
            crate::worksheet::fake::block_revise(ctx, req.attempt, &|m| has(req, m))
        }
        // 備課對話 (see `prep::fake`).
        Some("prep_activity") => crate::prep::fake::activity(ctx, req.attempt, &|m| has(req, m)),
        Some("prep_ask") => crate::prep::fake::ask(ctx, req.attempt, &|m| has(req, m)),
        Some("prep_revise") => crate::prep::fake::revise(ctx, req.attempt, &|m| has(req, m)),
        // 內容目標與語言目標的草擬與審查 (see `objectives::fake`).
        Some("objective_draft") => {
            crate::objectives::fake::draft(ctx, req.attempt, &|m| has(req, m))
        }
        Some("objective_check") => {
            crate::objectives::fake::check(ctx, req.attempt, &|m| has(req, m))
        }
        // SIOP 自我檢核 (see `prep::fake`).
        Some("siop_selfcheck") => crate::prep::fake::selfcheck(ctx, req.attempt, &|m| has(req, m)),
        other => {
            return Err(ProviderError::InvalidOutput(format!(
                "fake provider: unknown task {other:?}"
            )))
        }
    };
    // Pictures that came with the request are recorded in the reply of the revisions (the only
    // calls that take any), so a test can tell that they arrived: 「（收到 2 張圖片）」.
    let mut json = json;
    if !req.images.is_empty() {
        if let Some(reply) = json["reply"].as_str() {
            json["reply"] = json!(format!("{reply}（收到 {} 張圖片）", req.images.len()));
        }
    }
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
        2 if ctx["goal_emphasis"] == true => "可以從幾個面向想想：\n・選這篇課文或教材的理由是什麼\n・除了語言之外，這個重點有哪些內容目標\n・它和課綱學習表現及考點的連結在哪裡".to_string(),
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

fn plan_suggest(ctx: &Value, invalid: bool, kb_empty: bool, no_diff: bool) -> Value {
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
                "citations": ["NOT-A-REAL-CODE"],
                "duration_min": null,
                "support_note": null,
                "extension_note": null
            }]
        });
    }
    let differentiation = ctx["differentiation"] == true && !no_diff;
    let items = ctx["items"].as_array().cloned().unwrap_or_default();
    let suggestions: Vec<Value> = items
        .iter()
        .take(2)
        .enumerate()
        .map(|(i, it)| {
            let code = it["item_code"].as_str().unwrap_or_default();
            let title = it["title"].as_str().unwrap_or_default();
            let (duration, support, extension) = if differentiation {
                (
                    json!(10 + 5 * i),
                    json!("提供句型框架與兩個完成的例句，讓程度較弱的學生先照著改寫。"),
                    json!("請程度較好的學生改寫成更正式的語氣，並說明差別。"),
                )
            } else {
                (Value::Null, Value::Null, Value::Null)
            };
            json!({
                "title": format!("（測試建議）{title}"),
                "body": format!("參考條目 {code}「{title}」，安排一段兩兩練習：學生先觀察例句，再各自造句並互相檢查。這是測試用的示範建議。"),
                "activity_type": "配對討論",
                "icap_level": "interactive",
                "citations": [code],
                "duration_min": duration,
                "support_note": support,
                "extension_note": extension
            })
        })
        .collect();
    json!({ "kb_empty": suggestions.is_empty(), "suggestions": suggestions })
}

/// Sentences of `text` (split after . ! ? 。 ！ ？ and at line breaks), trimmed; each one is a
/// verbatim substring of `text`.
fn sentences(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, c) in text.char_indices() {
        if matches!(c, '.' | '!' | '?' | '。' | '！' | '？' | '\n') {
            let end = i + c.len_utf8();
            out.push(text[start..end].trim());
            start = end;
        }
    }
    out.push(text[start..].trim());
    out.retain(|s| s.chars().count() >= 6);
    out
}

fn material_focuses(ctx: &Value, invalid: bool) -> Value {
    if invalid {
        return json!({ "candidates": [{
            "big_idea": "Inversion",
            "rationale": "This passage is invented.",
            "excerpt": "This sentence does not appear in the material."
        }]});
    }
    let text = ctx["material_text"].as_str().unwrap_or_default();
    let candidates: Vec<Value> = sentences(text)
        .into_iter()
        .take(3)
        .enumerate()
        .map(|(i, s)| {
            json!({
                "big_idea": format!("（測試候選 {}）{}", i + 1, excerpt(s, 16)),
                "rationale": "這段教材可以讓學生觀察語言形式與內容，這是測試用的示範說明。",
                "excerpt": s
            })
        })
        .collect();
    json!({ "candidates": candidates })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::schemas::{CoreAssistOutput, MaterialFocusesOutput, PlanSuggestOutput};
    use crate::checks;

    fn req(user: &str, ctx: Value, attempt: i32) -> LlmRequest {
        LlmRequest {
            system: String::new(),
            user: user.into(),
            schema_name: "x",
            schema: json!({}),
            fake_context: ctx,
            attempt,
            max_tokens: None,
            temperature: None,
            images: Vec::new(),
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

    #[test]
    fn core_level2_goal_emphasis() {
        let ctx = json!({"task": "core_assist", "level": 2, "goal_emphasis": true});
        let r = complete(&req("", ctx, 1), "m").unwrap();
        let out: CoreAssistOutput = serde_json::from_value(r.json).unwrap();
        assert!(out.text.contains("選這篇課文或教材的理由"));
        assert!(checks::all_passed(&checks::check_core_assist(&out, 2)));
    }

    #[test]
    fn plan_differentiation_fields() {
        let ctx = json!({"task": "plan_suggest", "differentiation": true,
                         "items": [{"item_code": "A-1", "title": "t"}]});
        let r = complete(&req("", ctx.clone(), 1), "m").unwrap();
        let out: PlanSuggestOutput = serde_json::from_value(r.json).unwrap();
        assert!(checks::all_passed(&checks::check_plan_differentiation(
            &out, true
        )));
        let r = complete(&req("FAKE_NO_DIFF", ctx, 1), "m").unwrap();
        let out: PlanSuggestOutput = serde_json::from_value(r.json).unwrap();
        assert_eq!(out.suggestions[0].duration_min, None);
        assert!(!checks::all_passed(&checks::check_plan_differentiation(
            &out, true
        )));
    }

    #[test]
    fn material_candidates_quote_the_material() {
        let text = "Not only does Taiwan have mountains, but it also has beaches.\n夜市是台灣很有名的文化。\n短\nHe never gives up!";
        let ctx = json!({"task": "material_focuses", "material_text": text});
        let r = complete(&req("", ctx, 1), "m").unwrap();
        let out: MaterialFocusesOutput = serde_json::from_value(r.json).unwrap();
        assert_eq!(out.candidates.len(), 3);
        for c in &out.candidates {
            assert!(text.contains(&c.excerpt), "{}", c.excerpt);
        }
        assert!(checks::all_passed(&checks::check_material_focuses(
            &out, text
        )));
        let empty = complete(
            &req(
                "",
                json!({"task": "material_focuses", "material_text": ""}),
                1,
            ),
            "m",
        )
        .unwrap();
        assert_eq!(empty.json, json!({"candidates": []}));
        let blocked = complete(
            &req(
                "FAKE_BLOCK",
                json!({"task": "material_focuses", "material_text": text}),
                1,
            ),
            "m",
        )
        .unwrap();
        let out: MaterialFocusesOutput = serde_json::from_value(blocked.json).unwrap();
        assert!(!checks::all_passed(&checks::check_material_focuses(
            &out, text
        )));
    }
}
