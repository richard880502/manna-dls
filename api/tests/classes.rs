//! Phase 1.4 (docs/API.md「班級與備課庫」): classes, the design library (units assigned to
//! classes, many-to-many), runs per class, the class profile in AI context, and the migration of
//! legacy data into the default class 「我的班級」.

#[macro_use]
mod common;

use axum::http::{Method, StatusCode};
use serde_json::{json, Value};

fn err_code(v: &Value) -> &str {
    v["error"]["code"].as_str().unwrap_or("<none>")
}

fn ids(v: &Value) -> Vec<i64> {
    v.as_array()
        .unwrap_or_else(|| panic!("not an array: {v}"))
        .iter()
        .map(|c| c["id"].as_i64().unwrap())
        .collect()
}

fn unit_ids(detail: &Value) -> Vec<i64> {
    detail["units"]
        .as_array()
        .unwrap_or_else(|| panic!("no units: {detail}"))
        .iter()
        .map(|u| u["unit"]["id"].as_i64().unwrap())
        .collect()
}

fn class_names(unit: &Value) -> Vec<&str> {
    unit["classes"]
        .as_array()
        .unwrap_or_else(|| panic!("no classes: {unit}"))
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect()
}

fn labels(r: &Value) -> Vec<&str> {
    r["context"]
        .as_array()
        .unwrap_or_else(|| panic!("no context: {r}"))
        .iter()
        .map(|c| c["label"].as_str().unwrap())
        .collect()
}

fn context_preview<'a>(r: &'a Value, label: &str) -> &'a str {
    r["context"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["label"] == label)
        .unwrap_or_else(|| panic!("no {label} in {}", r["context"]))["preview"]
        .as_str()
        .unwrap()
}

async fn create_class(app: &common::TestApp, t: &str, body: Value) -> Value {
    let (s, v) = app.post("/api/classes", Some(t), body).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v
}

async fn class_id(app: &common::TestApp, t: &str, name: &str) -> i64 {
    create_class(app, t, json!({ "name": name, "class_profile": "" })).await["id"]
        .as_i64()
        .unwrap()
}

async fn assign(app: &common::TestApp, t: &str, class: i64, unit: i64) -> Value {
    let (s, v) = app
        .post(
            &format!("/api/classes/{class}/units"),
            Some(t),
            json!({ "unit_id": unit }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v
}

async fn start(app: &common::TestApp, t: &str, lesson: i64, body: Value) -> (StatusCode, Value) {
    app.post(&format!("/api/lessons/{lesson}/runs"), Some(t), body)
        .await
}

async fn end(app: &common::TestApp, t: &str, run: &Value) {
    let (s, v) = app
        .post(&format!("/api/runs/{}/end", run["id"]), Some(t), json!({}))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
}

async fn set_p6(app: &common::TestApp, t: &str, class_profile: &str) {
    let (s, v) = app
        .put(
            "/api/me/preferences",
            Some(t),
            json!({ "answers": {
                "experience": "3-9", "start_from": "goal", "hardest": ["ai_communication"],
                "ai_experience": "sometimes", "detail": "medium", "class_profile": class_profile
            }, "skipped": false, "source": "settings" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
}

async fn usage_events(app: &common::TestApp, event: &str) -> Vec<Value> {
    sqlx::query_scalar("SELECT payload FROM usage_logs WHERE event = $1 ORDER BY id")
        .bind(event)
        .fetch_all(&app.db)
        .await
        .unwrap()
}

/// The data-migration block of 0006_classes.sql, exactly as the migration runs it.
fn legacy_migration_sql() -> &'static str {
    let sql = include_str!("../migrations/0006_classes.sql");
    let start = sql
        .find("-- >>> legacy-data-migration")
        .expect("start marker");
    let end = sql
        .find("-- <<< legacy-data-migration")
        .expect("end marker");
    &sql[start..end]
}

// ---------------------------------------------------------------- legacy data migration

#[tokio::test]
async fn legacy_units_and_runs_move_into_the_default_class() {
    let app = test_app!();
    // Teacher A: the state before Phase 1.4 (units, lessons and runs, no classes).
    let a = app.teacher("T-LEG-A", "vocational").await;
    let (_, u1) = app
        .post(
            "/api/units",
            Some(&a),
            json!({ "title": "L1 舊單元", "class_profile": "高一：舊描述" }),
        )
        .await;
    let (_, u2) = app
        .post(
            "/api/units",
            Some(&a),
            json!({ "title": "L2 較新的單元", "class_profile": "  高二 207：最近的描述 " }),
        )
        .await;
    let (_, u3) = app
        .post(
            "/api/units",
            Some(&a),
            json!({ "title": "L3 沒寫班級特性", "class_profile": "" }),
        )
        .await;
    let (_, gone) = app
        .post(
            "/api/units",
            Some(&a),
            json!({ "title": "已刪除", "class_profile": "不該被採用" }),
        )
        .await;
    sqlx::query("UPDATE units SET deleted_at = now() WHERE id = $1")
        .bind(gone["id"].as_i64().unwrap())
        .execute(&app.db)
        .await
        .unwrap();
    let (u1, u2, u3) = (
        u1["id"].as_i64().unwrap(),
        u2["id"].as_i64().unwrap(),
        u3["id"].as_i64().unwrap(),
    );
    let lesson = app.create_lesson(&a, u1, "第 1 節").await;
    let (s, ended) = start(&app, &a, lesson, json!({})).await;
    assert_eq!(s, StatusCode::OK, "{ended}");
    assert_eq!(ended["class_id"], Value::Null, "no class before Phase 1.4");
    end(&app, &a, &ended).await;
    let (_, open) = start(&app, &a, lesson, json!({})).await;

    // Teacher B has no units; teacher C has units without any class profile.
    let b = app.teacher("T-LEG-B", "general").await;
    let c = app.teacher("T-LEG-C", "general").await;
    let (_, cu) = app
        .post(
            "/api/units",
            Some(&c),
            json!({ "title": "C 的單元", "class_profile": "" }),
        )
        .await;

    for t in [&a, &b, &c] {
        let (_, classes) = app.get("/api/classes", Some(t)).await;
        assert_eq!(classes, json!([]));
    }

    sqlx::raw_sql(legacy_migration_sql())
        .execute(&app.db)
        .await
        .expect("legacy data migration");

    // A: one class 「我的班級」 with the most recent non-empty unit profile, the teacher's track.
    let (_, classes) = app.get("/api/classes", Some(&a)).await;
    let classes = classes.as_array().unwrap();
    assert_eq!(classes.len(), 1, "{classes:?}");
    let class = &classes[0];
    assert_eq!(class["name"], "我的班級");
    assert_eq!(class["class_profile"], "高二 207：最近的描述");
    assert_eq!(class["class_profile_source"], "migration");
    assert_eq!(class["school_track"], "vocational");
    assert_eq!(class["unit_count"], 3);
    assert_eq!(class["archived_at"], Value::Null);
    // The open legacy run is the class's next action.
    assert_eq!(class["next_action"]["kind"], "resume_run");
    assert_eq!(class["next_action"]["run_id"], open["id"]);
    let cid = class["id"].as_i64().unwrap();

    // All live units are assigned, oldest first; the deleted one is not.
    let (_, detail) = app.get(&format!("/api/classes/{cid}"), Some(&a)).await;
    assert_eq!(unit_ids(&detail), vec![u1, u2, u3]);
    assert_eq!(detail["units"][0]["lessons_taught"], 1);
    assert_eq!(detail["units"][0]["lessons_total"], 1);
    let (_, units) = app.get("/api/units", Some(&a)).await;
    for u in units.as_array().unwrap() {
        assert_eq!(class_names(u), vec!["我的班級"], "{u}");
    }

    // Existing runs belong to the class.
    let (_, runs) = app.get("/api/runs", Some(&a)).await;
    let runs = runs.as_array().unwrap();
    assert_eq!(runs.len(), 2);
    for r in runs {
        assert_eq!(r["class_id"], cid, "{r}");
        assert_eq!(r["class_name"], "我的班級");
    }
    // The unit's legacy profile is untouched.
    let (_, unit) = app.get(&format!("/api/units/{u1}"), Some(&a)).await;
    assert_eq!(unit["class_profile"], "高一：舊描述");

    // B: nothing. C: a class with an empty profile.
    let (_, classes) = app.get("/api/classes", Some(&b)).await;
    assert_eq!(classes, json!([]));
    let (_, classes) = app.get("/api/classes", Some(&c)).await;
    assert_eq!(classes[0]["name"], "我的班級");
    assert_eq!(classes[0]["class_profile"], "");
    assert_eq!(classes[0]["class_profile_source"], "none");
    assert_eq!(classes[0]["school_track"], "general");
    let (_, unit) = app.get(&format!("/api/units/{}", cu["id"]), Some(&c)).await;
    assert_eq!(class_names(&unit), vec!["我的班級"]);

    // Running it again changes nothing.
    sqlx::raw_sql(legacy_migration_sql())
        .execute(&app.db)
        .await
        .unwrap();
    let counts: (i64, i64) =
        sqlx::query_as("SELECT (SELECT COUNT(*) FROM classes), (SELECT COUNT(*) FROM class_units)")
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(counts, (2, 4));

    // After the migration a new run of the legacy lesson defaults to the class.
    end(&app, &a, &open).await;
    let (s, again) = start(&app, &a, lesson, json!({})).await;
    assert_eq!(s, StatusCode::OK, "{again}");
    assert_eq!(again["class_id"], cid);
}

// ---------------------------------------------------------------- classes CRUD

#[tokio::test]
async fn classes_crud_archive_and_reorder() {
    let app = test_app!();
    let t = app.teacher("T-CLS-1", "vocational").await;
    set_p6(&app, &t, "問卷：高一普通班，程度落差大").await;

    // Validation.
    let (s, v) = app
        .post("/api/classes", Some(&t), json!({ "name": "  " }))
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(err_code(&v), "bad_request");
    let (s, _) = app
        .post(
            "/api/classes",
            Some(&t),
            json!({ "name": "A", "school_track": "junior" }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = app
        .post("/api/classes", Some(&t), json!({ "name": "名".repeat(61) }))
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = app.get("/api/classes", None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // First class: track from the teacher, profile from P6 when omitted.
    let first = create_class(&app, &t, json!({ "name": " 高一 3 班 ", "grade": "高一" })).await;
    assert_eq!(first["name"], "高一 3 班");
    assert_eq!(first["grade"], "高一");
    assert_eq!(first["school_track"], "vocational");
    assert_eq!(first["class_profile"], "問卷：高一普通班，程度落差大");
    assert_eq!(first["class_profile_source"], "preference");
    assert_eq!(first["unit_count"], 0);
    assert_eq!(first["next_action"], json!({ "kind": "add_unit" }));
    assert_eq!(first["archived_at"], Value::Null);

    // Later classes do not copy P6; explicit values win.
    let second = create_class(&app, &t, json!({ "name": "高二 5 班" })).await;
    assert_eq!(second["class_profile"], "");
    assert_eq!(second["class_profile_source"], "none");
    let third = create_class(
        &app,
        &t,
        json!({
            "name": "閱讀選修", "school_track": "general", "course_label": "英文閱讀選修",
            "class_profile": " 跨班選修 ", "school_year": "115", "term": "上學期"
        }),
    )
    .await;
    assert_eq!(third["school_track"], "general");
    assert_eq!(third["course_label"], "英文閱讀選修");
    assert_eq!(third["class_profile"], "跨班選修");
    assert_eq!(third["class_profile_source"], "teacher");
    assert_eq!(third["school_year"], "115");
    assert_eq!(third["term"], "上學期");
    let (a, b, c) = (
        first["id"].as_i64().unwrap(),
        second["id"].as_i64().unwrap(),
        third["id"].as_i64().unwrap(),
    );

    let (_, list) = app.get("/api/classes", Some(&t)).await;
    assert_eq!(ids(&list), vec![a, b, c]);

    // Update: only the given fields change; what changed is logged.
    let (s, v) = app
        .put(
            &format!("/api/classes/{b}"),
            Some(&t),
            json!({ "name": "高二 5 班（社會組）", "class_profile": "多數學生敢開口" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["name"], "高二 5 班（社會組）");
    assert_eq!(v["class_profile"], "多數學生敢開口");
    assert_eq!(v["school_track"], "vocational");
    let (s, _) = app
        .put(
            &format!("/api/classes/{b}"),
            Some(&t),
            json!({ "name": "" }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = app
        .put(
            &format!("/api/classes/{b}"),
            Some(&t),
            json!({ "class_profile": "字".repeat(2001) }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let logged = usage_events(&app, "class_updated").await;
    assert_eq!(logged.len(), 1);
    assert_eq!(logged[0]["fields"], json!(["name", "class_profile"]));
    assert_eq!(usage_events(&app, "class_created").await.len(), 3);

    // Reorder: listed first, the rest keep their order.
    let (s, v) = app
        .put(
            "/api/classes/order",
            Some(&t),
            json!({ "class_ids": [c, a] }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(ids(&v), vec![c, a, b]);
    let (s, _) = app
        .put(
            "/api/classes/order",
            Some(&t),
            json!({ "class_ids": [a, a] }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = app
        .put(
            "/api/classes/order",
            Some(&t),
            json!({ "class_ids": [a, 999_999] }),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // Archive instead of delete: hidden from the default list, still readable, reversible.
    let (s, v) = app
        .post(&format!("/api/classes/{a}/archive"), Some(&t), json!({}))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let archived_at = v["archived_at"].clone();
    assert!(archived_at.is_string());
    let (_, v) = app
        .post(&format!("/api/classes/{a}/archive"), Some(&t), json!({}))
        .await;
    assert_eq!(
        v["archived_at"], archived_at,
        "a retry keeps the first time"
    );
    let (_, list) = app.get("/api/classes", Some(&t)).await;
    assert_eq!(ids(&list), vec![c, b]);
    let (_, all) = app
        .get("/api/classes?include_archived=true", Some(&t))
        .await;
    assert_eq!(ids(&all), vec![c, b, a]);
    let (s, v) = app.get(&format!("/api/classes/{a}"), Some(&t)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["name"], "高一 3 班");
    // There is no delete.
    let (s, _) = app
        .send(
            Method::DELETE,
            &format!("/api/classes/{a}"),
            Some(&t),
            None,
            Vec::new(),
        )
        .await;
    assert_eq!(s, StatusCode::METHOD_NOT_ALLOWED);
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM classes")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(count, 3);
    let (_, v) = app
        .post(&format!("/api/classes/{a}/unarchive"), Some(&t), json!({}))
        .await;
    assert_eq!(v["archived_at"], Value::Null);
    let (_, list) = app.get("/api/classes", Some(&t)).await;
    assert_eq!(ids(&list), vec![c, a, b]);
    assert_eq!(usage_events(&app, "class_archived").await.len(), 1);
    assert_eq!(usage_events(&app, "class_unarchived").await.len(), 1);
}

// ---------------------------------------------------------------- 取用 (many-to-many)

#[tokio::test]
async fn units_are_assigned_to_classes_many_to_many() {
    let app = test_app!();
    let t = app.teacher("T-ASG-1", "general").await;
    let a = class_id(&app, &t, "高一 3 班").await;
    let b = class_id(&app, &t, "高一 7 班").await;
    let u1 = app.create_unit(&t, "L3 倒裝句").await;
    let u2 = app.create_unit(&t, "L4 假設語氣").await;

    // A library unit that no class uses yet.
    let (_, unit) = app.get(&format!("/api/units/{u1}"), Some(&t)).await;
    assert_eq!(unit["classes"], json!([]));

    // One unit in two classes; one class with two units.
    assign(&app, &t, a, u1).await;
    let detail = assign(&app, &t, a, u2).await;
    assert_eq!(unit_ids(&detail), vec![u1, u2]);
    assert_eq!(detail["unit_count"], 2);
    let (s, detail) = app
        .post(
            &format!("/api/classes/{b}/units"),
            Some(&t),
            json!({ "unit_id": u1, "note": " 這班用基礎版 " }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{detail}");
    assert_eq!(detail["units"][0]["note"], "這班用基礎版");
    assert_eq!(detail["units"][0]["unit"]["title"], "L3 倒裝句");
    assert_eq!(detail["units"][0]["lessons_total"], 0);
    assert_eq!(detail["units"][0]["last_run_at"], Value::Null);

    // The library lists each unit with the classes using it (in class order, with the note).
    let (_, units) = app.get("/api/units", Some(&t)).await;
    let find = |id: i64| {
        units
            .as_array()
            .unwrap()
            .iter()
            .find(|u| u["id"] == id)
            .unwrap()
            .clone()
    };
    assert_eq!(class_names(&find(u1)), vec!["高一 3 班", "高一 7 班"]);
    assert_eq!(find(u1)["classes"][1]["note"], "這班用基礎版");
    assert_eq!(class_names(&find(u2)), vec!["高一 3 班"]);

    // Assigning again is idempotent (no second row); a note in the request updates the note.
    assign(&app, &t, a, u1).await;
    let (_, detail) = app
        .post(
            &format!("/api/classes/{a}/units"),
            Some(&t),
            json!({ "unit_id": u1, "note": "進度較快" }),
        )
        .await;
    assert_eq!(unit_ids(&detail), vec![u1, u2]);
    assert_eq!(detail["units"][0]["note"], "進度較快");
    assert_eq!(usage_events(&app, "class_unit_assigned").await.len(), 3);

    // Per-class note.
    let (s, detail) = app
        .put(
            &format!("/api/classes/{a}/units/{u2}"),
            Some(&t),
            json!({ "note": "先上這個" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{detail}");
    assert_eq!(detail["units"][1]["note"], "先上這個");
    let (s, _) = app
        .put(
            &format!("/api/classes/{b}/units/{u2}"),
            Some(&t),
            json!({ "note": "x" }),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND, "u2 is not in class b");
    let (s, _) = app
        .put(
            &format!("/api/classes/{a}/units/{u2}"),
            Some(&t),
            json!({ "note": "字".repeat(501) }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // Order within a class.
    let (s, detail) = app
        .put(
            &format!("/api/classes/{a}/units/order"),
            Some(&t),
            json!({ "unit_ids": [u2, u1] }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{detail}");
    assert_eq!(unit_ids(&detail), vec![u2, u1]);
    let (s, _) = app
        .put(
            &format!("/api/classes/{b}/units/order"),
            Some(&t),
            json!({ "unit_ids": [u2] }),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // Creating a unit from inside a class: in the library and assigned, in one step. The class
    // carries the profile, so nothing is copied into the unit.
    let (s, u3) = app
        .post(
            "/api/units",
            Some(&t),
            json!({ "title": "L5 分詞構句", "class_id": b }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{u3}");
    assert_eq!(class_names(&u3), vec!["高一 7 班"]);
    assert_eq!(u3["class_profile"], "");
    assert_eq!(u3["class_profile_source"], "none");
    let (_, detail) = app.get(&format!("/api/classes/{b}"), Some(&t)).await;
    assert_eq!(unit_ids(&detail), vec![u1, u3["id"].as_i64().unwrap()]);

    // Removing a unit from a class is soft: the design stays in the library, the run stays.
    let lesson = app.create_lesson(&t, u1, "第 1 節").await;
    let (s, run) = start(&app, &t, lesson, json!({ "class_id": b })).await;
    assert_eq!(s, StatusCode::OK, "{run}");
    end(&app, &t, &run).await;
    let (s, detail) = app
        .send(
            Method::DELETE,
            &format!("/api/classes/{b}/units/{u1}"),
            Some(&t),
            None,
            Vec::new(),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{detail}");
    assert_eq!(unit_ids(&detail), vec![u3["id"].as_i64().unwrap()]);
    let (_, unit) = app.get(&format!("/api/units/{u1}"), Some(&t)).await;
    assert_eq!(class_names(&unit), vec!["高一 3 班"]);
    let (_, runs) = app.get(&format!("/api/runs?class_id={b}"), Some(&t)).await;
    assert_eq!(ids(&runs), vec![run["id"].as_i64().unwrap()]);
    let rows: (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), COUNT(removed_at) FROM class_units WHERE class_id = $1 AND unit_id = $2",
    )
    .bind(b)
    .bind(u1)
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(rows, (1, 1), "kept, marked removed");
    // Removing again is a no-op; adding it back keeps the history and the earlier progress.
    let (s, _) = app
        .send(
            Method::DELETE,
            &format!("/api/classes/{b}/units/{u1}"),
            Some(&t),
            None,
            Vec::new(),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(usage_events(&app, "class_unit_removed").await.len(), 1);
    let detail = assign(&app, &t, b, u1).await;
    assert_eq!(detail["units"][1]["unit"]["id"], u1);
    assert_eq!(
        detail["units"][1]["note"], "",
        "a new assignment starts clean"
    );
    assert_eq!(detail["units"][1]["lessons_taught"], 1);

    // An archived class takes no new units and drops out of each unit's class list.
    app.post(&format!("/api/classes/{a}/archive"), Some(&t), json!({}))
        .await;
    let (s, v) = app
        .post(
            &format!("/api/classes/{a}/units"),
            Some(&t),
            json!({ "unit_id": u3["id"] }),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    let (s, _) = app
        .post(
            "/api/units",
            Some(&t),
            json!({ "title": "x", "class_id": a }),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (_, unit) = app.get(&format!("/api/units/{u2}"), Some(&t)).await;
    assert_eq!(unit["classes"], json!([]));
}

// ---------------------------------------------------------------- ownership

#[tokio::test]
async fn other_teachers_classes_are_404() {
    let app = test_app!();
    let a = app.teacher("T-OWN-A", "general").await;
    let b = app.teacher("T-OWN-B", "general").await;
    let a_class = class_id(&app, &a, "A 的班").await;
    let a_unit = app.create_unit(&a, "A 的單元").await;
    assign(&app, &a, a_class, a_unit).await;
    let b_class = class_id(&app, &b, "B 的班").await;
    let b_unit = app.create_unit(&b, "B 的單元").await;
    let b_lesson = app.create_lesson(&b, b_unit, "B 的課").await;
    assign(&app, &b, b_class, b_unit).await;

    let none = Vec::new;
    let json_body = |v: Value| v.to_string().into_bytes();
    let cases: Vec<(Method, String, Vec<u8>)> = vec![
        (Method::GET, format!("/api/classes/{a_class}"), none()),
        (
            Method::PUT,
            format!("/api/classes/{a_class}"),
            json_body(json!({ "name": "搶走" })),
        ),
        (
            Method::POST,
            format!("/api/classes/{a_class}/archive"),
            json_body(json!({})),
        ),
        (
            Method::POST,
            format!("/api/classes/{a_class}/unarchive"),
            json_body(json!({})),
        ),
        (
            Method::POST,
            format!("/api/classes/{a_class}/units"),
            json_body(json!({ "unit_id": b_unit })),
        ),
        (
            Method::PUT,
            format!("/api/classes/{a_class}/units/{a_unit}"),
            json_body(json!({ "note": "x" })),
        ),
        (
            Method::DELETE,
            format!("/api/classes/{a_class}/units/{a_unit}"),
            none(),
        ),
        (
            Method::PUT,
            format!("/api/classes/{a_class}/units/order"),
            json_body(json!({ "unit_ids": [a_unit] })),
        ),
        (
            Method::PUT,
            "/api/classes/order".to_string(),
            json_body(json!({ "class_ids": [a_class] })),
        ),
        // B's own class cannot take A's unit.
        (
            Method::POST,
            format!("/api/classes/{b_class}/units"),
            json_body(json!({ "unit_id": a_unit })),
        ),
        // A's class as the class context of B's own work.
        (
            Method::POST,
            "/api/units".to_string(),
            json_body(json!({ "title": "x", "class_id": a_class })),
        ),
        (
            Method::POST,
            format!("/api/lessons/{b_lesson}/runs"),
            json_body(json!({ "class_id": a_class })),
        ),
        (Method::GET, format!("/api/runs?class_id={a_class}"), none()),
        (
            Method::GET,
            format!("/api/lessons/{b_lesson}/runs?class_id={a_class}"),
            none(),
        ),
        (
            Method::GET,
            format!("/api/lessons/{b_lesson}?class_id={a_class}"),
            none(),
        ),
        (
            Method::POST,
            format!("/api/lessons/{b_lesson}/suggestions"),
            json_body(json!({ "class_id": a_class })),
        ),
    ];
    for (method, path, body) in cases {
        let ct = (!body.is_empty()).then_some("application/json");
        let (s, v) = app.send(method.clone(), &path, Some(&b), ct, body).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{method} {path}: {v}");
        assert_eq!(err_code(&v), "not_found");
    }

    // Nothing of A's changed, and B sees only B's own.
    let (_, detail) = app.get(&format!("/api/classes/{a_class}"), Some(&a)).await;
    assert_eq!(detail["name"], "A 的班");
    assert_eq!(unit_ids(&detail), vec![a_unit]);
    assert_eq!(detail["archived_at"], Value::Null);
    let (_, list) = app.get("/api/classes", Some(&b)).await;
    assert_eq!(ids(&list), vec![b_class]);
    let (_, units) = app.get("/api/units", Some(&b)).await;
    assert_eq!(ids(&units), vec![b_unit]);
}

// ---------------------------------------------------------------- runs per class

#[tokio::test]
async fn starting_a_run_picks_the_class_zero_one_many() {
    let app = test_app!();
    let t = app.teacher("T-RUN-1", "general").await;
    let unit = app.create_unit(&t, "L3 倒裝句").await;
    let l1 = app.create_lesson(&t, unit, "第 1 節").await;
    let l2 = app.create_lesson(&t, unit, "第 2 節").await;

    // Zero classes use the unit: a run without a class, never blocked (also with an empty body).
    let (s, run) = app
        .send(
            Method::POST,
            &format!("/api/lessons/{l1}/runs"),
            Some(&t),
            None,
            Vec::new(),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{run}");
    assert_eq!(run["class_id"], Value::Null);
    assert_eq!(run["class_name"], Value::Null);
    end(&app, &t, &run).await;

    // Exactly one class: it is the default.
    let a = class_id(&app, &t, "高一 3 班").await;
    assign(&app, &t, a, unit).await;
    let (s, run_a) = start(&app, &t, l1, json!({})).await;
    assert_eq!(s, StatusCode::OK, "{run_a}");
    assert_eq!(run_a["class_id"], a);
    assert_eq!(run_a["class_name"], "高一 3 班");
    // The same lesson is already open in that class.
    let (s, v) = start(&app, &t, l1, json!({ "class_id": a })).await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");

    // Several classes: the client must choose.
    let b = class_id(&app, &t, "高一 7 班").await;
    assign(&app, &t, b, unit).await;
    let (s, v) = start(&app, &t, l2, json!({})).await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    assert_eq!(err_code(&v), "conflict");
    assert!(v["error"]["message"].as_str().unwrap().contains("哪一班"));
    let (s, v) = start(&app, &t, l2, json!({ "class_id": null })).await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");

    // The same lesson runs in class B while it is still open in class A.
    let (s, run_b) = start(&app, &t, l1, json!({ "class_id": b })).await;
    assert_eq!(s, StatusCode::OK, "{run_b}");
    assert_eq!(run_b["class_id"], b);
    assert_ne!(run_b["id"], run_a["id"]);

    // A class that does not use the unit, or an archived one, is refused.
    let c = class_id(&app, &t, "高二 1 班").await;
    let (s, v) = start(&app, &t, l2, json!({ "class_id": c })).await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    assert!(v["error"]["message"].as_str().unwrap().contains("取用"));
    assign(&app, &t, c, unit).await;
    app.post(&format!("/api/classes/{c}/archive"), Some(&t), json!({}))
        .await;
    let (s, _) = start(&app, &t, l2, json!({ "class_id": c })).await;
    assert_eq!(s, StatusCode::CONFLICT);

    // Run lists carry the class and can be filtered by it.
    let (_, all) = app.get("/api/runs", Some(&t)).await;
    assert_eq!(all.as_array().unwrap().len(), 3);
    let (_, only_a) = app.get(&format!("/api/runs?class_id={a}"), Some(&t)).await;
    assert_eq!(ids(&only_a), vec![run_a["id"].as_i64().unwrap()]);
    let (_, lesson_b) = app
        .get(&format!("/api/lessons/{l1}/runs?class_id={b}"), Some(&t))
        .await;
    assert_eq!(ids(&lesson_b), vec![run_b["id"].as_i64().unwrap()]);
    let (_, lesson_all) = app.get(&format!("/api/lessons/{l1}/runs"), Some(&t)).await;
    assert_eq!(lesson_all.as_array().unwrap().len(), 3);
    let (_, detail) = app
        .get(&format!("/api/runs/{}", run_b["id"]), Some(&t))
        .await;
    assert_eq!(detail["class_name"], "高一 7 班");

    // Per-class progress: class A is in lesson 1; after it ends, lesson 2 is next. Class B's
    // record is separate.
    let (_, classes) = app.get("/api/classes", Some(&t)).await;
    assert_eq!(classes[0]["next_action"]["kind"], "resume_run");
    assert_eq!(classes[0]["next_action"]["run_id"], run_a["id"]);
    end(&app, &t, &run_a).await;
    let (_, detail_a) = app.get(&format!("/api/classes/{a}"), Some(&t)).await;
    assert_eq!(
        detail_a["next_action"],
        json!({ "kind": "start_lesson", "lesson_id": l2, "lesson_title": "第 2 節",
                "unit_id": unit, "unit_title": "L3 倒裝句" })
    );
    let cu = &detail_a["units"][0];
    assert_eq!(cu["lessons_total"], 2);
    assert_eq!(cu["lessons_taught"], 1);
    assert_eq!(cu["next_lesson"], json!({ "id": l2, "title": "第 2 節" }));
    assert!(cu["last_run_at"].is_string());
    let (s, run_a2) = start(&app, &t, l2, json!({ "class_id": a })).await;
    assert_eq!(s, StatusCode::OK, "{run_a2}");
    end(&app, &t, &run_a2).await;
    let (_, detail_a) = app.get(&format!("/api/classes/{a}"), Some(&t)).await;
    assert_eq!(detail_a["next_action"], Value::Null, "everything taught");
    assert_eq!(detail_a["units"][0]["lessons_taught"], 2);
    assert_eq!(detail_a["units"][0]["next_lesson"], Value::Null);
    let (_, detail_b) = app.get(&format!("/api/classes/{b}"), Some(&t)).await;
    assert_eq!(detail_b["next_action"]["kind"], "resume_run");
    assert_eq!(detail_b["units"][0]["lessons_taught"], 1);

    // A class whose next unit has no lessons yet is asked to plan it.
    let d = class_id(&app, &t, "高三 2 班").await;
    let empty_unit = app.create_unit(&t, "L9 還沒備課").await;
    assign(&app, &t, d, empty_unit).await;
    let (_, detail_d) = app.get(&format!("/api/classes/{d}"), Some(&t)).await;
    assert_eq!(
        detail_d["next_action"],
        json!({ "kind": "plan_unit", "unit_id": empty_unit, "unit_title": "L9 還沒備課" })
    );
}

// ---------------------------------------------------------------- class profile in AI context

#[tokio::test]
async fn ai_context_uses_the_class_profile_by_precedence() {
    let app = test_app!();
    let t = app.teacher("T-CTX-1", "general").await;
    set_p6(&app, &t, "問卷寫的班級").await;
    let with_profile = create_class(
        &app,
        &t,
        json!({ "name": "高一 3 班", "class_profile": "三班：不太敢開口" }),
    )
    .await["id"]
        .as_i64()
        .unwrap();
    let no_profile = class_id(&app, &t, "高一 7 班").await;

    // A unit with a legacy profile and one without.
    let (_, legacy) = app
        .post(
            "/api/units",
            Some(&t),
            json!({ "title": "L3 倒裝句", "class_profile": "單元舊描述" }),
        )
        .await;
    let legacy = legacy["id"].as_i64().unwrap();
    let (_, bare) = app
        .post(
            "/api/units",
            Some(&t),
            json!({ "title": "L4 假設語氣", "class_profile": "" }),
        )
        .await;
    let bare = bare["id"].as_i64().unwrap();
    for (class, unit) in [
        (with_profile, legacy),
        (no_profile, legacy),
        (no_profile, bare),
    ] {
        assign(&app, &t, class, unit).await;
    }

    let focus_of = |unit: i64| {
        let (app, t) = (&app, &t);
        async move {
            let (_, core) = app
                .put(
                    &format!("/api/units/{unit}/core/draft"),
                    Some(t),
                    json!({ "focuses": [{ "position": 1, "big_idea": "倒裝句的語氣", "responses": {} }] }),
                )
                .await;
            core["focuses"][0]["id"].as_i64().unwrap()
        }
    };
    let legacy_focus = focus_of(legacy).await;
    let bare_focus = focus_of(bare).await;
    let assist = |unit: i64, focus: i64, class: Option<i64>| {
        let (app, t) = (&app, &t);
        async move {
            let mut body =
                json!({ "unit_id": unit, "focus_id": focus, "question_id": "Q1", "level": 1 });
            if let Some(c) = class {
                body["class_id"] = json!(c);
            }
            let (s, r) = app.post("/api/core/assist", Some(t), body).await;
            assert_eq!(s, StatusCode::OK, "{r}");
            assert_eq!(r["status"], "shown", "{r}");
            r
        }
    };
    let snapshot = |r: Value| {
        let app = &app;
        async move {
            let snap: Value =
                sqlx::query_scalar("SELECT input_snapshot FROM ai_interactions WHERE id = $1")
                    .bind(r["interaction_id"].as_i64().unwrap())
                    .fetch_one(&app.db)
                    .await
                    .unwrap();
            snap
        }
    };

    // No class context: the pre-classes behaviour (unit profile, plain label).
    let r = assist(legacy, legacy_focus, None).await;
    assert_eq!(context_preview(&r, "班級特性"), "單元舊描述");
    let snap = snapshot(r).await;
    assert_eq!(
        snap["class_context"],
        json!({ "class_id": null, "class_profile_source": "unit" })
    );
    // …and P6 is not used outside a class context.
    let r = assist(bare, bare_focus, None).await;
    assert!(!labels(&r).iter().any(|l| l.starts_with("班級特性")), "{r}");

    // Class context: the class profile beats the unit's legacy profile, and the context says
    // which class it is.
    let r = assist(legacy, legacy_focus, Some(with_profile)).await;
    assert_eq!(
        context_preview(&r, "班級特性（高一 3 班）"),
        "三班：不太敢開口"
    );
    let snap = snapshot(r).await;
    assert_eq!(
        snap["class_context"],
        json!({ "class_id": with_profile, "class_profile_source": "class" })
    );
    assert!(snap["prompt"]["user"]
        .as_str()
        .unwrap()
        .contains("班級特性：三班：不太敢開口"));

    // The class has no profile: the unit's legacy profile.
    let r = assist(legacy, legacy_focus, Some(no_profile)).await;
    assert_eq!(context_preview(&r, "班級特性（單元設定）"), "單元舊描述");
    // Neither has one: the preference answer P6.
    let r = assist(bare, bare_focus, Some(no_profile)).await;
    assert_eq!(context_preview(&r, "班級特性（備課偏好）"), "問卷寫的班級");
    let snap = snapshot(r).await;
    assert_eq!(snap["class_context"]["class_profile_source"], "preference");

    // Lesson level: the lesson's own class description overrides everything.
    let lesson = app.create_lesson(&t, legacy, "第 1 節").await;
    let detail = |class: Option<i64>| {
        let (app, t) = (&app, &t);
        async move {
            let q = class.map(|c| format!("?class_id={c}")).unwrap_or_default();
            let (s, v) = app.get(&format!("/api/lessons/{lesson}{q}"), Some(t)).await;
            assert_eq!(s, StatusCode::OK, "{v}");
            (
                v["effective_class_profile"].as_str().unwrap().to_string(),
                v["class_profile_source"].as_str().unwrap().to_string(),
            )
        }
    };
    assert_eq!(
        detail(None).await,
        ("單元舊描述".to_string(), "unit".to_string())
    );
    assert_eq!(
        detail(Some(with_profile)).await,
        ("三班：不太敢開口".to_string(), "class".to_string())
    );
    assert_eq!(
        detail(Some(no_profile)).await,
        ("單元舊描述".to_string(), "unit".to_string())
    );
    let suggest = |class: i64| {
        let (app, t) = (&app, &t);
        async move {
            let (s, r) = app
                .post(
                    &format!("/api/lessons/{lesson}/suggestions"),
                    Some(t),
                    json!({ "class_id": class }),
                )
                .await;
            assert_eq!(s, StatusCode::OK, "{r}");
            r
        }
    };
    let r = suggest(with_profile).await;
    assert_eq!(
        context_preview(&r, "班級特性（高一 3 班）"),
        "三班：不太敢開口"
    );

    let (s, v) = app
        .put(
            &format!("/api/lessons/{lesson}?class_id={with_profile}"),
            Some(&t),
            json!({ "class_description": "今天只有半班" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["effective_class_profile"], "今天只有半班");
    assert_eq!(v["class_profile_source"], "lesson");
    let r = suggest(with_profile).await;
    assert_eq!(context_preview(&r, "班級特性（本課覆寫）"), "今天只有半班");
    let snap = snapshot(r).await;
    assert_eq!(
        snap["class_context"],
        json!({ "class_id": with_profile, "class_profile_source": "lesson" })
    );

    // 教材優先 candidates take the class too.
    let (_, material) = app
        .post(
            &format!("/api/units/{legacy}/materials"),
            Some(&t),
            json!({ "text": "Night markets are popular in Taiwan. Never have I seen such a crowd. Not only is the food cheap, but it is also delicious.", "filename": "L3 課文" }),
        )
        .await;
    let (s, r) = app
        .post(
            &format!("/api/units/{legacy}/material-focuses"),
            Some(&t),
            json!({ "material_id": material["id"], "class_id": with_profile }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(
        context_preview(&r, "班級特性（高一 3 班）"),
        "三班：不太敢開口"
    );

    // Editing the class profile changes what the next call uses.
    app.put(
        &format!("/api/classes/{with_profile}"),
        Some(&t),
        json!({ "class_profile": "三班：期中後敢開口了" }),
    )
    .await;
    let r = assist(legacy, legacy_focus, Some(with_profile)).await;
    assert_eq!(
        context_preview(&r, "班級特性（高一 3 班）"),
        "三班：期中後敢開口了"
    );
}
