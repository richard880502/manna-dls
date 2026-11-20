//! 逐步設計 (docs/API.md「逐步設計」): the lessons list behind 我的作品, a lesson started from the
//! home page, and what the usage log records for it.

#[macro_use]
mod common;

use axum::http::StatusCode;
use serde_json::{json, Value};

const TEXT: &str =
    "Mrs. Lin put an umbrella stand outside her shop.\n\nHer neighbors laughed at the idea.";

async fn made(app: &common::TestApp, t: &str, unit: i64, body: Value) -> i64 {
    let (s, v) = app
        .post(&format!("/api/units/{unit}/lessons"), Some(t), body)
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v["id"].as_i64().unwrap()
}

async fn material(app: &common::TestApp, t: &str, lesson: i64) {
    let (s, v) = app
        .put(
            &format!("/api/lessons/{lesson}/sop/material"),
            Some(t),
            json!({ "title": "The Umbrella Stand", "text": TEXT }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
}

async fn save(app: &common::TestApp, t: &str, lesson: i64, content: Value) {
    let (s, v) = app
        .put(
            &format!("/api/lessons/{lesson}/sop"),
            Some(t),
            json!({ "content": content }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
}

async fn listed(app: &common::TestApp, t: &str) -> Vec<Value> {
    let (s, v) = app.get("/api/stepwise-lessons", Some(t)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v.as_array().unwrap().clone()
}

fn ids(rows: &[Value]) -> Vec<i64> {
    rows.iter().map(|r| r["id"].as_i64().unwrap()).collect()
}

#[tokio::test]
async fn the_list_has_the_teachers_stepwise_lessons_newest_first_with_progress() {
    let app = test_app!();
    let t = app.teacher("T-STEP-1", "general").await;
    let other = app.teacher("T-STEP-2", "general").await;
    let unit = app.create_unit(&t, "L3 The Umbrella Stand").await;
    let foreign_unit = app.create_unit(&other, "別人的單元").await;

    // Started from the home card: listed at once, nothing done yet.
    let started = made(
        &app,
        &t,
        unit,
        json!({ "title": "第 1 課", "entry": "home" }),
    )
    .await;
    // An old plan-only lesson nobody opened in the workspace: not listed.
    let plain = made(&app, &t, unit, json!({ "title": "舊課例" })).await;
    // A lesson made the old way, then given a material in the workspace: listed.
    let opened = made(&app, &t, unit, json!({ "title": "第 3 課" })).await;
    material(&app, &t, opened).await;
    // Someone else's lessons never show.
    let foreign = made(
        &app,
        &other,
        foreign_unit,
        json!({ "title": "別人的", "entry": "home" }),
    )
    .await;

    let rows = listed(&app, &t).await;
    assert_eq!(ids(&rows), vec![opened, started], "{rows:?}");
    assert!(!ids(&rows).contains(&plain) && !ids(&rows).contains(&foreign));
    let row = &rows[1];
    assert_eq!(row["title"], "第 1 課");
    assert_eq!(row["unit_id"], unit);
    assert_eq!(row["unit_title"], "L3 The Umbrella Stand");
    assert_eq!(
        (row["steps_done"].as_i64(), row["next_step"].as_i64()),
        (Some(0), Some(1))
    );
    assert_eq!(
        (
            rows[0]["steps_done"].as_i64(),
            rows[0]["next_step"].as_i64()
        ),
        (Some(1), Some(2))
    );
    assert_eq!(ids(&listed(&app, &other).await), vec![foreign]);

    // Saving in the workspace moves a lesson to the top, and the next step is the first empty one
    // (step 3 is filled but the material is not, so the next step is still 1).
    save(
        &app,
        &t,
        started,
        json!({ "objectives": [{ "id": "o1", "type": "content", "text": "學生能說出原因" }] }),
    )
    .await;
    let rows = listed(&app, &t).await;
    assert_eq!(ids(&rows), vec![started, opened]);
    assert_eq!(rows[0]["steps_done"], 1);
    assert_eq!(rows[0]["next_step"], 1);
}

#[tokio::test]
async fn all_seven_steps_with_content_is_done() {
    let app = test_app!();
    let t = app.teacher("T-STEP-3", "general").await;
    let unit = app.create_unit(&t, "單元").await;
    let id = made(
        &app,
        &t,
        unit,
        json!({ "title": "全部做完", "entry": "home" }),
    )
    .await;
    material(&app, &t, id).await;
    save(
        &app,
        &t,
        id,
        json!({
            "analysis": { "main_idea": "信任", "paragraphs": [] },
            "objectives": [{ "id": "o1", "type": "content", "text": "學生能說出原因" }],
            "paragraph_goals": [{ "paragraph_id": "p1", "objective_ids": ["o1"], "note": "" }],
            "task_blocks": [{ "id": "b1", "paragraph_ids": ["p1"],
                              "tasks": [{ "id": "t1", "text": "Complete the table." }] }],
            "warmup": { "background": [{ "id": "w1", "text": "Have you borrowed an umbrella?" }] },
            "exit_ticket": { "items": [{ "id": "e1", "kind": "reflection", "text": "今天哪裡最難？" }] }
        }),
    )
    .await;
    let rows = listed(&app, &t).await;
    assert_eq!(rows[0]["steps_done"], 7);
    assert_eq!(rows[0]["next_step"], Value::Null);
}

#[tokio::test]
async fn a_deleted_unit_or_lesson_is_not_listed_and_login_is_required() {
    let app = test_app!();
    let t = app.teacher("T-STEP-4", "general").await;
    let unit = app.create_unit(&t, "單元").await;
    let id = made(&app, &t, unit, json!({ "title": "課", "entry": "home" })).await;
    sqlx::query("UPDATE lessons SET deleted_at = now() WHERE id = $1")
        .bind(id)
        .execute(&app.db)
        .await
        .unwrap();
    assert!(listed(&app, &t).await.is_empty());
    let (s, _) = app.get("/api/stepwise-lessons", None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_usage_log_records_the_lesson_and_where_it_was_started() {
    let app = test_app!();
    let t = app.teacher("T-STEP-5", "general").await;
    let unit = app.create_unit(&t, "單元").await;
    let home = made(&app, &t, unit, json!({ "title": "A", "entry": "home" })).await;
    // Unknown entries are ignored, never an error; an old client sends none.
    let odd = made(&app, &t, unit, json!({ "title": "B", "entry": 7 })).await;
    let none = made(&app, &t, unit, json!({ "title": "C" })).await;
    let logged: Vec<Value> = sqlx::query_scalar(
        "SELECT payload FROM usage_logs WHERE event = 'lesson_created' ORDER BY id",
    )
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert_eq!(
        logged,
        vec![
            json!({ "lesson_id": home, "unit_id": unit, "entry": "home", "ui_version": 4 }),
            json!({ "lesson_id": odd, "unit_id": unit, "entry": null, "ui_version": null }),
            json!({ "lesson_id": none, "unit_id": unit, "entry": null, "ui_version": null }),
        ]
    );
}
