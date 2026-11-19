//! 試產生: the lead tries an unsaved prompt draft on a sample. The call must stay a preview: it
//! uses the draft text for that call only, writes no `ai_interactions`, worksheet or prompt
//! version row, and is not readable by teachers. Real model calls here go to the fake provider
//! (`AI_PROVIDER=fake`, as everywhere in the tests).

#[macro_use]
mod common;

use axum::http::StatusCode;
use serde_json::{json, Value};

const PASSAGE: &str = "Honeybees communicate by dancing. A returning forager shakes her body \
to show where the flowers are.\n\nOther bees follow the dance and fly to the same place. \
Scientists have studied this behaviour for decades.";

async fn count(app: &common::TestApp, table: &str) -> i64 {
    sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
        .fetch_one(&app.db)
        .await
        .unwrap()
}

async fn latest(app: &common::TestApp, pi: &str, key: &str) -> Value {
    let (_, v) = app
        .get(&format!("/api/admin/prompts/{key}/versions"), Some(pi))
        .await;
    v[0].clone()
}

fn body(system: &str, user: &str, sample: Value) -> Value {
    json!({ "system_text": system, "user_template": user, "sample": sample })
}

#[tokio::test]
async fn tries_each_supported_prompt_without_leaving_a_trace_outside_the_preview_table() {
    let app = test_app!();
    let pi = app.pi_token().await;
    let (_, list) = app.get("/api/admin/prompts", Some(&pi)).await;
    assert_eq!(
        list["try_keys"],
        json!(["prep_ask", "objective_draft", "worksheet_warmup"])
    );
    let versions_before = count(&app, "prompt_versions").await;
    let ai_before = count(&app, "ai_interactions").await;
    let ws_before = count(&app, "worksheets").await;

    for (key, sample) in [
        (
            "prep_ask",
            json!({ "passage": PASSAGE, "question": "這篇文章適合教哪些連接詞？" }),
        ),
        ("objective_draft", json!({ "passage": PASSAGE })),
        ("worksheet_warmup", json!({ "passage": PASSAGE })),
    ] {
        let saved = latest(&app, &pi, key).await;
        // The draft: the saved text plus a marker, which must reach the model.
        let system = format!(
            "{}\n（草稿標記 ZZ-DRAFT）",
            saved["system_text"].as_str().unwrap()
        );
        let user = saved["user_template"].as_str().unwrap();
        let (s, v) = app
            .post(
                &format!("/api/admin/prompts/{key}/try"),
                Some(&pi),
                body(&system, user, sample),
            )
            .await;
        assert_eq!(s, StatusCode::OK, "{key}: {v}");
        assert_eq!(v["status"], "ok", "{key}: {v}");
        assert_eq!(v["preview"], true);
        assert!(v["system"].as_str().unwrap().contains("ZZ-DRAFT"), "{key}");
        assert!(v["user"].as_str().unwrap().contains("Honeybees"), "{key}");
        assert!(v["output"].is_object(), "{key}: {v}");
        let checks = v["checks"].as_array().unwrap();
        assert!(!checks.is_empty(), "{key}");
        assert_eq!(checks[0]["name"], "schema");
        for c in checks {
            assert_eq!(c["passed"], true, "{key}: {c}");
        }
    }

    // Nothing but the preview table grew: no prompt version, interaction or worksheet.
    assert_eq!(count(&app, "prompt_versions").await, versions_before);
    assert_eq!(count(&app, "ai_interactions").await, ai_before);
    assert_eq!(count(&app, "worksheets").await, ws_before);
    assert_eq!(count(&app, "prompt_try_runs").await, 3);
    let not_preview = sqlx::query("INSERT INTO prompt_try_runs (teacher_id, prompt_key, system_text, user_template, sample, status, preview) SELECT id, 'x', 's', 'u', '{}', 'ok', false FROM teachers LIMIT 1")
        .execute(&app.db)
        .await;
    assert!(not_preview.is_err(), "a try row must always be a preview");
}

#[tokio::test]
async fn the_stored_try_keeps_the_sample_only_as_a_summary() {
    let app = test_app!();
    let pi = app.pi_token().await;
    let saved = latest(&app, &pi, "prep_ask").await;
    let (s, v) = app
        .post(
            "/api/admin/prompts/prep_ask/try",
            Some(&pi),
            body(
                saved["system_text"].as_str().unwrap(),
                saved["user_template"].as_str().unwrap(),
                json!({ "passage": PASSAGE, "question": "給我一個暖身想法" }),
            ),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (sample, preview, base, status): (Value, bool, Option<i32>, String) = sqlx::query_as(
        "SELECT sample, preview, base_version_no, status FROM prompt_try_runs WHERE id = $1",
    )
    .bind(v["run_id"].as_i64().unwrap())
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert!(preview);
    assert_eq!(status, "ok");
    assert_eq!(base, saved["version_no"].as_i64().map(|n| n as i32));
    assert!(!sample.to_string().contains("Honeybees"), "{sample}");
    assert_eq!(sample["passage_chars"], PASSAGE.chars().count());
}

#[tokio::test]
async fn refuses_what_cannot_be_tried_and_what_would_not_work() {
    let app = test_app!();
    let pi = app.pi_token().await;
    let teacher = app.teacher("T-TRY-1", "general").await;
    let saved = latest(&app, &pi, "prep_ask").await;
    let system = saved["system_text"].as_str().unwrap();
    let user = saved["user_template"].as_str().unwrap();
    let ok_sample = json!({ "passage": PASSAGE, "question": "問題" });

    let (s, _) = app
        .post(
            "/api/admin/prompts/prep_ask/try",
            Some(&teacher),
            body(system, user, ok_sample.clone()),
        )
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    // a prompt that is a step of the multi-step pipeline cannot be tried
    let (s, v) = app
        .post(
            "/api/admin/prompts/worksheet_tasks/try",
            Some(&pi),
            body("s", "u", ok_sample.clone()),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");

    // a draft without a variable the code cannot do without
    let (s, v) = app
        .post(
            "/api/admin/prompts/worksheet_warmup/try",
            Some(&pi),
            body("沒有變數", "沒有變數", json!({ "passage": PASSAGE })),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    assert!(v["error"]["message"]
        .as_str()
        .unwrap()
        .contains("{{passage_numbered}}"));

    // no passage, no question
    for sample in [
        json!({ "passage": "  ", "question": "問題" }),
        json!({ "passage": PASSAGE, "question": " " }),
    ] {
        let (s, _) = app
            .post(
                "/api/admin/prompts/prep_ask/try",
                Some(&pi),
                body(system, user, sample),
            )
            .await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
    }
    assert_eq!(count(&app, "prompt_try_runs").await, 0);
}

#[tokio::test]
async fn a_model_failure_is_reported_and_runs_are_capped_per_hour() {
    let app = test_app!();
    let pi = app.pi_token().await;
    let saved = latest(&app, &pi, "prep_ask").await;
    let system = saved["system_text"].as_str().unwrap();
    let user = saved["user_template"].as_str().unwrap();
    let (s, v) = app
        .post(
            "/api/admin/prompts/prep_ask/try",
            Some(&pi),
            body(
                system,
                user,
                json!({ "passage": PASSAGE, "question": "FAKE_ERROR" }),
            ),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["status"], "error");
    assert!(v["output"].is_null());
    assert!(v["message"].is_string());

    sqlx::query(
        "INSERT INTO prompt_try_runs (teacher_id, prompt_key, system_text, user_template, sample, status)
         SELECT t.id, 'prep_ask', 's', 'u', '{}', 'ok'
           FROM teachers t, generate_series(1, 40) WHERE t.role = 'pi'",
    )
    .execute(&app.db)
    .await
    .unwrap();
    let (s, v) = app
        .post(
            "/api/admin/prompts/prep_ask/try",
            Some(&pi),
            body(
                system,
                user,
                json!({ "passage": PASSAGE, "question": "問題" }),
            ),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
}
