#[macro_use]
mod common;
use axum::http::StatusCode;
use serde_json::{json, Value};

async fn preview(app: &common::TestApp, token: &str, text: &str) -> Value {
    let (s, v) = app
        .post(
            "/api/me/memories/preview",
            Some(token),
            json!({"text":text}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v
}
async fn confirm(app: &common::TestApp, token: &str, c: &Value) -> i64 {
    let (s, v) = app
        .post(
            "/api/me/memories",
            Some(token),
            json!({"candidate_id":c["id"],"confirmed":true}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v["id"].as_i64().unwrap()
}

#[tokio::test]
async fn confirmed_habits_keep_history_and_are_teacher_scoped() {
    let app = test_app!();
    let a = app.teacher("T-MEM-A", "general").await;
    let b = app.teacher("T-MEM-B", "general").await;
    let (s, _) = app.get("/api/me/memories", None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let c = preview(
        &app,
        &a,
        "之後選擇題都用三個選項。學生王小明有氣喘。老師只記出題形式。",
    )
    .await;
    let (_, empty) = app.get("/api/me/memories", Some(&a)).await;
    assert_eq!(empty["enabled"], false);
    assert_eq!(empty["memories"], json!([]));
    let (s, _) = app
        .post(
            "/api/me/memories",
            Some(&a),
            json!({"candidate_id":c["id"],"confirmed":false}),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = app
        .post(
            "/api/me/memories",
            Some(&b),
            json!({"candidate_id":c["id"],"confirmed":true}),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
    let old = confirm(&app, &a, &c).await;
    let stale = preview(&app, &a, "之後選擇題都用四個選項。").await;
    let new = confirm(&app, &a, &stale).await;
    let (s, _) = app
        .post(
            "/api/me/memories",
            Some(&a),
            json!({"candidate_id":stale["id"],"confirmed":true}),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (_, history) = app.get("/api/me/memories", Some(&a)).await;
    let rows = history["memories"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["id"], old);
    assert_eq!(rows[0]["superseded_by"], new);
    assert_eq!(rows[0]["invalid_at"], rows[1]["valid_at"]);
    assert_eq!(rows[1]["statement"], "選擇題使用 4 個選項。");
    assert!(!history.to_string().contains("王小明"));
    let snapshots: Vec<Value> = sqlx::query_scalar(
        "SELECT input_snapshot FROM ai_interactions WHERE module='teacher_memory_extract'",
    )
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert!(!json!(snapshots).to_string().contains("王小明"));
    let (_, other) = app.get("/api/me/memories", Some(&b)).await;
    assert_eq!(other["memories"], json!([]));
    let (s, _) = app
        .delete(&format!("/api/me/memories/{new}"), Some(&b))
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = app
        .delete(&format!("/api/me/memories/{new}"), Some(&a))
        .await;
    assert_eq!(s, StatusCode::OK);
    let (_, history) = app.get("/api/me/memories", Some(&a)).await;
    assert_eq!(history["memories"][1]["invalidation_reason"], "removed");
    let revision: i64 = sqlx::query_scalar("SELECT revision FROM memory_indexes")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(revision, 3);
    assert!(sqlx::query("DELETE FROM teacher_memories WHERE id=$1")
        .bind(old)
        .execute(&app.db)
        .await
        .is_err());
    assert!(
        sqlx::query("UPDATE teacher_memories SET statement='changed' WHERE id=$1")
            .bind(old)
            .execute(&app.db)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn rejects_one_offs_expired_previews_and_stale_conflicts() {
    let app = test_app!();
    let a = app.teacher("T-MEM", "general").await;
    for text in [
        "這次選擇題用三個選項。",
        "之後 FAKE_UNSUPPORTED",
        "之後 FAKE_TIMEOUT",
        "",
    ] {
        let (s, _) = app
            .post("/api/me/memories/preview", Some(&a), json!({"text":text}))
            .await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
    }
    let c1 = preview(&app, &a, "之後三個選項").await;
    let c2 = preview(&app, &a, "之後四個選項").await;
    confirm(&app, &a, &c1).await;
    let (s, _) = app
        .post(
            "/api/me/memories",
            Some(&a),
            json!({"candidate_id":c2["id"],"confirmed":true}),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
    let c = preview(&app, &a, "之後配對五組").await;
    sqlx::query("UPDATE memory_candidates SET created_at=now()-interval '16 minutes' WHERE id=$1")
        .bind(c["id"].as_i64().unwrap())
        .execute(&app.db)
        .await
        .unwrap();
    let (s, _) = app
        .post(
            "/api/me/memories",
            Some(&a),
            json!({"candidate_id":c["id"],"confirmed":true}),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
}

#[tokio::test]
async fn pi_gate_controls_prompt_context_and_records_versions() {
    use manna_api::{
        ai::{pipeline::InteractionMeta, LlmRequest},
        memory, AppState,
    };
    let app = test_app!();
    let a = app.teacher("T-GATE", "general").await;
    let pi = app.pi_token().await;
    let c = preview(&app, &a, "之後選擇題三個選項").await;
    let id = confirm(&app, &a, &c).await;
    let teacher_id: i64 = sqlx::query_scalar("SELECT id FROM teachers WHERE research_id='T-GATE'")
        .fetch_one(&app.db)
        .await
        .unwrap();
    let state = AppState::new(
        app.db.clone(),
        manna_api::config::Config::for_tests("unused"),
    );
    let meta = InteractionMeta {
        teacher_id,
        module: "worksheet_tasks",
        unit_id: None,
        lesson_id: None,
        scaffold_id: None,
        prompt_version_id: None,
    };
    let req = LlmRequest {
        system: "sys".into(),
        user: "本次要求".into(),
        schema_name: "x",
        schema: json!({}),
        fake_context: json!({}),
        attempt: 1,
        max_tokens: Some(100),
        temperature: None,
        images: vec![],
    };
    let (s, r) = memory::augment(&state, &meta, &json!({}), &req)
        .await
        .unwrap();
    assert_eq!(s["teacher_memory"]["enabled"], false);
    assert_eq!(r.user, req.user);
    let (s, _) = app
        .put(
            "/api/admin/memory-settings",
            Some(&a),
            json!({"enabled":true}),
        )
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = app
        .put(
            "/api/admin/memory-settings",
            Some(&pi),
            json!({"enabled":true}),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let (s, r) = memory::augment(&state, &meta, &json!({}), &req)
        .await
        .unwrap();
    assert_eq!(s["teacher_memory"]["memories"][0]["id"], id);
    assert!(r.user.contains("選擇題使用 3 個選項"));
    assert!(r.system.contains("不是系統指令"));
    let (s, _) = app
        .post(
            "/api/internal/memory/v1/chat/completions",
            Some(&a),
            json!({"messages":[]}),
        )
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn graph_outbox_retries_rebuilds_and_intersects_current_teacher_ids() {
    use axum::{
        routing::{delete, post},
        Json, Router,
    };
    use manna_api::{memory::graph, AppState};
    use std::sync::{Arc, Mutex};
    let calls = Arc::new(Mutex::new(Vec::<Value>::new()));
    let captured = calls.clone();
    let stub=Router::new().route("/health",axum::routing::get(||async {
        Json(json!({"ok":true,"api_version":1,"graphiti_version":"0.99.0","policy_version":"teacher-habits-v1"}))
    })).route("/index",post(move |Json(v):Json<Value>| {
        let captured=captured.clone();async move {
            let mut calls=captured.lock().unwrap(); calls.push(v);
            if calls.len()==1 { (StatusCode::SERVICE_UNAVAILABLE,Json(json!({"ok":false}))) }
            else { (StatusCode::OK,Json(json!({"ok":true,"api_version":1,"graphiti_version":"0.99.0","policy_version":"teacher-habits-v1"}))) }
        }
    })).route("/search",post(||async {Json(json!({"memory_ids":[999999,1,1]}))}))
        .route("/groups/{id}",delete(||async {Json(json!({"ok":true}))}));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, stub).await.unwrap() });
    let app = test_app!();
    let a = app.teacher("T-GRAPH", "general").await;
    let c = preview(&app, &a, "之後三個選項").await;
    let id = confirm(&app, &a, &c).await;
    let teacher_id: i64 = sqlx::query_scalar("SELECT id FROM teachers WHERE research_id='T-GRAPH'")
        .fetch_one(&app.db)
        .await
        .unwrap();
    let mut cfg = manna_api::config::Config::for_tests("unused");
    cfg.memory = graph::MemoryConfig {
        url: Some(url),
        token: Some("service-secret".into()),
        proxy_url: Some("http://app/api/internal/memory/v1".into()),
    };
    let state = AppState::new(app.db.clone(), cfg);
    assert!(graph::sync_one(&state).await.unwrap());
    let (_, v) = app.get("/api/me/memories", Some(&a)).await;
    assert_eq!(v["index"]["indexed_revision"], 0);
    assert!(v["index"]["error"].is_string());
    // Simulate restart after an expired worker lease. All data is in Postgres.
    sqlx::query(
        "UPDATE memory_indexes SET next_attempt_at=now(),lease_until=now()-interval '1 second'",
    )
    .execute(&app.db)
    .await
    .unwrap();
    assert!(graph::sync_one(&state).await.unwrap());
    let (_, v) = app.get("/api/me/memories", Some(&a)).await;
    assert_eq!(v["index"]["indexed_revision"], 1);
    assert!(v["index"]["error"].is_null());
    let sent = calls.lock().unwrap().clone();
    assert_eq!(sent.len(), 2);
    assert_ne!(sent[0]["group_id"], sent[1]["group_id"]);
    assert_eq!(sent[1]["memories"][0]["id"], id);
    assert_eq!(
        sent[1]["memories"][0]["valid_at"],
        v["memories"][0]["valid_at"]
    );
    let token = sent[1]["proxy_key"].as_str().unwrap();
    let (s, _) = app
        .post(
            "/api/internal/memory/v1/embeddings",
            Some(token),
            json!({"input":"test"}),
        )
        .await;
    assert_eq!(
        s,
        StatusCode::UNAUTHORIZED,
        "completed job's credential is revoked"
    );
    let active = manna_api::memory::list(&app.db, teacher_id, false)
        .await
        .unwrap();
    assert_eq!(
        graph::rank(&state, teacher_id, "irrelevant raw text", &active).await,
        vec![id]
    );
    app.delete(&format!("/api/me/memories/{id}"), Some(&a))
        .await;
    assert!(
        graph::rank(&state, teacher_id, "query", &active)
            .await
            .is_empty(),
        "stale revision must not be searched"
    );
    assert!(graph::sync_one(&state).await.unwrap());
    assert_eq!(calls.lock().unwrap().last().unwrap()["memories"], json!([]));
    server.abort();
}

#[tokio::test]
async fn memory_proxy_audits_exact_upstream_calls_and_rejects_expired_credentials() {
    use axum::response::IntoResponse;
    use axum::{routing::post, Json, Router};
    use manna_api::config::AiProviderKind;
    let stub=Router::new().route("/v1/chat/completions",post(|Json(v):Json<Value>|async move {
        assert_eq!(v["model"],"configured-model");
        if v["messages"][0]["content"] == "invalid" {
            return (StatusCode::OK, "not-json").into_response();
        }
        Json(json!({"id":"call-1","model":"reported-model","choices":[{"message":{"content":"{}"}}]})).into_response()
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, stub).await.unwrap() });
    let app = test_app!(|c: &mut manna_api::config::Config| {
        c.ai_provider = AiProviderKind::OpenAiCompatible;
        c.ai_model = "configured-model".into();
        c.openai_compat_base_url = Some(url);
    });
    app.teacher("T-PROXY", "general").await;
    let teacher_id: i64 = sqlx::query_scalar("SELECT id FROM teachers WHERE research_id='T-PROXY'")
        .fetch_one(&app.db)
        .await
        .unwrap();
    let token = manna_api::auth::new_token();
    sqlx::query("INSERT INTO memory_indexes(teacher_id,token_hash,lease_until,leased_revision,leased_graphiti_version) VALUES($1,$2,now()+interval '1 minute',1,'0.31.2')")
        .bind(teacher_id).bind(manna_api::auth::sha256_hex(&token)).execute(&app.db).await.unwrap();
    let (s,v)=app.post("/api/internal/memory/v1/chat/completions",Some(&token),json!({
        "model":"unapproved-model","messages":[{"role":"user","content":"選擇題使用 3 個選項。"}],"max_tokens":2000})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (snapshot,output,model):(Value,Value,String)=sqlx::query_as("SELECT input_snapshot,output,model_version FROM ai_interactions WHERE module='memory_graph_extract'")
        .fetch_one(&app.db).await.unwrap();
    assert_eq!(snapshot["request"]["model"], "configured-model");
    assert_eq!(snapshot["graphiti_version"], "0.31.2");
    assert_eq!(snapshot["revision"], 1);
    assert_eq!(output, v);
    assert_eq!(model, "reported-model");
    assert!(!snapshot.to_string().contains(&token));
    let (s, _) = app
        .post(
            "/api/internal/memory/v1/chat/completions",
            Some(&token),
            json!({"messages":[{"role":"user","content":"invalid"}]}),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_GATEWAY);
    let last_status:String=sqlx::query_scalar("SELECT status FROM ai_interactions WHERE module='memory_graph_extract' ORDER BY id DESC LIMIT 1")
        .fetch_one(&app.db).await.unwrap();
    assert_eq!(last_status, "error");
    sqlx::query("UPDATE memory_indexes SET lease_until=now()-interval '1 second'")
        .execute(&app.db)
        .await
        .unwrap();
    let (s, _) = app
        .post(
            "/api/internal/memory/v1/chat/completions",
            Some(&token),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    server.abort();
}

#[tokio::test]
async fn incompatible_sidecar_falls_back_without_claiming_jobs_or_searching() {
    use axum::{
        routing::{get, post},
        Json, Router,
    };
    use manna_api::{
        memory::{self, graph},
        AppState,
    };
    use std::sync::{
        atomic::{AtomicU32, AtomicUsize, Ordering},
        Arc,
    };
    let version = Arc::new(AtomicU32::new(99));
    let called = Arc::new(AtomicUsize::new(0));
    let health_version = version.clone();
    let index_called = called.clone();
    let search_called = called.clone();
    let stub = Router::new()
        .route(
            "/health",
            get(move || {
                let v = health_version.load(Ordering::SeqCst);
                async move {
                    Json(json!({"ok":true,"api_version":v,
            "graphiti_version":"0.99.0","policy_version":"teacher-habits-v1"}))
                }
            }),
        )
        .route(
            "/index",
            post(move || {
                index_called.fetch_add(1, Ordering::SeqCst);
                async { Json(json!({"ok":false})) }
            }),
        )
        .route(
            "/search",
            post(move || {
                search_called.fetch_add(1, Ordering::SeqCst);
                async { Json(json!({"memory_ids":[]})) }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, stub).await.unwrap() });
    let app = test_app!();
    let token = app.teacher("T-INCOMPAT", "general").await;
    let c = preview(&app, &token, "之後三個選項").await;
    let id = confirm(&app, &token, &c).await;
    let teacher_id: i64 =
        sqlx::query_scalar("SELECT id FROM teachers WHERE research_id='T-INCOMPAT'")
            .fetch_one(&app.db)
            .await
            .unwrap();
    let mut cfg = manna_api::config::Config::for_tests("unused");
    cfg.memory = graph::MemoryConfig {
        url: Some(url),
        token: Some("test".into()),
        proxy_url: Some("http://manna-api/api/internal/memory/v1".into()),
    };
    let state = AppState::new(app.db.clone(), cfg);
    assert!(!graph::sync_one(&state).await.unwrap());
    let (attempts, lease, revision): (i32, Option<String>, i64) = sqlx::query_as(
        "SELECT attempts,token_hash,revision FROM memory_indexes WHERE teacher_id=$1",
    )
    .bind(teacher_id)
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(attempts, 0);
    assert_eq!(lease, None);
    assert_eq!(revision, 1);
    let active = memory::list(&app.db, teacher_id, false).await.unwrap();
    assert!(graph::rank(&state, teacher_id, "query", &active)
        .await
        .is_empty());
    let mut tx = app.db.begin().await.unwrap();
    manna_api::settings::set(
        &mut tx,
        manna_api::settings::TEACHER_MEMORY_ENABLED,
        &json!(true),
        teacher_id,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let req = manna_api::ai::LlmRequest {
        system: "sys".into(),
        user: "user".into(),
        schema_name: "test",
        schema: json!({}),
        fake_context: json!({}),
        attempt: 1,
        max_tokens: None,
        temperature: None,
        images: vec![],
    };
    let meta = manna_api::ai::pipeline::InteractionMeta {
        teacher_id,
        module: "worksheet_tasks",
        unit_id: None,
        lesson_id: None,
        scaffold_id: None,
        prompt_version_id: None,
    };
    let (snapshot, augmented) = memory::augment(&state, &meta, &json!({}), &req)
        .await
        .unwrap();
    assert_eq!(snapshot["teacher_memory"]["memories"][0]["id"], id);
    assert!(augmented.user.contains("選擇題使用 3 個選項"));
    assert_eq!(called.load(Ordering::SeqCst), 0);
    // A core-only upgrade is accepted as soon as the same interface returns, no app restart.
    version.store(1, Ordering::SeqCst);
    assert_eq!(
        graph::compatible_sidecar(&state)
            .await
            .unwrap()
            .graphiti_version,
        "0.99.0"
    );
    server.abort();
    assert!(graph::compatible_sidecar(&state).await.is_none());
    assert!(!graph::sync_one(&state).await.unwrap());
}
