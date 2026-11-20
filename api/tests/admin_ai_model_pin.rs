//! A background job keeps the text model it started with, even when the PI switches the model
//! while it runs (docs/API.md「文字模型設定」); a job started after the switch uses the new one.

#[macro_use]
mod common;

use axum::http::StatusCode;
use manna_api::ai::{AiSettings, AiSource};
use manna_api::config::AiProviderKind;
use serde_json::json;
use sqlx::PgPool;

const PASSAGE: &str = "Mrs. Lin put an umbrella stand outside her shop on a rainy Monday. A sign said, “Take one if you need it.”\n\nHer neighbors laughed at the idea. They were sure that every umbrella would be gone within a week.\n\nIn the first month, twelve of the thirty umbrellas did disappear. However, something unexpected followed.\n\nToday the stand holds more umbrellas than it did at the start. Mrs. Lin says trust is like an umbrella. FAKE_PACED";

/// `(module, provider, model)` of every `ai_interactions` row, oldest first.
async fn interactions(db: &PgPool) -> Vec<(String, String, String)> {
    sqlx::query_as("SELECT module, provider, model FROM ai_interactions ORDER BY id")
        .fetch_all(db)
        .await
        .unwrap()
}

/// Starts a many-call worksheet run (three levels, the judge, the check; each call takes
/// 400 ms with `FAKE_PACED`), runs `switch` right after it started, and waits until it is over.
async fn run_session(app: &common::TestApp, t: &str, switch: impl FnOnce()) {
    let (s, session) = app
        .post(
            "/api/prep-sessions",
            Some(t),
            json!({ "kind": "tiered", "request": "差異化學習單",
                    "material": { "title": "The Umbrella Stand", "text": PASSAGE },
                    "options": { "bloom": ["remember", "understand", "analyze"] } }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{session}");
    switch();
    let id = session["id"].as_i64().unwrap();
    for _ in 0..2000 {
        let (_, v) = app.get(&format!("/api/prep-sessions/{id}"), Some(t)).await;
        let last = v["turns"].as_array().unwrap().last().unwrap().clone();
        let p = &last["progress"];
        if !p.is_null() && p["status"] != "running" && last["status"] != "running" {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    }
    panic!("the run did not finish");
}

#[tokio::test]
async fn a_background_run_keeps_its_model_and_the_next_run_uses_the_new_one() {
    let app = test_app!();
    let t = app.teacher("T-PIN", "general").await;
    let second = AiSettings {
        kind: AiProviderKind::Fake,
        model: "second-model".into(),
        effort: "medium".into(),
        timeout_secs: 60,
    };
    // The PI switches while the first run is still making its calls.
    let state = app.state.clone();
    let target = second.clone();
    run_session(&app, &t, move || {
        state.ai.set_active(target, AiSource::Admin);
    })
    .await;
    let first = interactions(&app.db).await;
    assert!(first.len() >= 4, "a run with several calls: {first:?}");
    for row in &first {
        assert_eq!(row.2, "fake-model", "every call of the run: {first:?}");
    }
    // A run started after the switch uses the new model throughout.
    let before = first.len();
    run_session(&app, &t, || {}).await;
    let all = interactions(&app.db).await;
    assert!(all.len() > before);
    for row in &all[before..] {
        assert_eq!(row.2, "second-model", "{all:?}");
    }
}
