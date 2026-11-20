//! 逐步設計 steps 3–7 with AI (docs/API.md「逐步設計：每一步的 AI 協助」): 換一個, 幫我修英文,
//! 逐段整合目標, 逐段問題或任務, 暖身, Exit ticket (fake provider), what the teacher's choices
//! on the way (學生任務, 作答方式, 產生設定) do to a worksheet made from the SOP.

#[macro_use]
mod common;

use std::time::Duration;

use axum::http::StatusCode;
use serde_json::{json, Value};

const P1: &str = "Mrs. Lin put an umbrella stand outside her shop on a rainy Monday. A sign said, “Take one if you need it.”";
const P2: &str = "Her neighbors laughed at the idea. They were sure that every umbrella would be gone within a week.";
const P3: &str = "In the first month, twelve of the thirty umbrellas did disappear. However, something unexpected followed.";
const P4: &str = "Today the stand holds more umbrellas than it did at the start. Mrs. Lin says trust is like an umbrella.";

fn passage(marker: &str) -> String {
    format!("{P1}\n\n{P2}\n\n{P3}\n\n{P4} {marker}")
}

fn objectives() -> Value {
    json!([
        { "id": "o-1", "type": "content", "text": "學生能說出事件發生的原因",
          "student_facing_en": "I can say why it happened." },
        { "id": "o-2", "type": "language", "text": "學生能用 because 說明原因",
          "student_facing_en": "I can use because to give a reason." }
    ])
}

async fn lesson(app: &common::TestApp, t: &str) -> i64 {
    let unit = app.create_unit(t, "L3 The Umbrella Stand").await;
    app.create_lesson(t, unit, "第 2 節 精讀與提問").await
}

async fn put_material(app: &common::TestApp, t: &str, lesson: i64, text: &str) -> Value {
    let (s, v) = app
        .put(
            &format!("/api/lessons/{lesson}/sop/material"),
            Some(t),
            json!({ "title": "The Umbrella Stand", "source_note": "課本第三課", "text": text }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v
}

async fn put_sop(app: &common::TestApp, t: &str, lesson: i64, content: Value) -> Value {
    let (s, v) = app
        .put(
            &format!("/api/lessons/{lesson}/sop"),
            Some(t),
            json!({ "content": content }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v
}

async fn get_sop(app: &common::TestApp, t: &str, lesson: i64) -> Value {
    let (s, v) = app
        .get(&format!("/api/lessons/{lesson}/sop"), Some(t))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v
}

/// A lesson with a four-paragraph material and the two objectives saved.
async fn ready(app: &common::TestApp, t: &str, marker: &str) -> i64 {
    let id = lesson(app, t).await;
    put_material(app, t, id, &passage(marker)).await;
    put_sop(app, t, id, json!({ "objectives": objectives() })).await;
    id
}

async fn start(
    app: &common::TestApp,
    t: &str,
    lesson: i64,
    path: &str,
    body: Value,
) -> (StatusCode, Value) {
    app.post(&format!("/api/lessons/{lesson}/sop/{path}"), Some(t), body)
        .await
}

async fn wait_job(app: &common::TestApp, t: &str, id: i64) -> Value {
    for _ in 0..600 {
        let (s, job) = app.get(&format!("/api/sop-jobs/{id}"), Some(t)).await;
        assert_eq!(s, StatusCode::OK, "{job}");
        if job["status"] == "done" {
            return job;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("job {id} did not finish");
}

async fn run_job(app: &common::TestApp, t: &str, lesson: i64, path: &str, body: Value) -> Value {
    let (s, job) = start(app, t, lesson, path, body).await;
    assert_eq!(s, StatusCode::ACCEPTED, "{job}");
    wait_job(app, t, job["id"].as_i64().unwrap()).await
}

#[derive(sqlx::FromRow, Debug)]
#[allow(dead_code)]
struct Row {
    id: i64,
    module: String,
    attempt: i32,
    retry_of: Option<i64>,
    status: String,
    input_snapshot: Value,
    check_results: Option<Value>,
    prompt_key: Option<String>,
    scaffold_code: Option<String>,
}

async fn rows(app: &common::TestApp, lesson: i64, module: &str) -> Vec<Row> {
    sqlx::query_as(
        "SELECT i.id, i.module, i.attempt, i.retry_of, i.status, i.input_snapshot,
                i.check_results, p.prompt_key, s.scaffold_code
           FROM ai_interactions i
           LEFT JOIN prompt_versions p ON p.id = i.prompt_version_id
           LEFT JOIN scaffolds s ON s.id = i.scaffold_id
          WHERE i.lesson_id = $1 AND i.module = $2 ORDER BY i.id",
    )
    .bind(lesson)
    .bind(module)
    .fetch_all(&app.db)
    .await
    .unwrap()
}

fn ids_of(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect()
}

// ---------------------------------------------------------------- step 4

#[tokio::test]
async fn goals_are_per_paragraph_mapped_to_real_ids_logged_and_never_written() {
    let app = test_app!();
    let t = app.teacher("T-STEP-01", "general").await;
    let id = ready(&app, &t, "").await;
    let before = get_sop(&app, &t, id).await;

    let job = run_job(&app, &t, id, "goals", json!({})).await;
    assert_eq!(job["kind"], "goals");
    assert_eq!(job["slot"], "");
    let result = &job["result"];
    assert_eq!(result["status"], "shown", "{result}");
    let paragraphs = result["output"]["paragraphs"].as_array().unwrap();
    assert_eq!(paragraphs.len(), 4);
    for (i, p) in paragraphs.iter().enumerate() {
        assert_eq!(p["paragraph_no"], i as i64 + 1, "in order");
        assert_eq!(p["paragraph_id"], format!("p{}", i + 1));
        assert!(!p["note"].as_str().unwrap().is_empty());
        for o in ids_of(&p["objective_ids"]) {
            assert!(
                o == "o-1" || o == "o-2",
                "{o}: only ids of the lesson's objectives"
            );
        }
    }
    assert_eq!(paragraphs[0]["objective_ids"], json!(["o-1"]));
    assert_eq!(paragraphs[2]["objective_ids"], json!(["o-1", "o-2"]));
    assert_eq!(
        paragraphs[3]["objective_ids"],
        json!([]),
        "a paragraph may serve none, and its note says so"
    );
    assert!(paragraphs[3]["note"]
        .as_str()
        .unwrap()
        .contains("不特別對應"));
    assert_eq!(result["output"]["missing_paragraph_ids"], json!([]));

    // One call, its own interaction with the new prompt key and scaffold, scoped to the lesson.
    let logged = rows(&app, id, "sop_paragraph_goals").await;
    assert_eq!(logged.len(), 1, "{logged:?}");
    assert_eq!(Some(logged[0].id), result["interaction_id"].as_i64());
    assert_eq!(logged[0].prompt_key.as_deref(), Some("sop_paragraph_goals"));
    assert_eq!(logged[0].scaffold_code.as_deref(), Some("SC-SOP-GOALS"));
    assert_eq!(logged[0].status, "shown");
    assert_eq!(logged[0].check_results.as_ref().unwrap()["passed"], true);
    let snapshot = logged[0].input_snapshot.to_string();
    assert!(!snapshot.contains("umbrella stand outside"), "{snapshot}");
    assert!(snapshot.contains("sha256"));

    // The AI wrote nothing: the saved document is as it was.
    let after = get_sop(&app, &t, id).await;
    assert_eq!(after["version_no"], before["version_no"]);
    assert_eq!(after["content"]["paragraph_goals"], json!([]));
    assert_eq!(after["jobs"]["goals"]["id"], job["id"]);

    // The decision goes through the usual endpoint and comes back as `decided`.
    let interaction = result["interaction_id"].as_i64().unwrap();
    let (s, v) = app
        .post(
            &format!("/api/ai/{interaction}/decision"),
            Some(&t),
            json!({ "item_index": 2, "decision": "adopt" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, job) = app
        .get(&format!("/api/sop-jobs/{}", job["id"]), Some(&t))
        .await;
    assert_eq!(job["decided"], json!([2]));
}

#[tokio::test]
async fn goals_need_objectives_and_material() {
    let app = test_app!();
    let t = app.teacher("T-STEP-02", "general").await;
    let id = lesson(&app, &t).await;
    let (s, v) = start(&app, &t, id, "goals", json!({})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    put_material(&app, &t, id, &passage("")).await;
    let (s, v) = start(&app, &t, id, "goals", json!({})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "no objectives yet: {v}");
    assert!(v["error"]["message"].as_str().unwrap().contains("第 3 步"));
    let (s, v) = start(&app, &t, id, "exit-ticket", json!({})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
}

#[tokio::test]
async fn an_invented_objective_code_is_repaired_once_and_otherwise_dropped() {
    let app = test_app!();
    let t = app.teacher("T-STEP-03", "general").await;
    let id = ready(&app, &t, "FAKE_SOP_GOALS_INVENT").await;
    let job = run_job(&app, &t, id, "goals", json!({})).await;
    assert_eq!(job["result"]["status"], "shown");
    let logged = rows(&app, id, "sop_paragraph_goals").await;
    assert_eq!(logged.len(), 2, "{logged:?}");
    assert_eq!(logged[0].status, "blocked");
    assert_eq!(
        (logged[1].attempt, logged[1].retry_of),
        (2, Some(logged[0].id))
    );
    assert!(logged[1].input_snapshot["prompt"]["user"]
        .as_str()
        .unwrap()
        .contains("O99"));
    assert_eq!(job["result"]["interaction_id"], logged[1].id);
    let first = &job["result"]["output"]["paragraphs"][0];
    assert_eq!(first["objective_ids"], json!(["o-1"]));

    let app2 = test_app!();
    let t2 = app2.teacher("T-STEP-03B", "general").await;
    let id2 = ready(&app2, &t2, "FAKE_SOP_GOALS_INVENT_ALWAYS").await;
    let job = run_job(&app2, &t2, id2, "goals", json!({})).await;
    assert_eq!(job["result"]["status"], "shown");
    assert_eq!(rows(&app2, id2, "sop_paragraph_goals").await.len(), 2);
    let all = job["result"]["output"]["paragraphs"].as_array().unwrap();
    for p in all {
        for o in ids_of(&p["objective_ids"]) {
            assert!(
                o == "o-1" || o == "o-2",
                "the invented code never reaches the teacher"
            );
        }
    }
}

#[tokio::test]
async fn missing_paragraphs_and_long_notes_are_asked_for_again() {
    let app = test_app!();
    let t = app.teacher("T-STEP-04", "general").await;
    let id = ready(&app, &t, "FAKE_SOP_GOALS_SKIP").await;
    let job = run_job(&app, &t, id, "goals", json!({})).await;
    let out = &job["result"]["output"];
    assert_eq!(out["paragraphs"].as_array().unwrap().len(), 4, "{out}");
    assert_eq!(out["missing_paragraph_ids"], json!([]));
    assert_eq!(rows(&app, id, "sop_paragraph_goals").await.len(), 2);

    let app2 = test_app!();
    let t2 = app2.teacher("T-STEP-04B", "general").await;
    let id2 = ready(&app2, &t2, "FAKE_SOP_GOALS_SKIP_ALWAYS").await;
    let job = run_job(&app2, &t2, id2, "goals", json!({})).await;
    let out = &job["result"]["output"];
    assert_eq!(out["paragraphs"].as_array().unwrap().len(), 3);
    assert_eq!(out["missing_paragraph_ids"], json!(["p2"]));

    let app3 = test_app!();
    let t3 = app3.teacher("T-STEP-04C", "general").await;
    let id3 = ready(&app3, &t3, "FAKE_SOP_GOALS_LONG").await;
    let job = run_job(&app3, &t3, id3, "goals", json!({})).await;
    let note = job["result"]["output"]["paragraphs"][0]["note"]
        .as_str()
        .unwrap();
    assert!(note.chars().count() <= 120, "{note}");
    let logged = rows(&app3, id3, "sop_paragraph_goals").await;
    assert_eq!(logged.len(), 2);
    assert_eq!(logged[0].check_results.as_ref().unwrap()["passed"], false);
}

#[tokio::test]
async fn failures_end_the_job_and_leave_the_step_usable() {
    let app = test_app!();
    let t = app.teacher("T-STEP-05", "general").await;
    for (marker, status) in [
        ("FAKE_BLOCK", "blocked"),
        ("FAKE_TIMEOUT", "timeout"),
        ("FAKE_ERROR", "error"),
    ] {
        let id = ready(&app, &t, marker).await;
        let job = run_job(&app, &t, id, "goals", json!({})).await;
        assert_eq!(job["result"]["status"], status, "{marker}: {job}");
        assert_eq!(job["result"]["output"], Value::Null);
    }
}

#[tokio::test]
async fn one_paragraph_can_be_suggested_again_and_a_second_click_joins() {
    let app = test_app!();
    let t = app.teacher("T-STEP-06", "general").await;
    let id = ready(&app, &t, "FAKE_SLOW").await;
    let whole = run_job(&app, &t, id, "goals", json!({})).await;
    let parent = whole["id"].as_i64().unwrap();

    let (s, first) = start(
        &app,
        &t,
        id,
        "goals",
        json!({ "paragraph_id": "p2", "parent_job_id": parent }),
    )
    .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{first}");
    assert_eq!(first["slot"], "p:p2");
    assert_eq!(first["parent_job_id"], parent);
    let (_, again) = start(&app, &t, id, "goals", json!({ "paragraph_id": "p2" })).await;
    assert_eq!(again["id"], first["id"], "a second click joins");
    let (_, other) = start(&app, &t, id, "goals", json!({ "paragraph_id": "p3" })).await;
    assert_ne!(other["id"], first["id"], "another paragraph runs beside it");
    // The page finds it again after a reload.
    let state = get_sop(&app, &t, id).await;
    assert!(state["jobs"]["redo"]
        .as_array()
        .unwrap()
        .iter()
        .any(|j| j["id"] == first["id"] && j["status"] == "running"));

    let done = wait_job(&app, &t, first["id"].as_i64().unwrap()).await;
    let paragraphs = done["result"]["output"]["paragraphs"].as_array().unwrap();
    assert_eq!(paragraphs.len(), 1);
    assert_eq!(paragraphs[0]["paragraph_id"], "p2");
    wait_job(&app, &t, other["id"].as_i64().unwrap()).await;
    // The whole run is still the stage's latest job.
    let state = get_sop(&app, &t, id).await;
    assert_eq!(state["jobs"]["goals"]["id"], whole["id"]);
    let (s, v) = start(&app, &t, id, "goals", json!({ "paragraph_id": "p99" })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");

    // A new whole run makes the old redo jobs history.
    let again_whole = run_job(&app, &t, id, "goals", json!({})).await;
    assert_ne!(again_whole["id"], whole["id"]);
    let state = get_sop(&app, &t, id).await;
    assert_eq!(state["jobs"]["redo"], json!([]));
}

// ---------------------------------------------------------------- step 3

#[tokio::test]
async fn one_objective_can_be_replaced_without_touching_the_others() {
    let app = test_app!();
    let t = app.teacher("T-STEP-07", "general").await;
    let id = ready(&app, &t, "").await;
    let whole = run_job(&app, &t, id, "objectives", json!({})).await;
    assert_eq!(whole["result"]["status"], "shown");
    let list = whole["result"]["output"]["objectives"]
        .as_array()
        .unwrap()
        .clone();
    assert!(list.len() >= 3);
    let index = list
        .iter()
        .position(|o| o["type"] == "language")
        .expect("a language suggestion");

    let swapped = run_job(
        &app,
        &t,
        id,
        "objectives",
        json!({ "parent_job_id": whole["id"], "item_index": index }),
    )
    .await;
    assert_eq!(swapped["parent_job_id"], whole["id"]);
    assert_eq!(swapped["slot"], format!("{}:{index}", whole["id"]));
    let out = swapped["result"]["output"]["objectives"]
        .as_array()
        .unwrap();
    assert_eq!(out.len(), 1, "one objective only");
    assert_eq!(out[0]["type"], "language", "the same type");
    assert_ne!(out[0]["text"], list[index]["text"], "a different one");
    assert!(out[0]["student_facing_en"]
        .as_str()
        .unwrap()
        .starts_with("I can"));

    // The batch itself and the teacher's objectives are untouched.
    let state = get_sop(&app, &t, id).await;
    assert_eq!(state["jobs"]["objectives"]["id"], whole["id"]);
    assert_eq!(
        state["jobs"]["objectives"]["result"]["output"]["objectives"]
            .as_array()
            .unwrap()
            .len(),
        list.len()
    );
    assert_eq!(state["content"]["objectives"].as_array().unwrap().len(), 2);
    assert_eq!(state["jobs"]["redo"][0]["id"], swapped["id"]);

    // Its own interaction (the objectives prompt, asked for one) and its own decisions.
    let logged = rows(&app, id, "sop_objectives").await;
    assert_eq!(logged.len(), 2, "{logged:?}");
    assert_eq!(swapped["result"]["interaction_id"], logged[1].id);
    assert!(logged[1].input_snapshot["prompt"]["user"]
        .as_str()
        .unwrap()
        .contains("只要 1 項"));
    let (s, v) = app
        .post(
            &format!("/api/ai/{}/decision", logged[1].id),
            Some(&t),
            json!({ "item_index": 0, "decision": "adopt" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");

    // A replacement can be replaced again (the chain goes through the redo job).
    let again = run_job(
        &app,
        &t,
        id,
        "objectives",
        json!({ "parent_job_id": swapped["id"], "item_index": 0 }),
    )
    .await;
    assert_eq!(
        again["result"]["output"]["objectives"][0]["type"],
        "language"
    );

    // Bad requests.
    let (s, _) = start(
        &app,
        &t,
        id,
        "objectives",
        json!({ "parent_job_id": whole["id"], "item_index": 99 }),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = start(
        &app,
        &t,
        id,
        "objectives",
        json!({ "parent_job_id": 999999, "item_index": 0 }),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let other = app.teacher("T-STEP-07B", "general").await;
    let other_lesson = ready(&app, &other, "").await;
    let (s, _) = start(
        &app,
        &other,
        other_lesson,
        "objectives",
        json!({ "parent_job_id": whole["id"], "item_index": 0 }),
    )
    .await;
    assert_eq!(
        s,
        StatusCode::NOT_FOUND,
        "another teacher's job is not found"
    );
}

#[tokio::test]
async fn the_english_of_an_objective_is_fixed_in_one_call_and_never_written() {
    let app = test_app!();
    let t = app.teacher("T-STEP-08", "general").await;
    let id = lesson(&app, &t).await;
    let url = format!("/api/lessons/{id}/sop/objective-english");
    let (s, v) = app
        .post(
            &url,
            Some(&t),
            json!({ "type": "language", "text": "學生能用 because 說明原因",
                    "student_facing_en": "i can use because to explain reason" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["status"], "shown", "{v}");
    assert_eq!(
        v["output"]["english"],
        "I can use because to explain reason."
    );
    assert_eq!(v["output"]["changed"], true);
    assert!(v["output"]["note"].as_str().unwrap().contains("測試"));

    // The same sentence back: nothing to change.
    let (_, same) = app
        .post(
            &url,
            Some(&t),
            json!({ "type": "content", "text": "學生能說出原因",
                    "student_facing_en": "I can say why it happened." }),
        )
        .await;
    assert_eq!(same["output"]["changed"], false);

    // Empty English: written from the Chinese objective.
    let (_, written) = app
        .post(
            &url,
            Some(&t),
            json!({ "type": "content", "text": "學生能說出原因", "student_facing_en": "" }),
        )
        .await;
    assert!(written["output"]["english"]
        .as_str()
        .unwrap()
        .starts_with("I can "));
    assert_eq!(written["output"]["changed"], true);

    let logged = rows(&app, id, "sop_objective_english").await;
    assert_eq!(logged.len(), 3);
    assert_eq!(
        logged[0].prompt_key.as_deref(),
        Some("sop_objective_english")
    );
    assert_eq!(logged[0].scaffold_code.as_deref(), Some("SC-SOP-ENGLISH"));
    assert_eq!(v["interaction_id"], logged[0].id);
    assert_eq!(logged[0].input_snapshot["request"]["max_tokens"], 400);
    // The decision (用這個 / 保留我的) is logged through the usual endpoint.
    for (i, decision) in [(0, "adopt"), (1, "reject")] {
        let (s, v) = app
            .post(
                &format!("/api/ai/{}/decision", logged[i].id),
                Some(&t),
                json!({ "item_index": 0, "decision": decision }),
            )
            .await;
        assert_eq!(s, StatusCode::OK, "{v}");
    }
    // Nothing was saved into the SOP.
    assert_eq!(get_sop(&app, &t, id).await["version_no"], Value::Null);

    // The Chinese objective is required; a model that cannot produce an answer says so.
    let (s, _) = app
        .post(&url, Some(&t), json!({ "type": "content", "text": " " }))
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, blocked) = app
        .post(
            &url,
            Some(&t),
            json!({ "type": "content", "text": "學生能說出 FAKE_BLOCK 原因" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(blocked["status"], "blocked");
    assert_eq!(blocked["output"], Value::Null);
    let blocked_rows = rows(&app, id, "sop_objective_english").await;
    assert_eq!(blocked_rows.len(), 5, "one repair call for the blocked one");
    // Other teachers' lessons are not reachable.
    let other = app.teacher("T-STEP-08B", "general").await;
    let (s, _) = app
        .post(
            &url,
            Some(&other),
            json!({ "type": "content", "text": "x" }),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------- step 5

#[tokio::test]
async fn tasks_cover_every_paragraph_in_order_with_labels_scaffolds_and_procedural_steps() {
    let app = test_app!();
    let t = app.teacher("T-STEP-09", "general").await;
    let id = ready(&app, &t, "").await;
    // What the teacher wrote stays and is shown to the model so that it is not repeated.
    put_sop(
        &app,
        &t,
        id,
        json!({
            "objectives": objectives(),
            "task_blocks": [{ "id": "mine", "paragraph_ids": ["p1"],
                "tasks": [{ "id": "t-mine", "function": "discover",
                           "text": "Why did Mrs. Lin put the stand outside?" }] }]
        }),
    )
    .await;
    let job = run_job(&app, &t, id, "tasks", json!({})).await;
    let result = &job["result"];
    assert_eq!(result["status"], "shown", "{result}");
    let out = &result["output"];
    let blocks = out["blocks"].as_array().unwrap();
    assert!(!blocks.is_empty());
    // Every paragraph is covered, blocks go in paragraph order.
    let mut covered: Vec<String> = Vec::new();
    let mut last_no = 0;
    for b in blocks {
        let nos: Vec<i64> = b["paragraph_nos"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_i64().unwrap())
            .collect();
        assert!(nos[0] > last_no, "blocks in paragraph order: {nos:?}");
        last_no = nos[0];
        covered.extend(ids_of(&b["paragraph_ids"]));
        // The generator's own labels, pre-selected by Manna.
        assert!(!b["student_tasks"].as_array().unwrap().is_empty(), "{b}");
        assert!(!b["answer_formats"].as_array().unwrap().is_empty(), "{b}");
        for task in b["tasks"].as_array().unwrap() {
            let text = task["text"].as_str().unwrap();
            assert!(!text.is_empty());
            assert!(
                !text.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)),
                "English for students"
            );
            assert!(!task["answer_format"].as_str().unwrap().is_empty());
            let evidence = task["evidence"].as_str().unwrap();
            if !evidence.is_empty() {
                assert!(
                    [P1, P2, P3, P4].iter().any(|p| p.contains(evidence)),
                    "verbatim: {evidence}"
                );
            }
            // The procedural scaffold is always there.
            assert!(
                task["scaffolds"]["procedural"]
                    .as_str()
                    .unwrap()
                    .starts_with("1. "),
                "{task}"
            );
        }
    }
    for n in 1..=4 {
        assert!(
            covered.contains(&format!("p{n}")),
            "p{n} uncovered: {covered:?}"
        );
    }
    assert_eq!(out["missing_paragraph_ids"], json!([]));
    assert_eq!(out["scaffolds_ok"], true);

    // Both calls are their own interactions with their prompt keys.
    let tasks = rows(&app, id, "sop_paragraph_tasks").await;
    let scaffolds = rows(&app, id, "sop_scaffolds").await;
    assert_eq!(
        (tasks.len(), scaffolds.len()),
        (1, 1),
        "{tasks:?} {scaffolds:?}"
    );
    assert_eq!(tasks[0].prompt_key.as_deref(), Some("sop_paragraph_tasks"));
    assert_eq!(scaffolds[0].prompt_key.as_deref(), Some("sop_scaffolds"));
    assert_eq!(tasks[0].scaffold_code.as_deref(), Some("SC-SOP-TASKS"));
    assert_eq!(result["interaction_id"], tasks[0].id);
    // The teacher's task was given to the model as hers (and the passage is not logged).
    let user = tasks[0].input_snapshot["prompt"]["user"].as_str().unwrap();
    assert!(user.contains("教師已經寫的問題或任務"), "{user}");
    assert!(!user.contains("umbrella stand outside her shop"), "{user}");
    // Habits are wired: the snapshot records the switch (off by default) for these steps.
    assert_eq!(tasks[0].input_snapshot["teacher_memory"]["enabled"], false);
    assert!(scaffolds[0].input_snapshot.get("teacher_memory").is_none());

    // Nothing was written, and the teacher's own task is still the only one saved.
    let state = get_sop(&app, &t, id).await;
    let saved = state["content"]["task_blocks"].as_array().unwrap();
    assert_eq!(saved.len(), 1);
    assert_eq!(
        saved[0]["tasks"][0]["text"],
        "Why did Mrs. Lin put the stand outside?"
    );
    assert_eq!(saved[0]["tasks"][0]["origin"], "teacher");
}

#[tokio::test]
async fn task_checks_repair_once_and_the_scaffold_call_is_not_fatal() {
    // Worksheet markers apply: the first tasks call has a Chinese prompt, the second does not.
    let app = test_app!();
    let t = app.teacher("T-STEP-10", "general").await;
    let id = ready(&app, &t, "FAKE_WS_CJK").await;
    let job = run_job(&app, &t, id, "tasks", json!({})).await;
    assert_eq!(job["result"]["status"], "shown", "{job}");
    let tasks = rows(&app, id, "sop_paragraph_tasks").await;
    assert_eq!(tasks.len(), 2, "{tasks:?}");
    assert_eq!(tasks[0].status, "blocked");
    assert_eq!(tasks[1].retry_of, Some(tasks[0].id));
    assert_eq!(job["result"]["interaction_id"], tasks[1].id);

    // Blocked on every attempt: the job says so and offers nothing.
    let app2 = test_app!();
    let t2 = app2.teacher("T-STEP-10B", "general").await;
    let id2 = ready(&app2, &t2, "FAKE_BLOCK").await;
    let job = run_job(&app2, &t2, id2, "tasks", json!({})).await;
    assert_eq!(job["result"]["status"], "blocked");
    assert_eq!(rows(&app2, id2, "sop_scaffolds").await.len(), 0);
}

#[tokio::test]
async fn procedural_steps_are_made_by_the_program_for_a_teachers_own_task() {
    let app = test_app!();
    let t = app.teacher("T-STEP-11", "general").await;
    let id = ready(&app, &t, "").await;
    let (s, v) = app
        .post(
            &format!("/api/lessons/{id}/sop/procedural-steps"),
            Some(&t),
            json!({ "paragraph_ids": ["p1"], "student_tasks": ["小組共同完成"],
                    "answer_formats": ["簡答題"] }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let steps = v["steps"].as_array().unwrap();
    assert!(steps.len() >= 3);
    assert!(steps.iter().any(|s| s.as_str().unwrap().contains("group")));
    assert!(v["text"].as_str().unwrap().starts_with("1. "));
    // No model was asked.
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM ai_interactions WHERE lesson_id = $1")
        .bind(id)
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(n, 0);
    let other = app.teacher("T-STEP-11B", "general").await;
    let (s, _) = app
        .post(
            &format!("/api/lessons/{id}/sop/procedural-steps"),
            Some(&other),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------- step 6

#[tokio::test]
async fn warmup_picks_words_from_candidates_found_by_code_and_never_gives_levels() {
    let app = test_app!();
    let t = app.teacher("T-STEP-12", "general").await;
    let id = ready(&app, &t, "").await;
    let job = run_job(&app, &t, id, "warmup", json!({})).await;
    let result = &job["result"];
    assert_eq!(result["status"], "shown", "{result}");
    let out = &result["output"];
    let background = out["background"].as_array().unwrap();
    assert!((1..=2).contains(&background.len()), "{out}");
    for a in background {
        assert!(a["scaffolds"]["procedural"]
            .as_str()
            .unwrap()
            .starts_with("1. "));
        assert!(!a["text"].as_str().unwrap().is_empty());
    }
    let vocabulary = out["vocabulary"].as_array().unwrap();
    assert!(!vocabulary.is_empty(), "{out}");
    let all = [P1, P2, P3, P4].join(" ").to_lowercase();
    for v in vocabulary {
        let word = v["word"].as_str().unwrap();
        assert!(
            all.contains(&word.to_lowercase()),
            "{word} comes from the material"
        );
        // No word list is imported in the test database: the level is unknown, never invented.
        assert_eq!(v["level"], Value::Null);
        let sentence = v["evidence"].as_str().unwrap();
        assert!(
            [P1, P2, P3, P4].iter().any(|p| p.contains(sentence)),
            "{sentence}"
        );
    }
    assert_eq!(out["levels_available"], false);
    assert!(out["candidates_available"].as_i64().unwrap() > 0);

    let logged = rows(&app, id, "sop_warmup").await;
    assert_eq!(logged.len(), 1);
    assert_eq!(logged[0].prompt_key.as_deref(), Some("sop_warmup"));
    assert_eq!(logged[0].scaffold_code.as_deref(), Some("SC-SOP-WARMUP"));
    // The model's schema has no level field at all.
    assert!(!logged[0].input_snapshot.to_string().contains("\"level\""));
    // The candidates are in the log as a length and a hash, never as the words of the material.
    assert!(logged[0].input_snapshot.to_string().contains("sha256"));
    assert_eq!(logged[0].input_snapshot["teacher_memory"]["enabled"], false);
    // Nothing written.
    assert_eq!(
        get_sop(&app, &t, id).await["content"]["warmup"],
        json!({ "background": [], "vocabulary": [] })
    );
}

// ---------------------------------------------------------------- step 7

#[tokio::test]
async fn exit_ticket_has_a_formative_item_on_a_real_objective_and_a_reflection() {
    let app = test_app!();
    let t = app.teacher("T-STEP-13", "general").await;
    let id = ready(&app, &t, "").await;
    let job = run_job(&app, &t, id, "exit-ticket", json!({})).await;
    assert_eq!(job["kind"], "exit_ticket");
    let result = &job["result"];
    assert_eq!(result["status"], "shown", "{result}");
    let items = result["output"]["items"].as_array().unwrap();
    assert!((2..=4).contains(&items.len()), "{items:?}");
    let formative: Vec<&Value> = items.iter().filter(|i| i["kind"] == "formative").collect();
    assert!(!formative.is_empty());
    for f in &formative {
        assert!(
            f["objective_id"] == "o-1" || f["objective_id"] == "o-2",
            "linked to a current objective: {f}"
        );
        assert!(
            !f["answer"].as_str().unwrap().is_empty(),
            "with a reference answer"
        );
    }
    assert!(
        items.iter().any(|i| i["kind"] != "formative"),
        "a reflection or questions item"
    );
    for i in items {
        assert!(!i["text"]
            .as_str()
            .unwrap()
            .chars()
            .any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)));
    }
    let logged = rows(&app, id, "sop_exit_ticket").await;
    assert_eq!(logged.len(), 1);
    assert_eq!(logged[0].prompt_key.as_deref(), Some("sop_exit_ticket"));
    assert_eq!(logged[0].scaffold_code.as_deref(), Some("SC-SOP-EXIT"));
}

// ---------------------------------------------------------------- the document

#[tokio::test]
async fn adopted_items_keep_their_origin_and_older_documents_still_read() {
    let app = test_app!();
    let t = app.teacher("T-STEP-14", "general").await;
    let id = ready(&app, &t, "").await;
    let saved = put_sop(
        &app,
        &t,
        id,
        json!({
            "objectives": objectives(),
            "paragraph_goals": [
                { "paragraph_id": "p1", "objective_ids": ["o-1"], "note": "先找關鍵句",
                  "origin": "ai_adopted" },
                { "paragraph_id": "p2", "objective_ids": [], "note": "自己寫的" }
            ],
            "task_blocks": [{ "id": "b1", "paragraph_ids": ["p1"],
                "student_tasks": ["理解內容", "推論／預測"], "answer_formats": ["選擇題"],
                "ai_student_tasks": ["理解內容"], "ai_answer_formats": ["選擇題"],
                "tasks": [{ "id": "t1", "function": "discover", "text": "Why?",
                    "origin": "ai_edited", "answer_format": "選擇題",
                    "options": ["a", "b", "c", "d"], "answer": "a", "evidence": "Take one" }] }],
            "warmup": { "vocabulary": [{ "id": "v1", "word": "umbrella", "note": "n",
                                         "origin": "ai_adopted", "level": 6 }] },
            "exit_ticket": { "items": [{ "id": "e1", "kind": "formative", "text": "Q?",
                "origin": "ai_adopted", "objective_id": "o-1", "answer": "A" }] },
            "options": { "generation": ["顯示學習目標"], "exit_ticket": false }
        }),
    )
    .await;
    let c = &saved["content"];
    assert_eq!(c["paragraph_goals"][0]["origin"], "ai_adopted");
    assert_eq!(c["paragraph_goals"][1]["origin"], "teacher", "the default");
    assert_eq!(c["task_blocks"][0]["tasks"][0]["origin"], "ai_edited");
    assert_eq!(
        c["task_blocks"][0]["student_tasks"],
        json!(["理解內容", "推論／預測"])
    );
    assert_eq!(c["task_blocks"][0]["ai_student_tasks"], json!(["理解內容"]));
    assert_eq!(
        c["task_blocks"][0]["tasks"][0]["options"],
        json!(["a", "b", "c", "d"])
    );
    assert_eq!(c["exit_ticket"]["items"][0]["objective_id"], "o-1");
    assert_eq!(c["options"]["exit_ticket"], false);
    // A level a client sends is the server's to decide (no word list here: none).
    assert_eq!(c["warmup"]["vocabulary"][0]["level"], Value::Null);
    assert_eq!(c["warmup"]["vocabulary"][0]["origin"], "ai_adopted");

    // Labels are the generator's own.
    let (s, v) = app
        .put(
            &format!("/api/lessons/{id}/sop"),
            Some(&t),
            json!({ "content": { "task_blocks": [{ "id": "b", "student_tasks": ["高層次思考"] }] } }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    let (s, _) = app
        .put(
            &format!("/api/lessons/{id}/sop"),
            Some(&t),
            json!({ "content": { "options": { "generation": ["自創的選項"] } } }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // A document saved before these fields existed reads with the defaults.
    let old = put_sop(
        &app,
        &t,
        id,
        json!({ "paragraph_goals": [{ "paragraph_id": "p1", "objective_ids": [], "note": "舊的" }],
                "task_blocks": [{ "id": "b", "paragraph_ids": [], "tasks": [{ "id": "t", "text": "x" }] }],
                "exit_ticket": { "items": [{ "id": "e", "kind": "reflection", "text": "r" }] } }),
    )
    .await;
    let c = &old["content"];
    assert_eq!(c["paragraph_goals"][0]["origin"], "teacher");
    assert_eq!(c["task_blocks"][0]["student_tasks"], json!([]));
    assert_eq!(c["task_blocks"][0]["tasks"][0]["answer_format"], "");
    assert_eq!(c["exit_ticket"]["items"][0]["objective_id"], "");
    assert_eq!(
        c["options"],
        json!({ "material_types": null, "generation": null,
        "levels": null, "minutes": null, "exit_ticket": null })
    );
}

// ---------------------------------------------------------------- the worksheet made from the SOP

async fn wait_worksheet(app: &common::TestApp, t: &str, id: i64) -> Value {
    for _ in 0..800 {
        let (s, v) = app
            .get(&format!("/api/worksheets/{id}/jobs"), Some(t))
            .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        if v["running"] == false {
            let (_, state) = app.get(&format!("/api/worksheets/{id}"), Some(t)).await;
            return state;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("worksheet {id} did not finish");
}

#[tokio::test]
async fn a_worksheet_from_the_sop_uses_the_choices_made_in_the_steps_and_asks_nothing_again() {
    let app = test_app!();
    let t = app.teacher("T-STEP-15", "general").await;
    let id = ready(&app, &t, "").await;
    put_sop(
        &app,
        &t,
        id,
        json!({
            "objectives": objectives(),
            "task_blocks": [
                { "id": "b1", "paragraph_ids": ["p1", "p2"],
                  "student_tasks": ["理解內容", "討論／分享"], "answer_formats": ["簡答題"],
                  "tasks": [{ "id": "t1", "function": "discover", "text": "Why did she do it?" }] },
                { "id": "b2", "paragraph_ids": ["p3"],
                  "student_tasks": ["推論／預測"],
                  "tasks": [{ "id": "t2", "function": "analyze", "text": "What will happen?",
                             "answer_format": "選擇題" }] }
            ],
            "options": { "material_types": ["閱讀文本"],
                         "generation": ["顯示學習目標", "提供字詞表"],
                         "levels": ["basic", "standard"], "minutes": 40, "exit_ticket": false }
        }),
    )
    .await;

    // The lesson page already shows what the worksheet will start with (and the ✦ defaults).
    let state = get_sop(&app, &t, id).await;
    let settings = &state["worksheet_settings"];
    assert_eq!(
        settings["student_tasks"],
        json!(["理解內容", "推論／預測", "討論／分享"])
    );
    assert_eq!(settings["answer_formats"], json!(["選擇題", "簡答題"]));
    assert_eq!(
        settings["generation"],
        json!(["顯示學習目標", "提供字詞表"])
    );
    assert_eq!(settings["levels"], json!(["basic", "standard"]));
    assert_eq!(settings["minutes"], 40);
    assert_eq!(settings["skip_exit_ticket"], true);
    let suggested = &state["worksheet_suggested"];
    assert_eq!(
        suggested["answer_formats"],
        json!([]),
        "the content alone chooses none"
    );
    assert_eq!(suggested["skip_exit_ticket"], false);

    // The worksheet made from the lesson starts with exactly those choices.
    let (s, ws) = app
        .post("/api/worksheets", Some(&t), json!({ "lesson_id": id }))
        .await;
    assert_eq!(s, StatusCode::CREATED, "{ws}");
    for key in [
        "student_tasks",
        "answer_formats",
        "generation",
        "levels",
        "minutes",
        "skip_exit_ticket",
        "material_types",
    ] {
        assert_eq!(ws["settings"][key], settings[key], "{key}");
    }
    // Logged like the generator's settings.
    let payload: Value = sqlx::query_scalar(
        "SELECT payload FROM usage_logs WHERE event = 'worksheet_created'
          ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(payload["from_sop"], true);
    assert_eq!(
        payload["settings"]["answer_formats"],
        json!(["選擇題", "簡答題"])
    );
    assert_eq!(payload["settings"]["skip_exit_ticket"], true);

    // Generating needs nothing more from the teacher, and the sheet has no exit ticket.
    let ws_id = ws["id"].as_i64().unwrap();
    let (s, v) = app
        .post(
            &format!("/api/worksheets/{ws_id}/generate"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    let done = wait_worksheet(&app, &t, ws_id).await;
    let kinds: Vec<&str> = done["content"]["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["kind"].as_str().unwrap())
        .collect();
    assert!(kinds.contains(&"activity"), "{kinds:?}");
    assert!(!kinds.contains(&"exit_ticket"), "{kinds:?}");
    assert!(
        !kinds.contains(&"warmup"),
        "warm-up was not ticked: {kinds:?}"
    );
    // The generator's own flow is unchanged: a direct worksheet still ends with an exit ticket.
    let (s, direct) = app
        .post(
            "/api/worksheets",
            Some(&t),
            json!({ "material": { "title": "x", "text": passage("") } }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{direct}");
    assert_eq!(direct["settings"]["skip_exit_ticket"], false);
    let direct_id = direct["id"].as_i64().unwrap();
    app.post(
        &format!("/api/worksheets/{direct_id}/generate"),
        Some(&t),
        json!({}),
    )
    .await;
    let direct_done = wait_worksheet(&app, &t, direct_id).await;
    let direct_kinds: Vec<&str> = direct_done["content"]["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["kind"].as_str().unwrap())
        .collect();
    assert!(direct_kinds.contains(&"exit_ticket"), "{direct_kinds:?}");
}
