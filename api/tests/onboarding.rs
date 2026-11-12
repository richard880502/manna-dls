//! Phase 1.1 (docs/API.md「初次使用設定與備課菜單」): preferences, prep routes, platform
//! settings, lesson/step fields, unit materials, 教材優先 candidates and AI context.

#[macro_use]
mod common;

use axum::http::{Method, StatusCode};
use serde_json::{json, Value};

fn err_code(v: &Value) -> &str {
    v["error"]["code"].as_str().unwrap_or("<none>")
}

fn answers(start_from: Value, hardest: Value, detail: Value, class_profile: &str) -> Value {
    json!({
        "experience": "3-9", "start_from": start_from, "hardest": hardest,
        "ai_experience": "sometimes", "detail": detail, "class_profile": class_profile
    })
}

async fn set_prefs(app: &common::TestApp, t: &str, answers: Value) -> Value {
    let (s, v) = app
        .put(
            "/api/me/preferences",
            Some(t),
            json!({ "answers": answers, "skipped": false, "source": "settings" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v
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

async fn snapshot(app: &common::TestApp, interaction_id: &Value) -> Value {
    sqlx::query_scalar("SELECT input_snapshot FROM ai_interactions WHERE id = $1")
        .bind(interaction_id.as_i64().unwrap())
        .fetch_one(&app.db)
        .await
        .unwrap()
}

fn failed_checks(report: &Value) -> Vec<String> {
    report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["passed"] == false)
        .map(|c| c["name"].as_str().unwrap().to_string())
        .collect()
}

// ---------------------------------------------------------------- preferences

#[tokio::test]
async fn preferences_versioning_and_skip() {
    let app = test_app!();
    let t = app.teacher("T-P1", "general").await;

    let (s, q) = app.get("/api/preferences/questions", None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(q["question_set_version"], 1);
    let qs = q["questions"].as_array().unwrap();
    assert_eq!(qs.len(), 6);
    assert_eq!(qs[1]["id"], "P2");
    assert_eq!(qs[1]["prompt"], "你通常從哪裡開始備課？");
    assert_eq!(
        qs[1]["options"][0],
        json!({ "value": "material", "label": "課文或教材" })
    );
    assert_eq!(qs[2]["multi"], true);
    assert_eq!(qs[2]["options"].as_array().unwrap().len(), 6);
    assert_eq!(qs[5]["options"], json!([]));

    let (s, v) = app.get("/api/me/preferences", Some(&t)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v, Value::Null, "no onboarding yet");
    let (s, _) = app.get("/api/me/preferences", None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // Skipping the wizard still records a version.
    let empty = json!({ "experience": null, "start_from": null, "hardest": [],
                        "ai_experience": null, "detail": null, "class_profile": "" });
    let (s, v1) = app
        .put(
            "/api/me/preferences",
            Some(&t),
            json!({ "answers": empty, "skipped": true, "source": "onboarding" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v1}");
    assert_eq!(v1["version_no"], 1);
    assert_eq!(v1["skipped"], true);
    assert_eq!(v1["source"], "onboarding");
    assert_eq!(v1["question_set_version"], 1);
    assert_eq!(v1["answers"], empty);
    assert!(v1["created_at"].is_string());

    let full = answers(
        json!("student"),
        json!(["differentiation", "goal", "differentiation"]),
        json!("concise"),
        "  高一普通班，程度落差大  ",
    );
    let v2 = set_prefs(&app, &t, full).await;
    assert_eq!(v2["version_no"], 2);
    assert_eq!(v2["skipped"], false);
    assert_eq!(v2["answers"]["hardest"], json!(["differentiation", "goal"]));
    assert_eq!(v2["answers"]["class_profile"], "高一普通班，程度落差大");
    let (_, latest) = app.get("/api/me/preferences", Some(&t)).await;
    assert_eq!(latest, v2);

    // Invalid values → 400, nothing stored.
    for bad in [
        json!({ "answers": { "start_from": "textbook" }, "skipped": false, "source": "settings" }),
        json!({ "answers": { "hardest": ["time"] }, "skipped": false, "source": "settings" }),
        json!({ "answers": { "detail": "long" }, "skipped": false, "source": "settings" }),
        json!({ "answers": { "experience": "5" }, "skipped": false, "source": "settings" }),
        json!({ "answers": { "colour": "red" }, "skipped": false, "source": "settings" }),
        json!({ "answers": {}, "skipped": false, "source": "admin" }),
        json!({ "answers": {}, "source": "settings" }),
        json!({ "answers": { "class_profile": "字".repeat(2001) }, "skipped": false, "source": "settings" }),
    ] {
        let (s, v) = app.put("/api/me/preferences", Some(&t), bad.clone()).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{bad} → {v}");
        assert_eq!(err_code(&v), "bad_request");
    }

    // Append-only: both versions are kept unchanged.
    let rows: Vec<(i32, bool, String, Value)> = sqlx::query_as(
        "SELECT version_no, skipped, source, answers FROM teacher_preferences ORDER BY version_no",
    )
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].0, 1);
    assert!(rows[0].1);
    assert_eq!(rows[0].3, empty);
    assert_eq!(rows[1].2, "settings");

    // Preferences are per teacher.
    let other = app.teacher("T-P2", "general").await;
    let (_, v) = app.get("/api/me/preferences", Some(&other)).await;
    assert_eq!(v, Value::Null);

    // Preference-driven scaffold events are accepted.
    let (s, v) = app
        .post(
            "/api/scaffold-events",
            Some(&t),
            json!({ "scaffold_code": "SC-PREF", "action": "show",
                    "trigger": "preference:hardest=differentiation" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    for code in ["SC-MATERIAL", "SC-CONTEXT"] {
        let (s, _) = app
            .post(
                "/api/scaffold-events",
                Some(&t),
                json!({ "scaffold_code": code, "action": "expand" }),
            )
            .await;
        assert_eq!(s, StatusCode::OK, "{code}");
    }
    let categories: Vec<(String, String)> = sqlx::query_as(
        "SELECT scaffold_code, category FROM scaffolds
          WHERE scaffold_code IN ('SC-MATERIAL', 'SC-PREF', 'SC-CONTEXT') ORDER BY scaffold_code",
    )
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert_eq!(
        categories,
        vec![
            ("SC-CONTEXT".into(), "teacher_control_fading".into()),
            ("SC-MATERIAL".into(), "content_design_support".into()),
            ("SC-PREF".into(), "teacher_control_fading".into()),
        ]
    );
}

// ---------------------------------------------------------------- units

#[tokio::test]
async fn unit_defaults_follow_preferences() {
    let app = test_app!();
    let t = app.teacher("T-U1", "general").await;

    let (s, u) = app
        .post("/api/units", Some(&t), json!({ "title": "No prefs" }))
        .await;
    assert_eq!(s, StatusCode::OK, "{u}");
    assert_eq!(u["prep_route"], "goal");
    assert_eq!(u["class_profile"], "");

    set_prefs(
        &app,
        &t,
        answers(
            json!("material"),
            json!([]),
            json!(null),
            "高二普通班，程度中等",
        ),
    )
    .await;
    let (_, u) = app
        .post("/api/units", Some(&t), json!({ "title": "From prefs" }))
        .await;
    assert_eq!(u["prep_route"], "material");
    assert_eq!(u["class_profile"], "高二普通班，程度中等");

    // Explicit values win over preferences (an explicit empty profile too).
    let (_, u) = app
        .post(
            "/api/units",
            Some(&t),
            json!({ "title": "Explicit", "prep_route": "activity", "class_profile": "" }),
        )
        .await;
    assert_eq!(u["prep_route"], "activity");
    assert_eq!(u["class_profile"], "");

    let (s, v) = app
        .post(
            "/api/units",
            Some(&t),
            json!({ "title": "Bad", "prep_route": "textbook" }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");

    let (_, list) = app.get("/api/units", Some(&t)).await;
    let routes: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["prep_route"].as_str().unwrap())
        .collect();
    assert_eq!(routes, vec!["activity", "material", "goal"]);
}

#[tokio::test]
async fn prep_route_changes_are_logged_and_locked_when_disabled() {
    let app = test_app!();
    let pi = app.pi_token().await;
    let t = app.teacher("T-U2", "general").await;
    let unit = app.create_unit(&t, "Unit route").await;
    let path = format!("/api/units/{unit}");

    let (s, u) = app
        .put(&path, Some(&t), json!({ "prep_route": "student" }))
        .await;
    assert_eq!(s, StatusCode::OK, "{u}");
    assert_eq!(u["prep_route"], "student");
    assert_eq!(u["title"], "Unit route", "absent fields are unchanged");
    // Re-sending the current route is not a change.
    let (s, _) = app
        .put(
            &path,
            Some(&t),
            json!({ "prep_route": "student", "title": "改名" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let logs: Vec<(String, Value)> =
        sqlx::query_as("SELECT event, payload FROM usage_logs WHERE event = 'prep_route_changed'")
            .fetch_all(&app.db)
            .await
            .unwrap();
    assert_eq!(
        logs,
        vec![(
            "prep_route_changed".to_string(),
            json!({ "unit_id": unit, "from": "goal", "to": "student" })
        )]
    );

    let (s, u) = app
        .put(
            &path,
            Some(&t),
            json!({ "class_profile": "  三十人，口說較弱 " }),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(u["class_profile"], "三十人，口說較弱");
    assert_eq!(u["title"], "改名");
    let (s, _) = app.put(&path, Some(&t), json!({ "title": " " })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = app
        .put(&path, Some(&t), json!({ "prep_route": "nope" }))
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let other = app.teacher("T-U3", "general").await;
    let (s, _) = app
        .put(&path, Some(&other), json!({ "prep_route": "goal" }))
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // Admin settings: PI only.
    let (s, v) = app.get("/api/admin/settings", Some(&t)).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert_eq!(err_code(&v), "forbidden");
    let (s, _) = app
        .put(
            "/api/admin/settings",
            Some(&t),
            json!({ "route_choice_enabled": false }),
        )
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = app.get("/api/admin/settings", None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, v) = app.get("/api/admin/settings", Some(&pi)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v, json!({ "route_choice_enabled": true }));
    let (s, v) = app
        .put(
            "/api/admin/settings",
            Some(&pi),
            json!({ "route_choice_enabled": false }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v, json!({ "route_choice_enabled": false }));
    let (_, cfg) = app.get("/api/config", None).await;
    assert_eq!(cfg["route_choice_enabled"], false);
    let (s, _) = app
        .put(
            "/api/admin/settings",
            Some(&pi),
            json!({ "route_choice_enabled": "no" }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let setting_logs: Vec<Value> =
        sqlx::query_scalar("SELECT payload FROM usage_logs WHERE event = 'setting_changed'")
            .fetch_all(&app.db)
            .await
            .unwrap();
    assert_eq!(
        setting_logs,
        vec![json!({ "key": "route_choice_enabled", "from": true, "to": false })]
    );

    // Disabled: changing the route → 409; the same route and other fields still save.
    let (s, v) = app
        .put(&path, Some(&t), json!({ "prep_route": "activity" }))
        .await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    assert_eq!(err_code(&v), "conflict");
    let (_, u) = app.get(&path, Some(&t)).await;
    assert_eq!(u["prep_route"], "student");
    let (s, u) = app
        .put(
            &path,
            Some(&t),
            json!({ "prep_route": "student", "class_profile": "改過" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{u}");
    assert_eq!(u["class_profile"], "改過");
    let n: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM usage_logs WHERE event = 'prep_route_changed'")
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(n, 1, "refused changes are not logged");

    // New units are always goal-first while disabled, whatever was asked or preferred.
    set_prefs(
        &app,
        &t,
        answers(json!("material"), json!([]), json!(null), ""),
    )
    .await;
    let (_, u) = app
        .post(
            "/api/units",
            Some(&t),
            json!({ "title": "Locked", "prep_route": "activity" }),
        )
        .await;
    assert_eq!(u["prep_route"], "goal");
    let (_, u) = app
        .post("/api/units", Some(&t), json!({ "title": "Locked 2" }))
        .await;
    assert_eq!(u["prep_route"], "goal");

    // Re-enabled.
    app.put(
        "/api/admin/settings",
        Some(&pi),
        json!({ "route_choice_enabled": true }),
    )
    .await;
    let (s, u) = app
        .put(&path, Some(&t), json!({ "prep_route": "activity" }))
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(u["prep_route"], "activity");
}

// ---------------------------------------------------------------- lessons

#[tokio::test]
async fn lesson_update_step_fields_and_effective_class_profile() {
    let app = test_app!();
    let t = app.teacher("T-L1", "general").await;
    let (_, unit) = app
        .post(
            "/api/units",
            Some(&t),
            json!({ "title": "Unit L", "class_profile": "高二普通班，程度中等" }),
        )
        .await;
    let unit_id = unit["id"].as_i64().unwrap();
    let lid = app.create_lesson(&t, unit_id, "L1").await;
    let path = format!("/api/lessons/{lid}");

    let (_, d) = app.get(&path, Some(&t)).await;
    assert_eq!(d["effective_class_profile"], "高二普通班，程度中等");
    assert_eq!(d["my_example"], "");
    assert_eq!(d["class_description"], "");

    let (s, d) = app
        .put(
            &path,
            Some(&t),
            json!({ "class_description": "今天只有半班", "my_example": "我的舊學習單：Not only 句型重組" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{d}");
    assert_eq!(
        d["effective_class_profile"], "今天只有半班",
        "lesson overrides unit"
    );
    assert_eq!(d["my_example"], "我的舊學習單：Not only 句型重組");
    assert_eq!(d["title"], "L1");
    assert!(
        d.get("versions_count").is_some(),
        "PUT returns LessonDetail"
    );

    let (_, d) = app
        .put(
            &path,
            Some(&t),
            json!({ "lesson_date": "2026-12-01", "title": "L1 改" }),
        )
        .await;
    assert_eq!(d["lesson_date"], "2026-12-01");
    assert_eq!(d["title"], "L1 改");
    assert_eq!(
        d["my_example"], "我的舊學習單：Not only 句型重組",
        "absent → unchanged"
    );
    let (_, d) = app
        .put(&path, Some(&t), json!({ "title": "L1 再改" }))
        .await;
    assert_eq!(d["lesson_date"], "2026-12-01", "absent date is kept");
    let (_, d) = app
        .put(&path, Some(&t), json!({ "lesson_date": null }))
        .await;
    assert_eq!(d["lesson_date"], Value::Null, "null clears the date");
    for bad in [
        json!({ "title": "" }),
        json!({ "lesson_date": "2026/12/01" }),
    ] {
        let (s, _) = app.put(&path, Some(&t), bad.clone()).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{bad}");
    }

    // Empty class_description → back to the unit's profile, which follows unit edits.
    let (_, d) = app
        .put(&path, Some(&t), json!({ "class_description": "" }))
        .await;
    assert_eq!(d["effective_class_profile"], "高二普通班，程度中等");
    app.put(
        &format!("/api/units/{unit_id}"),
        Some(&t),
        json!({ "class_profile": "高二技藝班" }),
    )
    .await;
    let (_, d) = app.get(&path, Some(&t)).await;
    assert_eq!(d["effective_class_profile"], "高二技藝班");

    // 我的範例 is overwritten in place: clearing it leaves nothing in the database.
    let (_, d) = app.put(&path, Some(&t), json!({ "my_example": "" })).await;
    assert_eq!(d["my_example"], "");
    let left: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM lessons WHERE my_example LIKE '%舊學習單%'")
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(left, 0);

    // Step fields round-trip.
    let (s, v) = app
        .post(
            &format!("{path}/versions"),
            Some(&t),
            json!({ "objective": "學生能用倒裝句", "steps": [
                { "position": 1, "activity_type": "全班講解", "icap_level": "passive", "content": "說明句型",
                  "duration_min": 10, "support_note": "提供句型框架", "extension_note": "改寫成正式語氣" },
                { "position": 2, "activity_type": "配對討論", "icap_level": null, "content": "兩兩造句" }
            ] }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["steps"][0]["duration_min"], 10);
    assert_eq!(v["steps"][1]["duration_min"], Value::Null);
    assert_eq!(v["steps"][1]["support_note"], "");
    let (_, d) = app.get(&path, Some(&t)).await;
    let step = &d["latest_version"]["steps"][0];
    assert_eq!(step["duration_min"], 10);
    assert_eq!(step["support_note"], "提供句型框架");
    assert_eq!(step["extension_note"], "改寫成正式語氣");
    assert_eq!(d["latest_version"]["steps"][1]["extension_note"], "");
    for d in [-1, 601] {
        let (s, _) = app
            .post(
                &format!("{path}/versions"),
                Some(&t),
                json!({ "objective": "o", "steps": [{ "position": 1, "icap_level": null, "duration_min": d }] }),
            )
            .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "duration {d}");
    }

    let (_, list) = app
        .get(&format!("/api/units/{unit_id}/lessons"), Some(&t))
        .await;
    assert!(list[0].get("my_example").is_some());
    let other = app.teacher("T-L2", "general").await;
    let (s, _) = app.put(&path, Some(&other), json!({ "title": "x" })).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------- materials

const MATERIAL: &str = "Night markets are an important part of Taiwanese culture.\n\
Not only do they sell delicious food, but they also offer games and clothes.\n\n\
夜市通常從傍晚開始營業，一直到深夜才結束。\n\
Many tourists visit Shilin Night Market every year.";

async fn add_material(app: &common::TestApp, t: &str, unit: i64, text: &str) -> Value {
    let (s, v) = app
        .post(
            &format!("/api/units/{unit}/materials"),
            Some(t),
            json!({ "text": text, "filename": "Lesson 3 課文" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v
}

#[tokio::test]
async fn materials_create_list_hard_delete_and_limit() {
    let app = test_app!();
    let t = app.teacher("T-M1", "general").await;
    let unit = app.create_unit(&t, "Unit M").await;
    let base = format!("/api/units/{unit}/materials");

    let m = add_material(&app, &t, unit, MATERIAL).await;
    assert_eq!(m["filename"], "Lesson 3 課文");
    assert_eq!(m["char_count"], MATERIAL.chars().count());
    assert!(m["created_at"].is_string());
    let mid = m["id"].as_i64().unwrap();
    let (s, m2) = app
        .post(&base, Some(&t), json!({ "text": "第二份教材內容。" }))
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(m2["filename"], "貼上的教材");
    let (_, list) = app.get(&base, Some(&t)).await;
    assert_eq!(list.as_array().unwrap().len(), 2);
    assert_eq!(list[0]["id"], mid);
    assert!(list[0].get("text").is_none(), "list carries no text");

    let chunks: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM unit_material_chunks WHERE unit_material_id = $1")
            .bind(mid)
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert!(chunks >= 1);

    // Limit: 50,000 characters (CJK counts as one each).
    let (s, _) = app
        .post(&base, Some(&t), json!({ "text": "字".repeat(50_000) }))
        .await;
    assert_eq!(s, StatusCode::OK);
    let (s, v) = app
        .post(&base, Some(&t), json!({ "text": "字".repeat(50_001) }))
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    assert!(v["error"]["message"].as_str().unwrap().contains("50000"));
    let (s, _) = app.post(&base, Some(&t), json!({ "text": "  \n " })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // Owner only.
    let other = app.teacher("T-M2", "general").await;
    let (s, _) = app.get(&base, Some(&other)).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = app.post(&base, Some(&other), json!({ "text": "x" })).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = app
        .send(
            Method::DELETE,
            &format!("{base}/{mid}"),
            Some(&other),
            None,
            vec![],
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let other_unit = app.create_unit(&t, "Other unit").await;
    let (s, _) = app
        .send(
            Method::DELETE,
            &format!("/api/units/{other_unit}/materials/{mid}"),
            Some(&t),
            None,
            vec![],
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND, "material of another unit");

    // DELETE removes the row and its chunks physically.
    let (s, v) = app
        .send(
            Method::DELETE,
            &format!("{base}/{mid}"),
            Some(&t),
            None,
            vec![],
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v, json!({ "ok": true }));
    let (rows, chunk_rows, text_left): (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM unit_materials WHERE id = $1),
                (SELECT COUNT(*) FROM unit_material_chunks WHERE unit_material_id = $1),
                (SELECT COUNT(*) FROM unit_material_chunks WHERE text LIKE '%Shilin%')",
    )
    .bind(mid)
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!((rows, chunk_rows, text_left), (0, 0, 0));
    let log: Value =
        sqlx::query_scalar("SELECT payload FROM usage_logs WHERE event = 'material_deleted'")
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(log["material_id"], mid);
    let (_, list) = app.get(&base, Some(&t)).await;
    assert_eq!(list.as_array().unwrap().len(), 2);
    let (s, _) = app
        .send(
            Method::DELETE,
            &format!("{base}/{mid}"),
            Some(&t),
            None,
            vec![],
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn material_focuses_quote_the_material_or_are_blocked() {
    let app = test_app!();
    let t = app.teacher("T-M3", "general").await;
    set_prefs(
        &app,
        &t,
        answers(
            json!("material"),
            json!(["ai_communication"]),
            json!("concise"),
            "高一普通班",
        ),
    )
    .await;
    let unit = app.create_unit(&t, "Night markets").await;
    let m = add_material(&app, &t, unit, MATERIAL).await;
    let path = format!("/api/units/{unit}/material-focuses");

    let (s, r) = app
        .post(&path, Some(&t), json!({ "material_id": m["id"] }))
        .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["status"], "shown", "{r}");
    assert_eq!(r["sources"], json!([]), "no knowledge-base retrieval");
    let candidates = r["output"]["candidates"].as_array().unwrap();
    assert!((1..=5).contains(&candidates.len()), "{r}");
    for c in candidates {
        let excerpt = c["excerpt"].as_str().unwrap();
        assert!(!excerpt.is_empty());
        assert!(MATERIAL.contains(excerpt), "verbatim: {excerpt}");
        assert!(c["big_idea"]
            .as_str()
            .unwrap()
            .chars()
            .any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch)));
        assert!(!c["rationale"].as_str().unwrap().is_empty());
    }
    assert!(context_preview(&r, "教材摘錄").starts_with("Lesson 3 課文：Night markets"));
    assert_eq!(context_preview(&r, "班級特性"), "高一普通班");
    assert!(context_preview(&r, "你的備課偏好").contains("精簡"));

    let iid = &r["interaction_id"];
    let (module, scaffold, checks): (String, String, Value) = sqlx::query_as(
        "SELECT i.module, s.scaffold_code, i.check_results
           FROM ai_interactions i JOIN scaffolds s ON s.id = i.scaffold_id WHERE i.id = $1",
    )
    .bind(iid.as_i64().unwrap())
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(module, "material_focuses");
    assert_eq!(scaffold, "SC-MATERIAL");
    assert_eq!(checks["passed"], true);
    let snap = snapshot(&app, iid).await;
    assert_eq!(snap["prompt"]["key"], "material_focuses");
    assert_eq!(snap["material"]["material_id"], m["id"]);
    assert_eq!(
        snap["material"]["sha256"],
        manna_api::auth::sha256_hex(MATERIAL)
    );
    assert!(snap["prompt"]["user"]
        .as_str()
        .unwrap()
        .contains("高一普通班"));
    assert!(
        !snap.to_string().contains("Shilin"),
        "material text never enters the input snapshot"
    );

    // An output that does not quote the material is blocked (after one regeneration).
    let bad = add_material(
        &app,
        &t,
        unit,
        "FAKE_BLOCK This text asks for a broken answer.",
    )
    .await;
    let (_, r) = app
        .post(&path, Some(&t), json!({ "material_id": bad["id"] }))
        .await;
    assert_eq!(r["status"], "blocked", "{r}");
    assert_eq!(r["output"], Value::Null);
    assert!(
        labels(&r).contains(&"教材摘錄"),
        "context is listed even when blocked"
    );
    let rows: Vec<(i32, Value)> = sqlx::query_as(
        "SELECT attempt, check_results FROM ai_interactions
          WHERE id = $1 OR id = (SELECT retry_of FROM ai_interactions WHERE id = $1) ORDER BY attempt",
    )
    .bind(r["interaction_id"].as_i64().unwrap())
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert_eq!(rows.len(), 2);
    for (_, report) in &rows {
        let failed = failed_checks(report);
        assert!(
            failed.contains(&"excerpt_verbatim".to_string()),
            "{failed:?}"
        );
        assert!(failed.contains(&"big_idea_zh".to_string()), "{failed:?}");
    }

    // Unknown / foreign materials.
    let (s, _) = app
        .post(&path, Some(&t), json!({ "material_id": 99_999_999 }))
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let other_unit = app.create_unit(&t, "Other").await;
    let (s, _) = app
        .post(
            &format!("/api/units/{other_unit}/material-focuses"),
            Some(&t),
            json!({ "material_id": m["id"] }),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let other = app.teacher("T-M4", "general").await;
    let (s, _) = app
        .post(&path, Some(&other), json!({ "material_id": m["id"] }))
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------- AI behaviour

async fn lesson_with_plan(app: &common::TestApp, t: &str) -> (i64, i64) {
    let unit = app.create_unit(t, "Unit inversion").await;
    let lid = app
        .create_lesson(t, unit, "Inversion: Not only … but also")
        .await;
    let (s, v) = app
        .post(
            &format!("/api/lessons/{lid}/versions"),
            Some(t),
            json!({ "objective": "學生能用 Not only … but also 倒裝句型介紹自己的家鄉", "steps": [
                { "position": 1, "activity_type": "全班講解", "icap_level": "passive",
                  "content": "示範倒裝句型", "duration_min": 10 }] }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    (unit, lid)
}

#[tokio::test]
async fn plan_suggestions_follow_preferences_and_list_context() {
    let app = test_app!();
    let t = app.teacher("T-S1", "general").await;
    set_prefs(
        &app,
        &t,
        answers(
            json!("goal"),
            json!(["differentiation", "ai_communication"]),
            json!("detailed"),
            "高二普通班，程度落差大",
        ),
    )
    .await;
    let (_, lid) = lesson_with_plan(&app, &t).await;
    let my_example = "我的舊學習單 SECRET-EXAMPLE：先給三個例句，再請學生改寫成倒裝句。";
    app.put(
        &format!("/api/lessons/{lid}"),
        Some(&t),
        json!({ "my_example": my_example }),
    )
    .await;
    let path = format!("/api/lessons/{lid}/suggestions");

    let (s, r) = app.post(&path, Some(&t), json!({})).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["status"], "shown", "{r}");
    let suggestions = r["output"]["suggestions"].as_array().unwrap();
    assert!(!suggestions.is_empty());
    for sug in suggestions {
        let d = sug["duration_min"].as_i64().expect("duration_min");
        assert!((1..=120).contains(&d));
        assert!(!sug["support_note"].as_str().unwrap().is_empty());
        assert!(!sug["extension_note"].as_str().unwrap().is_empty());
    }

    // Context: what Manna injected, with short previews.
    let l = labels(&r);
    for want in [
        "班級特性",
        "本課目標",
        "教案步驟",
        "我的範例",
        "你的備課偏好",
    ] {
        assert!(l.contains(&want), "{want} in {l:?}");
    }
    assert_eq!(context_preview(&r, "班級特性"), "高二普通班，程度落差大");
    assert!(context_preview(&r, "我的範例").starts_with("我的舊學習單"));
    assert!(context_preview(&r, "教案步驟").starts_with("共 1 步"));
    assert!(context_preview(&r, "你的備課偏好").contains("詳細"));
    for c in r["context"].as_array().unwrap() {
        assert!(c["preview"].as_str().unwrap().chars().count() <= 61);
    }

    // Prompt v2 carries class profile, detail and the differentiation rule; 我的範例 reaches
    // the model but the logged snapshot keeps only a hash.
    let snap = snapshot(&app, &r["interaction_id"]).await;
    assert_eq!(snap["prompt"]["version_no"], 2);
    let system = snap["prompt"]["system"].as_str().unwrap();
    assert!(system.contains("每一項建議都必須填寫"));
    assert!(system.contains("五句以內"));
    let user = snap["prompt"]["user"].as_str().unwrap();
    assert!(user.contains("班級特性：高二普通班，程度落差大"));
    assert!(user.contains("預估 10 分鐘"));
    assert!(user.contains("［我的範例："));
    assert!(!snap.to_string().contains("SECRET-EXAMPLE"));
    assert_eq!(
        snap["preferences"]["hardest"],
        json!(["differentiation", "ai_communication"])
    );

    // Missing differentiation fields fail the check → blocked.
    let (_, r) = app
        .post(&path, Some(&t), json!({ "request_note": "FAKE_NO_DIFF" }))
        .await;
    assert_eq!(r["status"], "blocked", "{r}");
    let report: Value =
        sqlx::query_scalar("SELECT check_results FROM ai_interactions WHERE id = $1")
            .bind(r["interaction_id"].as_i64().unwrap())
            .fetch_one(&app.db)
            .await
            .unwrap();
    let failed = failed_checks(&report);
    assert!(!failed.is_empty(), "{report}");
    assert!(
        failed.iter().all(|n| n == "differentiation_fields"),
        "{failed:?}"
    );

    // Without the differentiation preference the fields stay null.
    let t2 = app.teacher("T-S2", "general").await;
    let (_, lid2) = lesson_with_plan(&app, &t2).await;
    let (_, r) = app
        .post(
            &format!("/api/lessons/{lid2}/suggestions"),
            Some(&t2),
            json!({}),
        )
        .await;
    assert_eq!(r["status"], "shown", "{r}");
    let first = &r["output"]["suggestions"][0];
    assert_eq!(first["duration_min"], Value::Null);
    assert_eq!(first["support_note"], Value::Null);
    let l = labels(&r);
    assert!(
        !l.contains(&"班級特性"),
        "no class profile → not listed: {l:?}"
    );
    assert!(!l.contains(&"我的範例"));
    let snap = snapshot(&app, &r["interaction_id"]).await;
    assert!(snap["prompt"]["system"]
        .as_str()
        .unwrap()
        .contains("可以填 null"));
}

#[tokio::test]
async fn core_assist_uses_class_profile_and_goal_emphasis() {
    let app = test_app!();
    let t = app.teacher("T-C1", "general").await;
    set_prefs(
        &app,
        &t,
        answers(
            json!("goal"),
            json!(["goal"]),
            json!("medium"),
            "高三升學班",
        ),
    )
    .await;
    let unit = app.create_unit(&t, "Unit goal").await;
    let (_, core) = app
        .put(
            &format!("/api/units/{unit}/core/draft"),
            Some(&t),
            json!({ "focuses": [{ "position": 1, "big_idea": "倒裝句的語氣", "responses": {
                "Q1": { "answer": "學生能用倒裝句", "status": "filled", "content_source": "self" } } }] }),
        )
        .await;
    let focus = core["focuses"][0]["id"].as_i64().unwrap();
    let assist = |level: u8| json!({ "unit_id": unit, "focus_id": focus, "question_id": "Q1", "level": level });

    let (_, r) = app.post("/api/core/assist", Some(&t), assist(2)).await;
    assert_eq!(r["status"], "shown", "{r}");
    assert!(r["output"]["text"]
        .as_str()
        .unwrap()
        .contains("選這篇課文或教材的理由"));
    assert_eq!(context_preview(&r, "班級特性"), "高三升學班");
    assert_eq!(context_preview(&r, "學習重點"), "倒裝句的語氣");
    assert_eq!(context_preview(&r, "Q1 目前寫的內容"), "學生能用倒裝句");
    assert!(context_preview(&r, "你的備課偏好").contains("選文理由"));
    let snap = snapshot(&app, &r["interaction_id"]).await;
    assert_eq!(snap["prompt"]["key"], "core_assist_l2");
    assert_eq!(snap["prompt"]["version_no"], 2);
    let system = snap["prompt"]["system"].as_str().unwrap();
    assert!(system.contains("選這篇課文或教材的理由"));
    assert!(system.contains("語言以外的內容目標"));
    assert!(system.contains("三句以內"));
    assert!(snap["prompt"]["user"]
        .as_str()
        .unwrap()
        .contains("班級特性：高三升學班"));

    // Levels 1 and 3 keep their v1 prompts; class profile and length are appended.
    for level in [1u8, 3] {
        let (_, r) = app.post("/api/core/assist", Some(&t), assist(level)).await;
        assert_eq!(r["status"], "shown", "{r}");
        assert!(labels(&r).contains(&"班級特性"));
        let snap = snapshot(&app, &r["interaction_id"]).await;
        assert_eq!(snap["prompt"]["version_no"], 1);
        let user = snap["prompt"]["user"].as_str().unwrap();
        assert!(user.contains("班級特性：高三升學班"), "{user}");
        assert!(user.contains("回覆長度：適中"), "{user}");
        assert!(!user.contains("選這篇課文"), "emphasis is level 2 only");
    }

    // Without 學習目標 in P3, level 2 is not steered.
    set_prefs(
        &app,
        &t,
        answers(json!("goal"), json!(["worksheet"]), json!(null), ""),
    )
    .await;
    let (_, r) = app.post("/api/core/assist", Some(&t), assist(2)).await;
    assert_eq!(r["status"], "shown", "{r}");
    assert!(!r["output"]["text"].as_str().unwrap().contains("選這篇課文"));
    let snap = snapshot(&app, &r["interaction_id"]).await;
    assert!(!snap["prompt"]["system"]
        .as_str()
        .unwrap()
        .contains("選這篇課文或教材的理由"));
}
