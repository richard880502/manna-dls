//! A synchronous generation keeps its model through a repair, even during an admin switch.

#[macro_use]
mod common;

use axum::{extract::State, http::StatusCode, routing::post, Json, Router};
use manna_api::{
    ai::{AiSettings, AiSource},
    config::AiProviderKind,
};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;

#[derive(Clone, Default)]
struct Stub {
    models: Arc<Mutex<Vec<String>>>,
    arrived: Arc<Notify>,
    release: Arc<Notify>,
}

async fn chat(State(stub): State<Stub>, Json(body): Json<Value>) -> Json<Value> {
    let number = {
        let mut models = stub.models.lock().unwrap();
        models.push(body["model"].as_str().unwrap().to_owned());
        models.len()
    };
    if number == 1 {
        stub.arrived.notify_one();
        stub.release.notified().await;
    }
    let english = if number == 1 {
        "invalid"
    } else {
        "I can explain the reason."
    };
    let content = json!({"english": english, "changed": true, "note_zh": "修正英文"}).to_string();
    Json(json!({"choices": [{"message": {"content": content}, "finish_reason": "stop"}]}))
}

#[tokio::test]
async fn repair_and_original_use_same_model_after_switch() {
    let stub = Stub::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let router = Router::new()
        .route("/v1/chat/completions", post(chat))
        .with_state(stub.clone());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let app = test_app!(|c: &mut manna_api::config::Config| {
        c.ai_provider = AiProviderKind::OpenAiCompatible;
        c.ai_model = "first-model".into();
        c.openai_compat_base_url = Some(url);
    });
    let teacher = app.teacher("T-GENERATION-PIN", "general").await;
    let unit = app.create_unit(&teacher, "Unit").await;
    let lesson = app.create_lesson(&teacher, unit, "Lesson").await;
    let path = format!("/api/lessons/{lesson}/sop/objective-english");
    let body = json!({"type": "language", "text": "學生能說明原因", "student_facing_en": ""});
    let switch = async {
        tokio::time::timeout(std::time::Duration::from_secs(10), stub.arrived.notified())
            .await
            .expect("provider received first request");
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
    let (result, ()) = tokio::join!(app.post(&path, Some(&teacher), body.clone()), switch);
    assert_eq!(result.0, StatusCode::OK, "{}", result.1);
    assert_eq!(result.1["status"], "shown", "{}", result.1);
    let (status, next) = app.post(&path, Some(&teacher), body).await;
    assert_eq!(status, StatusCode::OK, "{next}");
    assert_eq!(next["status"], "shown", "{next}");
    assert_eq!(
        *stub.models.lock().unwrap(),
        ["first-model", "first-model", "second-model"]
    );
    let rows: Vec<(String, Option<i64>)> = sqlx::query_as("SELECT model, retry_of FROM ai_interactions WHERE lesson_id = $1 AND module = 'sop_objective_english' ORDER BY id")
        .bind(lesson).fetch_all(&app.db).await.unwrap();
    assert_eq!(rows.len(), 3, "{rows:?}");
    assert_eq!(
        rows.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(),
        ["first-model", "first-model", "second-model"]
    );
    assert!(
        rows[0].1.is_none() && rows[1].1.is_some() && rows[2].1.is_none(),
        "{rows:?}"
    );
    server.abort();
}
