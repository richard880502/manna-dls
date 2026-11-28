//! 備課庫：單元與課例的改名與刪除 (docs/API.md「備課庫：單元與課例改名、刪除」, #132). A delete is
//! soft (`deleted_at`): the unit or lesson and the worksheets under it leave every list, while
//! the rows the research record is made of stay (#131). Only the owner can rename or delete.

#[macro_use]
mod common;

use axum::http::StatusCode;
use serde_json::{json, Value};

async fn worksheet_for(app: &common::TestApp, t: &str, lesson: Option<i64>) -> i64 {
    let body = match lesson {
        Some(id) => json!({ "lesson_id": id }),
        None => json!({}),
    };
    let (s, v) = app.post("/api/worksheets", Some(t), body).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    v["id"].as_i64().unwrap()
}

fn ids(list: &Value) -> Vec<i64> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|v| v["id"].as_i64().unwrap())
        .collect()
}

async fn count(app: &common::TestApp, sql: &str, id: i64) -> i64 {
    sqlx::query_scalar(sql)
        .bind(id)
        .fetch_one(&app.db)
        .await
        .unwrap()
}

async fn logged(app: &common::TestApp, event: &str) -> Vec<Value> {
    sqlx::query_scalar("SELECT payload FROM usage_logs WHERE event = $1 ORDER BY id")
        .bind(event)
        .fetch_all(&app.db)
        .await
        .unwrap()
}

#[tokio::test]
async fn only_the_owner_renames_or_deletes_a_unit_or_lesson() {
    let app = test_app!();
    let a = app.teacher("T-DEL-A", "general").await;
    let b = app.teacher("T-DEL-B", "general").await;
    let unit = app.create_unit(&a, "L3 倒裝句").await;
    let lesson = app.create_lesson(&a, unit, "第 1 節").await;

    // Someone else's unit or lesson is not found, whatever the request.
    for path in [
        format!("/api/units/{unit}"),
        format!("/api/lessons/{lesson}"),
    ] {
        let (s, _) = app.delete(&path, Some(&b)).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "DELETE {path}");
        let (s, _) = app.put(&path, Some(&b), json!({ "title": "別人的" })).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "PUT {path}");
        let (s, _) = app.get(&format!("{path}/delete-preview"), Some(&b)).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "preview {path}");
        let (s, _) = app.delete(&path, None).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED, "no login {path}");
    }
    let (_, mine) = app.get("/api/units", Some(&a)).await;
    assert_eq!(ids(&mine), vec![unit]);
    assert_eq!(mine[0]["title"], "L3 倒裝句");
    assert_eq!(mine[0]["lesson_count"], 1);
    assert!(logged(&app, "unit_deleted").await.is_empty());
    assert!(logged(&app, "lesson_deleted").await.is_empty());

    // The owner renames both (a blank name is refused, the old one stays).
    let (s, v) = app
        .put(
            &format!("/api/units/{unit}"),
            Some(&a),
            json!({ "title": "  L3 Not only… but also  " }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["title"], "L3 Not only… but also");
    let (s, _) = app
        .put(
            &format!("/api/units/{unit}"),
            Some(&a),
            json!({ "title": " " }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, v) = app
        .put(
            &format!("/api/lessons/{lesson}"),
            Some(&a),
            json!({ "title": "第 1 節：句型" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["title"], "第 1 節：句型");
    let (_, lessons) = app
        .get(&format!("/api/units/{unit}/lessons"), Some(&a))
        .await;
    assert_eq!(lessons[0]["title"], "第 1 節：句型");
}

#[tokio::test]
async fn a_deleted_lesson_and_its_worksheets_leave_the_lists_and_the_research_record_stays() {
    let app = test_app!();
    let t = app.teacher("T-DEL-L", "general").await;
    let unit = app.create_unit(&t, "單元").await;
    let gone = app.create_lesson(&t, unit, "要刪的課").await;
    let kept = app.create_lesson(&t, unit, "留著的課").await;
    let (s, v) = app
        .post(
            &format!("/api/lessons/{gone}/versions"),
            Some(&t),
            json!({ "objective": "學生能用倒裝句", "steps": [] }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, run) = app
        .post(&format!("/api/lessons/{gone}/runs"), Some(&t), json!({}))
        .await;
    assert_eq!(s, StatusCode::OK, "{run}");
    let linked = worksheet_for(&app, &t, Some(gone)).await;
    let other_lesson = worksheet_for(&app, &t, Some(kept)).await;
    let standalone = worksheet_for(&app, &t, None).await;

    let (s, preview) = app
        .get(&format!("/api/lessons/{gone}/delete-preview"), Some(&t))
        .await;
    assert_eq!(s, StatusCode::OK, "{preview}");
    assert_eq!(preview, json!({ "worksheets": 1, "runs": 1 }));

    let (s, v) = app.delete(&format!("/api/lessons/{gone}"), Some(&t)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v, json!({ "ok": true, "worksheets_deleted": 1 }));

    // Gone from every list and page.
    let (_, lessons) = app
        .get(&format!("/api/units/{unit}/lessons"), Some(&t))
        .await;
    assert_eq!(ids(&lessons), vec![kept]);
    let (_, u) = app.get(&format!("/api/units/{unit}"), Some(&t)).await;
    assert_eq!(u["lesson_count"], 1);
    for path in [
        format!("/api/lessons/{gone}"),
        format!("/api/lessons/{gone}/delete-preview"),
        format!("/api/worksheets/{linked}"),
    ] {
        let (s, _) = app.get(&path, Some(&t)).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{path}");
    }
    let (_, worksheets) = app.get("/api/worksheets", Some(&t)).await;
    let mut listed = ids(&worksheets);
    listed.sort_unstable();
    assert_eq!(listed, vec![other_lesson, standalone]);
    let (_, runs) = app.get("/api/runs", Some(&t)).await;
    assert_eq!(runs, json!([]));
    // A second delete finds nothing.
    let (s, _) = app.delete(&format!("/api/lessons/{gone}"), Some(&t)).await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // The research record stays: the rows are only marked.
    let deleted = "SELECT COUNT(*) FROM lessons WHERE id = $1 AND deleted_at IS NOT NULL";
    assert_eq!(count(&app, deleted, gone).await, 1);
    let ws_deleted = "SELECT COUNT(*) FROM worksheets WHERE id = $1 AND deleted_at IS NOT NULL";
    assert_eq!(count(&app, ws_deleted, linked).await, 1);
    assert_eq!(count(&app, ws_deleted, other_lesson).await, 0);
    let versions = "SELECT COUNT(*) FROM worksheet_versions WHERE worksheet_id = $1";
    assert!(count(&app, versions, linked).await >= 1);
    let plans = "SELECT COUNT(*) FROM lesson_versions WHERE lesson_id = $1";
    assert_eq!(count(&app, plans, gone).await, 1);
    let lesson_runs = "SELECT COUNT(*) FROM lesson_runs WHERE lesson_id = $1";
    assert_eq!(count(&app, lesson_runs, gone).await, 1);
    let created = "SELECT COUNT(*) FROM usage_logs
                    WHERE event = 'lesson_created' AND (payload->>'lesson_id')::bigint = $1";
    assert_eq!(count(&app, created, gone).await, 1);
    assert_eq!(
        logged(&app, "lesson_deleted").await,
        vec![json!({ "lesson_id": gone, "unit_id": unit, "worksheet_ids": [linked] })]
    );
}

#[tokio::test]
async fn a_deleted_unit_takes_its_lessons_and_worksheets_out_of_every_list_and_keeps_its_core() {
    let app = test_app!();
    let t = app.teacher("T-DEL-U", "general").await;
    let (s, class) = app
        .post(
            "/api/classes",
            Some(&t),
            json!({ "name": "高一 3 班", "class_profile": "" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{class}");
    let class = class["id"].as_i64().unwrap();
    let (s, v) = app
        .post(
            "/api/units",
            Some(&t),
            json!({ "title": "要刪的單元", "class_id": class }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let unit = v["id"].as_i64().unwrap();
    let kept_unit = app.create_unit(&t, "留著的單元").await;
    let first = app.create_lesson(&t, unit, "第 1 節").await;
    let (s, v) = app
        .post(
            &format!("/api/units/{unit}/lessons"),
            Some(&t),
            json!({ "title": "第 2 節", "entry": "home" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let second = v["id"].as_i64().unwrap();
    let ws = worksheet_for(&app, &t, Some(second)).await;
    let standalone = worksheet_for(&app, &t, None).await;
    let (s, _) = app.get(&format!("/api/units/{unit}/core"), Some(&t)).await;
    assert_eq!(s, StatusCode::OK);
    let (s, core) = app
        .put(
            &format!("/api/units/{unit}/core/draft"),
            Some(&t),
            json!({ "focuses": [{ "position": 1, "big_idea": "倒裝句的語氣", "responses": {
                "Q1": { "answer": "學生能用 Not only 句型", "status": "filled", "content_source": "self" }
            }}]}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{core}");

    let (s, preview) = app
        .get(&format!("/api/units/{unit}/delete-preview"), Some(&t))
        .await;
    assert_eq!(s, StatusCode::OK, "{preview}");
    assert_eq!(preview, json!({ "lessons": 2, "worksheets": 1, "runs": 0 }));

    let (s, v) = app.delete(&format!("/api/units/{unit}"), Some(&t)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(
        v,
        json!({ "ok": true, "lessons_deleted": 2, "worksheets_deleted": 1 })
    );

    // Gone from 備課庫, the class, 我的作品 and every page under it.
    let (_, units) = app.get("/api/units", Some(&t)).await;
    assert_eq!(ids(&units), vec![kept_unit]);
    for path in [
        format!("/api/units/{unit}"),
        format!("/api/units/{unit}/lessons"),
        format!("/api/units/{unit}/core"),
        format!("/api/units/{unit}/delete-preview"),
        format!("/api/lessons/{first}"),
        format!("/api/lessons/{second}"),
        format!("/api/worksheets/{ws}"),
    ] {
        let (s, _) = app.get(&path, Some(&t)).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{path}");
    }
    let (_, detail) = app.get(&format!("/api/classes/{class}"), Some(&t)).await;
    assert_eq!(detail["units"], json!([]));
    assert_eq!(detail["unit_count"], 0);
    let (_, worksheets) = app.get("/api/worksheets", Some(&t)).await;
    assert_eq!(ids(&worksheets), vec![standalone]);
    let (_, stepwise) = app.get("/api/stepwise-lessons", Some(&t)).await;
    assert_eq!(stepwise, json!([]));
    let (s, _) = app.delete(&format!("/api/units/{unit}"), Some(&t)).await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // The research record stays: CoRe with its revisions, the lessons, the class assignment.
    let unit_deleted = "SELECT COUNT(*) FROM units WHERE id = $1 AND deleted_at IS NOT NULL";
    assert_eq!(count(&app, unit_deleted, unit).await, 1);
    let lessons_deleted =
        "SELECT COUNT(*) FROM lessons WHERE unit_id = $1 AND deleted_at IS NOT NULL";
    assert_eq!(count(&app, lessons_deleted, unit).await, 2);
    let focuses = "SELECT COUNT(*) FROM core_focuses f
                     JOIN core_versions v ON v.id = f.core_version_id
                    WHERE v.unit_id = $1 AND f.deleted_at IS NULL";
    assert_eq!(count(&app, focuses, unit).await, 1);
    let revisions = "SELECT COUNT(*) FROM core_response_revisions r
                       JOIN core_responses c ON c.id = r.core_response_id
                       JOIN core_focuses f ON f.id = c.core_focus_id
                       JOIN core_versions v ON v.id = f.core_version_id
                      WHERE v.unit_id = $1";
    assert!(count(&app, revisions, unit).await >= 1);
    let assigned = "SELECT COUNT(*) FROM class_units WHERE unit_id = $1 AND removed_at IS NULL";
    assert_eq!(count(&app, assigned, unit).await, 1);
    let mut lesson_ids = vec![first, second];
    lesson_ids.sort_unstable();
    let payload = logged(&app, "unit_deleted").await;
    assert_eq!(payload.len(), 1);
    let mut logged_lessons: Vec<i64> = payload[0]["lesson_ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_i64().unwrap())
        .collect();
    logged_lessons.sort_unstable();
    assert_eq!(payload[0]["unit_id"], unit);
    assert_eq!(logged_lessons, lesson_ids);
    assert_eq!(payload[0]["worksheet_ids"], json!([ws]));

    // The unit that stayed still works as before.
    let (s, preview) = app
        .get(&format!("/api/units/{kept_unit}/delete-preview"), Some(&t))
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(preview, json!({ "lessons": 0, "worksheets": 0, "runs": 0 }));
}
