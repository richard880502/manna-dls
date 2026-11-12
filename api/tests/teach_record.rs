//! Phase 1.2 (docs/API.md「Teach & Record」): lesson runs, offline bookmark sync, revisions,
//! post-lesson review, ownership, QR pairing and the phone remote's token scope.

#[macro_use]
mod common;

use axum::http::{Method, StatusCode};
use serde_json::{json, Value};

fn err_code(v: &Value) -> &str {
    v["error"]["code"].as_str().unwrap_or("<none>")
}

fn statuses(v: &Value) -> Vec<&str> {
    v["results"]
        .as_array()
        .unwrap_or_else(|| panic!("no results: {v}"))
        .iter()
        .map(|r| r["status"].as_str().unwrap())
        .collect()
}

fn create_op(op_id: &str, device_event_id: &str, device_ts: &str, polarity: &str) -> Value {
    json!({
        "op_id": op_id, "kind": "create", "device_event_id": device_event_id,
        "device_ts": device_ts, "polarity": polarity, "device_type": "desktop"
    })
}

/// Teacher + unit + lesson (with a saved plan) + a started run.
struct Setup {
    t: String,
    lesson: i64,
    run: i64,
}

async fn setup(app: &common::TestApp, research_id: &str) -> Setup {
    let t = app.teacher(research_id, "general").await;
    let unit = app.create_unit(&t, "倒裝句").await;
    let lesson = app.create_lesson(&t, unit, "第 1 節").await;
    let (s, v) = app
        .post(
            &format!("/api/lessons/{lesson}/versions"),
            Some(&t),
            json!({ "objective": "倒裝句", "steps": [
                { "position": 1, "activity_type": "暖身", "content": "比較句子" },
                { "position": 2, "activity_type": "兩兩討論", "content": "改寫" }
            ]}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, v) = app
        .post(&format!("/api/lessons/{lesson}/runs"), Some(&t), json!({}))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    Setup {
        t,
        lesson,
        run: v["id"].as_i64().unwrap(),
    }
}

async fn sync(app: &common::TestApp, t: &str, run: i64, ops: Value) -> Value {
    let (s, v) = app
        .post(
            &format!("/api/runs/{run}/events/sync"),
            Some(t),
            json!({ "ops": ops }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v
}

async fn revisions(
    app: &common::TestApp,
    run: i64,
    device_event_id: &str,
) -> Vec<(String, String)> {
    sqlx::query_as(
        "SELECT rv.change_type, rv.actor FROM record_event_revisions rv
           JOIN record_events e ON e.id = rv.record_event_id
          WHERE e.lesson_run_id = $1 AND e.device_event_id = $2 ORDER BY rv.id",
    )
    .bind(run)
    .bind(device_event_id)
    .fetch_all(&app.db)
    .await
    .unwrap()
}

// ---------------------------------------------------------------- lifecycle

#[tokio::test]
async fn run_lifecycle_one_open_run_per_lesson() {
    let app = test_app!();
    let Setup { t, lesson, run } = setup(&app, "T-R1").await;

    let (s, v) = app.get(&format!("/api/runs/{run}"), Some(&t)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["lesson_id"], lesson);
    assert_eq!(v["lesson_title"], "第 1 節");
    assert_eq!(v["unit_title"], "倒裝句");
    assert!(v["started_at"].is_string());
    assert_eq!(v["ended_at"], Value::Null);
    assert_eq!(v["events"], json!([]));
    assert_eq!(v["remotes"], json!([]));
    assert_eq!(v["plan"]["version_no"], 1);
    let steps = v["plan"]["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 2);
    assert!(steps[0]["id"].is_i64());
    assert_eq!(steps[1]["activity_type"], "兩兩討論");
    // The run remembers the plan version it started with.
    let version_id: Option<i64> =
        sqlx::query_scalar("SELECT lesson_version_id FROM lesson_runs WHERE id = $1")
            .bind(run)
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(version_id, v["plan"]["lesson_version_id"].as_i64());

    // A second 開始本節課 while one is open → 409.
    let (s, v) = app
        .post(&format!("/api/lessons/{lesson}/runs"), Some(&t), json!({}))
        .await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    assert_eq!(err_code(&v), "conflict");

    // Review cannot be completed before the run ends.
    let (s, _) = app
        .post(
            &format!("/api/runs/{run}/review/complete"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT);

    // 結束本節課並回顧 — idempotent.
    let (s, ended) = app
        .post(&format!("/api/runs/{run}/end"), Some(&t), json!({}))
        .await;
    assert_eq!(s, StatusCode::OK, "{ended}");
    assert!(ended["ended_at"].is_string());
    let (s, again) = app
        .post(&format!("/api/runs/{run}/end"), Some(&t), json!({}))
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(again["ended_at"], ended["ended_at"]);

    // Completing the review needs nothing filled in (§6.4), and keeps the first time.
    let (s, done) = app
        .post(
            &format!("/api/runs/{run}/review/complete"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{done}");
    assert!(done["review_completed_at"].is_string());
    assert_eq!(done["good_count"], 0);
    let (_, done2) = app
        .post(
            &format!("/api/runs/{run}/review/complete"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(done2["review_completed_at"], done["review_completed_at"]);

    // After the end a new run can start; lists show the open one first.
    let (s, second) = app
        .post(&format!("/api/lessons/{lesson}/runs"), Some(&t), json!({}))
        .await;
    assert_eq!(s, StatusCode::OK, "{second}");
    let (_, mine) = app.get("/api/runs", Some(&t)).await;
    let ids: Vec<i64> = mine
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_i64().unwrap())
        .collect();
    assert_eq!(ids, vec![second["id"].as_i64().unwrap(), run]);
    let (_, for_lesson) = app
        .get(&format!("/api/lessons/{lesson}/runs"), Some(&t))
        .await;
    assert_eq!(for_lesson.as_array().unwrap().len(), 2);

    let (s, _) = app.get("/api/runs", None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

// ---------------------------------------------------------------- sync

#[tokio::test]
async fn same_device_event_twice_is_one_bookmark() {
    let app = test_app!();
    let Setup { t, run, .. } = setup(&app, "T-R2").await;

    let ts = "2026-11-12T09:41:05.123+08:00";
    let v = sync(
        &app,
        &t,
        run,
        json!([create_op("op-1", "ev-1", ts, "good")]),
    )
    .await;
    assert_eq!(statuses(&v), vec!["applied"]);
    let ev = &v["events"][0];
    assert_eq!(ev["device_event_id"], "ev-1");
    // Device time kept as sent (same instant), server receive time stored separately.
    assert_eq!(ev["device_ts"], "2026-11-12T01:41:05.123Z");
    assert!(ev["received_at"].is_string());
    assert_eq!(ev["device_type"], "desktop");
    assert_eq!(ev["via"], "console");
    assert_eq!(ev["behaviors"], json!([]));
    assert_eq!(ev["followup_text"], "");

    // Retry of the same op, a retry with a new op id and different content, and a duplicate
    // inside one batch: still one row, and the original content wins.
    let v = sync(
        &app,
        &t,
        run,
        json!([
            create_op("op-1", "ev-1", ts, "good"),
            create_op("op-1b", "ev-1", "2026-11-12T02:00:00Z", "stuck"),
            create_op("op-2", "ev-2", "2026-11-12T01:50:00Z", "stuck"),
            create_op("op-2", "ev-2", "2026-11-12T01:50:00Z", "stuck"),
        ]),
    )
    .await;
    assert_eq!(
        statuses(&v),
        vec!["duplicate", "duplicate", "applied", "duplicate"]
    );
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM record_events WHERE lesson_run_id = $1")
            .bind(run)
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(count, 2);
    let events = v["events"].as_array().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["polarity"], "good");
    assert_eq!(events[0]["device_ts"], "2026-11-12T01:41:05.123Z");
    assert_eq!(events[1]["device_event_id"], "ev-2");

    // Malformed ops are rejected one by one without blocking the rest.
    let v = sync(
        &app,
        &t,
        run,
        json!([
            { "op_id": "bad-1", "kind": "create", "device_event_id": "ev-x", "device_ts": "yesterday",
              "polarity": "good", "device_type": "desktop" },
            { "op_id": "bad-2", "kind": "create", "device_event_id": "ev-y",
              "device_ts": "2026-11-12T01:00:00Z", "polarity": "great", "device_type": "desktop" },
            create_op("op-3", "ev-3", "2026-11-12T01:55:00Z", "good"),
        ]),
    )
    .await;
    assert_eq!(statuses(&v), vec!["rejected", "rejected", "applied"]);
    assert_eq!(v["results"][0]["op_id"], "bad-1");
    assert_eq!(v["results"][1]["code"], "bad_request");
    assert_eq!(v["events"].as_array().unwrap().len(), 3);

    let (_, run_v) = app.get(&format!("/api/runs/{run}"), Some(&t)).await;
    assert_eq!(run_v["good_count"], 2);
    assert_eq!(run_v["stuck_count"], 1);

    let too_many: Vec<Value> = (0..501)
        .map(|i| {
            create_op(
                &format!("o{i}"),
                &format!("e{i}"),
                "2026-11-12T01:00:00Z",
                "good",
            )
        })
        .collect();
    let (s, _) = app
        .post(
            &format!("/api/runs/{run}/events/sync"),
            Some(&t),
            json!({ "ops": too_many }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn edits_undo_and_delete_append_revisions() {
    let app = test_app!();
    let Setup { t, run, .. } = setup(&app, "T-R3").await;
    sync(
        &app,
        &t,
        run,
        json!([
            create_op("c1", "ev-1", "2026-11-12T01:00:00Z", "good"),
            create_op("c2", "ev-2", "2026-11-12T01:01:00Z", "stuck"),
            create_op("c3", "ev-3", "2026-11-12T01:02:00Z", "good"),
        ]),
    )
    .await;

    // Switch 👍 → ⚠ and add a sentence, then retry the same op.
    let edit = json!({ "op_id": "e1", "kind": "edit", "device_event_id": "ev-1",
                       "polarity": "stuck", "note": "  例句太難  ", "at": "2026-11-12T01:03:00Z" });
    let v = sync(&app, &t, run, json!([edit.clone()])).await;
    assert_eq!(statuses(&v), vec!["applied"]);
    assert_eq!(v["events"][0]["polarity"], "stuck");
    assert_eq!(v["events"][0]["note"], "例句太難");
    assert_eq!(v["events"][0]["revision_count"], 1);
    let v = sync(&app, &t, run, json!([edit])).await;
    assert_eq!(statuses(&v), vec!["duplicate"]);
    // An edit that changes nothing records nothing.
    let v = sync(
        &app,
        &t,
        run,
        json!([{ "op_id": "e2", "kind": "edit", "device_event_id": "ev-1", "note": "例句太難" }]),
    )
    .await;
    assert_eq!(statuses(&v), vec!["applied"]);
    assert_eq!(
        revisions(&app, run, "ev-1").await,
        vec![("edit".to_string(), "teacher".to_string())]
    );
    let (before, after, device_ts): (Value, Value, Option<chrono::DateTime<chrono::Utc>>) =
        sqlx::query_as(
            "SELECT rv.before_value, rv.after_value, rv.device_ts FROM record_event_revisions rv
               JOIN record_events e ON e.id = rv.record_event_id
              WHERE e.lesson_run_id = $1 AND e.device_event_id = 'ev-1'",
        )
        .bind(run)
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(
        before,
        json!({ "polarity": "good", "note": "", "deleted": false })
    );
    assert_eq!(
        after,
        json!({ "polarity": "stuck", "note": "例句太難", "deleted": false })
    );
    assert_eq!(device_ts.unwrap().to_rfc3339(), "2026-11-12T01:03:00+00:00");

    // Undo the last tap; delete a mis-tap. Both are soft and hidden from the teacher.
    let v = sync(
        &app,
        &t,
        run,
        json!([
            { "op_id": "u1", "kind": "undo", "device_event_id": "ev-3" },
            { "op_id": "d1", "kind": "delete", "device_event_id": "ev-2" },
            { "op_id": "u1", "kind": "undo", "device_event_id": "ev-3" },
            { "op_id": "e3", "kind": "edit", "device_event_id": "ev-2", "note": "改" },
            { "op_id": "u2", "kind": "undo", "device_event_id": "ev-404" },
        ]),
    )
    .await;
    assert_eq!(
        statuses(&v),
        vec!["applied", "applied", "duplicate", "rejected", "rejected"]
    );
    assert_eq!(v["results"][3]["code"], "deleted");
    assert_eq!(v["results"][4]["code"], "not_found");
    let ids: Vec<&str> = v["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["device_event_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["ev-1"]);
    assert_eq!(
        revisions(&app, run, "ev-3").await,
        vec![("undo".to_string(), "teacher".to_string())]
    );
    assert_eq!(
        revisions(&app, run, "ev-2").await,
        vec![("delete".to_string(), "teacher".to_string())]
    );
    // Original events are kept (soft delete).
    let kept: Vec<(String, String, bool)> = sqlx::query_as(
        "SELECT device_event_id, polarity, deleted_at IS NOT NULL FROM record_events
          WHERE lesson_run_id = $1 ORDER BY device_ts",
    )
    .bind(run)
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert_eq!(
        kept,
        vec![
            ("ev-1".into(), "stuck".into(), false),
            ("ev-2".into(), "stuck".into(), true),
            ("ev-3".into(), "good".into(), true),
        ]
    );

    // Teachers can still correct bookmarks after the run ended (課後更正).
    app.post(&format!("/api/runs/{run}/end"), Some(&t), json!({}))
        .await;
    let v = sync(
        &app,
        &t,
        run,
        json!([
            { "op_id": "e4", "kind": "edit", "device_event_id": "ev-1", "polarity": "good" },
            create_op("late", "ev-late", "2026-11-12T01:30:00Z", "good"),
        ]),
    )
    .await;
    assert_eq!(statuses(&v), vec!["applied", "applied"]);
    assert!(v["ended_at"].is_string());
}

// ---------------------------------------------------------------- 課後補寫

#[tokio::test]
async fn review_fields_are_optional_and_versioned() {
    let app = test_app!();
    let Setup { t, lesson, run } = setup(&app, "T-R4").await;
    sync(
        &app,
        &t,
        run,
        json!([
            create_op("c1", "ev-1", "2026-11-12T01:00:00Z", "stuck"),
            create_op("c2", "ev-2", "2026-11-12T01:05:00Z", "good"),
        ]),
    )
    .await;
    app.post(&format!("/api/runs/{run}/end"), Some(&t), json!({}))
        .await;
    let (_, detail) = app.get(&format!("/api/runs/{run}"), Some(&t)).await;
    let step2 = detail["plan"]["steps"][1]["id"].as_i64().unwrap();
    let path = format!("/api/runs/{run}/events/ev-1/review");

    let body = json!({
        "activity_step_id": step2,
        "behaviors": ["discussing", "listening", "discussing"],
        "handling": "add_example",
        "followup_text": "  多給兩個生活例句後就能改寫  "
    });
    let (s, v) = app.put(&path, Some(&t), body.clone()).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["activity_step_id"], step2);
    assert_eq!(v["behaviors"], json!(["listening", "discussing"]));
    assert_eq!(v["handling"], "add_example");
    assert_eq!(v["followup_text"], "多給兩個生活例句後就能改寫");
    assert!(v["followup_at"].is_string());
    assert_eq!(v["revision_count"], 1);
    // Saving the same thing again adds no revision.
    let (_, v) = app.put(&path, Some(&t), body).await;
    assert_eq!(v["revision_count"], 1);
    // Clearing everything is allowed (全部可留白) and is a new revision.
    let (s, v) = app
        .put(
            &path,
            Some(&t),
            json!({ "activity_step_id": null, "behaviors": [], "handling": null, "followup_text": "" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["handling"], Value::Null);
    assert_eq!(v["revision_count"], 2);
    assert_eq!(
        revisions(&app, run, "ev-1").await,
        vec![
            ("review".to_string(), "teacher".to_string()),
            ("review".to_string(), "teacher".to_string())
        ]
    );
    // An empty body is fine too.
    let (s, _) = app
        .put(
            &format!("/api/runs/{run}/events/ev-2/review"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::OK);

    // Validation.
    let (s, v) = app
        .put(&path, Some(&t), json!({ "behaviors": ["sleeping"] }))
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    let (s, _) = app
        .put(&path, Some(&t), json!({ "handling": "shout" }))
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = app
        .put(
            &path,
            Some(&t),
            json!({ "followup_text": "字".repeat(2001) }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    // A step from another lesson cannot be linked.
    let unit2 = app.create_unit(&t, "其他單元").await;
    let other = app.create_lesson(&t, unit2, "其他課").await;
    let (_, ov) = app
        .post(
            &format!("/api/lessons/{other}/versions"),
            Some(&t),
            json!({ "objective": "", "steps": [{ "position": 1, "content": "x" }] }),
        )
        .await;
    let other_step: i64 =
        sqlx::query_scalar("SELECT id FROM lesson_steps WHERE lesson_version_id = $1")
            .bind(ov["id"].as_i64().unwrap())
            .fetch_one(&app.db)
            .await
            .unwrap();
    let (s, _) = app
        .put(&path, Some(&t), json!({ "activity_step_id": other_step }))
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    // Steps of an older plan version of the same lesson are still accepted.
    app.post(
        &format!("/api/lessons/{lesson}/versions"),
        Some(&t),
        json!({ "objective": "v2", "steps": [] }),
    )
    .await;
    let (s, _) = app
        .put(&path, Some(&t), json!({ "activity_step_id": step2 }))
        .await;
    assert_eq!(s, StatusCode::OK);

    // Deleted bookmarks cannot be reviewed; unknown ones are 404.
    sync(
        &app,
        &t,
        run,
        json!([{ "op_id": "d1", "kind": "delete", "device_event_id": "ev-2" }]),
    )
    .await;
    let (s, _) = app
        .put(
            &format!("/api/runs/{run}/events/ev-2/review"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, _) = app
        .put(
            &format!("/api/runs/{run}/events/nope/review"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------- ownership

#[tokio::test]
async fn teachers_only_reach_their_own_runs() {
    let app = test_app!();
    let a = setup(&app, "T-OWN-A").await;
    sync(
        &app,
        &a.t,
        a.run,
        json!([create_op("c1", "ev-1", "2026-11-12T01:00:00Z", "good")]),
    )
    .await;
    let b = app.teacher("T-OWN-B", "general").await;
    let run = a.run;

    let not_found = [
        (Method::GET, format!("/api/runs/{run}")),
        (Method::POST, format!("/api/runs/{run}/end")),
        (Method::POST, format!("/api/runs/{run}/events/sync")),
        (Method::PUT, format!("/api/runs/{run}/events/ev-1/review")),
        (Method::POST, format!("/api/runs/{run}/review/complete")),
        (Method::POST, format!("/api/runs/{run}/pairing")),
        (Method::DELETE, format!("/api/runs/{run}/pairing")),
        (Method::GET, format!("/api/lessons/{}/runs", a.lesson)),
        (Method::POST, format!("/api/lessons/{}/runs", a.lesson)),
    ];
    for (method, path) in not_found {
        let body = if path.ends_with("/sync") {
            json!({ "ops": [create_op("x", "ev-b", "2026-11-12T01:00:00Z", "stuck")] })
        } else {
            json!({})
        };
        let (s, v) = app
            .send(
                method.clone(),
                &path,
                Some(&b),
                Some("application/json"),
                body.to_string().into_bytes(),
            )
            .await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{method} {path}: {v}");
        assert_eq!(err_code(&v), "not_found");
    }
    let (_, mine) = app.get("/api/runs", Some(&b)).await;
    assert_eq!(mine, json!([]));
    // Nothing leaked into A's run.
    let (_, v) = app.get(&format!("/api/runs/{run}"), Some(&a.t)).await;
    assert_eq!(v["ended_at"], Value::Null);
    assert_eq!(v["events"].as_array().unwrap().len(), 1);
}

// ---------------------------------------------------------------- QR pairing

async fn new_pairing(app: &common::TestApp, t: &str, run: i64) -> String {
    let (s, v) = app
        .post(&format!("/api/runs/{run}/pairing"), Some(t), json!({}))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert!(v["expires_at"].is_string());
    assert!(v["pairing_id"].is_i64());
    v["code"].as_str().unwrap().to_string()
}

async fn pair(app: &common::TestApp, code: &str) -> (StatusCode, Value) {
    app.post(
        "/api/remote/pair",
        None,
        json!({ "code": code, "device_type": "phone" }),
    )
    .await
}

#[tokio::test]
async fn qr_pairing_is_single_use_expiring_and_revocable() {
    let app = test_app!();
    let Setup { t, run, .. } = setup(&app, "T-QR").await;

    // Only the hash is stored; expiry is 10 minutes.
    let code = new_pairing(&app, &t, run).await;
    assert_eq!(code.len(), 64);
    let (stored, minutes): (String, f64) = sqlx::query_as(
        "SELECT code_hash, EXTRACT(EPOCH FROM expires_at - created_at)::float8 / 60
           FROM run_pairings WHERE lesson_run_id = $1",
    )
    .bind(run)
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_ne!(stored, code);
    assert_eq!(stored, manna_api::auth::sha256_hex(&code));
    assert!((minutes - 10.0).abs() < 0.01, "{minutes}");

    let (s, v) = pair(&app, &code).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["token"].as_str().unwrap().len(), 64);
    assert_eq!(v["run"]["run_id"], run);
    assert_eq!(v["run"]["lesson_title"], "第 1 節");
    assert_eq!(v["run"]["events"], json!([]));
    let (_, detail) = app.get(&format!("/api/runs/{run}"), Some(&t)).await;
    assert_eq!(detail["remotes"][0]["device_type"], "phone");

    // Single use.
    let (s, v) = pair(&app, &code).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED, "{v}");
    assert!(v["error"]["message"]
        .as_str()
        .unwrap()
        .contains("只能用一次"));

    // Unknown code / bad device type.
    let (s, _) = pair(&app, "not-a-code").await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _) = app
        .post(
            "/api/remote/pair",
            None,
            json!({ "code": code, "device_type": "toaster" }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // Expired.
    let expired = new_pairing(&app, &t, run).await;
    sqlx::query(
        "UPDATE run_pairings SET expires_at = now() - interval '1 second' WHERE code_hash = $1",
    )
    .bind(manna_api::auth::sha256_hex(&expired))
    .execute(&app.db)
    .await
    .unwrap();
    let (s, v) = pair(&app, &expired).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert!(v["error"]["message"].as_str().unwrap().contains("過期"));

    // A new QR code retires the previous unused one.
    let older = new_pairing(&app, &t, run).await;
    let newer = new_pairing(&app, &t, run).await;
    let (s, v) = pair(&app, &older).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert!(v["error"]["message"].as_str().unwrap().contains("撤銷"));

    // Revoke: unused codes and paired remotes stop working.
    let (s, v) = app
        .send(
            Method::DELETE,
            &format!("/api/runs/{run}/pairing"),
            Some(&t),
            None,
            vec![],
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["revoked_pairings"], 1);
    assert_eq!(v["revoked_remotes"], 1);
    let (s, _) = pair(&app, &newer).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (_, detail) = app.get(&format!("/api/runs/{run}"), Some(&t)).await;
    assert_eq!(detail["remotes"], json!([]));

    // Run ended: a code made before the end cannot pair, and no new code can be made.
    let before_end = new_pairing(&app, &t, run).await;
    app.post(&format!("/api/runs/{run}/end"), Some(&t), json!({}))
        .await;
    let (s, v) = pair(&app, &before_end).await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    let (s, _) = app
        .post(&format!("/api/runs/{run}/pairing"), Some(&t), json!({}))
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
}

#[tokio::test]
async fn remote_token_is_scoped_to_its_run_and_own_bookmarks() {
    let app = test_app!();
    let Setup { t, run, lesson } = setup(&app, "T-REM").await;
    // Another run (another lesson of the same teacher) must stay untouched.
    let (_, lv) = app.get(&format!("/api/lessons/{lesson}"), Some(&t)).await;
    let other_lesson = app
        .create_lesson(&t, lv["unit_id"].as_i64().unwrap(), "第 2 節")
        .await;
    let (_, other_run) = app
        .post(
            &format!("/api/lessons/{other_lesson}/runs"),
            Some(&t),
            json!({}),
        )
        .await;
    let other_run = other_run["id"].as_i64().unwrap();

    let code = new_pairing(&app, &t, run).await;
    let (_, paired) = pair(&app, &code).await;
    let remote = paired["token"].as_str().unwrap().to_string();

    // Teacher taps one bookmark on the console.
    sync(
        &app,
        &t,
        run,
        json!([create_op(
            "c1",
            "ev-console",
            "2026-11-12T01:00:00Z",
            "good"
        )]),
    )
    .await;

    // Remote: add (forced via=remote) and see only its own bookmarks.
    let remote_sync = |ops: Value| {
        let app = &app;
        let remote = remote.clone();
        async move {
            let (s, v) = app
                .post(
                    "/api/remote/events/sync",
                    Some(&remote),
                    json!({ "ops": ops }),
                )
                .await;
            (s, v)
        }
    };
    let (s, v) = remote_sync(json!([
        { "op_id": "r1", "kind": "create", "device_event_id": "ev-phone",
          "device_ts": "2026-11-12T01:01:00Z", "polarity": "stuck", "device_type": "phone" },
        { "op_id": "r1b", "kind": "create", "device_event_id": "ev-phone-2",
          "device_ts": "2026-11-12T01:02:00Z", "polarity": "good", "device_type": "phone" }
    ]))
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(statuses(&v), vec!["applied", "applied"]);
    let ids: Vec<&str> = v["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["device_event_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["ev-phone", "ev-phone-2"]);
    assert_eq!(v["events"][0]["via"], "remote");
    assert_eq!(v["events"][0]["device_type"], "phone");

    // Remote cannot edit or delete anything, nor undo the console's bookmark.
    let (_, v) = remote_sync(json!([
        { "op_id": "r2", "kind": "edit", "device_event_id": "ev-phone", "polarity": "good" },
        { "op_id": "r3", "kind": "delete", "device_event_id": "ev-phone" },
        { "op_id": "r4", "kind": "undo", "device_event_id": "ev-console" },
        { "op_id": "r5", "kind": "undo", "device_event_id": "ev-phone-2" }
    ]))
    .await;
    assert_eq!(
        statuses(&v),
        vec!["rejected", "rejected", "rejected", "applied"]
    );
    assert_eq!(v["results"][0]["code"], "forbidden");
    assert_eq!(v["results"][2]["code"], "forbidden");
    assert_eq!(
        revisions(&app, run, "ev-phone-2").await,
        vec![("undo".to_string(), "remote".to_string())]
    );

    // The teacher sees the remote's bookmark in the run; the other run is untouched.
    let (_, detail) = app.get(&format!("/api/runs/{run}"), Some(&t)).await;
    let ids: Vec<&str> = detail["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["device_event_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["ev-console", "ev-phone"]);
    let (_, other) = app.get(&format!("/api/runs/{other_run}"), Some(&t)).await;
    assert_eq!(other["events"], json!([]));

    // The remote token is not a teacher session, and vice versa.
    for path in ["/api/me", "/api/runs", &format!("/api/runs/{run}")] {
        let (s, _) = app.get(path, Some(&remote)).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED, "{path}");
    }
    let (s, _) = app
        .post(
            &format!("/api/runs/{run}/events/sync"),
            Some(&remote),
            json!({ "ops": [] }),
        )
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _) = app.get("/api/remote/run", Some(&t)).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _) = app.get("/api/remote/run", None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, v) = app.get("/api/remote/run", Some(&remote)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["run_id"], run);
    assert_eq!(v["events"].as_array().unwrap().len(), 1);

    // After the end: bookmarks tapped before the end still sync (offline phone), new ones and
    // undo do not.
    app.post(&format!("/api/runs/{run}/end"), Some(&t), json!({}))
        .await;
    let ended: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("SELECT ended_at FROM lesson_runs WHERE id = $1")
            .bind(run)
            .fetch_one(&app.db)
            .await
            .unwrap();
    let before_end = (ended - chrono::Duration::minutes(1)).to_rfc3339();
    let after_end = (ended + chrono::Duration::minutes(30)).to_rfc3339();
    let (s, v) = remote_sync(json!([
        { "op_id": "r6", "kind": "create", "device_event_id": "ev-offline",
          "device_ts": before_end, "polarity": "good", "device_type": "phone" },
        { "op_id": "r7", "kind": "create", "device_event_id": "ev-too-late",
          "device_ts": after_end, "polarity": "good", "device_type": "phone" },
        { "op_id": "r8", "kind": "undo", "device_event_id": "ev-phone" },
        // a retried op that was applied before the end stays a duplicate
        { "op_id": "r5", "kind": "undo", "device_event_id": "ev-phone-2" }
    ]))
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(
        statuses(&v),
        vec!["applied", "rejected", "rejected", "duplicate"]
    );
    assert_eq!(v["results"][1]["code"], "run_ended");
    assert_eq!(v["results"][2]["code"], "run_ended");
    assert!(v["ended_at"].is_string());

    // Once the late-sync window closes the token is dead.
    sqlx::query("UPDATE lesson_runs SET ended_at = now() - interval '13 hours' WHERE id = $1")
        .bind(run)
        .execute(&app.db)
        .await
        .unwrap();
    let (s, v) = app.get("/api/remote/run", Some(&remote)).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED, "{v}");
    let (s, _) = remote_sync(json!([])).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn revoked_remote_token_stops_working() {
    let app = test_app!();
    let Setup { t, run, .. } = setup(&app, "T-REV").await;
    let code = new_pairing(&app, &t, run).await;
    let (_, paired) = pair(&app, &code).await;
    let remote = paired["token"].as_str().unwrap().to_string();
    let (s, _) = app.get("/api/remote/run", Some(&remote)).await;
    assert_eq!(s, StatusCode::OK);
    app.send(
        Method::DELETE,
        &format!("/api/runs/{run}/pairing"),
        Some(&t),
        None,
        vec![],
    )
    .await;
    let (s, v) = app.get("/api/remote/run", Some(&remote)).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert_eq!(err_code(&v), "unauthorized");
}

// ---------------------------------------------------------------- scaffold

#[tokio::test]
async fn record_scaffold_is_registered() {
    let app = test_app!();
    let t = app.teacher("T-SC", "general").await;
    let (name, category): (String, String) =
        sqlx::query_as("SELECT name, category FROM scaffolds WHERE scaffold_code = 'SC-RECORD'")
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(name, "課中書籤與課後補寫");
    assert_eq!(category, "low_burden_evidence");
    for (action, trigger) in [
        ("show", ""),
        ("use", ""),
        ("expand", "qr"),
        ("expand", "pip"),
    ] {
        let (s, v) = app
            .post(
                "/api/scaffold-events",
                Some(&t),
                json!({ "scaffold_code": "SC-RECORD", "action": action, "trigger": trigger }),
            )
            .await;
        assert_eq!(s, StatusCode::OK, "{v}");
    }
}
