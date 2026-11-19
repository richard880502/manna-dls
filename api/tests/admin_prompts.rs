//! 後台管理 → 提示詞版本: the registry returned with the list, the history of a key, restoring an
//! older version (a new version, never an edit), who saved it and why, and the check of the
//! `{{variables}}` a new version goes through.

#[macro_use]
mod common;

use std::collections::BTreeSet;

use axum::http::StatusCode;
use manna_api::prompt_registry::check_variables;
use serde_json::{json, Value};

fn err_message(v: &Value) -> &str {
    v["error"]["message"].as_str().unwrap_or_default()
}

async fn post_version(app: &common::TestApp, pi: &str, body: Value) -> (StatusCode, Value) {
    app.post("/api/admin/prompts", Some(pi), body).await
}

#[tokio::test]
async fn every_seeded_key_has_a_registry_entry_and_passes_its_own_check() {
    let app = test_app!();
    let pi = app.pi_token().await;
    let (s, v) = app.get("/api/admin/prompts", Some(&pi)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let registry: BTreeSet<String> = v["registry"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["key"].as_str().unwrap().to_string())
        .collect();
    let prompts = v["prompts"].as_array().unwrap();
    let seeded: BTreeSet<String> = prompts
        .iter()
        .map(|p| p["prompt_key"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(seeded.len(), 30, "{seeded:?}");
    assert_eq!(
        seeded.difference(&registry).collect::<Vec<_>>(),
        Vec::<&String>::new(),
        "seeded keys without a registry entry"
    );
    assert_eq!(
        registry.difference(&seeded).collect::<Vec<_>>(),
        Vec::<&String>::new(),
        "registry entries without a seeded prompt"
    );
    // Every group a prompt names is in the group list.
    let groups: Vec<&str> = v["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g.as_str().unwrap())
        .collect();
    for p in v["registry"].as_array().unwrap() {
        assert!(groups.contains(&p["group"].as_str().unwrap()));
    }
    // The seeded text of every key keeps the variables the registry says are required, and
    // names no variable the code does not fill: restoring any seed must always be allowed.
    for p in prompts {
        let key = p["prompt_key"].as_str().unwrap();
        let check = check_variables(
            key,
            p["system_text"].as_str().unwrap(),
            p["user_template"].as_str().unwrap(),
        );
        let missing: Vec<_> = check.missing.iter().map(|m| m.name).collect();
        assert!(missing.is_empty(), "{key} lacks {missing:?}");
        assert!(
            check.unknown.is_empty(),
            "{key} unknown {:?}",
            check.unknown
        );
    }
}

#[tokio::test]
async fn only_the_pi_reads_and_writes_prompts() {
    let app = test_app!();
    let teacher = app.teacher("T-PROMPT-1", "general").await;
    for path in ["/api/admin/prompts", "/api/admin/prompts/prep_ask/versions"] {
        let (s, _) = app.get(path, Some(&teacher)).await;
        assert_eq!(s, StatusCode::FORBIDDEN, "{path}");
    }
    let (s, _) = post_version(
        &app,
        &teacher,
        json!({ "prompt_key": "prep_ask", "system_text": "x", "user_template": "y" }),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn history_lists_every_version_with_author_note_and_restore_source() {
    let app = test_app!();
    let pi = app.pi_token().await;
    let (_, v) = app
        .get("/api/admin/prompts/worksheet_warmup/versions", Some(&pi))
        .await;
    let original = v[0].clone();
    assert_eq!(original["version_no"], 1);
    assert!(original["created_by_research_id"].is_null());
    let system = original["system_text"].as_str().unwrap().to_string();
    let user = original["user_template"].as_str().unwrap().to_string();

    let (s, v2) = post_version(
        &app,
        &pi,
        json!({
            "prompt_key": "worksheet_warmup",
            "system_text": format!("{system}\n補一句。"),
            "user_template": user,
            "note": "  加一句提醒  ",
        }),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v2}");
    assert_eq!(v2["version_no"], 2);
    assert_eq!(v2["note"], "加一句提醒");
    assert_eq!(v2["by_me"], true);
    assert_eq!(v2["unknown_variables"], json!([]));

    // Restore version 1: a new version 3 with the old text, pointing back at 1.
    let (s, v3) = post_version(
        &app,
        &pi,
        json!({
            "prompt_key": "worksheet_warmup",
            "system_text": system,
            "user_template": user,
            "restored_from": 1,
        }),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v3}");
    assert_eq!(v3["version_no"], 3);
    assert_eq!(v3["restored_from"], 1);

    let (s, list) = app
        .get("/api/admin/prompts/worksheet_warmup/versions", Some(&pi))
        .await;
    assert_eq!(s, StatusCode::OK);
    let numbers: Vec<i64> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["version_no"].as_i64().unwrap())
        .collect();
    assert_eq!(numbers, [3, 2, 1]);
    // Older rows are untouched.
    assert_eq!(list[2]["system_text"], original["system_text"]);
    assert!(list[1]["created_by_research_id"].is_string());

    // The list shows the newest one.
    let (_, all) = app.get("/api/admin/prompts", Some(&pi)).await;
    let latest = all["prompts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["prompt_key"] == "worksheet_warmup")
        .unwrap();
    assert_eq!(latest["version_no"], 3);

    // The event: key, version number and where a restore came from; nothing else.
    let payloads: Vec<Value> = sqlx::query_scalar(
        "SELECT payload FROM usage_logs WHERE event = 'prompt_version_created' ORDER BY id",
    )
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert_eq!(
        payloads,
        [
            json!({ "prompt_key": "worksheet_warmup", "version_no": 2, "restored_from": null }),
            json!({ "prompt_key": "worksheet_warmup", "version_no": 3, "restored_from": 1 }),
        ]
    );

    let (s, _) = app
        .get("/api/admin/prompts/no_such_key/versions", Some(&pi))
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn restoring_a_version_that_does_not_exist_is_refused() {
    let app = test_app!();
    let pi = app.pi_token().await;
    let (s, v) = post_version(
        &app,
        &pi,
        json!({
            "prompt_key": "prep_ask", "system_text": "s {{curriculum_items}}",
            "user_template": "u", "restored_from": 99,
        }),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    assert!(err_message(&v).contains("還原"));
}

#[tokio::test]
async fn a_new_version_must_keep_the_variables_the_code_cannot_do_without() {
    let app = test_app!();
    let pi = app.pi_token().await;
    let (s, v) = post_version(
        &app,
        &pi,
        json!({
            "prompt_key": "worksheet_bloom_check",
            "system_text": "判斷每一題。",
            "user_template": "請判斷。",
        }),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    let msg = err_message(&v);
    assert!(
        msg.contains("{{passage_numbered}}") && msg.contains("{{questions}}"),
        "{msg}"
    );
    // Nothing was written.
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM prompt_versions WHERE prompt_key = 'worksheet_bloom_check'",
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(n, 1);

    // A name the code never fills only warns.
    let (s, ok) = post_version(
        &app,
        &pi,
        json!({
            "prompt_key": "worksheet_bloom_check",
            "system_text": "判斷每一題。{{made_up}}",
            "user_template": "教材：{{ passage_numbered }}\n題目：{{questions}}",
        }),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{ok}");
    assert_eq!(ok["unknown_variables"], json!(["made_up"]));
}

#[tokio::test]
async fn blank_text_and_long_notes_are_refused_and_unknown_keys_still_save() {
    let app = test_app!();
    let pi = app.pi_token().await;
    for body in [
        json!({ "prompt_key": "prep_ask", "system_text": " ", "user_template": "u" }),
        json!({ "prompt_key": "prep_ask", "system_text": "s", "user_template": "" }),
        json!({
            "prompt_key": "prep_ask", "system_text": "s {{curriculum_items}}",
            "user_template": "u", "note": "字".repeat(201),
        }),
    ] {
        let (s, v) = post_version(&app, &pi, body).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    }
    // worksheet_image_slots reads only system_text, so its template may be empty.
    let (s, v) = post_version(
        &app,
        &pi,
        json!({ "prompt_key": "worksheet_image_slots", "system_text": "規則", "user_template": "" }),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    // A key nobody registered is saved as before (no variable check).
    let (s, v) = post_version(
        &app,
        &pi,
        json!({ "prompt_key": "brand_new_key", "system_text": "s {{x}}", "user_template": "u" }),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["version_no"], 1);
}
