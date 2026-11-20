//! 後台管理 → 平台設定 → 文字模型: the PI switches the text model without a redeploy. The
//! environment is the default; the override is tested with one small call before it is saved,
//! applies to new AI calls at once, is recorded per call in `ai_interactions`, and every change
//! is logged. Contract: docs/API.md「文字模型設定」.

#[macro_use]
mod common;

use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use manna_api::ai::{AiClient, AiSettings, AiSource};
use manna_api::config::{AiProviderKind, Config};
use serde_json::{json, Value};
use sqlx::PgPool;
use tokio::sync::Notify;

/// A stand-in for the NCHC gateway. `bad-model` answers 404 (a typo in the model name);
/// `slow-model` holds its answer until the test lets it go.
#[derive(Clone, Default)]
struct Stub {
    bodies: Arc<Mutex<Vec<Value>>>,
    arrived: Arc<Notify>,
    release: Arc<Notify>,
    held: Arc<std::sync::atomic::AtomicBool>,
}

impl Stub {
    fn models(&self) -> Vec<String> {
        self.bodies
            .lock()
            .unwrap()
            .iter()
            .map(|b| b["model"].as_str().unwrap_or_default().to_string())
            .collect()
    }
}

async fn chat(State(stub): State<Stub>, Json(v): Json<Value>) -> axum::response::Response {
    let model = v["model"].as_str().unwrap_or_default().to_string();
    stub.bodies.lock().unwrap().push(v);
    if model == "bad-model" {
        return (StatusCode::NOT_FOUND, "model not found").into_response();
    }
    // Only the first call waits (a retry of the same request must not).
    if model == "slow-model" && !stub.held.swap(true, std::sync::atomic::Ordering::SeqCst) {
        stub.arrived.notify_one();
        stub.release.notified().await;
    }
    Json(json!({"model": format!("reported-{model}"),
        "choices": [{"message": {"content": "{\"ok\":true}"}, "finish_reason": "stop"}]}))
    .into_response()
}

async fn start_stub() -> (Stub, String) {
    let stub = Stub::default();
    let router = Router::new()
        .route("/v1/chat/completions", post(chat))
        .with_state(stub.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (stub, url)
}

fn msg(v: &Value) -> &str {
    v["error"]["message"].as_str().unwrap_or_default()
}

fn put_body(provider: &str, model: &str) -> Value {
    json!({"provider": provider, "model": model, "effort": "medium", "timeout_secs": 120})
}

async fn override_row(db: &PgPool) -> Option<Value> {
    sqlx::query_scalar("SELECT value FROM app_settings WHERE key = 'ai_text_model'")
        .fetch_optional(db)
        .await
        .unwrap()
}

async fn changes(db: &PgPool) -> Vec<Value> {
    sqlx::query_scalar(
        "SELECT payload FROM usage_logs WHERE event = 'setting_changed'
            AND payload->>'key' = 'ai_text_model' ORDER BY id",
    )
    .fetch_all(db)
    .await
    .unwrap()
}

/// `(module, provider, model, status)` of every `ai_interactions` row, oldest first.
async fn interactions(db: &PgPool) -> Vec<(String, String, String, String)> {
    sqlx::query_as("SELECT module, provider, model, status FROM ai_interactions ORDER BY id")
        .fetch_all(db)
        .await
        .unwrap()
}

/// A teacher with a unit whose first core question has an answer, and the body of a core
/// assist request (a real AI call that goes through `pipeline::run`).
async fn assist_request(app: &common::TestApp) -> (String, Value) {
    let t = app.teacher("T-AI", "general").await;
    let unit = app.create_unit(&t, "Unit 1").await;
    let (s, core) = app
        .put(
            &format!("/api/units/{unit}/core/draft"),
            Some(&t),
            json!({ "focuses": [{ "position": 1, "big_idea": "倒裝句", "responses": {
                "Q1": { "answer": "學生能用倒裝句", "status": "filled", "content_source": "self" } } }] }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{core}");
    let focus = core["focuses"][0]["id"].as_i64().unwrap();
    (
        t,
        json!({ "unit_id": unit, "focus_id": focus, "question_id": "Q1", "level": 1 }),
    )
}

fn nchc(url: &str) -> impl FnOnce(&mut Config) + '_ {
    move |c: &mut Config| {
        c.openai_compat_base_url = Some(url.to_string());
        c.openai_compat_api_key = Some("stub-key".into());
    }
}

#[tokio::test]
async fn shows_the_environment_default_and_is_pi_only() {
    let (_stub, url) = start_stub().await;
    let app = test_app!(nchc(&url));
    let pi = app.pi_token().await;
    let (s, v) = app.get("/api/admin/ai-model", Some(&pi)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["source"], "env");
    assert_eq!(v["active"]["provider"], "fake");
    assert_eq!(v["active"]["model"], "fake-model");
    assert_eq!(v["active"], v["env_default"]);
    assert_eq!(v["changed_by"], Value::Null);
    assert_eq!(v["timeout_range"], json!({"min": 30, "max": 900}));
    assert_eq!(
        v["efforts"],
        json!(["low", "medium", "high", "xhigh", "max"])
    );
    let providers = v["providers"].as_array().unwrap();
    let ids: Vec<&str> = providers
        .iter()
        .map(|p| p["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        ["openai", "openai_compatible"],
        "no anthropic, fake or none"
    );
    // Only the provider the environment configures can be picked.
    assert_eq!(providers[0]["available"], false);
    assert_eq!(providers[1]["available"], true);
    assert_eq!(
        providers[0]["models"],
        json!(["gpt-5.6-sol", "gpt-5.6-terra"])
    );
    assert_eq!(providers[1]["models"], json!(["gemma-4-31B-it"]));

    // Nobody but the PI: not a teacher, not a signed-out visitor.
    let teacher = app.teacher("T-X", "general").await;
    for token in [Some(teacher.as_str()), None] {
        let want = if token.is_some() {
            StatusCode::FORBIDDEN
        } else {
            StatusCode::UNAUTHORIZED
        };
        let (s, _) = app.get("/api/admin/ai-model", token).await;
        assert_eq!(s, want);
        let (s, _) = app
            .put(
                "/api/admin/ai-model",
                token,
                put_body("openai_compatible", "x"),
            )
            .await;
        assert_eq!(s, want);
        let (s, _) = app.delete("/api/admin/ai-model", token).await;
        assert_eq!(s, want);
    }
    assert!(override_row(&app.db).await.is_none());
}

#[tokio::test]
async fn refuses_what_the_pi_may_not_choose_and_saves_nothing() {
    let (stub, url) = start_stub().await;
    let app = test_app!(nchc(&url));
    let pi = app.pi_token().await;
    let ok = put_body("openai_compatible", "gemma-4-31B-it");
    let with = |k: &str, v: Value| {
        let mut b = ok.clone();
        b[k] = v;
        b
    };
    let cases = [
        ("anthropic", with("provider", json!("anthropic"))),
        ("fake", with("provider", json!("fake"))),
        ("none", with("provider", json!("none"))),
        ("empty provider", with("provider", json!(""))),
        ("unknown provider", with("provider", json!("gemini"))),
        // OpenAI has no key in this environment.
        ("unconfigured", put_body("openai", "gpt-5.6-sol")),
        ("bad effort", with("effort", json!("ultra"))),
        ("timeout too short", with("timeout_secs", json!(29))),
        ("timeout too long", with("timeout_secs", json!(901))),
        ("empty model", with("model", json!("  "))),
        ("model with a space", with("model", json!("gpt 5"))),
        ("model with symbols", with("model", json!("a;b"))),
        ("model too long", with("model", json!("m".repeat(101)))),
        ("extra field", with("api_key", json!("x"))),
    ];
    for (name, body) in cases {
        let (s, v) = app.put("/api/admin/ai-model", Some(&pi), body).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{name}: {v}");
    }
    let (_, v) = app
        .put(
            "/api/admin/ai-model",
            Some(&pi),
            put_body("openai", "gpt-5.6-sol"),
        )
        .await;
    assert!(msg(&v).contains("還沒有設定"), "{v}");
    assert!(
        stub.models().is_empty(),
        "a refused change never calls a model"
    );
    assert!(override_row(&app.db).await.is_none());
    assert!(changes(&app.db).await.is_empty());
    assert!(interactions(&app.db).await.is_empty());
}

#[tokio::test]
async fn a_failed_test_call_saves_and_changes_nothing() {
    let (stub, url) = start_stub().await;
    let app = test_app!(nchc(&url));
    let pi = app.pi_token().await;
    let (s, v) = app
        .put(
            "/api/admin/ai-model",
            Some(&pi),
            put_body("openai_compatible", "bad-model"),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    assert!(msg(&v).contains("設定沒有儲存"), "{v}");
    assert!(msg(&v).contains("模型名稱"), "names the likely cause: {v}");
    assert_eq!(stub.models(), ["bad-model"]);
    assert!(override_row(&app.db).await.is_none());
    assert!(changes(&app.db).await.is_empty());
    let (_, v) = app.get("/api/admin/ai-model", Some(&pi)).await;
    assert_eq!(v["source"], "env");
    assert_eq!(v["active"]["model"], "fake-model");
    // The attempt is on record, honestly: its own module, the model that was tried, an error.
    assert_eq!(
        interactions(&app.db).await,
        [(
            "admin_model_test".to_string(),
            "openai_compatible".to_string(),
            "bad-model".to_string(),
            "error".to_string()
        )]
    );
}

#[tokio::test]
async fn a_tested_change_is_saved_active_at_once_recorded_per_call_and_logged() {
    let (stub, url) = start_stub().await;
    let app = test_app!(nchc(&url));
    let (pi, pi_teacher) = app.login(common::BOOTSTRAP_CODE).await;
    let (t, assist) = assist_request(&app).await;

    // Before: the environment's fake provider answers and is recorded.
    let (s, r) = app.post("/api/core/assist", Some(&t), assist.clone()).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["status"], "shown", "{r}");

    let (s, v) = app
        .put(
            "/api/admin/ai-model",
            Some(&pi),
            json!({"provider":"openai_compatible","model":" gemma-4-31B-it ",
                   "effort":"high","timeout_secs":240}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert!(v["test_latency_ms"].as_i64().is_some(), "{v}");
    assert_eq!(v["source"], "admin");
    assert_eq!(v["changed_by"], pi_teacher["research_id"]);
    assert!(v["changed_at"].is_string());
    assert_eq!(
        v["active"],
        json!({"provider":"openai_compatible","model":"gemma-4-31B-it",
               "effort":"high","timeout_secs":240})
    );
    assert_eq!(v["env_default"]["provider"], "fake");
    assert_eq!(override_row(&app.db).await.unwrap(), v["active"]);
    assert_eq!(
        stub.models(),
        ["gemma-4-31B-it"],
        "the test call used the NEW model"
    );
    let (state_now, source) = app.state.ai.active_settings();
    assert_eq!(source, AiSource::Admin);
    assert_eq!(state_now.model, "gemma-4-31B-it");
    assert_eq!(state_now.timeout_secs, 240);

    // The public config (shown in the app) follows the active model.
    let (_, cfg) = app.get("/api/config", None).await;
    assert_eq!(cfg["ai_provider"], "openai_compatible");
    assert_eq!(cfg["ai_model"], "gemma-4-31B-it");

    // A new AI call uses the override and records exactly that.
    let (s, r) = app.post("/api/core/assist", Some(&t), assist.clone()).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert!(stub.models().len() >= 2);
    assert!(stub.models().iter().all(|m| m == "gemma-4-31B-it"));
    let rows = interactions(&app.db).await;
    // The first call (before the change) is the fake provider's; the test call and every
    // call after it are the new model's, each row carrying what answered it.
    assert_eq!(
        (rows[0].0.as_str(), rows[0].1.as_str(), rows[0].2.as_str()),
        ("core_assist", "fake", "fake-model")
    );
    assert_eq!(
        (
            rows[1].0.as_str(),
            rows[1].1.as_str(),
            rows[1].2.as_str(),
            rows[1].3.as_str()
        ),
        (
            "admin_model_test",
            "openai_compatible",
            "gemma-4-31B-it",
            "shown"
        )
    );
    assert!(rows.len() > 2);
    for row in &rows[2..] {
        assert_eq!(
            (row.0.as_str(), row.1.as_str(), row.2.as_str()),
            ("core_assist", "openai_compatible", "gemma-4-31B-it")
        );
    }

    // One setting_changed row with the previous (none) and the new value.
    let log = changes(&app.db).await;
    assert_eq!(log.len(), 1);
    assert_eq!(log[0]["from"], Value::Null);
    assert_eq!(log[0]["to"], v["active"]);

    // A second change logs the first as "from".
    let (s, v2) = app
        .put(
            "/api/admin/ai-model",
            Some(&pi),
            put_body("openai_compatible", "other-model"),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v2}");
    let log = changes(&app.db).await;
    assert_eq!(log.len(), 2);
    assert_eq!(log[1]["from"], v["active"]);
    assert_eq!(log[1]["to"]["model"], "other-model");
    // The effort was kept as sent.
    assert_eq!(v2["active"]["effort"], "medium");
}

#[tokio::test]
async fn effort_defaults_to_the_environments_when_not_sent() {
    let (_stub, url) = start_stub().await;
    let app = test_app!(nchc(&url));
    let pi = app.pi_token().await;
    let (s, v) = app
        .put(
            "/api/admin/ai-model",
            Some(&pi),
            json!({"provider":"openai_compatible","model":"gemma-4-31B-it","timeout_secs":60}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["active"]["effort"], "medium");
}

#[tokio::test]
async fn reset_returns_to_the_environment_default_and_is_logged() {
    let (stub, url) = start_stub().await;
    let app = test_app!(nchc(&url));
    let pi = app.pi_token().await;
    let (t, assist) = assist_request(&app).await;
    let (s, v) = app
        .put(
            "/api/admin/ai-model",
            Some(&pi),
            put_body("openai_compatible", "gemma-4-31B-it"),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let before = changes(&app.db).await;

    let (s, v) = app.delete("/api/admin/ai-model", Some(&pi)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["source"], "env");
    assert_eq!(v["active"], v["env_default"]);
    assert!(override_row(&app.db).await.is_none());
    let log = changes(&app.db).await;
    assert_eq!(log.len(), 2);
    assert_eq!(log[1]["from"], before[0]["to"]);
    assert_eq!(log[1]["to"], Value::Null);

    // New calls are the environment's again (the fake provider; the stub is not called).
    let calls = stub.models().len();
    let (s, r) = app.post("/api/core/assist", Some(&t), assist).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(stub.models().len(), calls);
    let rows = interactions(&app.db).await;
    let last = rows.last().unwrap();
    assert_eq!((last.0.as_str(), last.1.as_str()), ("core_assist", "fake"));

    // Resetting again changes nothing and logs nothing more.
    let (s, _) = app.delete("/api/admin/ai-model", Some(&pi)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(changes(&app.db).await.len(), 2);
}

#[tokio::test]
async fn a_call_in_flight_finishes_and_is_recorded_with_the_model_it_started_with() {
    let (stub, url) = start_stub().await;
    let app = test_app!(nchc(&url));
    let (t, assist) = assist_request(&app).await;
    app.state.ai.set_active(
        AiSettings {
            kind: AiProviderKind::OpenAiCompatible,
            model: "slow-model".into(),
            effort: "medium".into(),
            timeout_secs: 60,
        },
        AiSource::Admin,
    );
    let call = app.post("/api/core/assist", Some(&t), assist);
    let switch = async {
        // The call has reached the model; now the PI switches.
        stub.arrived.notified().await;
        app.state.ai.set_active(
            AiSettings {
                kind: AiProviderKind::OpenAiCompatible,
                model: "second-model".into(),
                effort: "medium".into(),
                timeout_secs: 60,
            },
            AiSource::Admin,
        );
        stub.release.notify_one();
    };
    let ((s, r), ()) = tokio::join!(call, switch);
    assert_eq!(s, StatusCode::OK, "{r}");
    let rows = interactions(&app.db).await;
    assert!(!rows.is_empty());
    for row in &rows {
        assert_eq!(
            (row.1.as_str(), row.2.as_str()),
            ("openai_compatible", "slow-model"),
            "{rows:?}"
        );
    }
    assert!(stub.models().iter().all(|m| m == "slow-model"));
    // The next call is the new model's.
    assert_eq!(app.state.ai.active().model, "second-model");
}

#[tokio::test]
async fn startup_loads_the_override_and_ignores_one_that_is_no_longer_allowed() {
    let (_stub, url) = start_stub().await;
    let app = test_app!(nchc(&url));
    let pi = app.pi_token().await;
    let mut cfg = Config::for_tests("unused");
    nchc(&url)(&mut cfg);

    // Nothing stored: the environment.
    let fresh = AiClient::from_config(&cfg);
    manna_api::ai::admin::load_override(&fresh, &app.db)
        .await
        .unwrap();
    assert_eq!(fresh.active_settings().1, AiSource::Env);

    let (s, v) = app
        .put(
            "/api/admin/ai-model",
            Some(&pi),
            put_body("openai_compatible", "gemma-4-31B-it"),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let fresh = AiClient::from_config(&cfg);
    manna_api::ai::admin::load_override(&fresh, &app.db)
        .await
        .unwrap();
    let (active, source) = fresh.active_settings();
    assert_eq!(source, AiSource::Admin);
    assert_eq!(active.model, "gemma-4-31B-it");
    assert_eq!(fresh.longest_timeout_secs(), 120.max(cfg.ai_timeout_secs));

    // The NCHC address was removed from the environment: the stored choice cannot be used.
    let without = AiClient::from_config(&Config::for_tests("unused"));
    manna_api::ai::admin::load_override(&without, &app.db)
        .await
        .unwrap();
    assert_eq!(without.active_settings().1, AiSource::Env);
    // Nor can a stored anthropic or garbage value get in.
    sqlx::query("UPDATE app_settings SET value = $1 WHERE key = 'ai_text_model'")
        .bind(json!({"provider":"anthropic","model":"m","effort":"medium","timeout_secs":60}))
        .execute(&app.db)
        .await
        .unwrap();
    let fresh = AiClient::from_config(&cfg);
    manna_api::ai::admin::load_override(&fresh, &app.db)
        .await
        .unwrap();
    assert_eq!(fresh.active_settings().1, AiSource::Env);
}
