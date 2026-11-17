//! SIOP 自我檢核 (docs/API.md「SIOP 自我檢核」; docs/SIOP_DESIGN_RULES.md section 八) with the
//! fake provider: a check opens when a /create turn closes `done` (activity, comprehension, kit)
//! and runs in the background; the classroom items never reach the model; the results are
//! stored per turn and run; a check that fails or times out leaves the output usable and can be
//! run again; the rules of `POST …/siop-check`; a check that outlived a restart fails on read.

#[macro_use]
mod common;

use std::time::Duration;

use axum::http::StatusCode;
use serde_json::{json, Value};

const P: [&str; 4] = [
    "Mrs. Lin put an umbrella stand outside her shop on a rainy Monday. A sign said, “Take one if you need it.”",
    "Her neighbors laughed at the idea. They were sure that every umbrella would be gone within a week.",
    "In the first month, twelve of the thirty umbrellas did disappear. However, something unexpected followed.",
    "Today the stand holds more umbrellas than it did at the start. Mrs. Lin says trust is like an umbrella.",
];

const CLASSROOM: [&str; 4] = ["10", "18", "25", "26"];

fn material(marker: &str) -> Value {
    json!({
        "title": "The Umbrella Stand",
        "text": format!("{}\n\n{}\n\n{}\n\n{} {marker}", P[0], P[1], P[2], P[3]),
    })
}

async fn create(app: &common::TestApp, t: &str, kind: &str, marker: &str) -> Value {
    let (s, v) = app
        .post(
            "/api/prep-sessions",
            Some(t),
            json!({ "kind": kind, "material": material(marker) }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    v
}

async fn wait(app: &common::TestApp, t: &str, id: i64) -> Value {
    for _ in 0..1200 {
        let (s, v) = app.get(&format!("/api/prep-sessions/{id}"), Some(t)).await;
        assert_eq!(s, StatusCode::OK, "{v}");
        if v["running"] == false {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("prep session {id} is still running");
}

fn last(session: &Value) -> Value {
    session["turns"].as_array().unwrap().last().unwrap().clone()
}

fn check_path(session: i64, turn: i64) -> String {
    format!("/api/prep-sessions/{session}/turns/{turn}/siop-check")
}

/// Polls a turn's check until it is no longer running.
async fn settled(app: &common::TestApp, t: &str, session: i64, turn: i64) -> Value {
    for _ in 0..1200 {
        let (s, v) = app.get(&check_path(session, turn), Some(t)).await;
        assert_eq!(s, StatusCode::OK, "{v}");
        if v["status"] != "running" {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("the check of turn {turn} is still running");
}

async fn calls(app: &common::TestApp) -> Vec<(i32, String, Value)> {
    sqlx::query_as(
        "SELECT attempt, status, input_snapshot FROM ai_interactions
          WHERE module = 'siop_selfcheck' ORDER BY id",
    )
    .fetch_all(&app.db)
    .await
    .unwrap()
}

fn ids(check: &Value) -> Vec<String> {
    check["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["id"].as_str().unwrap().to_string())
        .collect()
}

fn item<'a>(check: &'a Value, id: &str) -> &'a Value {
    check["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == id)
        .unwrap_or_else(|| panic!("item {id} missing"))
}

#[tokio::test]
async fn an_activity_plan_is_checked_on_the_page_only() {
    let app = test_app!();
    let t = app.teacher("T-SIOP-1", "general").await;
    let session = create(&app, &t, "activity", "").await;
    let id = session["id"].as_i64().unwrap();
    // While the turn runs there is no check yet. (The fake model can finish before the response
    // is read; the check opens with the turn's closing, so only a turn still running is asked.)
    if last(&session)["status"] != "done" {
        assert_eq!(last(&session)["siop_check"], Value::Null);
    }
    let done = wait(&app, &t, id).await;
    let turn = last(&done);
    assert_eq!(turn["status"], "done", "{turn}");
    // The check opened with the turn's closing: the page never sees the output without it.
    let shown = &turn["siop_check"];
    assert!(shown.is_object(), "{turn}");
    assert_eq!(shown["run_no"], 1);
    let turn_id = turn["id"].as_i64().unwrap();
    let check = settled(&app, &t, id, turn_id).await;
    assert_eq!(check["status"], "done", "{check}");
    assert_eq!(check["error"], Value::Null);
    assert_eq!(check["worksheet_version_no"], Value::Null);
    assert!(check["interaction_id"].is_i64());

    // Every item of the protocol, in order; no total score anywhere.
    assert_eq!(
        ids(&check),
        [
            "1-2", "3", "4", "5", "6", "7-8", "9", "10", "11", "12", "13", "14", "15", "16", "17",
            "18", "19", "20", "21", "22", "23-24", "25", "26", "27-28", "29", "30"
        ]
    );
    assert!(!check.to_string().contains("score"));
    for id in CLASSROOM {
        let i = item(&check, id);
        assert_eq!(i["scope"], "classroom", "{i}");
        assert_eq!(i["rating"], Value::Null, "需課堂觀察: never rated");
        assert_eq!(i["reason"], "");
        assert!(!i["note"].as_str().unwrap().is_empty());
    }
    let partial = item(&check, "23-24");
    assert_eq!(partial["scope"], "partial");
    assert_eq!(partial["note"], "只看得到教學流程是否對應目標");
    assert_eq!(partial["rating"], "met");
    assert_eq!(item(&check, "3")["rating"], "partial");
    assert_eq!(item(&check, "17")["rating"], "not_seen");
    assert_eq!(item(&check, "1-2")["name"], "內容目標與語言目標");
    assert!(item(&check, "1-2")["reason"]
        .as_str()
        .unwrap()
        .contains("課堂活動"));

    // One call. The classroom items never reach the model: not in the prompt, not in the
    // schema. The output and the material are logged only as their length and hash.
    let rows = calls(&app).await;
    assert_eq!(rows.len(), 1);
    let (attempt, status, snapshot) = &rows[0];
    assert_eq!((*attempt, status.as_str()), (1, "shown"));
    assert_eq!(snapshot["prompt"]["key"], "siop_selfcheck");
    assert_eq!(snapshot["prompt"]["version_no"], 1);
    assert_eq!(snapshot["schema"], "siop_selfcheck");
    assert_eq!(snapshot["prep"]["turn_id"], turn_id);
    let user = snapshot["prompt"]["user"].as_str().unwrap();
    assert!(user.contains("產出種類：課堂活動"), "{user}");
    assert!(user.contains("要檢核的項目（共 22 項）"), "{user}");
    assert!(
        user.contains("12｜讓概念清楚的技巧（只看書面部分："),
        "{user}"
    );
    for id in CLASSROOM {
        assert!(
            !user.contains(&format!("\n{id}｜")),
            "{id} reached the model"
        );
    }
    for name in ["教師語速與用語", "等待時間", "學生投入", "節奏"] {
        assert!(!user.contains(name), "{name} reached the model");
    }
    let logged = snapshot.to_string();
    assert!(
        !logged.contains("umbrella stand outside"),
        "material logged"
    );
    assert!(!logged.contains("看圖猜主題"), "output logged");
    assert!(logged.contains("檢核的產出"));
    let stored: Value = sqlx::query_scalar("SELECT items FROM siop_checks WHERE turn_id = $1")
        .bind(turn_id)
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(
        stored.as_array().unwrap().len(),
        22,
        "only the judged items are stored"
    );
    assert_eq!(
        stored[0],
        json!({ "id": "1-2", "rating": "met", "reason": "（測試）課堂活動（一節課的教學活動規劃）裡看得到第 1-2 項的做法。" })
    );

    // The session shows the finished check with the turn.
    let (_, again) = app.get(&format!("/api/prep-sessions/{id}"), Some(&t)).await;
    assert_eq!(last(&again)["siop_check"]["status"], "done");
}

#[tokio::test]
async fn worksheets_and_kits_are_checked_with_their_version() {
    let app = test_app!();
    let t = app.teacher("T-SIOP-2", "general").await;
    let session = create(&app, &t, "comprehension", "").await;
    let id = session["id"].as_i64().unwrap();
    let turn = last(&wait(&app, &t, id).await);
    assert_eq!(turn["status"], "done", "{turn}");
    let turn_id = turn["id"].as_i64().unwrap();
    let check = settled(&app, &t, id, turn_id).await;
    assert_eq!(check["status"], "done", "{check}");
    assert_eq!(
        check["worksheet_version_no"],
        turn["worksheet"]["version_no"]
    );
    let rows = calls(&app).await;
    let user = rows.last().unwrap().2["prompt"]["user"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(user.contains("產出種類：閱讀理解題學習單"), "{user}");

    // A kit: one check of both parts, once both are done.
    let (s, v) = app
        .post(
            &format!("/api/prep-sessions/{id}/turns"),
            Some(&t),
            json!({ "kind": "kit" }),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    let kit = last(&wait(&app, &t, id).await);
    assert_eq!(kit["status"], "done", "{kit}");
    let kit_id = kit["id"].as_i64().unwrap();
    let check = settled(&app, &t, id, kit_id).await;
    assert_eq!(check["status"], "done", "{check}");
    assert_eq!(
        check["worksheet_version_no"],
        kit["worksheet"]["version_no"]
    );
    let rows = calls(&app).await;
    assert_eq!(rows.len(), 2);
    let snapshot = &rows[1].2;
    assert!(snapshot["prompt"]["user"]
        .as_str()
        .unwrap()
        .contains("產出種類：教學套件"));
    assert_eq!(
        snapshot["prep"]["worksheet"]["id"], kit["worksheet"]["id"],
        "{snapshot}"
    );

    // A turn without a check (找教學資源): no `siop_check` at all, and no check to read or run.
    let (s, _) = app
        .post(
            &format!("/api/prep-sessions/{id}/turns"),
            Some(&t),
            json!({ "kind": "resources" }),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED);
    let resources = last(&wait(&app, &t, id).await);
    assert!(resources.get("siop_check").is_none(), "{resources}");
    let rid = resources["id"].as_i64().unwrap();
    let (s, _) = app.get(&check_path(id, rid), Some(&t)).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, v) = app.post(&check_path(id, rid), Some(&t), json!({})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(v["error"]["message"], "這種產出沒有 SIOP 自我檢核。");
}

#[tokio::test]
async fn a_check_that_fails_leaves_the_output_and_runs_again() {
    let app = test_app!();
    let t = app.teacher("T-SIOP-3", "general").await;
    let other = app.teacher("T-SIOP-4", "general").await;
    // The first check of the turn times out.
    let session = create(&app, &t, "activity", "FAKE_SIOP_FAIL_FIRST").await;
    let id = session["id"].as_i64().unwrap();
    let turn = last(&wait(&app, &t, id).await);
    assert_eq!(turn["status"], "done", "the output is usable: {turn}");
    assert!(turn["result"]["steps"].is_array());
    let turn_id = turn["id"].as_i64().unwrap();
    let failed = settled(&app, &t, id, turn_id).await;
    assert_eq!(failed["status"], "failed", "{failed}");
    assert!(
        failed["error"]
            .as_str()
            .unwrap()
            .contains("AI 目前沒有回應"),
        "{failed}"
    );
    assert!(failed["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|i| i["rating"].is_null()));
    // Another teacher sees nothing of it.
    let (s, _) = app.get(&check_path(id, turn_id), Some(&other)).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = app
        .post(&check_path(id, turn_id), Some(&other), json!({}))
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // 再試一次: a new run, at another temperature (the gateway caches identical requests).
    let (s, v) = app
        .post(&check_path(id, turn_id), Some(&t), json!({}))
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    assert_eq!(v["run_no"], 2);
    assert!(
        matches!(v["status"].as_str(), Some("running" | "done")),
        "{v}"
    );
    let check = settled(&app, &t, id, turn_id).await;
    assert_eq!(check["status"], "done", "{check}");
    assert_eq!(check["run_no"], 2);
    let rows = calls(&app).await;
    let temps: Vec<f64> = rows
        .iter()
        .map(|r| r.2["request"]["temperature"].as_f64().unwrap())
        .collect();
    assert_eq!(
        rows.len(),
        2,
        "the timed-out call (not retried), then the new run"
    );
    assert_eq!(rows[0].1, "timeout");
    assert!((temps[0] - 0.2).abs() < 1e-6, "{temps:?}");
    assert!((temps[1] - 0.3).abs() < 1e-6, "{temps:?}");
    let runs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM siop_checks WHERE turn_id = $1")
        .bind(turn_id)
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(runs, 2, "every run is kept");
    let logged: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM usage_logs WHERE event = 'siop_check_rerun'")
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(logged, 1);

    // One run at a time.
    sqlx::query("UPDATE siop_checks SET status = 'running' WHERE turn_id = $1 AND run_no = 2")
        .bind(turn_id)
        .execute(&app.db)
        .await
        .unwrap();
    let (s, v) = app
        .post(&check_path(id, turn_id), Some(&t), json!({}))
        .await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    assert_eq!(
        v["error"]["message"],
        "SIOP 自我檢核還在進行，好了之後再試。"
    );
    // A run that outlived a restart fails on the next read.
    sqlx::query(
        "UPDATE siop_checks SET created_at = now() - interval '1 day'
          WHERE turn_id = $1 AND run_no = 2",
    )
    .bind(turn_id)
    .execute(&app.db)
    .await
    .unwrap();
    let (_, s) = app.get(&format!("/api/prep-sessions/{id}"), Some(&t)).await;
    let stale = &last(&s)["siop_check"];
    assert_eq!(stale["status"], "failed", "{stale}");
    assert!(stale["error"].as_str().unwrap().contains("再試一次"));
}

#[tokio::test]
async fn a_check_is_retried_once_and_can_fail_honestly() {
    let app = test_app!();
    let t = app.teacher("T-SIOP-5", "general").await;
    // The first call leaves out the last item: the retry is asked for it.
    let session = create(&app, &t, "activity", "FAKE_SIOP_PARTIAL_ONCE").await;
    let id = session["id"].as_i64().unwrap();
    let turn_id = last(&wait(&app, &t, id).await)["id"].as_i64().unwrap();
    let check = settled(&app, &t, id, turn_id).await;
    assert_eq!(check["status"], "done", "{check}");
    assert_eq!(item(&check, "30")["rating"], "met");
    let rows = calls(&app).await;
    assert_eq!(rows.len(), 2);
    assert_eq!((rows[0].0, rows[0].1.as_str()), (1, "blocked"));
    assert_eq!((rows[1].0, rows[1].1.as_str()), (2, "shown"));
    let user = rows[1].2["prompt"]["user"].as_str().unwrap();
    assert!(user.contains("這些項目沒有檢核：30"), "{user}");

    // Nothing usable in either call: the check fails, the output stays.
    let session = create(&app, &t, "activity", "FAKE_SIOP_BLOCK").await;
    let id = session["id"].as_i64().unwrap();
    let turn = last(&wait(&app, &t, id).await);
    assert_eq!(turn["status"], "done");
    let check = settled(&app, &t, id, turn["id"].as_i64().unwrap()).await;
    assert_eq!(check["status"], "failed");
    assert!(
        check["error"].as_str().unwrap().contains("再試一次"),
        "{check}"
    );

    // A turn that failed has no output to check.
    let session = create(&app, &t, "activity", "FAKE_BLOCK").await;
    let id = session["id"].as_i64().unwrap();
    let turn = last(&wait(&app, &t, id).await);
    assert_eq!(turn["status"], "failed");
    assert_eq!(turn["siop_check"], Value::Null);
    let (s, v) = app
        .post(
            &check_path(id, turn["id"].as_i64().unwrap()),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    assert_eq!(
        v["error"]["message"],
        "這一輪還沒有完成，沒有可以檢核的產出。"
    );
}

#[tokio::test]
async fn a_turn_made_before_the_check_can_be_checked_on_request() {
    let app = test_app!();
    let t = app.teacher("T-SIOP-6", "general").await;
    let session = create(&app, &t, "activity", "").await;
    let id = session["id"].as_i64().unwrap();
    let turn = last(&wait(&app, &t, id).await);
    let turn_id = turn["id"].as_i64().unwrap();
    settled(&app, &t, id, turn_id).await;
    // As if the turn had been made before the check existed, with a plan of version 3.
    sqlx::query("DELETE FROM siop_checks WHERE turn_id = $1")
        .bind(turn_id)
        .execute(&app.db)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE prep_turns SET result = $2 WHERE id = $1",
    )
    .bind(turn_id)
    .bind(json!({
        "title": "舊的規劃",
        "goals": [{ "kind": "content", "zh": "學生能說出主旨",
                    "en": "Students will be able to tell the main idea.", "curriculum_codes": [] }],
        "steps": [{ "phase": "引起動機", "title": "舊步驟", "minutes": 5, "idea": "看圖",
                    "options": [], "script": [], "stuck_tip": "" }],
        "curriculum": [],
    }))
    .execute(&app.db)
    .await
    .unwrap();
    let (_, s) = app.get(&format!("/api/prep-sessions/{id}"), Some(&t)).await;
    assert_eq!(
        last(&s)["siop_check"],
        Value::Null,
        "checked kind, no check yet"
    );
    let (s, _) = app.get(&check_path(id, turn_id), Some(&t)).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, v) = app
        .post(&check_path(id, turn_id), Some(&t), json!({}))
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    assert_eq!(v["run_no"], 1);
    let check = settled(&app, &t, id, turn_id).await;
    assert_eq!(check["status"], "done", "{check}");
    let rows = calls(&app).await;
    let user = rows.last().unwrap().2["prompt"]["user"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(user.contains("產出種類：課堂活動"), "{user}");
}
