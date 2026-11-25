//! Issue #82 (docs/TEACHER_MEMORY.md): confirmed teaching habits, their history, the PI switch
//! (off: nothing changes; on: habits in the generation prompts and the option checks relaxed per
//! confirmed habit), the optional Graphiti index and the internal model forwarder.
#[macro_use]
mod common;

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use manna_api::ai::pipeline::InteractionMeta;
use manna_api::ai::LlmRequest;
use manna_api::memory::{self, graph};
use serde_json::{json, Value};

const SERVICE: &str = "service-secret";
const PROXY: &str = "http://manna-api:8081/api/internal/memory/v1";

async fn preview(app: &common::TestApp, token: &str, text: &str) -> Value {
    // Candidate extraction is a deterministic fixture; graph tests independently configure
    // a forwardable provider, whose real calls belong to the sidecar/forwarder tests.
    let mut fixture = app.state.ai.active();
    fixture.kind = manna_api::config::AiProviderKind::Fake;
    fixture.model = "fake".into();
    let (s, v) = fixture
        .scope(app.post(
            "/api/me/memories/preview",
            Some(token),
            json!({"text":text}),
        ))
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
async fn teacher_id(app: &common::TestApp, research_id: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM teachers WHERE research_id=$1")
        .bind(research_id)
        .fetch_one(&app.db)
        .await
        .unwrap()
}
async fn switch(app: &common::TestApp, on: bool) {
    let pi = app.pi_token().await;
    let (s, v) = app
        .put(
            "/api/admin/memory-settings",
            Some(&pi),
            json!({"enabled":on}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
}
fn step(teacher_id: i64, module: &'static str) -> (InteractionMeta, LlmRequest) {
    (
        InteractionMeta {
            teacher_id,
            module,
            unit_id: None,
            lesson_id: None,
            scaffold_id: None,
            prompt_version_id: None,
        },
        LlmRequest {
            system: "原本的系統提示詞".into(),
            user: "本次要求".into(),
            schema_name: "x",
            schema: json!({}),
            fake_context: json!({"task": module}),
            attempt: 1,
            max_tokens: Some(100),
            temperature: None,
            images: vec![],
        },
    )
}
async fn count(app: &common::TestApp, sql: &str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(&app.db).await.unwrap()
}

// ------------------------------------------------------------------ stub sidecar

struct Stub {
    api_version: AtomicU32,
    policy: Mutex<String>,
    graphiti: Mutex<String>,
    health_calls: AtomicUsize,
    index_bodies: Mutex<Vec<Value>>,
    index_failures: AtomicUsize,
    index_gate: AtomicBool,
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
    search_bodies: Mutex<Vec<Value>>,
    /// `None`: rank the sent ids in reverse, group ok.
    search_reply: Mutex<Option<Value>>,
    groups: Mutex<Vec<String>>,
    deleted: Mutex<Vec<String>>,
}

impl Stub {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            api_version: AtomicU32::new(memory::SIDECAR_API_VERSION),
            policy: Mutex::new(memory::POLICY_VERSION.into()),
            graphiti: Mutex::new("0.99.0".into()),
            health_calls: AtomicUsize::new(0),
            index_bodies: Mutex::new(vec![]),
            index_failures: AtomicUsize::new(0),
            index_gate: AtomicBool::new(false),
            entered: tokio::sync::Notify::new(),
            release: tokio::sync::Notify::new(),
            search_bodies: Mutex::new(vec![]),
            search_reply: Mutex::new(None),
            groups: Mutex::new(vec![]),
            deleted: Mutex::new(vec![]),
        })
    }
    fn contract(&self) -> Value {
        json!({"ok":true,"api_version":self.api_version.load(Ordering::SeqCst),
            "graphiti_version":self.graphiti.lock().unwrap().clone(),
            "policy_version":self.policy.lock().unwrap().clone()})
    }
}

async fn serve(stub: Arc<Stub>) -> (String, tokio::task::JoinHandle<()>) {
    let router = Router::new()
        .route(
            "/health",
            get(|State(s): State<Arc<Stub>>| async move {
                s.health_calls.fetch_add(1, Ordering::SeqCst);
                Json(s.contract())
            }),
        )
        .route(
            "/index",
            post(
                |State(s): State<Arc<Stub>>, Json(v): Json<Value>| async move {
                    s.index_bodies.lock().unwrap().push(v);
                    if s.index_gate.load(Ordering::SeqCst) {
                        s.entered.notify_one();
                        s.release.notified().await;
                    }
                    if s.index_failures.load(Ordering::SeqCst) > 0 {
                        s.index_failures.fetch_sub(1, Ordering::SeqCst);
                        return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"ok":false})));
                    }
                    (StatusCode::OK, Json(s.contract()))
                },
            ),
        )
        .route(
            "/search",
            post(
                |State(s): State<Arc<Stub>>, Json(v): Json<Value>| async move {
                    s.search_bodies.lock().unwrap().push(v.clone());
                    let reply = s.search_reply.lock().unwrap().clone();
                    Json(reply.unwrap_or_else(|| {
                        let mut ids = v["memory_ids"].as_array().cloned().unwrap_or_default();
                        ids.reverse();
                        json!({"memory_ids": ids, "group": "ok"})
                    }))
                },
            ),
        )
        .route(
            "/groups",
            get(|State(s): State<Arc<Stub>>| async move {
                Json(json!({"groups": s.groups.lock().unwrap().clone()}))
            }),
        )
        .route(
            "/groups/{id}",
            delete(
                |State(s): State<Arc<Stub>>, Path(id): Path<String>| async move {
                    s.deleted.lock().unwrap().push(id);
                    Json(json!({"ok":true}))
                },
            ),
        )
        .with_state(stub);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (url, server)
}

fn memory_config(url: &str) -> graph::MemoryConfig {
    graph::MemoryConfig {
        url: Some(url.into()),
        token: Some(SERVICE.into()),
        proxy_url: Some(PROXY.into()),
        health_ttl: Duration::ZERO,
        warning: None,
    }
}

/// The sidecar stub never forwards model requests. A loopback closed endpoint ensures
/// accidental upstream calls fail locally rather than reaching an external provider.
fn configure_index(c: &mut manna_api::config::Config, url: &str) {
    c.memory = memory_config(url);
    c.ai_provider = manna_api::config::AiProviderKind::OpenAiCompatible;
    c.openai_compat_base_url = Some("http://127.0.0.1:9/v1".into());
    c.embedding.provider = manna_api::embedding::EmbeddingProviderKind::OpenAiCompatible;
    c.embedding.base_url = Some("http://127.0.0.1:9/v1".into());
}

// ------------------------------------------------------------------ Phase 1

#[tokio::test]
async fn confirmed_habits_keep_history_cancel_is_recorded_and_teacher_scoped() {
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
            Some(&b),
            json!({"candidate_id":c["id"],"confirmed":true}),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
    // 取消 reaches the server: recorded as the teacher's rejection, and final.
    let cancel = json!({"candidate_id":c["id"],"confirmed":false});
    let (s, v) = app.post("/api/me/memories", Some(&a), cancel.clone()).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["cancelled"], true);
    let (s, _) = app.post("/api/me/memories", Some(&a), cancel).await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, _) = app
        .post(
            "/api/me/memories",
            Some(&a),
            json!({"candidate_id":c["id"],"confirmed":true}),
        )
        .await;
    assert_eq!(
        s,
        StatusCode::CONFLICT,
        "a cancelled preview cannot be confirmed"
    );
    let (decision, note): (String, String) = sqlx::query_as(
        "SELECT d.decision, d.reason_note FROM teacher_decisions d
         JOIN memory_candidates c ON c.interaction_id = d.ai_interaction_id WHERE c.id = $1",
    )
    .bind(c["id"].as_i64().unwrap())
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!((decision.as_str(), note.as_str()), ("reject", "cancelled"));

    let old = confirm(
        &app,
        &a,
        &preview(&app, &a, "之後選擇題都用三個選項。").await,
    )
    .await;
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
    let research: Vec<Value> = sqlx::query_scalar(
        "SELECT json_build_object('in', input_snapshot, 'out', output)::jsonb FROM ai_interactions
         WHERE module='teacher_memory_extract'",
    )
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert!(!json!(research).to_string().contains("王小明"));
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
    // Usage and decisions like other AI suggestions.
    for (event, n) in [
        ("memory_confirmed", 2),
        ("memory_cancelled", 1),
        ("memory_removed", 1),
    ] {
        let found: i64 = sqlx::query_scalar("SELECT count(*) FROM usage_logs WHERE event=$1")
            .bind(event)
            .fetch_one(&app.db)
            .await
            .unwrap();
        assert_eq!(found, n, "{event}");
    }
    assert_eq!(
        count(
            &app,
            "SELECT count(*) FROM teacher_decisions WHERE decision='adopt' AND reason_note='confirmed'"
        )
        .await,
        2
    );
    // No Graphiti configured: nothing is queued, so the admin counter is zero.
    assert_eq!(count(&app, "SELECT count(*) FROM memory_indexes").await, 0);
    let pi = app.pi_token().await;
    let (_, status) = app.get("/api/admin/memory-index", Some(&pi)).await;
    assert_eq!(
        (status["pending"].clone(), status["failed"].clone()),
        (json!(0), json!(0))
    );
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

/// The newest `teacher_memory_extract` row: status, output, check results, error detail.
async fn last_extract(
    app: &common::TestApp,
) -> (String, Option<Value>, Option<Value>, Option<String>) {
    sqlx::query_as(
        "SELECT status, output, check_results, error_detail FROM ai_interactions
         WHERE module='teacher_memory_extract' ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&app.db)
    .await
    .unwrap()
}

#[tokio::test]
async fn habit_sorting_keeps_wellformed_rejections_and_classifies_failures() {
    let app = test_app!();
    let a = app.teacher("T-MEM", "general").await;
    for (text, reasons, value) in [
        ("這次選擇題用三個選項。", json!(["not_persistent"]), "3"),
        (
            "之後選擇題六個選項",
            json!(["value_not_allowed_for_kind"]),
            "6",
        ),
        ("之後選擇題 FAKE_UNSUPPORTED", json!(["unsupported"]), "3"),
    ] {
        let (s, _) = app
            .post("/api/me/memories/preview", Some(&a), json!({"text":text}))
            .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{text}");
        let (status, output, checks, detail) = last_extract(&app).await;
        assert_eq!(status, "blocked");
        // Only closed codes, flags and an id: kept with the server's reasons.
        let output = output.expect("well-formed answer kept");
        assert_eq!(output["value"], value);
        assert_eq!(checks.unwrap()["reasons"], reasons, "{text}");
        assert_eq!(detail, None);
    }
    for (text, status, detail) in [
        ("之後 FAKE_FREE_TEXT", "blocked", "bad_format:"),
        ("之後 FAKE_TIMEOUT", "timeout", "timeout:"),
        ("之後 FAKE_REFUSAL", "blocked", "refusal:"),
        ("之後 FAKE_ERROR", "error", "http_error: HTTP 500"),
    ] {
        let (s, _) = app
            .post("/api/me/memories/preview", Some(&a), json!({"text":text}))
            .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{text}");
        let (got, output, _, error_detail) = last_extract(&app).await;
        assert_eq!(got, status, "{text}");
        assert_eq!(output, None, "{text}: no free text is stored");
        let error_detail = error_detail.unwrap();
        assert!(error_detail.starts_with(detail), "{text}: {error_detail}");
        // Provider text (which could quote the draft) is never stored.
        assert!(!error_detail.contains("fake"), "{error_detail}");
    }
    let (s, _) = app
        .post("/api/me/memories/preview", Some(&a), json!({"text":""}))
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
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
async fn reconfirming_an_identical_active_habit_changes_nothing() {
    let stub = Stub::new();
    let (url, server) = serve(stub.clone()).await;
    let app = test_app!(|c: &mut manna_api::config::Config| configure_index(c, &url));
    let a = app.teacher("T-SAME", "general").await;
    let id = confirm(&app, &a, &preview(&app, &a, "之後三個選項").await).await;
    assert_eq!(
        count(&app, "SELECT revision FROM memory_indexes").await,
        1,
        "configured index: the change is queued"
    );
    let again = preview(&app, &a, "之後選擇題三個選項").await;
    assert_eq!(again["replaces_id"], id);
    let (s, v) = app
        .post(
            "/api/me/memories",
            Some(&a),
            json!({"candidate_id":again["id"],"confirmed":true}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v, json!({"id": id, "unchanged": true}));
    assert_eq!(
        count(&app, "SELECT count(*) FROM teacher_memories").await,
        1
    );
    assert_eq!(
        count(&app, "SELECT revision FROM memory_indexes").await,
        1,
        "no re-index"
    );
    assert_eq!(
        count(
            &app,
            "SELECT count(*) FROM teacher_decisions WHERE reason_note='already_active'"
        )
        .await,
        1
    );
    server.abort();
}

#[tokio::test]
async fn the_history_trigger_guards_inserts_truncate_and_successors() {
    let app = test_app!();
    let a = app.teacher("T-TRIG-A", "general").await;
    let b = app.teacher("T-TRIG-B", "general").await;
    let mine = confirm(&app, &a, &preview(&app, &a, "之後三個選項").await).await;
    let theirs = confirm(&app, &b, &preview(&app, &b, "之後三個選項").await).await;
    let (ta, interaction): (i64, i64) =
        sqlx::query_as("SELECT teacher_id, interaction_id FROM teacher_memories WHERE id=$1")
            .bind(mine)
            .fetch_one(&app.db)
            .await
            .unwrap();
    let insert = |valid_at: &'static str, extra: &'static str| {
        format!(
            "INSERT INTO teacher_memories (teacher_id,kind,value,statement,valid_at,interaction_id{})
             VALUES ($1,'matching_pairs','5','配對題使用 5 組配對。',{valid_at},$2{})",
            if extra.is_empty() { "" } else { ",invalid_at,invalidation_reason" },
            extra
        )
    };
    for (sql, why) in [
        (insert("now() - interval '1 day'", ""), "back-dated"),
        (insert("now() + interval '1 day'", ""), "future"),
        (
            insert("clock_timestamp()", ",clock_timestamp(),'removed'"),
            "pre-invalidated",
        ),
    ] {
        let r = sqlx::query(&sql)
            .bind(ta)
            .bind(interaction)
            .execute(&app.db)
            .await;
        assert!(r.is_err(), "{why} insert must fail");
    }
    // A row confirmed now is accepted.
    sqlx::query(&insert("clock_timestamp()", ""))
        .bind(ta)
        .bind(interaction)
        .execute(&app.db)
        .await
        .unwrap();
    assert!(sqlx::query("TRUNCATE teacher_memories CASCADE")
        .execute(&app.db)
        .await
        .is_err());
    // A successor must be the same teacher's next version of the habit (checked at commit).
    let mut tx = app.db.begin().await.unwrap();
    let r = sqlx::query(
        "UPDATE teacher_memories SET invalid_at=clock_timestamp(),superseded_by=$2,
         invalidation_reason='replaced' WHERE id=$1",
    )
    .bind(mine)
    .bind(theirs)
    .execute(&mut *tx)
    .await;
    let failed = match r {
        Err(_) => true,
        Ok(_) => tx.commit().await.is_err(),
    };
    assert!(failed, "another teacher's row cannot supersede this one");
    assert_eq!(
        count(
            &app,
            "SELECT count(*) FROM teacher_memories WHERE invalid_at IS NULL"
        )
        .await,
        3
    );
}

// ------------------------------------------------------------------ the PI switch

const RECEIVING: [&str; 15] = [
    "worksheet_tasks",
    "worksheet_warmup",
    "worksheet_scaffolds",
    "worksheet_exit_ticket",
    "worksheet_bloom_level",
    "worksheet_tiered_level",
    "worksheet_template_section",
    "worksheet_block_revise",
    "prep_activity",
    "prep_ask",
    "prep_revise",
    "prep_chat",
    "sop_paragraph_tasks",
    "sop_warmup",
    "sop_exit_ticket",
];
const NOT_RECEIVING: [&str; 13] = [
    "worksheet_bloom_check",
    "worksheet_template_outline",
    "worksheet_template_details",
    "worksheet_vocab_gloss",
    "worksheet_vocab_replace",
    "objective_draft",
    "objective_check",
    "siop_selfcheck",
    "sop_analysis",
    "sop_objectives",
    "sop_objective_english",
    "sop_paragraph_goals",
    "sop_scaffolds",
];

#[tokio::test]
async fn the_switch_off_leaves_every_prompt_and_check_unchanged_and_on_applies_to_listed_steps() {
    let app = test_app!();
    let a = app.teacher("T-GATE", "general").await;
    let teacher = teacher_id(&app, "T-GATE").await;
    let ids = [
        confirm(&app, &a, &preview(&app, &a, "之後選擇題三個選項").await).await,
        confirm(&app, &a, &preview(&app, &a, "之後配對兩組").await).await,
        confirm(
            &app,
            &a,
            &preview(&app, &a, "之後不要用 Which of the following").await,
        )
        .await,
    ];
    let snapshot = json!({"prompt": {"system": "原本的系統提示詞"}});
    // ---- off (the default)
    for module in RECEIVING.iter().chain(NOT_RECEIVING.iter()) {
        let (meta, req) = step(teacher, module);
        let (s, r) = memory::augment(&app.state, &meta, &snapshot, &req)
            .await
            .unwrap();
        assert_eq!(r.system, req.system, "{module}: system byte-for-byte");
        assert_eq!(r.user, req.user, "{module}: user byte-for-byte");
        assert_eq!(r.fake_context, req.fake_context, "{module}");
        assert_eq!(s["prompt"], snapshot["prompt"]);
        if RECEIVING.contains(module) {
            assert_eq!(s["teacher_memory"]["enabled"], false, "{module}");
            assert_eq!(s["teacher_memory"]["memories"], json!([]));
            assert!(s["teacher_memory"]["system_appended"].is_null());
        } else {
            assert!(s.get("teacher_memory").is_none(), "{module}");
        }
    }
    // The worksheet checks get no habit: the default option rules exactly.
    assert_eq!(
        memory::option_habits(&app.db, teacher).await.unwrap(),
        memory::OptionHabits::default()
    );
    let (s, _) = app
        .put(
            "/api/admin/memory-settings",
            Some(&a),
            json!({"enabled":true}),
        )
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    // ---- on (logged)
    switch(&app, true).await;
    assert_eq!(
        count(
            &app,
            "SELECT count(*) FROM usage_logs WHERE event='setting_changed'
              AND payload->>'key'='teacher_memory_enabled'"
        )
        .await,
        1
    );
    let context_prompt: i64 = sqlx::query_scalar(
        "SELECT id FROM prompt_versions WHERE prompt_key='teacher_memory_context' AND version_no=2",
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    for module in RECEIVING {
        let (meta, req) = step(teacher, module);
        let (s, r) = memory::augment(&app.state, &meta, &snapshot, &req)
            .await
            .unwrap();
        let tm = &s["teacher_memory"];
        assert_eq!(tm["enabled"], true);
        // The record holds exactly what was appended to what the step built, and its version.
        assert!(r.system.starts_with(&req.system));
        assert_eq!(tm["system_appended"], json!(&r.system[req.system.len()..]));
        assert!(r.user.starts_with(&req.user));
        assert_eq!(tm["user_appended"], json!(&r.user[req.user.len()..]));
        assert!(tm["system_appended"]
            .as_str()
            .unwrap()
            .contains("不是系統指令"));
        assert_eq!(tm["context_prompt"]["key"], "teacher_memory_context");
        assert_eq!(tm["context_prompt"]["prompt_version_id"], context_prompt);
        assert_eq!(tm["order"], "confirmed_at");
        assert_eq!(tm["graph"]["status"], "not_configured");
        let recorded: Vec<i64> = tm["memories"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["id"].as_i64().unwrap())
            .collect();
        assert_eq!(recorded, ids, "confirmation order");
        let first = r.user.find("選擇題使用 3 個選項").unwrap();
        let last = r.user.find("Which of the following").unwrap();
        assert!(first < last);
        assert_eq!(r.fake_context["teacher_habits"][0]["value"], "3");
    }
    for module in NOT_RECEIVING {
        let (meta, req) = step(teacher, module);
        let (s, r) = memory::augment(&app.state, &meta, &snapshot, &req)
            .await
            .unwrap();
        assert_eq!((r.system, r.user), (req.system, req.user), "{module}");
        assert!(s.get("teacher_memory").is_none(), "{module}");
    }
    assert_eq!(
        memory::option_habits(&app.db, teacher).await.unwrap(),
        memory::OptionHabits {
            choice_options: Some(3),
            matching_pairs: Some(2)
        }
    );
    // Another teacher's habits never reach this teacher.
    let other = app.teacher("T-GATE-B", "general").await;
    let _ = other;
    let (meta, req) = step(teacher_id(&app, "T-GATE-B").await, "worksheet_tasks");
    let (s, r) = memory::augment(&app.state, &meta, &snapshot, &req)
        .await
        .unwrap();
    assert_eq!(r.user, req.user);
    assert_eq!(s["teacher_memory"]["memories"], json!([]));
}

#[tokio::test]
async fn a_prompt_try_applies_the_leads_habits_exactly_like_a_real_run() {
    let app = test_app!();
    let pi = app.pi_token().await;
    confirm(&app, &pi, &preview(&app, &pi, "之後選擇題三個選項").await).await;
    let passage = "Honeybees communicate by dancing.\n\nOther bees follow the dance.";
    let try_key = |key: &'static str| {
        let app = &app;
        let pi = pi.clone();
        async move {
            let (_, versions) = app
                .get(&format!("/api/admin/prompts/{key}/versions"), Some(&pi))
                .await;
            let (s, v) = app
                .post(
                    &format!("/api/admin/prompts/{key}/try"),
                    Some(&pi),
                    json!({"system_text": versions[0]["system_text"],
                        "user_template": versions[0]["user_template"],
                        "sample": {"passage": passage, "question": "這篇適合教什麼？"}}),
                )
                .await;
            assert_eq!(s, StatusCode::OK, "{key}: {v}");
            v
        }
    };
    let off = try_key("prep_ask").await;
    assert_eq!(off["teacher_memory"]["enabled"], false);
    assert!(!off["user"].as_str().unwrap().contains("teacher_habits"));
    let interactions = count(&app, "SELECT count(*) FROM ai_interactions").await;
    switch(&app, true).await;
    for key in ["prep_ask", "worksheet_warmup"] {
        let on = try_key(key).await;
        let tm = &on["teacher_memory"];
        assert_eq!(tm["enabled"], true, "{key}");
        let user = on["user"].as_str().unwrap();
        assert!(
            user.ends_with(tm["user_appended"].as_str().unwrap()),
            "{key}"
        );
        assert!(user.contains("選擇題使用 3 個選項"));
        let system = on["system"].as_str().unwrap();
        assert!(
            system.ends_with(tm["system_appended"].as_str().unwrap()),
            "{key}"
        );
    }
    // A step that never receives habits in a real run does not get them in a try either.
    let draft = try_key("objective_draft").await;
    assert!(draft.get("teacher_memory").is_none());
    assert!(!draft["user"].as_str().unwrap().contains("teacher_habits"));
    assert_eq!(
        count(&app, "SELECT count(*) FROM ai_interactions").await,
        interactions,
        "a try is still a preview"
    );
}

async fn generate(app: &common::TestApp, t: &str, id: i64) -> Value {
    let (s, v) = app
        .post(
            &format!("/api/worksheets/{id}/generate"),
            Some(t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    for _ in 0..600 {
        let (_, jobs) = app
            .get(&format!("/api/worksheets/{id}/jobs"), Some(t))
            .await;
        if jobs["running"] == false {
            let (s, state) = app.get(&format!("/api/worksheets/{id}"), Some(t)).await;
            assert_eq!(s, StatusCode::OK, "{state}");
            return state;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("worksheet {id} did not finish");
}

/// Option counts of the 選擇題 in the activity blocks (the `worksheet_tasks` step).
fn choice_option_counts(state: &Value) -> Vec<usize> {
    let mut out = vec![];
    for b in state["content"]["blocks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|b| b["kind"] == "activity")
    {
        for i in b["items"].as_array().into_iter().flatten() {
            if i["answer_format"] == "選擇題" {
                out.push(i["options"].as_array().map_or(0, Vec::len));
            }
        }
    }
    out
}

#[tokio::test]
async fn a_worksheet_run_uses_confirmed_habits_only_with_the_switch_on() {
    let app = test_app!();
    let t = app.teacher("T-RUN", "general").await;
    confirm(&app, &t, &preview(&app, &t, "之後選擇題三個選項").await).await;
    let text = "Mrs. Lin put an umbrella stand outside her shop on a rainy Monday. A sign said, “Take one if you need it.”\n\nHer neighbors laughed at the idea. They were sure that every umbrella would be gone within a week.\n\nIn the first month, twelve of the thirty umbrellas did disappear. However, something unexpected followed.";
    let (s, ws) = app
        .post(
            "/api/worksheets",
            Some(&t),
            json!({"material": {"title": "The Umbrella Stand", "text": text}}),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{ws}");
    let id = ws["id"].as_i64().unwrap();
    let (s, v) = app
        .put(
            &format!("/api/worksheets/{id}"),
            Some(&t),
            json!({"settings": {"material_types": [], "student_tasks": [],
                "answer_formats": ["選擇題"], "generation": [], "levels": [], "minutes": null}}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    // ---- off: the default prompts and rules (4 options), no habit recorded as used.
    let off = generate(&app, &t, id).await;
    let counts = choice_option_counts(&off);
    assert!(!counts.is_empty());
    assert!(counts.iter().all(|n| *n == 4), "{counts:?}");
    assert!(!off.to_string().contains("已確認的出題習慣"));
    let last_off: i64 = count(&app, "SELECT max(id) FROM ai_interactions").await;
    let off_rows: Vec<Value> = sqlx::query_scalar(
        "SELECT input_snapshot->'teacher_memory' FROM ai_interactions
          WHERE module LIKE 'worksheet_%' AND module <> 'worksheet_vocab_gloss'",
    )
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert!(
        off_rows.iter().all(|m| m["enabled"] == false),
        "{off_rows:?}"
    );
    // ---- on: every generation step gets the habit, the 3-option items pass without repair.
    switch(&app, true).await;
    let on = generate(&app, &t, id).await;
    let counts = choice_option_counts(&on);
    assert!(!counts.is_empty());
    assert!(counts.iter().all(|n| *n == 3), "{counts:?}");
    let rows: Vec<(String, i32, Value)> = sqlx::query_as(
        "SELECT module, attempt, input_snapshot FROM ai_interactions WHERE id > $1 ORDER BY id",
    )
    .bind(last_off)
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert!(rows.iter().any(|(m, _, _)| m == "worksheet_tasks"));
    for (module, attempt, snapshot) in &rows {
        assert_eq!(*attempt, 1, "{module}: no repair call was needed");
        // The word bank's meanings are written by a reader step, which never gets habits.
        if module == "worksheet_vocab_gloss" {
            continue;
        }
        let tm = &snapshot["teacher_memory"];
        assert_eq!(tm["enabled"], true, "{module}");
        assert!(tm["user_appended"]
            .as_str()
            .unwrap()
            .contains("選擇題使用 3 個選項"));
        // The step's own prompt record is unchanged; the appended text is beside it.
        assert!(!snapshot["prompt"]["system"]
            .as_str()
            .unwrap()
            .contains("teacher_habits"));
    }
    assert!(on.to_string().contains("已確認的出題習慣"));
}

// ------------------------------------------------------------------ Phase 2: the index

#[tokio::test]
async fn graph_outbox_retries_publishes_and_ranking_is_metadata_only() {
    let stub = Stub::new();
    let (url, server) = serve(stub.clone()).await;
    let app = test_app!(|c: &mut manna_api::config::Config| configure_index(c, &url));
    let a = app.teacher("T-GRAPH", "general").await;
    let teacher = teacher_id(&app, "T-GRAPH").await;
    let first = confirm(&app, &a, &preview(&app, &a, "之後三個選項").await).await;
    let second = confirm(
        &app,
        &a,
        &preview(&app, &a, "之後不要用 Which of the following").await,
    )
    .await;
    stub.index_failures.store(1, Ordering::SeqCst);
    assert!(graph::sync_one(&app.state).await.unwrap());
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
    assert!(graph::sync_one(&app.state).await.unwrap());
    let (_, v) = app.get("/api/me/memories", Some(&a)).await;
    assert_eq!(v["index"]["indexed_revision"], 2);
    assert!(v["index"]["error"].is_null());
    let built_by: String =
        sqlx::query_scalar("SELECT indexed_graphiti_version FROM memory_indexes")
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(built_by, "0.99.0");
    let sent = stub.index_bodies.lock().unwrap().clone();
    assert_eq!(sent.len(), 2);
    assert_ne!(sent[0]["group_id"], sent[1]["group_id"]);
    assert_eq!(sent[1]["memories"][0]["id"], first);
    assert_eq!(sent[1]["proxy_url"], PROXY);
    // The completed job's credential is revoked.
    let token = sent[1]["proxy_key"].as_str().unwrap().to_string();
    let bearer = format!("Bearer {token}");
    let (s, _) = app
        .internal_post(
            &format!("{}/embeddings", graph::INTERNAL_PREFIX),
            &[
                ("authorization", &bearer),
                (graph::SERVICE_TOKEN_HEADER, SERVICE),
                ("content-type", "application/json"),
            ],
            br#"{"input":"x"}"#.to_vec(),
        )
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    // With the switch on, the ranking (reversed by the stub) is recorded, never used for order.
    switch(&app, true).await;
    let (meta, req) = step(teacher, "worksheet_tasks");
    let (s, r) = memory::augment(&app.state, &meta, &json!({}), &req)
        .await
        .unwrap();
    let tm = &s["teacher_memory"];
    assert_eq!(tm["graph"]["status"], "ranked");
    assert_eq!(tm["graph"]["ranked_ids"], json!([second, first]));
    assert_eq!(tm["graph"]["graphiti_version"], "0.99.0");
    assert_eq!(tm["memories"][0]["id"], first);
    assert!(
        r.user.find("選擇題使用 3 個選項").unwrap()
            < r.user.find("Which of the following").unwrap()
    );
    let searched = stub.search_bodies.lock().unwrap().clone();
    assert_eq!(searched.len(), 1);
    assert!(
        !searched[0].to_string().contains("本次要求"),
        "only canonical habits are sent"
    );
    // A newer change: the published (older) group is no longer searched.
    app.delete(&format!("/api/me/memories/{second}"), Some(&a))
        .await;
    let active = memory::list(&app.db, teacher, false).await.unwrap();
    assert_eq!(
        graph::observe(&app.state, teacher, &active).await.status,
        "not_indexed"
    );
    assert!(graph::sync_one(&app.state).await.unwrap());
    // The change is incremental: only the stopped habit leaves the published group; no new
    // group is built and nothing is queued for deletion.
    let sent = stub.index_bodies.lock().unwrap().clone();
    assert_eq!(sent[1]["mode"], "full");
    let delta = sent.last().unwrap();
    assert_eq!(delta["mode"], "delta");
    assert_eq!(delta["group_id"], sent[1]["group_id"]);
    assert_eq!(delta["memories"], json!([]));
    assert_eq!(delta["remove_ids"], json!([second]));
    let published = sent[1]["group_id"].as_str().unwrap();
    let garbage: i64 =
        sqlx::query_scalar("SELECT count(*) FROM memory_index_garbage WHERE group_id=$1")
            .bind(published)
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(garbage, 0, "an incremental job replaces no group");
    let active = memory::list(&app.db, teacher, false).await.unwrap();
    assert_eq!(
        graph::observe(&app.state, teacher, &active).await.status,
        "ranked"
    );
    server.abort();
}

#[tokio::test]
async fn an_index_finished_after_a_newer_change_is_not_published() {
    let stub = Stub::new();
    let (url, server) = serve(stub.clone()).await;
    let app = test_app!(|c: &mut manna_api::config::Config| configure_index(c, &url));
    let a = app.teacher("T-RACE", "general").await;
    confirm(&app, &a, &preview(&app, &a, "之後三個選項").await).await;
    stub.index_gate.store(true, Ordering::SeqCst);
    let state = app.state.clone();
    let job = tokio::spawn(async move { graph::sync_one(&state).await.unwrap() });
    stub.entered.notified().await;
    // The teacher changes the habit while the sidecar is still indexing revision 1.
    confirm(&app, &a, &preview(&app, &a, "之後四個選項").await).await;
    stub.release.notify_one();
    assert!(job.await.unwrap());
    let (indexed, group, revision): (i64, Option<String>, i64) =
        sqlx::query_as("SELECT indexed_revision, group_id, revision FROM memory_indexes")
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(
        (indexed, group, revision),
        (0, None, 2),
        "stale snapshot not published"
    );
    let attempt = stub.index_bodies.lock().unwrap()[0]["group_id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        count(
            &app,
            &format!("SELECT count(*) FROM memory_index_garbage WHERE group_id='{attempt}'")
        )
        .await,
        1,
        "the unpublished attempt is queued for deletion"
    );
    stub.index_gate.store(false, Ordering::SeqCst);
    assert!(graph::sync_one(&app.state).await.unwrap());
    assert_eq!(
        count(&app, "SELECT indexed_revision FROM memory_indexes").await,
        2
    );
    let latest = stub.index_bodies.lock().unwrap().last().unwrap().clone();
    assert_eq!(latest["memories"][0]["value"], "4");
    server.abort();
}

#[tokio::test]
async fn a_core_upgrade_or_a_missing_group_is_rebuilt_not_searched() {
    let stub = Stub::new();
    let (url, server) = serve(stub.clone()).await;
    let app = test_app!(|c: &mut manna_api::config::Config| configure_index(c, &url));
    let a = app.teacher("T-CORE", "general").await;
    let teacher = teacher_id(&app, "T-CORE").await;
    confirm(&app, &a, &preview(&app, &a, "之後三個選項").await).await;
    assert!(graph::sync_one(&app.state).await.unwrap());
    let active = memory::list(&app.db, teacher, false).await.unwrap();
    assert_eq!(
        graph::observe(&app.state, teacher, &active).await.status,
        "ranked"
    );
    // The memory resource was upgraded to another core: the old group is never searched.
    *stub.graphiti.lock().unwrap() = "1.0.0".into();
    let searches = stub.search_bodies.lock().unwrap().len();
    let seen = graph::observe(&app.state, teacher, &active).await;
    assert_eq!(seen.status, "core_changed");
    assert!(seen.reindex_requested);
    assert_eq!(stub.search_bodies.lock().unwrap().len(), searches);
    assert!(graph::sync_one(&app.state).await.unwrap());
    let built_by: String =
        sqlx::query_scalar("SELECT indexed_graphiti_version FROM memory_indexes")
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(built_by, "1.0.0");
    for (reply, status) in [("missing", "missing_group"), ("empty", "empty_group")] {
        *stub.search_reply.lock().unwrap() = Some(json!({"memory_ids": [], "group": reply}));
        let seen = graph::observe(&app.state, teacher, &active).await;
        assert_eq!(seen.status, status);
        assert!(
            seen.reindex_requested,
            "{status}: Postgres has habits, so rebuild"
        );
        assert!(
            count(
                &app,
                "SELECT revision - indexed_revision FROM memory_indexes"
            )
            .await
                > 0
        );
        assert!(graph::sync_one(&app.state).await.unwrap());
    }
    server.abort();
}

#[tokio::test]
async fn cleanup_never_deletes_the_published_group_and_collects_orphans() {
    let stub = Stub::new();
    let (url, server) = serve(stub.clone()).await;
    let app = test_app!(|c: &mut manna_api::config::Config| configure_index(c, &url));
    let a = app.teacher("T-GC", "general").await;
    confirm(&app, &a, &preview(&app, &a, "之後三個選項").await).await;
    assert!(graph::sync_one(&app.state).await.unwrap());
    let published: String = sqlx::query_scalar("SELECT group_id FROM memory_indexes")
        .fetch_one(&app.db)
        .await
        .unwrap();
    let orphan = "manna_t9_r1_0123456789ab";
    // Even a published group queued by mistake is never deleted.
    sqlx::query(
        "INSERT INTO memory_index_garbage(group_id,not_before) VALUES ($1, now()-interval '1 hour'),
         ($2, now()-interval '1 hour')",
    )
    .bind(&published)
    .bind(orphan)
    .execute(&app.db)
    .await
    .unwrap();
    graph::collect_garbage(&app.state).await.unwrap();
    let deleted = stub.deleted.lock().unwrap().clone();
    assert_eq!(deleted, [orphan]);
    // Groups nothing refers to (e.g. left empty by an older sidecar) are queued, with a delay.
    *stub.groups.lock().unwrap() = vec![
        published.clone(),
        "manna_t9_r2_aaaaaaaaaaaa".into(),
        "manna_memory".into(),
    ];
    assert_eq!(graph::sweep_orphans(&app.state).await.unwrap(), 1);
    let queued: Vec<(String, bool)> = sqlx::query_as(
        "SELECT group_id, not_before > now() FROM memory_index_garbage ORDER BY group_id",
    )
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert!(queued.contains(&("manna_t9_r2_aaaaaaaaaaaa".into(), true)));
    graph::collect_garbage(&app.state).await.unwrap();
    assert!(!stub.deleted.lock().unwrap().contains(&published));
    server.abort();
}

#[tokio::test]
async fn sidecar_health_is_cached() {
    let stub = Stub::new();
    let (url, server) = serve(stub.clone()).await;
    let mut cfg = memory_config(&url);
    cfg.health_ttl = Duration::from_secs(60);
    let app = test_app!(|c: &mut manna_api::config::Config| c.memory = cfg);
    for _ in 0..3 {
        assert!(graph::compatible_sidecar(&app.state).await.is_some());
    }
    assert_eq!(stub.health_calls.load(Ordering::SeqCst), 1);
    graph::forget_health(&app.state).await;
    assert!(graph::compatible_sidecar(&app.state).await.is_some());
    assert_eq!(stub.health_calls.load(Ordering::SeqCst), 2);
    server.abort();
}

#[tokio::test]
async fn an_incompatible_sidecar_falls_back_to_postgres_without_claiming_or_searching() {
    let stub = Stub::new();
    stub.api_version.store(99, Ordering::SeqCst);
    let (url, server) = serve(stub.clone()).await;
    let app = test_app!(|c: &mut manna_api::config::Config| configure_index(c, &url));
    let token = app.teacher("T-INCOMPAT", "general").await;
    let id = confirm(&app, &token, &preview(&app, &token, "之後三個選項").await).await;
    let teacher = teacher_id(&app, "T-INCOMPAT").await;
    switch(&app, true).await;
    for (api, policy) in [
        (99, memory::POLICY_VERSION),
        (memory::SIDECAR_API_VERSION, "teacher-habits-v0"),
    ] {
        stub.api_version.store(api, Ordering::SeqCst);
        *stub.policy.lock().unwrap() = policy.into();
        assert!(
            !graph::sync_one(&app.state).await.unwrap(),
            "{api} {policy}"
        );
        let (attempts, lease, revision): (i32, Option<String>, i64) = sqlx::query_as(
            "SELECT attempts,token_hash,revision FROM memory_indexes WHERE teacher_id=$1",
        )
        .bind(teacher)
        .fetch_one(&app.db)
        .await
        .unwrap();
        assert_eq!((attempts, lease, revision), (0, None, 1));
        let (meta, req) = step(teacher, "worksheet_tasks");
        let (snapshot, augmented) = memory::augment(&app.state, &meta, &json!({}), &req)
            .await
            .unwrap();
        assert_eq!(snapshot["teacher_memory"]["memories"][0]["id"], id);
        assert_eq!(snapshot["teacher_memory"]["graph"]["status"], "unavailable");
        assert!(augmented.user.contains("選擇題使用 3 個選項"));
    }
    assert!(stub.index_bodies.lock().unwrap().is_empty());
    assert!(stub.search_bodies.lock().unwrap().is_empty());
    // A core-only upgrade is accepted as soon as the same interface returns, no app restart.
    stub.api_version
        .store(memory::SIDECAR_API_VERSION, Ordering::SeqCst);
    *stub.policy.lock().unwrap() = memory::POLICY_VERSION.into();
    assert_eq!(
        graph::compatible_sidecar(&app.state)
            .await
            .unwrap()
            .graphiti_version,
        "0.99.0"
    );
    server.abort();
    assert!(graph::compatible_sidecar(&app.state).await.is_none());
    assert!(!graph::sync_one(&app.state).await.unwrap());
    let (meta, req) = step(teacher, "worksheet_tasks");
    let (_, augmented) = memory::augment(&app.state, &meta, &json!({}), &req)
        .await
        .unwrap();
    assert!(
        augmented.user.contains("選擇題使用 3 個選項"),
        "offline: Postgres habits"
    );
}

// ------------------------------------------------------------------ the internal forwarder

#[tokio::test]
async fn the_forwarder_is_internal_authenticated_first_allowlisted_and_audited() {
    use axum::response::IntoResponse;
    use manna_api::config::AiProviderKind;
    let upstream: Arc<Mutex<Vec<Value>>> = Arc::default();
    let captured = upstream.clone();
    let captured_embeddings = upstream.clone();
    let stub = Router::new()
        .route(
            "/v1/chat/completions",
            post(move |Json(v): Json<Value>| {
                let captured = captured.clone();
                async move {
                    captured.lock().unwrap().push(v.clone());
                    if v["messages"][0]["content"] == "invalid" {
                        return (StatusCode::OK, "not-json").into_response();
                    }
                    Json(json!({"id":"call-1","model":"reported-model",
                        "choices":[{"message":{"content":"{}"}}]}))
                    .into_response()
                }
            }),
        )
        .route(
            "/v1/embeddings",
            post(move |Json(v): Json<Value>| {
                let captured = captured_embeddings.clone();
                async move {
                    captured.lock().unwrap().push(v.clone());
                    let n = v["input"].as_array().map_or(1, Vec::len);
                    Json(json!({"model":"embed-reported","data":(0..n)
                        .map(|i| json!({"index":i,"embedding":[0.125,0.25,0.5,1.0]}))
                        .collect::<Vec<_>>()}))
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, stub).await.unwrap() });
    let upstream_url = url.clone();
    let app = test_app!(move |c: &mut manna_api::config::Config| {
        c.ai_provider = AiProviderKind::OpenAiCompatible;
        c.ai_model = "configured-model".into();
        c.openai_compat_base_url = Some(upstream_url.clone());
        c.embedding.provider = manna_api::embedding::EmbeddingProviderKind::OpenAiCompatible;
        c.embedding.base_url = Some(upstream_url.clone());
        c.embedding.model = "embed-model".into();
        c.memory = memory_config("http://127.0.0.1:9");
    });
    app.teacher("T-PROXY", "general").await;
    let teacher = teacher_id(&app, "T-PROXY").await;
    let token = manna_api::auth::new_token();
    sqlx::query(
        "INSERT INTO memory_indexes(teacher_id,token_hash,lease_until,leased_revision,leased_graphiti_version)
         VALUES($1,$2,now()+interval '1 minute',1,'0.31.2')",
    )
    .bind(teacher)
    .bind(manna_api::auth::sha256_hex(&token))
    .execute(&app.db)
    .await
    .unwrap();
    let chat = format!("{}/chat/completions", graph::INTERNAL_PREFIX);
    let embeddings = format!("{}/embeddings", graph::INTERNAL_PREFIX);
    let bearer = format!("Bearer {token}");
    let full: Vec<(&str, &str)> = vec![
        ("authorization", &bearer),
        (graph::SERVICE_TOKEN_HEADER, SERVICE),
        (graph::API_VERSION_HEADER, "2"),
        (graph::GRAPHITI_VERSION_HEADER, "0.31.2"),
        ("content-type", "application/json"),
    ];
    let without = |name: &str| -> Vec<(&str, &str)> {
        full.iter().copied().filter(|(k, _)| *k != name).collect()
    };
    let body = |v: Value| v.to_string().into_bytes();
    let ok_chat = json!({"model":"unapproved-model","max_tokens":100000,
        "messages":[{"role":"user","content":"選擇題使用 3 個選項。"}]});

    // Not on the public router at all.
    let (s, _) = app.post(&chat, Some(&token), ok_chat.clone()).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    // Both credentials are checked before the body is read: garbage gets 401, not 400.
    for headers in [
        without(graph::SERVICE_TOKEN_HEADER),
        without("authorization"),
        full.iter()
            .copied()
            .map(|(k, v)| {
                if k == graph::SERVICE_TOKEN_HEADER {
                    (k, "wrong")
                } else {
                    (k, v)
                }
            })
            .collect(),
    ] {
        let (s, _) = app
            .internal_post(&chat, &headers, b"not json".to_vec())
            .await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
    }
    // Versions: the interface header is required; the core must be the one leased.
    let (s, _) = app
        .internal_post(
            &chat,
            &without(graph::API_VERSION_HEADER),
            body(ok_chat.clone()),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
    let mut other_core = without(graph::GRAPHITI_VERSION_HEADER);
    other_core.push((graph::GRAPHITI_VERSION_HEADER, "0.31.3"));
    let (s, _) = app
        .internal_post(&chat, &other_core, body(ok_chat.clone()))
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
    // Allowlisted fields only.
    for bad in [
        json!({"messages":[{"role":"user","content":"x"}],"tools":[{"type":"function"}]}),
        json!({"messages":[{"role":"user","content":"x"}],"n":2}),
        json!({"messages":[{"role":"user","content":"x"}],"user":"someone"}),
        json!({"messages":[{"role":"user","content":"x"}],"stream":true}),
    ] {
        let (s, _) = app.internal_post(&chat, &full, body(bad.clone())).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{bad}");
    }
    assert!(
        upstream.lock().unwrap().is_empty(),
        "nothing rejected reached the model"
    );
    let (s, v) = app.internal_post(&chat, &full, body(ok_chat)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let sent = upstream.lock().unwrap()[0].clone();
    assert_eq!(sent["model"], "configured-model");
    assert_eq!(sent["n"], 1);
    assert_eq!(sent["max_tokens"], graph::MAX_CHAT_TOKENS);
    let (snapshot, output, model): (Value, Value, String) = sqlx::query_as(
        "SELECT input_snapshot,output,model_version FROM ai_interactions WHERE module='memory_graph_extract'",
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(snapshot["request"], sent);
    assert_eq!(snapshot["graphiti_version"], "0.31.2");
    assert_eq!(snapshot["graphiti_version_reported_by_sidecar"], "0.31.2");
    assert_eq!(snapshot["revision"], 1);
    assert_eq!(snapshot["job_call_no"], 1);
    assert_eq!(output, v);
    assert_eq!(model, "reported-model");
    assert!(!snapshot.to_string().contains(&token));
    assert!(!snapshot.to_string().contains(SERVICE));
    // Embeddings: bounded batches; the research row keeps dimension and hash, not the vector.
    let many: Vec<String> = (0..=graph::MAX_EMBEDDING_BATCH)
        .map(|i| i.to_string())
        .collect();
    let (s, _) = app
        .internal_post(&embeddings, &full, body(json!({"input": many})))
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, v) = app
        .internal_post(
            &embeddings,
            &full,
            body(json!({"model":"x","input":["a","b"],"encoding_format":"float"})),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["data"][0]["embedding"], json!([0.125, 0.25, 0.5, 1.0]));
    let (snapshot, output): (Value, Value) = sqlx::query_as(
        "SELECT input_snapshot, output FROM ai_interactions WHERE module='memory_graph_embedding'",
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(snapshot["request"]["model"], "embed-model");
    assert_eq!(output["data"][1]["embedding"]["dims"], 4);
    assert!(output["data"][1]["embedding"]["sha256"].is_string());
    assert!(!output.to_string().contains("0.125"));
    // A bad upstream answer is recorded as such.
    let (s, _) = app
        .internal_post(
            &chat,
            &full,
            body(json!({"messages":[{"role":"user","content":"invalid"}]})),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_GATEWAY);
    let (status, detail): (String, Option<String>) = sqlx::query_as(
        "SELECT status, error_detail FROM ai_interactions WHERE module='memory_graph_extract'
         ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(status, "error");
    assert!(detail.unwrap().starts_with("bad_format"));
    // Calls per job are capped.
    sqlx::query("UPDATE memory_indexes SET job_calls=$1")
        .bind(graph::MAX_CALLS_PER_JOB)
        .execute(&app.db)
        .await
        .unwrap();
    let calls = upstream.lock().unwrap().len();
    let (s, _) = app
        .internal_post(
            &chat,
            &full,
            body(json!({"messages":[{"role":"user","content":"x"}]})),
        )
        .await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(upstream.lock().unwrap().len(), calls);
    // An expired lease revokes the job token.
    sqlx::query("UPDATE memory_indexes SET job_calls=0, lease_until=now()-interval '1 second'")
        .execute(&app.db)
        .await
        .unwrap();
    let (s, _) = app.internal_post(&chat, &full, body(json!({}))).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    server.abort();
}

// ------------------------------------------------------------------ concurrency

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_confirms_and_removals_stay_consistent() {
    let app = test_app!();
    let a = app.teacher("T-CONC", "general").await;
    let teacher = teacher_id(&app, "T-CONC").await;
    let post = |c: &Value| {
        app.post(
            "/api/me/memories",
            Some(&a),
            json!({"candidate_id":c["id"],"confirmed":true}),
        )
    };
    // The same preview confirmed twice at once: exactly one wins.
    let c = preview(&app, &a, "之後三個選項").await;
    let ((s1, _), (s2, _)) = tokio::join!(post(&c), post(&c));
    let mut statuses = [s1, s2];
    statuses.sort();
    assert_eq!(statuses, [StatusCode::OK, StatusCode::CONFLICT]);
    assert_eq!(
        count(&app, "SELECT count(*) FROM teacher_memories").await,
        1
    );
    // Two different replacements of the same habit at once: one wins, the other is stale.
    confirm(&app, &a, &preview(&app, &a, "之後配對五組").await).await;
    let x = preview(&app, &a, "之後配對六組").await;
    let y = preview(&app, &a, "之後配對兩組").await;
    assert_eq!(x["replaces_id"], y["replaces_id"]);
    let ((s1, _), (s2, _)) = tokio::join!(post(&x), post(&y));
    let mut statuses = [s1, s2];
    statuses.sort();
    assert_eq!(statuses, [StatusCode::OK, StatusCode::CONFLICT]);
    let active: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, value FROM teacher_memories
          WHERE teacher_id=$1 AND kind='matching_pairs' AND invalid_at IS NULL",
    )
    .bind(teacher)
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert_eq!(active.len(), 1);
    assert!(active[0].1 == "6" || active[0].1 == "2");
    let current = active[0].0;
    // Confirm a replacement of X while X is being removed: one of the two, never both.
    let replacement = preview(&app, &a, "之後配對五組").await;
    assert_eq!(replacement["replaces_id"], current);
    let path = format!("/api/me/memories/{current}");
    let ((sc, _), (sr, _)) = tokio::join!(post(&replacement), app.delete(&path, Some(&a)));
    let (reason, successor): (String, Option<i64>) = sqlx::query_as(
        "SELECT invalidation_reason, superseded_by FROM teacher_memories WHERE id=$1",
    )
    .bind(current)
    .fetch_one(&app.db)
    .await
    .unwrap();
    let still_active = count(
        &app,
        "SELECT count(*) FROM teacher_memories WHERE kind='matching_pairs' AND invalid_at IS NULL",
    )
    .await;
    match (sc, sr) {
        (StatusCode::OK, StatusCode::NOT_FOUND) => {
            assert_eq!(reason, "replaced");
            assert!(successor.is_some());
            assert_eq!(still_active, 1);
        }
        (StatusCode::CONFLICT, StatusCode::OK) => {
            assert_eq!(reason, "removed");
            assert!(successor.is_none());
            assert_eq!(still_active, 0);
        }
        other => panic!("inconsistent outcome {other:?}"),
    }
}

#[tokio::test]
async fn index_readiness_follows_live_settings_without_losing_pending_work() {
    use manna_api::ai::{AiSettings, AiSource};
    use manna_api::config::AiProviderKind;
    let stub = Stub::new();
    let (url, server) = serve(stub.clone()).await;
    let app = test_app!(|c: &mut manna_api::config::Config| {
        configure_index(c, &url);
        c.ai_provider = AiProviderKind::None;
    });
    let a = app.teacher("T-READINESS", "general").await;
    let teacher = teacher_id(&app, "T-READINESS").await;
    let pi = app.pi_token().await;
    confirm(&app, &a, &preview(&app, &a, "之後三個選項").await).await;
    let (_, status) = app.get("/api/admin/memory-index", Some(&pi)).await;
    assert_eq!(status["graph_enabled"], true, "deployment is still wired");
    assert_eq!(status["providers_ready"], false);
    assert_eq!(status["pending"], 1);
    assert!(!graph::sync_one(&app.state).await.unwrap());
    assert_eq!(
        count(&app, "SELECT attempts::bigint FROM memory_indexes").await,
        0
    );
    assert!(stub.index_bodies.lock().unwrap().is_empty());

    // The stored PI override is loaded after AppState is built at startup. Its provider
    // must enable an index whose environment default was `none`.
    sqlx::query("INSERT INTO app_settings(key,value) VALUES('ai_text_model',$1)")
        .bind(json!({"provider":"openai_compatible","model":"first-model",
            "effort":"medium","timeout_secs":120}))
        .execute(&app.db)
        .await
        .unwrap();
    manna_api::ai::admin::load_override(&app.state.ai, &app.db)
        .await
        .unwrap();
    assert!(graph::providers_ready(&app.state));
    stub.index_gate.store(true, Ordering::SeqCst);
    let state = app.state.clone();
    let job = tokio::spawn(async move { graph::sync_one(&state).await });
    tokio::time::timeout(Duration::from_secs(5), stub.entered.notified())
        .await
        .unwrap();
    // A provider switch cannot change a leased indexing job's model.
    let mut off = app.state.ai.env_default().clone();
    off.kind = AiProviderKind::None;
    app.state.ai.set_active(off, AiSource::Env);
    stub.release.notify_one();
    assert!(job.await.unwrap().unwrap());
    assert_eq!(stub.index_bodies.lock().unwrap()[0]["model"], "first-model");
    assert_eq!(
        count(&app, "SELECT indexed_revision FROM memory_indexes").await,
        1
    );
    assert!(!graph::providers_ready(&app.state));

    let active = memory::list(&app.db, teacher, false).await.unwrap();
    assert_eq!(
        graph::observe(&app.state, teacher, &active).await.status,
        "unavailable"
    );
    assert!(stub.search_bodies.lock().unwrap().is_empty());
    confirm(&app, &a, &preview(&app, &a, "之後四個選項").await).await;
    assert!(!graph::sync_one(&app.state).await.unwrap());
    let (_, status) = app.get("/api/admin/memory-index", Some(&pi)).await;
    assert_eq!(status["pending"], 1);
    app.state.ai.set_active(
        AiSettings {
            kind: AiProviderKind::OpenAiCompatible,
            model: "next-model".into(),
            effort: "medium".into(),
            timeout_secs: 120,
        },
        AiSource::Admin,
    );
    stub.index_gate.store(false, Ordering::SeqCst);
    assert!(graph::sync_one(&app.state).await.unwrap());
    assert_eq!(stub.index_bodies.lock().unwrap()[1]["model"], "next-model");
    let (_, status) = app.get("/api/admin/memory-index", Some(&pi)).await;
    assert_eq!(status["providers_ready"], true);
    assert_eq!(status["pending"], 0);
    assert_eq!(
        graph::observe(
            &app.state,
            teacher,
            &memory::list(&app.db, teacher, false).await.unwrap()
        )
        .await
        .status,
        "ranked"
    );
    server.abort();
}
