//! End-to-end API tests with the fake AI provider. Each test gets its own database.

#[macro_use]
mod common;

use axum::http::StatusCode;
use serde_json::{json, Value};
use sqlx::Row;

use manna_api::auth::sha256_hex;
use manna_api::config::AiProviderKind;

fn err_code(v: &Value) -> &str {
    v["error"]["code"].as_str().unwrap_or("<none>")
}

// ---------------------------------------------------------------- public

#[tokio::test]
async fn health_config_and_api_errors() {
    let app = test_app!();
    let (s, v) = app.get("/api/health", None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["ok"], true);
    assert_eq!(v["db"], true);
    assert!(v["commit"].is_string());

    let (s, v) = app.get("/api/config", None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["ai_enabled"], true);
    assert_eq!(v["ai_provider"], "fake");
    assert_eq!(v["ai_model"], "fake-model");
    assert_eq!(v["demo_mode"], false);
    assert_eq!(v["route_choice_enabled"], true);

    let (s, v) = app.get("/api/does-not-exist", None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert_eq!(err_code(&v), "not_found");

    let (s, v) = app.get("/api/me", None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert_eq!(err_code(&v), "unauthorized");
    assert!(v["error"]["message"].as_str().unwrap().contains("登入"));

    let (s, v) = app.get("/api/me", Some("not-a-real-token")).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED, "{v}");

    // Without a static dir, non-API paths are 404s (and the server does not crash).
    let (s, _) = app.get("/units/1", None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn spa_fallback_serves_index_html() {
    let dir = std::env::temp_dir().join(format!("manna-web-{}", manna_api::auth::new_token()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("index.html"), "<html>manna-index</html>").unwrap();
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    std::fs::write(dir.join("assets/app.js"), "console.log('app')").unwrap();
    let static_dir = dir.clone();
    let app = test_app!(move |c: &mut manna_api::config::Config| c.static_dir = static_dir);

    let (s, v) = app.get("/", None).await;
    assert_eq!(s, StatusCode::OK);
    assert!(v.as_str().unwrap().contains("manna-index"));
    let (s, v) = app.get("/units/12/core", None).await;
    assert_eq!(s, StatusCode::OK, "SPA routes fall back to index.html");
    assert!(v.as_str().unwrap().contains("manna-index"));
    let (s, v) = app.get("/assets/app.js", None).await;
    assert_eq!(s, StatusCode::OK);
    assert!(v.as_str().unwrap().contains("console.log"));
    // Missing files are 404s, never the SPA shell.
    let (s, _) = app.get("/assets/missing-abc123.js", None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = app.get("/favicon.ico", None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    // API paths never fall back to the SPA.
    let (s, v) = app.get("/api/nope", None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert_eq!(err_code(&v), "not_found");
    std::fs::remove_dir_all(dir).ok();
}

// ---------------------------------------------------------------- auth

#[tokio::test]
async fn invite_login_flow() {
    let app = test_app!();
    let (s, v) = app
        .post("/api/auth/login", None, json!({ "code": "WRONG-CODE" }))
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert_eq!(err_code(&v), "unauthorized");

    // Bootstrap code (case-insensitive) logs in as the PI.
    let (pi, teacher) = app.login(&common::BOOTSTRAP_CODE.to_uppercase()).await;
    assert_eq!(teacher["research_id"], "PI-001");
    assert_eq!(teacher["role"], "pi");
    assert_eq!(teacher["school_track"], "general");
    assert_eq!(pi.len(), 64);
    let (pi2, teacher2) = app.login(common::BOOTSTRAP_CODE).await;
    assert_eq!(teacher2["id"], teacher["id"], "same code → same teacher");
    assert_ne!(pi, pi2);

    // Only hashes are stored.
    let stored: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE token_hash = $1")
        .bind(sha256_hex(&pi))
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(stored, 1);
    let plain: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE token_hash = $1")
        .bind(&pi)
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(plain, 0);

    // PI invites a vocational teacher.
    let (s, v) = app
        .post(
            "/api/admin/invites",
            Some(&pi),
            json!({ "research_id": "T-001", "school_track": "vocational" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let code = v["code"].as_str().unwrap().to_string();
    let (t, teacher) = app.login(&code).await;
    assert_eq!(teacher["research_id"], "T-001");
    assert_eq!(teacher["role"], "teacher");
    assert_eq!(teacher["school_track"], "vocational");
    let used: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT used_at FROM invite_codes WHERE code_hash = $1")
            .bind(sha256_hex(&code))
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert!(used.is_some());
    let (_, again) = app.login(&code).await;
    assert_eq!(again["id"], teacher["id"]);

    let (s, me) = app.get("/api/me", Some(&t)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(me["research_id"], "T-001");

    // Logout revokes the session.
    let (s, v) = app
        .send(
            axum::http::Method::POST,
            "/api/auth/logout",
            Some(&t),
            None,
            vec![],
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v, json!({ "ok": true }));
    let (s, _) = app.get("/api/me", Some(&t)).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // Invite validation.
    let (s, v) = app
        .post(
            "/api/admin/invites",
            Some(&pi),
            json!({ "research_id": "T-002", "school_track": "college" }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(err_code(&v), "bad_request");
    let (s, _) = app
        .post(
            "/api/admin/invites",
            Some(&pi),
            json!({ "research_id": "" , "school_track": "general"}),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // Malformed JSON keeps the error format.
    let (s, v) = app
        .send(
            axum::http::Method::POST,
            "/api/auth/login",
            None,
            Some("application/json"),
            b"{not json".to_vec(),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(err_code(&v), "bad_request");
}

// ---------------------------------------------------------------- units

#[tokio::test]
async fn units_crud_and_ownership() {
    let app = test_app!();
    let a = app.teacher("T-A", "general").await;
    let b = app.teacher("T-B", "general").await;

    let (s, v) = app
        .post("/api/units", Some(&a), json!({ "title": "  " }))
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");

    let (s, unit) = app
        .post(
            "/api/units",
            Some(&a),
            json!({ "title": "Unit 3 Inversion" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(unit["title"], "Unit 3 Inversion");
    assert_eq!(unit["core_status"], "none");
    assert_eq!(unit["lesson_count"], 0);
    assert!(unit["created_at"].as_str().unwrap().contains('T'));
    let id = unit["id"].as_i64().unwrap();
    app.create_unit(&a, "Unit 4").await;

    let (_, list) = app.get("/api/units", Some(&a)).await;
    assert_eq!(list.as_array().unwrap().len(), 2);
    let (s, one) = app.get(&format!("/api/units/{id}"), Some(&a)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(one["id"], id);

    // Another teacher sees nothing and gets 404 everywhere.
    let (_, list_b) = app.get("/api/units", Some(&b)).await;
    assert_eq!(list_b, json!([]));
    for path in [
        format!("/api/units/{id}"),
        format!("/api/units/{id}/core"),
        format!("/api/units/{id}/lessons"),
    ] {
        let (s, v) = app.get(&path, Some(&b)).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{path}: {v}");
        assert_eq!(err_code(&v), "not_found");
    }
    let (s, _) = app
        .post(
            &format!("/api/units/{id}/lessons"),
            Some(&b),
            json!({ "title": "x" }),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = app
        .put(
            &format!("/api/units/{id}/core/draft"),
            Some(&b),
            json!({ "focuses": [] }),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // Lessons owned by A are hidden from B too.
    let lesson = app.create_lesson(&a, id, "L1").await;
    let (s, _) = app.get(&format!("/api/lessons/{lesson}"), Some(&b)).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (_, one) = app.get(&format!("/api/units/{id}"), Some(&a)).await;
    assert_eq!(one["lesson_count"], 1);

    // Non-numeric ids → 400 in the API error format.
    let (s, v) = app.get("/api/units/abc", Some(&a)).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(err_code(&v), "bad_request");
}

// ---------------------------------------------------------------- CoRe

#[tokio::test]
async fn core_draft_and_complete() {
    let app = test_app!();
    let t = app.teacher("T-C", "general").await;
    let unit = app.create_unit(&t, "Unit 1").await;

    let (s, v) = app.get("/api/core/questions", Some(&t)).await;
    assert_eq!(s, StatusCode::OK);
    let qs = v.as_array().unwrap();
    assert_eq!(qs.len(), 8);
    assert_eq!(
        qs[0],
        json!({ "id": "Q1", "prompt": "你希望學生學會什麼？", "help": "學生最後要真正學會、能運用或能表達什麼" })
    );
    assert_eq!(qs[7]["prompt"], "你怎麼知道學生懂了？");

    let (s, core) = app.get(&format!("/api/units/{unit}/core"), Some(&t)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(core["phase"], "pre");
    assert_eq!(core["version_no"], 1);
    assert_eq!(core["focuses"], json!([]));
    assert_eq!(core["completed_at"], Value::Null);
    assert_eq!(core["saw_ai_assist"], false);
    let (_, u) = app.get(&format!("/api/units/{unit}"), Some(&t)).await;
    assert_eq!(u["core_status"], "draft");

    let draft = json!({ "focuses": [
        { "position": 1, "big_idea": "倒裝句的語氣", "responses": {
            "Q1": { "answer": "學生能用 Not only 句型", "status": "filled", "content_source": "self" },
            "Q2": { "answer": "should be dropped", "status": "no_idea", "content_source": "self" },
            "Q3": { "answer": "", "status": "blank", "content_source": "self" }
        }},
        { "position": 2, "big_idea": "關係子句", "responses": {} }
    ]});
    let (s, core) = app
        .put(&format!("/api/units/{unit}/core/draft"), Some(&t), draft)
        .await;
    assert_eq!(s, StatusCode::OK, "{core}");
    let focuses = core["focuses"].as_array().unwrap();
    assert_eq!(focuses.len(), 2);
    let f1 = &focuses[0];
    assert_eq!(f1["big_idea"], "倒裝句的語氣");
    assert_eq!(f1["responses"].as_object().unwrap().len(), 8);
    assert_eq!(f1["responses"]["Q1"]["status"], "filled");
    assert!(f1["responses"]["Q1"]["updated_at"].is_string());
    assert_eq!(f1["responses"]["Q2"]["status"], "no_idea");
    assert_eq!(
        f1["responses"]["Q2"]["answer"], "",
        "no_idea stores no answer text"
    );
    assert_eq!(f1["responses"]["Q3"]["status"], "blank");
    assert_eq!(f1["responses"]["Q4"]["updated_at"], Value::Null);
    let f1_id = f1["id"].as_i64().unwrap();
    let f2_id = focuses[1]["id"].as_i64().unwrap();

    // Edit Q1, drop focus 2, add a new focus.
    let draft2 = json!({ "focuses": [
        { "id": f1_id, "position": 1, "big_idea": "倒裝句的語氣", "responses": {
            "Q1": { "answer": "學生能用 Not only 與 Never 倒裝", "status": "filled", "content_source": "self_after_ai" }
        }},
        { "position": 2, "big_idea": "被動語態", "responses": {} }
    ]});
    let (s, core) = app
        .put(&format!("/api/units/{unit}/core/draft"), Some(&t), draft2)
        .await;
    assert_eq!(s, StatusCode::OK, "{core}");
    let focuses = core["focuses"].as_array().unwrap();
    assert_eq!(focuses.len(), 2);
    assert_eq!(
        focuses[0]["responses"]["Q1"]["content_source"],
        "self_after_ai"
    );
    assert_eq!(
        focuses[0]["responses"]["Q2"]["status"], "no_idea",
        "untouched cells stay"
    );
    assert_eq!(focuses[1]["big_idea"], "被動語態");
    assert!(focuses.iter().all(|f| f["id"] != f2_id));
    let deleted: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT deleted_at FROM core_focuses WHERE id = $1")
            .bind(f2_id)
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert!(deleted.is_some(), "removed focus is soft-deleted");
    let revisions: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM core_response_revisions r JOIN core_responses c ON c.id = r.core_response_id
          WHERE c.core_focus_id = $1 AND c.question_id = 'Q1'",
    )
    .bind(f1_id)
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(revisions, 2, "every change of a cell is kept");

    // Autosave re-sends all eight responses of every focus; unchanged cells add no revisions.
    let total_revisions = || async {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM core_response_revisions")
            .fetch_one(&app.db)
            .await
            .unwrap()
    };
    let before = total_revisions().await;
    let full: Vec<Value> = focuses
        .iter()
        .map(|f| {
            let responses: serde_json::Map<String, Value> = f["responses"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(q, r)| {
                    (
                        q.clone(),
                        json!({ "answer": r["answer"], "status": r["status"],
                                "content_source": r["content_source"] }),
                    )
                })
                .collect();
            json!({ "id": f["id"], "position": f["position"], "big_idea": f["big_idea"],
                    "responses": responses })
        })
        .collect();
    for _ in 0..2 {
        let (s, v) = app
            .put(
                &format!("/api/units/{unit}/core/draft"),
                Some(&t),
                json!({ "focuses": full }),
            )
            .await;
        assert_eq!(s, StatusCode::OK, "{v}");
    }
    assert_eq!(
        total_revisions().await,
        before,
        "no duplicate revisions on autosave"
    );

    // Validation.
    let four = json!({ "focuses": (1..=4).map(|p| json!({ "position": p, "big_idea": "x" })).collect::<Vec<_>>() });
    let (s, v) = app
        .put(&format!("/api/units/{unit}/core/draft"), Some(&t), four)
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    let other_unit = app.create_unit(&t, "Other").await;
    let (s, v) = app
        .put(
            &format!("/api/units/{other_unit}/core/draft"),
            Some(&t),
            json!({ "focuses": [{ "id": f1_id, "position": 1, "big_idea": "steal" }] }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    let (s, _) = app
        .put(
            &format!("/api/units/{unit}/core/draft"),
            Some(&t),
            json!({ "focuses": [{ "id": f1_id, "position": 1, "big_idea": "x",
                "responses": { "Q9": { "answer": "", "status": "blank", "content_source": "self" } } }] }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = app
        .put(
            &format!("/api/units/{unit}/core/draft"),
            Some(&t),
            json!({ "focuses": [{ "id": f1_id, "position": 1, "big_idea": "x",
                "responses": { "Q1": { "answer": "", "status": "maybe", "content_source": "self" } } }] }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // Completing with blanks is allowed; drafts still save afterwards.
    let (s, core) = app
        .post(
            &format!("/api/units/{unit}/core/complete"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{core}");
    assert!(core["completed_at"].is_string());
    let (_, u) = app.get(&format!("/api/units/{unit}"), Some(&t)).await;
    assert_eq!(u["core_status"], "completed");
    let (s, _) = app
        .put(
            &format!("/api/units/{unit}/core/draft"),
            Some(&t),
            json!({ "focuses": [{ "id": f1_id, "position": 1, "big_idea": "倒裝句的語氣（修）" }] }),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
}

async fn setup_focus(
    app: &common::TestApp,
    token: &str,
    unit: i64,
    big_idea: &str,
    q1: &str,
) -> i64 {
    let (s, core) = app
        .put(
            &format!("/api/units/{unit}/core/draft"),
            Some(token),
            json!({ "focuses": [{ "position": 1, "big_idea": big_idea, "responses": {
                "Q1": { "answer": q1, "status": "filled", "content_source": "self" } } }] }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{core}");
    core["focuses"][0]["id"].as_i64().unwrap()
}

#[tokio::test]
async fn core_assist_levels_continued_and_failures() {
    let app = test_app!();
    let t = app.teacher("T-D", "general").await;
    let unit = app.create_unit(&t, "Unit 2").await;
    let focus = setup_focus(
        &app,
        &t,
        unit,
        "倒裝句 Not only … but also",
        "學生能用倒裝句描述兩個優點",
    )
    .await;

    let assist = |level: u8, qid: &'static str| json!({ "unit_id": unit, "focus_id": focus, "question_id": qid, "level": level });
    let (s, r1) = app
        .post("/api/core/assist", Some(&t), assist(1, "Q1"))
        .await;
    assert_eq!(s, StatusCode::OK, "{r1}");
    assert_eq!(r1["status"], "shown", "{r1}");
    assert_eq!(r1["message"], Value::Null);
    assert_eq!(r1["output"]["level"], 1);
    assert!(r1["output"]["text"]
        .as_str()
        .unwrap()
        .contains("學生最後要真正學會"));
    assert_eq!(r1["sources"], json!([]));
    let i1 = r1["interaction_id"].as_i64().unwrap();

    let (_, r2) = app
        .post("/api/core/assist", Some(&t), assist(2, "Q1"))
        .await;
    assert_eq!(r2["status"], "shown", "{r2}");
    assert_eq!(r2["output"]["level"], 2);
    let (_, r3) = app
        .post("/api/core/assist", Some(&t), assist(3, "Q1"))
        .await;
    assert_eq!(r3["status"], "shown", "{r3}");
    let q = r3["output"]["text"].as_str().unwrap();
    assert!(q.ends_with('？'), "{q}");
    assert!(
        q.contains("學生能用倒裝句描述兩個優點"),
        "level 3 is grounded in the teacher's text: {q}"
    );

    let logs: Vec<(i32, Option<bool>, Option<i64>)> = sqlx::query_as(
        "SELECT level, continued, ai_interaction_id FROM core_assist_logs ORDER BY id",
    )
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert_eq!(logs.len(), 3);
    assert_eq!(logs[0], (1, Some(true), Some(i1)));
    assert_eq!(logs[1].1, Some(true));
    assert_eq!(logs[2].1, None);

    let row = sqlx::query(
        "SELECT module, status, provider, model, model_version, prompt_version_id, scaffold_id,
                check_results, input_snapshot, latency_ms, unit_id
           FROM ai_interactions WHERE id = $1",
    )
    .bind(i1)
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(row.get::<String, _>("module"), "core_assist");
    assert_eq!(row.get::<String, _>("status"), "shown");
    assert_eq!(row.get::<String, _>("provider"), "fake");
    assert_eq!(row.get::<String, _>("model"), "fake-model");
    assert_eq!(
        row.get::<Option<String>, _>("model_version").as_deref(),
        Some("fake-model-fake-1")
    );
    assert!(row.get::<Option<i64>, _>("prompt_version_id").is_some());
    let scaffold: String = sqlx::query_scalar("SELECT scaffold_code FROM scaffolds WHERE id = $1")
        .bind(row.get::<Option<i64>, _>("scaffold_id").unwrap())
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(scaffold, "SC-CORE-HELP");
    assert_eq!(row.get::<Value, _>("check_results")["passed"], true);
    let snapshot: Value = row.get("input_snapshot");
    assert_eq!(snapshot["prompt"]["key"], "core_assist_l1");
    assert!(snapshot["prompt"]["user"]
        .as_str()
        .unwrap()
        .contains("倒裝句 Not only"));
    assert!(row.get::<Option<i32>, _>("latency_ms").is_some());
    assert_eq!(row.get::<Option<i64>, _>("unit_id"), Some(unit));

    let (_, core) = app.get(&format!("/api/units/{unit}/core"), Some(&t)).await;
    assert_eq!(core["saw_ai_assist"], true);
    // AI never fills fields.
    assert_eq!(core["focuses"][0]["responses"]["Q2"]["status"], "blank");
    assert_eq!(
        core["focuses"][0]["responses"]["Q1"]["answer"],
        "學生能用倒裝句描述兩個優點"
    );

    // Teacher's judgement on an assist output.
    let (s, v) = app
        .post(
            &format!("/api/ai/{i1}/decision"),
            Some(&t),
            json!({ "decision": "answer", "thumbs": "up" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");

    // Blocked: the teacher's text makes the fake model produce a pasteable answer twice.
    let (s, _) = app
        .put(
            &format!("/api/units/{unit}/core/draft"),
            Some(&t),
            json!({ "focuses": [{ "id": focus, "position": 1, "big_idea": "倒裝句 Not only … but also",
                "responses": { "Q4": { "answer": "FAKE_BLOCK", "status": "filled", "content_source": "self" } } }] }),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let (_, blocked) = app
        .post("/api/core/assist", Some(&t), assist(3, "Q4"))
        .await;
    assert_eq!(blocked["status"], "blocked", "{blocked}");
    assert_eq!(blocked["message"], "目前暫無建議，你可以繼續手動完成。");
    assert_eq!(blocked["output"], Value::Null);
    let final_id = blocked["interaction_id"].as_i64().unwrap();
    type AttemptRow = (i64, i32, String, Option<i64>, Option<Value>, Option<Value>);
    let attempts: Vec<AttemptRow> = sqlx::query_as(
        "SELECT id, attempt, status, retry_of, output, check_results FROM ai_interactions
          WHERE id = $1 OR id = (SELECT retry_of FROM ai_interactions WHERE id = $1) ORDER BY id",
    )
    .bind(final_id)
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert_eq!(attempts.len(), 2, "regenerated once");
    assert_eq!(attempts[0].1, 1);
    assert_eq!(attempts[1].1, 2);
    assert_eq!(attempts[1].3, Some(attempts[0].0));
    for a in &attempts {
        assert_eq!(a.2, "blocked");
        assert!(
            a.4.as_ref().unwrap()["text"]
                .as_str()
                .unwrap()
                .starts_with("答案"),
            "blocked output is kept"
        );
        assert_eq!(a.5.as_ref().unwrap()["passed"], false);
    }
    let last_log: Option<i64> = sqlx::query_scalar(
        "SELECT ai_interaction_id FROM core_assist_logs ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(last_log, Some(final_id));

    // Timeout on a second focus.
    let unit2 = app.create_unit(&t, "Unit timeout").await;
    let focus2 = setup_focus(&app, &t, unit2, "被動語態", "FAKE_TIMEOUT").await;
    let (_, timeout) = app
        .post(
            "/api/core/assist",
            Some(&t),
            json!({ "unit_id": unit2, "focus_id": focus2, "question_id": "Q1", "level": 1 }),
        )
        .await;
    assert_eq!(timeout["status"], "timeout", "{timeout}");
    assert!(timeout["message"].as_str().unwrap().contains("手動完成"));
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ai_interactions WHERE unit_id = $1")
        .bind(unit2)
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(n, 1, "timeouts are not retried");

    // Validation and ownership.
    let (s, _) = app
        .post("/api/core/assist", Some(&t), assist(4, "Q1"))
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = app
        .post("/api/core/assist", Some(&t), assist(1, "Q0"))
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = app
        .post(
            "/api/core/assist",
            Some(&t),
            json!({ "unit_id": unit2, "focus_id": focus, "question_id": "Q1", "level": 1 }),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND, "focus from another unit");
    let other = app.teacher("T-E", "general").await;
    let (s, _) = app
        .post("/api/core/assist", Some(&other), assist(1, "Q1"))
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = app
        .post(
            &format!("/api/ai/{i1}/decision"),
            Some(&other),
            json!({ "decision": "skip" }),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------- lessons

#[tokio::test]
async fn lessons_versions_core_links_and_siop() {
    let app = test_app!();
    let t = app.teacher("T-F", "general").await;
    let unit = app.create_unit(&t, "Unit 5").await;
    let (_, core) = app
        .put(
            &format!("/api/units/{unit}/core/draft"),
            Some(&t),
            json!({ "focuses": [
                { "position": 1, "big_idea": "A" }, { "position": 2, "big_idea": "B" }, { "position": 3, "big_idea": "C" }
            ]}),
        )
        .await;
    let fids: Vec<i64> = core["focuses"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["id"].as_i64().unwrap())
        .collect();
    assert_eq!(fids.len(), 3);
    let other_unit = app.create_unit(&t, "Unit 6").await;
    let foreign = setup_focus(&app, &t, other_unit, "X", "y").await;

    let (s, _) = app
        .post(
            &format!("/api/units/{unit}/lessons"),
            Some(&t),
            json!({ "title": "L1", "lesson_date": "2026/11/20" }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, lesson) = app
        .post(
            &format!("/api/units/{unit}/lessons"),
            Some(&t),
            json!({ "title": "L1 倒裝句", "lesson_date": "2026-11-20", "class_description": "二年級，30 人" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{lesson}");
    assert_eq!(lesson["lesson_date"], "2026-11-20");
    assert_eq!(lesson["unit_id"], unit);
    let lid = lesson["id"].as_i64().unwrap();
    let (_, list) = app
        .get(&format!("/api/units/{unit}/lessons"), Some(&t))
        .await;
    assert_eq!(list.as_array().unwrap().len(), 1);

    let (_, detail) = app.get(&format!("/api/lessons/{lid}"), Some(&t)).await;
    assert_eq!(detail["latest_version"], Value::Null);
    assert_eq!(detail["versions_count"], 0);
    assert_eq!(detail["core_focus_ids"], json!([]));
    assert_eq!(detail["previous_next_change"], Value::Null);
    assert_eq!(detail["title"], "L1 倒裝句");

    let (s, _) = app
        .post(
            &format!("/api/lessons/{lid}/versions"),
            Some(&t),
            json!({ "objective": "o", "steps": [{ "position": 1, "activity_type": "x", "icap_level": "deep", "content": "c" }] }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = app
        .post(
            &format!("/api/lessons/{lid}/versions"),
            Some(&t),
            json!({ "objective": "o", "steps": [
                { "position": 1, "activity_type": "x", "icap_level": null, "content": "c" },
                { "position": 1, "activity_type": "y", "icap_level": null, "content": "d" }] }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    let (s, v1) = app
        .post(
            &format!("/api/lessons/{lid}/versions"),
            Some(&t),
            json!({ "objective": "學生能使用倒裝句", "steps": [
                { "position": 1, "activity_type": "全班講解", "icap_level": "passive", "content": "說明句型" }] }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v1}");
    assert_eq!(v1["version_no"], 1);
    let (_, v2) = app
        .post(
            &format!("/api/lessons/{lid}/versions"),
            Some(&t),
            json!({ "objective": "學生能使用倒裝句描述優點", "steps": [
                { "position": 2, "activity_type": "配對討論", "icap_level": "interactive", "content": "兩兩造句" },
                { "position": 1, "activity_type": "全班講解", "icap_level": "passive", "content": "說明句型" }] }),
        )
        .await;
    assert_eq!(v2["version_no"], 2);
    assert_eq!(v2["steps"][0]["position"], 1, "steps are returned in order");
    let (_, detail) = app.get(&format!("/api/lessons/{lid}"), Some(&t)).await;
    assert_eq!(detail["versions_count"], 2);
    assert_eq!(detail["latest_version"]["version_no"], 2);
    assert_eq!(
        detail["latest_version"]["objective"],
        "學生能使用倒裝句描述優點"
    );
    assert_eq!(
        detail["latest_version"]["steps"].as_array().unwrap().len(),
        2
    );
    assert_eq!(
        detail["latest_version"]["steps"][1]["icap_level"],
        "interactive"
    );

    // CoRe links: 0–3 focuses of the same unit.
    let path = format!("/api/lessons/{lid}/core-links");
    let (s, v) = app
        .put(&path, Some(&t), json!({ "focus_ids": [fids[0], fids[1]] }))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v, json!({ "focus_ids": [fids[0], fids[1]] }));
    let (s, _) = app
        .put(
            &path,
            Some(&t),
            json!({ "focus_ids": [fids[0], fids[1], fids[2], foreign] }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "max 3");
    let (s, _) = app
        .put(&path, Some(&t), json!({ "focus_ids": [foreign] }))
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "same unit only");
    let (s, _) = app
        .put(&path, Some(&t), json!({ "focus_ids": [fids[1], fids[2]] }))
        .await;
    assert_eq!(s, StatusCode::OK);
    let (_, detail) = app.get(&format!("/api/lessons/{lid}"), Some(&t)).await;
    let mut linked: Vec<i64> = detail["core_focus_ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_i64().unwrap())
        .collect();
    linked.sort();
    assert_eq!(linked, vec![fids[1], fids[2]]);
    let removed: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM lesson_core_links WHERE lesson_id = $1 AND removed_at IS NOT NULL",
    )
    .bind(lid)
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(removed, 1, "unlinking is a soft delete");
    let (s, v) = app.put(&path, Some(&t), json!({ "focus_ids": [] })).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v, json!({ "focus_ids": [] }));

    // SIOP: only approved features; selection recorded.
    let (s, siop) = app.get("/api/siop", Some(&t)).await;
    assert_eq!(s, StatusCode::OK);
    let siop = siop.as_array().unwrap();
    assert_eq!(siop.len(), 6);
    assert!(siop[0]["summary"].as_str().unwrap().starts_with("（示範）"));
    let ids: Vec<i64> = siop
        .iter()
        .take(2)
        .map(|f| f["id"].as_i64().unwrap())
        .collect();
    let (s, v) = app
        .put(
            &format!("/api/lessons/{lid}/siop"),
            Some(&t),
            json!({ "siop_feature_ids": ids }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, detail) = app.get(&format!("/api/lessons/{lid}"), Some(&t)).await;
    assert_eq!(detail["siop_feature_ids"], json!(ids));
    let (s, _) = app
        .put(
            &format!("/api/lessons/{lid}/siop"),
            Some(&t),
            json!({ "siop_feature_ids": [999999] }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------- plan suggestions

async fn lesson_with_plan(app: &common::TestApp, t: &str, title: &str, objective: &str) -> i64 {
    let unit = app.create_unit(t, "Unit RAG").await;
    let lid = app.create_lesson(t, unit, title).await;
    if !objective.is_empty() {
        let (s, v) = app
            .post(
                &format!("/api/lessons/{lid}/versions"),
                Some(t),
                json!({ "objective": objective, "steps": [
                    { "position": 1, "activity_type": "全班講解", "icap_level": "passive", "content": "示範倒裝句型" }] }),
            )
            .await;
        assert_eq!(s, StatusCode::OK, "{v}");
    }
    lid
}

#[tokio::test]
async fn plan_suggestions_shown_kb_empty_and_failures() {
    let app = test_app!();
    let t = app.teacher("T-G", "general").await;
    let lid = lesson_with_plan(
        &app,
        &t,
        "Inversion: Not only … but also",
        "學生能用 Not only … but also 倒裝句型介紹自己的家鄉",
    )
    .await;
    let path = format!("/api/lessons/{lid}/suggestions");

    let (s, r) = app.post(&path, Some(&t), json!({})).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["status"], "shown", "{r}");
    assert_eq!(r["output"]["kb_empty"], false);
    let suggestions = r["output"]["suggestions"].as_array().unwrap();
    assert!(!suggestions.is_empty());
    let sources: Vec<&str> = r["sources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["item_code"].as_str().unwrap())
        .collect();
    assert!(sources.contains(&"DEMO-GRM-001"), "{sources:?}");
    for s in suggestions {
        let cites = s["citations"].as_array().unwrap();
        assert!(!cites.is_empty());
        for c in cites {
            assert!(sources.contains(&c.as_str().unwrap()));
        }
    }
    let src = &r["sources"][0];
    assert_eq!(
        src["source_doc"],
        "示範條目（非官方，待主持人提供正式來源）"
    );
    for key in ["title", "category", "source_page"] {
        assert!(src.get(key).is_some(), "source has {key}");
    }
    let iid = r["interaction_id"].as_i64().unwrap();

    // Retrieval log, track filtering, and the interaction record.
    let (codes, track): (Vec<String>, String) = sqlx::query_as(
        "SELECT item_codes, school_track FROM retrieval_logs WHERE ai_interaction_id = $1",
    )
    .bind(iid)
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(track, "general");
    assert!(codes.contains(&"DEMO-GRM-001".to_string()));
    assert!(codes.len() <= 8);
    for vocational_only in ["DEMO-GRM-006", "DEMO-VOC-003", "DEMO-CUR-003"] {
        assert!(!codes.contains(&vocational_only.to_string()));
    }
    let row = sqlx::query(
        "SELECT status, citations, check_results, model_version, lesson_id, scaffold_id, prompt_version_id
           FROM ai_interactions WHERE id = $1",
    )
    .bind(iid)
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(row.get::<String, _>("status"), "shown");
    assert!(!row.get::<Vec<String>, _>("citations").is_empty());
    assert_eq!(row.get::<Value, _>("check_results")["passed"], true);
    assert_eq!(row.get::<Option<i64>, _>("lesson_id"), Some(lid));
    assert!(row.get::<Option<i64>, _>("prompt_version_id").is_some());
    let scaffold: String = sqlx::query_scalar("SELECT scaffold_code FROM scaffolds WHERE id = $1")
        .bind(row.get::<Option<i64>, _>("scaffold_id").unwrap())
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(scaffold, "SC-RAG");

    // Decisions on the shown suggestion.
    let dpath = format!("/api/ai/{iid}/decision");
    let (s, v) = app
        .post(
            &dpath,
            Some(&t),
            json!({ "item_index": 0, "decision": "adopt" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v, json!({ "ok": true }));
    let (s, _) = app
        .post(
            &dpath,
            Some(&t),
            json!({ "item_index": 1, "decision": "revise", "revised_content": "改成小組任務",
                    "reason": "not_feasible", "reason_note": "時間不夠" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = app
        .post(
            &dpath,
            Some(&t),
            json!({ "decision": "reject", "reason": "source_problem" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = app
        .post(&dpath, Some(&t), json!({ "decision": "love" }))
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = app
        .post(
            &dpath,
            Some(&t),
            json!({ "decision": "reject", "reason": "boring" }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = app
        .post(
            "/api/ai/99999999/decision",
            Some(&t),
            json!({ "decision": "adopt" }),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let decisions: Vec<(i32, String, Option<String>)> = sqlx::query_as(
        "SELECT item_index, decision, reason FROM teacher_decisions WHERE ai_interaction_id = $1 ORDER BY id",
    )
    .bind(iid)
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert_eq!(
        decisions,
        vec![
            (0, "adopt".into(), None),
            (1, "revise".into(), Some("not_feasible".into())),
            (0, "reject".into(), Some("source_problem".into()))
        ]
    );

    // Nothing retrieved → kb_empty without calling the model.
    let empty_lesson = lesson_with_plan(&app, &t, "zzqx qwvb", "").await;
    let (s, r) = app
        .send(
            axum::http::Method::POST,
            &format!("/api/lessons/{empty_lesson}/suggestions"),
            Some(&t),
            None,
            vec![],
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["status"], "shown");
    assert_eq!(r["output"], json!({ "kb_empty": true, "suggestions": [] }));
    assert_eq!(r["sources"], json!([]));
    let (model_version, snapshot): (Option<String>, Value) =
        sqlx::query_as("SELECT model_version, input_snapshot FROM ai_interactions WHERE id = $1")
            .bind(r["interaction_id"].as_i64().unwrap())
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(model_version, None);
    assert_eq!(snapshot["model_called"], false);

    // The model itself may answer kb_empty when the items are irrelevant.
    let (_, r) = app
        .post(&path, Some(&t), json!({ "request_note": "FAKE_KB_EMPTY" }))
        .await;
    assert_eq!(r["status"], "shown", "{r}");
    assert_eq!(r["output"]["kb_empty"], true);

    // Invalid citations twice → blocked, both attempts logged with their outputs.
    let (_, r) = app
        .post(&path, Some(&t), json!({ "request_note": "FAKE_BLOCK" }))
        .await;
    assert_eq!(r["status"], "blocked", "{r}");
    assert_eq!(r["message"], "目前暫無建議，你可以繼續手動完成。");
    assert_eq!(r["output"], Value::Null);
    let last = r["interaction_id"].as_i64().unwrap();
    let rows: Vec<(i32, String, Value, Value)> = sqlx::query_as(
        "SELECT attempt, status, output, check_results FROM ai_interactions
          WHERE id = $1 OR id = (SELECT retry_of FROM ai_interactions WHERE id = $1) ORDER BY attempt",
    )
    .bind(last)
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert_eq!(rows.len(), 2);
    for (attempt, status, output, checks) in &rows {
        assert_eq!(status, "blocked", "attempt {attempt}");
        assert_eq!(output["suggestions"][0]["citations"][0], "NOT-A-REAL-CODE");
        assert_eq!(checks["passed"], false);
        let failed: Vec<&str> = checks["checks"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["passed"] == false)
            .map(|c| c["name"].as_str().unwrap())
            .collect();
        assert_eq!(failed, vec!["citations_retrieved"]);
    }

    // Fails once, regeneration passes → shown from attempt 2.
    let (_, r) = app
        .post(
            &path,
            Some(&t),
            json!({ "request_note": "FAKE_BLOCK_ONCE" }),
        )
        .await;
    assert_eq!(r["status"], "shown", "{r}");
    let (attempt, retry_of): (i32, Option<i64>) =
        sqlx::query_as("SELECT attempt, retry_of FROM ai_interactions WHERE id = $1")
            .bind(r["interaction_id"].as_i64().unwrap())
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(attempt, 2);
    assert!(retry_of.is_some());

    let (_, r) = app
        .post(&path, Some(&t), json!({ "request_note": "FAKE_TIMEOUT" }))
        .await;
    assert_eq!(r["status"], "timeout", "{r}");
    assert!(r["interaction_id"].is_i64());
    let (_, r) = app
        .post(&path, Some(&t), json!({ "request_note": "FAKE_ERROR" }))
        .await;
    assert_eq!(r["status"], "error", "{r}");
    assert!(r["message"].as_str().unwrap().contains("手動完成"));
    let (_, r) = app
        .post(&path, Some(&t), json!({ "request_note": "FAKE_REFUSAL" }))
        .await;
    assert_eq!(r["status"], "blocked", "{r}");

    // Ownership.
    let other = app.teacher("T-H", "general").await;
    let (s, _) = app.post(&path, Some(&other), json!({})).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn vocational_teacher_gets_vocational_items() {
    let app = test_app!();
    let t = app.teacher("T-V", "vocational").await;
    let lid = lesson_with_plan(
        &app,
        &t,
        "餐旅服務用語 reservation menu",
        "學生能在櫃台情境使用餐旅服務用語",
    )
    .await;
    let (_, r) = app
        .post(
            &format!("/api/lessons/{lid}/suggestions"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(r["status"], "shown", "{r}");
    let codes: Vec<String> = sqlx::query_scalar(
        "SELECT unnest(item_codes) FROM retrieval_logs WHERE ai_interaction_id = $1",
    )
    .bind(r["interaction_id"].as_i64().unwrap())
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert!(codes.contains(&"DEMO-VOC-003".to_string()), "{codes:?}");
    for general_only in ["DEMO-GRM-005", "DEMO-VOC-004", "DEMO-CUR-002"] {
        assert!(!codes.contains(&general_only.to_string()));
    }
}

// ---------------------------------------------------------------- admin

#[tokio::test]
async fn admin_prompts_imports_and_permissions() {
    let app = test_app!();
    let pi = app.pi_token().await;
    let t = app.teacher("T-I", "general").await;

    // Non-PI → 403 on every admin endpoint.
    for (path, body) in [
        (
            "/api/admin/invites",
            json!({ "research_id": "X", "school_track": "general" }),
        ),
        (
            "/api/admin/prompts",
            json!({ "prompt_key": "plan_suggest", "system_text": "s", "user_template": "u" }),
        ),
    ] {
        let (s, v) = app.post(path, Some(&t), body).await;
        assert_eq!(s, StatusCode::FORBIDDEN, "{path}");
        assert_eq!(err_code(&v), "forbidden");
    }
    let (s, _) = app.get("/api/admin/prompts", Some(&t)).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    for path in [
        "/api/admin/knowledge/import",
        "/api/admin/teacher-cases/import",
    ] {
        let (s, _) = app.post_csv(path, Some(&t), "a,b\n").await;
        assert_eq!(s, StatusCode::FORBIDDEN, "{path}");
    }
    let (s, _) = app.get("/api/admin/prompts", None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // Prompts: latest per key; new versions increment.
    let (s, listed) = app.get("/api/admin/prompts", Some(&pi)).await;
    assert_eq!(s, StatusCode::OK);
    let prompts = listed["prompts"].clone();
    let keys: Vec<&str> = prompts
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["prompt_key"].as_str().unwrap())
        .collect();
    assert_eq!(
        keys,
        vec![
            "core_assist_l1",
            "core_assist_l2",
            "core_assist_l3",
            "image_style",
            "material_focuses",
            "objective_check",
            "objective_draft",
            "plan_suggest",
            "prep_activity",
            "prep_ask",
            "prep_revise",
            "siop_selfcheck",
            "sop_exit_ticket",
            "sop_objective_english",
            "sop_objectives",
            "sop_paragraph_goals",
            "sop_paragraph_tasks",
            "sop_passage_analysis",
            "sop_scaffolds",
            "sop_warmup",
            "teacher_memory_context",
            "teacher_memory_extract",
            "worksheet_block_revise",
            "worksheet_bloom_check",
            "worksheet_bloom_level",
            "worksheet_exit_ticket",
            "worksheet_image_slots",
            "worksheet_scaffolds",
            "worksheet_tasks",
            "worksheet_template_details",
            "worksheet_template_outline",
            "worksheet_template_section",
            "worksheet_tiered_level",
            "worksheet_vocab_gloss",
            "worksheet_vocab_replace",
            "worksheet_warmup"
        ]
    );
    // Phase 1.1 seeds v2 of core_assist_l2 and plan_suggest (v1 rows are kept).
    let versions: Vec<i64> = prompts
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["version_no"].as_i64().unwrap())
        .collect();
    assert_eq!(
        versions,
        // worksheet_tasks has v2 since Phase 2.7 (0014_bloom.sql), prep_activity v2 since
        // 0015_prep_bloom.sql, v3 since 0019_prep_activity_goals.sql and v4 since
        // 0023_prep_activity_siop.sql, worksheet_template_details and worksheet_template_section
        // v2 since 0016_mimic.sql, worksheet_block_revise v1 since 0020_worksheet_block_revise.sql,
        // image_style and worksheet_image_slots since 0021_worksheet_images.sql, objective_check
        // and objective_draft v1, worksheet_exit_ticket v2 and worksheet_tasks v3 since
        // 0022_siop_objectives.sql, siop_selfcheck v1 since 0024_siop_selfcheck.sql,
        // worksheet_bloom_level v2 and worksheet_tiered_level v1 since 0025_siop_differentiated.sql.
        // worksheet_image_slots v2 since 0026_images_always_drawn.sql. worksheet_vocab_gloss and
        // worksheet_vocab_replace v1 since 0028_vocab_level_actions.sql. worksheet_bloom_level v3,
        // worksheet_tasks v4, worksheet_template_section v3 and worksheet_tiered_level v2 since
        // 0029_graphic_organizers.sql. prep_revise v2, worksheet_block_revise v2,
        // worksheet_bloom_level v4, worksheet_exit_ticket v3, worksheet_tasks v5,
        // worksheet_template_section v4 and worksheet_tiered_level v3 since 0030_fill_in_blanks.sql.
        // prep_revise v3, worksheet_block_revise v3, worksheet_exit_ticket v4 and worksheet_tasks
        // v6 since 0031_sentence_frames_apart.sql. prep_revise and worksheet_block_revise v4 since
        // 0032_revise_pictures_and_uploads.sql. worksheet_bloom_level v5, worksheet_exit_ticket v5,
        // worksheet_scaffolds v2, worksheet_tasks v7, worksheet_template_details v3,
        // worksheet_template_outline v2 and worksheet_template_section v5 since
        // 0036_template_supports.sql. teacher_memory_extract v1 since 0040_teacher_memory.sql and
        // teacher_memory_context v1 since 0042_teacher_memory_hardening.sql (both v2 since
        // 0046_memory_in_conversation.sql; teacher_memory_extract v3 since
        // 0047_memory_mixed_requests.sql). worksheet_block_revise
        // v5 since 0044_block_revise_scaffolds.sql and v6 since 0049_word_table_position.sql.
        // sop_exit_ticket, sop_objective_english,
        // sop_paragraph_goals, sop_paragraph_tasks, sop_scaffolds and sop_warmup v1 since
        // 0045_sop_every_step.sql. objective_draft v2, prep_activity v5 and sop_objectives v2 since
        // 0048_language_objective_after_content.sql. sop_paragraph_tasks v2, worksheet_bloom_level
        // v6, worksheet_tasks v8, worksheet_template_section v6 and worksheet_tiered_level v4 since
        // 0051_table_organizer_headers.sql.
        vec![
            1, 2, 1, 1, 1, 1, 2, 2, 5, 1, 4, 1, 1, 1, 2, 1, 2, 1, 1, 1, 2, 3, 6, 1, 6, 5, 2, 2, 8,
            3, 2, 6, 4, 1, 1, 1
        ]
    );
    let v1_kept: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM prompt_versions WHERE version_no = 1
            AND prompt_key IN ('core_assist_l2', 'plan_suggest')",
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(v1_kept, 2);
    let (s, created) = app
        .post(
            "/api/admin/prompts",
            Some(&pi),
            json!({ "prompt_key": "plan_suggest", "system_text": "新版系統提示", "user_template": "{{lesson_title}}\n{{steps}}\n{{knowledge_items}}\n{{allowed_codes}}" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{created}");
    assert_eq!(created["version_no"], 3);
    let (_, listed) = app.get("/api/admin/prompts", Some(&pi)).await;
    let plan = listed["prompts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["prompt_key"] == "plan_suggest")
        .unwrap();
    assert_eq!(plan["version_no"], 3);
    assert_eq!(plan["system_text"], "新版系統提示");
    let (s, _) = app
        .post(
            "/api/admin/prompts",
            Some(&pi),
            json!({ "prompt_key": "Bad Key", "system_text": "s", "user_template": "u" }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // Knowledge import.
    let csv = "item_code,category,school_track,title,content,source_doc,source_version,source_page\n\
               IMP-001,vocab,general,片語動詞 phrasal verbs,\"look up, give up, take off 等片語動詞\",測試文件,v1,p.1\n\
               IMP-002,grammar,vocational,技高測試條目,只給技高,測試文件,v1,p.2\n";
    let (s, v) = app
        .post_csv("/api/admin/knowledge/import", Some(&pi), csv)
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["imported"], 2);
    assert_eq!(v["sha256"], sha256_hex(csv));
    let (s, v) = app
        .post_csv("/api/admin/knowledge/import", Some(&pi), csv)
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["imported"], 2);
    let versions: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM knowledge_items WHERE item_code = 'IMP-001'")
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(
        versions, 1,
        "re-importing an unchanged row creates no new version"
    );
    let changed = csv.replace("等片語動詞", "等常用片語動詞");
    let (_, v) = app
        .post_csv("/api/admin/knowledge/import", Some(&pi), &changed)
        .await;
    assert_eq!(v["sha256"], sha256_hex(&changed));
    let rows: Vec<(i32, bool, String, String)> = sqlx::query_as(
        "SELECT version, retired_at IS NULL, source_sha256, approved_by FROM knowledge_items
          WHERE item_code = 'IMP-001' ORDER BY version",
    )
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert_eq!(rows.len(), 2);
    assert!(!rows[0].1, "old version retired");
    assert!(rows[1].1);
    assert_eq!(rows[1].2, sha256_hex(&changed));
    assert_eq!(rows[1].3, "PI-001");
    let (s, v) = app
        .post_csv(
            "/api/admin/knowledge/import",
            Some(&pi),
            "item_code,title\nX,Y\n",
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(v["error"]["message"].as_str().unwrap().contains("缺少欄位"));

    // Imported items are retrievable and citable.
    let lid = lesson_with_plan(&app, &t, "Phrasal verbs 片語動詞", "").await;
    let (_, r) = app
        .post(
            &format!("/api/lessons/{lid}/suggestions"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(r["status"], "shown", "{r}");
    let cited: Vec<&str> = r["sources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["item_code"].as_str().unwrap())
        .collect();
    assert!(cited.contains(&"IMP-001"), "{r}");
    assert_eq!(r["sources"][0]["source_page"], "p.1");
    assert!(!cited.contains(&"IMP-002"));

    // Teacher cases import + listing (approved only, track-filtered, keyword search).
    let cases = "case_code,school_track,grade,topic,summary,details,approved\n\
                 C-1,both,高二,閱讀策略,用圖表整理段落,細節一,true\n\
                 C-2,vocational,高一,口說,餐廳角色扮演,細節二,true\n\
                 C-3,general,高三,寫作,未核准的案例,細節三,false\n";
    let (s, v) = app
        .post_csv("/api/admin/teacher-cases/import", Some(&pi), cases)
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["imported"], 3);
    assert_eq!(v["sha256"], sha256_hex(cases));
    let (s, list) = app.get("/api/teacher-cases", Some(&t)).await;
    assert_eq!(s, StatusCode::OK);
    let codes: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["case_code"].as_str().unwrap())
        .collect();
    assert_eq!(codes, vec!["C-1"]);
    assert_eq!(list[0]["topic"], "閱讀策略");
    let voc = app.teacher("T-J", "vocational").await;
    let (_, list) = app.get("/api/teacher-cases", Some(&voc)).await;
    assert_eq!(list.as_array().unwrap().len(), 2);
    let (_, list) = app
        .get("/api/teacher-cases?q=%E8%A7%92%E8%89%B2", Some(&voc))
        .await; // 角色
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["case_code"], "C-2");
    let in_rag: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM teacher_cases WHERE in_rag")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(in_rag, 0, "teacher cases stay out of RAG by default");
    let (s, _) = app
        .post_csv(
            "/api/admin/teacher-cases/import",
            Some(&pi),
            "case_code,school_track,grade,topic,summary,details,approved\nC-9,both,,,s,,perhaps\n",
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // Scaffold events.
    let (s, v) = app
        .post(
            "/api/scaffold-events",
            Some(&t),
            json!({ "scaffold_code": "SC-SIOP", "action": "expand", "trigger": "lesson_page", "dwell_ms": 1200 }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, _) = app
        .post(
            "/api/scaffold-events",
            Some(&t),
            json!({ "scaffold_code": "SC-NOPE", "action": "show" }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = app
        .post(
            "/api/scaffold-events",
            Some(&t),
            json!({ "scaffold_code": "SC-RAG", "action": "dance" }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------- AI disabled

#[tokio::test]
async fn ai_disabled_returns_disabled_status() {
    let app = test_app!(|c: &mut manna_api::config::Config| {
        c.ai_provider = AiProviderKind::None;
        c.ai_model = String::new();
    });
    let (_, cfg) = app.get("/api/config", None).await;
    assert_eq!(cfg["ai_enabled"], false);
    assert_eq!(cfg["ai_provider"], "none");

    let t = app.teacher("T-K", "general").await;
    let unit = app.create_unit(&t, "Unit").await;
    let focus = setup_focus(&app, &t, unit, "重點", "內容").await;
    let (s, r) = app
        .post(
            "/api/core/assist",
            Some(&t),
            json!({ "unit_id": unit, "focus_id": focus, "question_id": "Q1", "level": 1 }),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(r["status"], "disabled");
    assert_eq!(r["interaction_id"], Value::Null);
    assert!(r["message"].as_str().unwrap().contains("手動完成"));
    let lid = app.create_lesson(&t, unit, "Inversion").await;
    let (_, r) = app
        .post(
            &format!("/api/lessons/{lid}/suggestions"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(r["status"], "disabled");
    assert_eq!(r["context"], json!([]), "nothing was sent");
    let (s, m) = app
        .post(
            &format!("/api/units/{unit}/materials"),
            Some(&t),
            json!({ "text": "Not only is it fun, but it is also useful." }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{m}");
    let (s, r) = app
        .post(
            &format!("/api/units/{unit}/material-focuses"),
            Some(&t),
            json!({ "material_id": m["id"] }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["status"], "disabled");
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ai_interactions")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(n, 0);
}
