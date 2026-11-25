//! The tool-based prep chat (docs/PREP_CHAT.md): a `chat` turn answers and edits the worksheet
//! with tools (the fake model scripts them with `FAKE_CHAT_*`), writes ONE version, lists what
//! changed from the difference, and can be undone as a whole.

#[macro_use]
mod common;

use std::path::PathBuf;
use std::time::Duration;

use axum::http::StatusCode;
use manna_api::config::{AiProviderKind, Config};
use manna_api::storage::StorageConfig;
use manna_api::worksheet::images::ImageConfig;
use serde_json::{json, Value};

const P: [&str; 4] = [
    "Mrs. Lin put an umbrella stand outside her shop on a rainy Monday. A sign said, “Take one if you need it.”",
    "Her trust laughed at the idea. They were sure that every umbrella would be gone within a week.",
    "In the first month, twelve of the thirty umbrellas did disappear. However, something unexpected followed.",
    "Today the stand holds more umbrellas than it did at the start. Mrs. Lin says trust is like an umbrella.",
];

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("manna-chat-{}", manna_api::auth::new_token()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn configure(&self, change: impl FnOnce(&mut ImageConfig)) -> impl FnOnce(&mut Config) + '_ {
        let mut images = ImageConfig::fake();
        change(&mut images);
        move |c: &mut Config| {
            c.storage = StorageConfig::fs(self.0.clone());
            c.images = images;
        }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn wait(app: &common::TestApp, t: &str, id: i64) -> Value {
    for _ in 0..800 {
        let (s, v) = app.get(&format!("/api/prep-sessions/{id}"), Some(t)).await;
        assert_eq!(s, StatusCode::OK, "{v}");
        if v["running"] == false {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("prep session {id} is still running");
}

/// A session whose first turn made a worksheet: (session id, worksheet id).
async fn session(app: &common::TestApp, t: &str) -> (i64, i64) {
    let text = format!("{}\n\n{}\n\n{}\n\n{}", P[0], P[1], P[2], P[3]);
    let (s, v) = app
        .post(
            "/api/prep-sessions",
            Some(t),
            json!({ "kind": "worksheet",
                    "material": { "title": "The Umbrella Stand", "text": text } }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    let id = v["id"].as_i64().unwrap();
    let done = wait(app, t, id).await;
    assert_eq!(done["turns"][0]["status"], "done");
    let ws = done["turns"][0]["worksheet"]["id"].as_i64().unwrap();
    // The pictures the fake model asked for are drawn (or skipped) before the tests go on.
    for _ in 0..400 {
        let (_, w) = app.get(&format!("/api/worksheets/{ws}"), Some(t)).await;
        let open = w["images"]["slots"]
            .as_array()
            .map(|a| {
                a.iter()
                    .any(|s| s["status"] == "queued" || s["status"] == "running")
            })
            .unwrap_or(false);
        if !open {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    (id, ws)
}

async fn chat_raw(app: &common::TestApp, t: &str, id: i64, request: &str) -> (StatusCode, Value) {
    app.post(
        &format!("/api/prep-sessions/{id}/turns"),
        Some(t),
        json!({ "kind": "chat", "request": request }),
    )
    .await
}

/// A chat turn, waited for: the turn.
async fn chat(app: &common::TestApp, t: &str, id: i64, request: &str) -> Value {
    let (s, v) = chat_raw(app, t, id, request).await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    let done = wait(app, t, id).await;
    done["turns"].as_array().unwrap().last().unwrap().clone()
}

async fn worksheet(app: &common::TestApp, t: &str, ws: i64) -> Value {
    let (s, v) = app.get(&format!("/api/worksheets/{ws}"), Some(t)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v
}

fn first_block(w: &Value) -> Value {
    w["content"]["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| !b["items"].as_array().unwrap().is_empty())
        .unwrap()
        .clone()
}

async fn usage(app: &common::TestApp, event: &str) -> Vec<Value> {
    sqlx::query_scalar("SELECT payload FROM usage_logs WHERE event = $1 ORDER BY id")
        .bind(event)
        .fetch_all(&app.db)
        .await
        .unwrap()
}

async fn calls(app: &common::TestApp) -> Vec<(i32, String, Value, Option<Value>)> {
    sqlx::query_as(
        "SELECT attempt, status, input_snapshot, usage FROM ai_interactions
          WHERE module = 'prep_chat' ORDER BY id",
    )
    .fetch_all(&app.db)
    .await
    .unwrap()
}

// ---------------------------------------------------------------- answering and editing

#[tokio::test]
async fn a_question_gets_an_answer_and_no_new_version() {
    let app = test_app!();
    let t = app.teacher("T-CHAT-1", "general").await;
    let (id, ws) = session(&app, &t).await;
    let before = worksheet(&app, &t, ws).await;
    let turn = chat(&app, &t, id, "第 2 段在講什麼？ FAKE_CHAT_ASK").await;
    assert_eq!(turn["kind"], "chat");
    assert_eq!(turn["status"], "done", "{turn}");
    let r = &turn["result"];
    assert!(r["answer"].as_str().unwrap().contains("測試用的回答"));
    assert!(
        r["answer"].as_str().unwrap().contains("\n\n"),
        "paragraphs joined"
    );
    assert_eq!(r["follow_ups"], json!(["再幫我改一個地方"]));
    assert_eq!(r["changes"], json!([]));
    assert_eq!(r["not_applied"], json!([]));
    assert_eq!(r["version_no"], Value::Null);
    assert_eq!(r["undoable"], false);
    assert_eq!(r["tool_calls"], 0);
    let after = worksheet(&app, &t, ws).await;
    assert_eq!(after["versions_count"], before["versions_count"]);
    let turn_log = usage(&app, "prep_chat_turn").await;
    assert_eq!(turn_log[0]["changed"], false);
    assert_eq!(turn_log[0]["model_calls"], 1);
}

#[tokio::test]
async fn an_edit_writes_one_version_with_history_and_the_server_lists_the_changes() {
    let app = test_app!();
    let t = app.teacher("T-CHAT-2", "general").await;
    let (id, ws) = session(&app, &t).await;
    let before = worksheet(&app, &t, ws).await;
    let block = first_block(&before);
    let block_id = block["id"].as_str().unwrap().to_string();
    let versions = before["versions_count"].as_i64().unwrap();

    let turn = chat(&app, &t, id, "第一題改難一點 FAKE_CHAT_EDIT").await;
    assert_eq!(turn["status"], "done", "{turn}");
    let r = &turn["result"];
    assert_eq!(r["tool_calls"], 1);
    assert_eq!(r["version_no"].as_i64().unwrap(), versions + 1);
    assert_eq!(r["undoable"], true);
    assert_eq!(r["not_applied"], json!([]));
    let changes = r["changes"].as_array().unwrap();
    assert_eq!(changes.len(), 1, "{r}");
    assert_eq!(changes[0]["target"], "block");
    assert_eq!(changes[0]["block_id"], block_id);
    assert!(changes[0]["label"].as_str().unwrap().ends_with("區"));
    assert_eq!(changes[0]["summary"], "第 1 題改了題目");

    let after = worksheet(&app, &t, ws).await;
    assert_eq!(after["versions_count"].as_i64().unwrap(), versions + 1);
    let new_block = first_block(&after);
    assert!(new_block["items"][0]["prompt"]
        .as_str()
        .unwrap()
        .ends_with("(revised)"));
    assert_eq!(new_block["items"][0]["origin"], "ai");
    // The block history has the block as it was; the editor's undo knows about it.
    assert_eq!(after["undoable"], json!([block_id]));
    let (cause, source): (String, Value) = sqlx::query_as(
        "SELECT cause, source FROM worksheet_versions WHERE worksheet_id = $1
          ORDER BY version_no DESC LIMIT 1",
    )
    .bind(ws)
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(cause, "revise");
    assert_eq!(source["cause"], "prep_turn");
    assert_eq!(source["turn_id"], turn["id"]);
    // One row per model call; the tokens the provider reported are kept, the second call
    // reads the cached prefix.
    let rows = calls(&app).await;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].3.as_ref().unwrap()["cached_tokens"], 0);
    assert!(
        rows[1].3.as_ref().unwrap()["cached_tokens"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert!(
        rows[1].3.as_ref().unwrap()["prompt_tokens"]
            .as_u64()
            .unwrap()
            > rows[0].3.as_ref().unwrap()["prompt_tokens"]
                .as_u64()
                .unwrap()
    );
    // The conversation grows by appending only.
    assert_eq!(rows[0].2["messages"].as_array().unwrap().len(), 4);
    assert_eq!(rows[1].2["messages"].as_array().unwrap().len(), 6);
    assert_eq!(
        rows[0].2["messages"].as_array().unwrap()[..],
        rows[1].2["messages"].as_array().unwrap()[..4]
    );
    // The log: one line per tool call and one for the turn, never the teacher's words.
    let tools = usage(&app, "prep_chat_tool").await;
    assert_eq!(
        tools,
        vec![
            json!({ "turn_id": turn["id"], "tool": "edit_block", "block_id": block_id,
                     "ok": true, "reason_code": "ok" })
        ]
    );
    let done = usage(&app, "prep_chat_turn").await;
    assert_eq!(
        done[0],
        json!({ "turn_id": turn["id"], "tool_calls": 1, "model_calls": 2, "changed": true,
                "changes": 1, "not_applied": 0, "undone": false })
    );
    // The turn's steps were shown as they ran.
    let steps = turn["progress"]["steps"].as_array().unwrap();
    let labels: Vec<&str> = steps.iter().map(|s| s["label"].as_str().unwrap()).collect();
    assert_eq!(labels[0], "讀學習單");
    assert!(labels[1].starts_with("修改 "), "{labels:?}");
    assert_eq!(*labels.last().unwrap(), "整理回答");
}

#[tokio::test]
async fn a_bad_edit_is_fixed_after_the_reasons_come_back() {
    let app = test_app!();
    let t = app.teacher("T-CHAT-3", "general").await;
    let (id, ws) = session(&app, &t).await;
    let versions = worksheet(&app, &t, ws).await["versions_count"]
        .as_i64()
        .unwrap();
    let turn = chat(&app, &t, id, "改一下 FAKE_CHAT_BAD_EDIT").await;
    assert_eq!(turn["status"], "done", "{turn}");
    let r = &turn["result"];
    assert_eq!(r["tool_calls"], 2);
    assert_eq!(r["not_applied"], json!([]), "{r}");
    assert_eq!(r["changes"].as_array().unwrap().len(), 1);
    assert_eq!(
        worksheet(&app, &t, ws).await["versions_count"]
            .as_i64()
            .unwrap(),
        versions + 1
    );
    let tools = usage(&app, "prep_chat_tool").await;
    assert_eq!(tools[0]["ok"], false);
    assert_eq!(tools[0]["reason_code"], "check_failed");
    assert_eq!(tools[1]["ok"], true);
    // The second call carried the reasons (as lengths and hashes in the log, in full to the model).
    assert_eq!(calls(&app).await.len(), 3);
}

#[tokio::test]
async fn a_refused_edit_is_listed_and_changes_nothing() {
    let app = test_app!();
    let t = app.teacher("T-CHAT-4", "general").await;
    let (id, ws) = session(&app, &t).await;
    let before = worksheet(&app, &t, ws).await;
    let turn = chat(&app, &t, id, "改一下 FAKE_CHAT_REFUSED").await;
    assert_eq!(turn["status"], "done", "{turn}");
    let r = &turn["result"];
    // The first try and two retries failed; the fourth call was stopped by the server.
    assert_eq!(r["tool_calls"], 4);
    assert_eq!(r["changes"], json!([]));
    assert_eq!(r["version_no"], Value::Null);
    let not_applied = r["not_applied"].as_array().unwrap();
    assert_eq!(not_applied.len(), 1, "{r}");
    assert!(not_applied[0]["label"].as_str().unwrap().ends_with("區"));
    assert!(!not_applied[0]["reason"].as_str().unwrap().is_empty());
    let after = worksheet(&app, &t, ws).await;
    assert_eq!(after["content"], before["content"]);
    assert_eq!(after["versions_count"], before["versions_count"]);
    let codes: Vec<Value> = usage(&app, "prep_chat_tool")
        .await
        .iter()
        .map(|p| p["reason_code"].clone())
        .collect();
    assert_eq!(
        codes,
        vec![
            json!("check_failed"),
            json!("check_failed"),
            json!("check_failed"),
            json!("retry_limit")
        ]
    );
}

#[tokio::test]
async fn a_table_edit_is_checked_like_a_generated_table() {
    let app = test_app!();
    let t = app.teacher("T-CHAT-5", "general").await;
    let (id, ws) = session(&app, &t).await;
    let turn = chat(&app, &t, id, "第一題改成比較表 FAKE_CHAT_TABLE").await;
    assert_eq!(turn["status"], "done", "{turn}");
    let change = &turn["result"]["changes"][0];
    assert_eq!(
        change["summary"],
        "第 1 題改成表格整理（欄：Reading One、Reading Two）"
    );
    let item = &first_block(&worksheet(&app, &t, ws).await)["items"][0];
    assert_eq!(item["answer_format"], "表格整理");
    assert_eq!(
        item["table"]["columns"],
        json!(["Reading One", "Reading Two"])
    );
    assert_eq!(
        item["table"]["row_labels"],
        json!(["Claim", "Supporting detail"])
    );
    assert_eq!(item["table"]["rows"], 2);
}

#[tokio::test]
async fn scaffolds_vocabulary_and_blocks_have_their_tools() {
    let app = test_app!();
    let t = app.teacher("T-CHAT-6", "general").await;
    let (id, ws) = session(&app, &t).await;
    let before = worksheet(&app, &t, ws).await;

    let turn = chat(
        &app,
        &t,
        id,
        "第一個簡答題加句型框架和提示 FAKE_CHAT_SCAFFOLD",
    )
    .await;
    assert_eq!(turn["status"], "done", "{turn}");
    let r = &turn["result"];
    assert_eq!(r["not_applied"], json!([]), "{r}");
    assert!(r["changes"][0]["summary"]
        .as_str()
        .unwrap()
        .contains("句型框架"));
    let blocks = worksheet(&app, &t, ws).await["content"]["blocks"].clone();
    let framed = blocks
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|b| b["items"].as_array().unwrap().clone())
        .find(|i| i["frame"] == "I think ___ because ___.")
        .expect("the frame was written");
    assert_eq!(framed["hint"], "Look at the first paragraph.");

    let turn = chat(&app, &t, id, "加 trust，字義用中文 FAKE_CHAT_VOCAB").await;
    assert_eq!(turn["status"], "done", "{turn}");
    let r = &turn["result"];
    assert_eq!(r["not_applied"], json!([]), "{r}");
    let labels: Vec<&str> = r["changes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["label"].as_str().unwrap())
        .collect();
    assert!(labels.iter().all(|l| *l == "Key Vocabulary"), "{labels:?}");
    assert!(r["changes"]
        .as_array()
        .unwrap()
        .iter()
        .all(|c| c["target"] == "vocabulary"));
    let w = worksheet(&app, &t, ws).await;
    assert_eq!(w["content"]["vocabulary_language"], "zh");
    let words: Vec<Value> = w["content"]["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|b| b["vocabulary"].as_array().cloned().unwrap_or_default())
        .collect();
    let umbrella = words.iter().find(|v| v["word"] == "trust").expect("added");
    assert_eq!(umbrella["meaning_zh"], "信任");
    assert_eq!(umbrella["pos"], "n.");

    let count = |w: &Value| w["content"]["blocks"].as_array().unwrap().len();
    let n = count(&before);
    let turn = chat(&app, &t, id, "加一區再移到最前面 FAKE_CHAT_BLOCKS").await;
    assert_eq!(turn["status"], "done", "{turn}");
    let r = &turn["result"];
    assert_eq!(r["not_applied"], json!([]), "{r}");
    let w = worksheet(&app, &t, ws).await;
    assert_eq!(count(&w), n + 1);
    assert_eq!(w["content"]["blocks"][0]["title"], "Extra practice");
    assert_eq!(w["content"]["blocks"][0]["items"][0]["origin"], "ai");
    assert!(r["changes"][0]["summary"]
        .as_str()
        .unwrap()
        .starts_with("新增了這一區"));
    assert!(r["changes"].as_array().unwrap().iter().any(
        |c| c["summary"] == "調整了順序" || c["summary"].as_str().unwrap().starts_with("新增")
    ));
    // One version for the two tool calls, and the whole turn can be taken back.
    assert_eq!(r["tool_calls"], 2);
    let (s, v) = app
        .post(
            &format!("/api/prep-sessions/{id}/turns/{}/undo", turn["id"]),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(count(&worksheet(&app, &t, ws).await), n);
}

// ---------------------------------------------------------------- pictures

#[tokio::test]
async fn a_redraw_is_queued_after_the_version_is_written() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure(|_| {}));
    let t = app.teacher("T-CHAT-7", "general").await;
    let (id, ws) = session(&app, &t).await;
    let before = worksheet(&app, &t, ws).await;
    let left = before["images"]["regenerations_left"].as_i64().unwrap();
    assert!(before["images"]["slots"]
        .as_array()
        .is_some_and(|s| !s.is_empty()));
    let turn = chat(
        &app,
        &t,
        id,
        "第一題改一下，圖重畫成女生 FAKE_CHAT_EDIT_PICTURE",
    )
    .await;
    assert_eq!(turn["status"], "done", "{turn}");
    let r = &turn["result"];
    assert_eq!(r["tool_calls"], 2);
    let version = r["version_no"].as_i64().unwrap();
    let picture = r["changes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["target"] == "picture")
        .expect("the redraw is listed");
    assert!(picture["label"].as_str().unwrap().ends_with("區的圖"));
    // The redraw row was made after the version row.
    let (version_at, image_at): (chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>) =
        sqlx::query_as(
            "SELECT (SELECT created_at FROM worksheet_versions WHERE worksheet_id = $1
                      AND version_no = $2),
                    (SELECT MIN(created_at) FROM worksheet_images
                      WHERE worksheet_id = $1 AND regeneration)",
        )
        .bind(ws)
        .bind(version as i32)
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert!(image_at >= version_at);
    let after = worksheet(&app, &t, ws).await;
    assert_eq!(
        after["images"]["regenerations_left"].as_i64().unwrap(),
        left - 1
    );
    // The picture is not undone with the text.
    let (s, _) = app
        .post(
            &format!("/api/prep-sessions/{id}/turns/{}/undo", turn["id"]),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        worksheet(&app, &t, ws).await["images"]["regenerations_left"]
            .as_i64()
            .unwrap(),
        left - 1
    );
}

#[tokio::test]
async fn a_redraw_beyond_the_cap_is_refused_and_listed() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure(|c| c.max_regenerations = 0));
    let t = app.teacher("T-CHAT-8", "general").await;
    let (id, _) = session(&app, &t).await;
    let turn = chat(&app, &t, id, "圖重畫 FAKE_CHAT_PICTURE").await;
    assert_eq!(turn["status"], "done", "{turn}");
    let r = &turn["result"];
    assert_eq!(r["changes"], json!([]));
    let na = &r["not_applied"][0];
    assert!(na["label"].as_str().unwrap().ends_with("的圖"), "{r}");
    assert!(na["reason"].as_str().unwrap().contains("上限"));
}

// ---------------------------------------------------------------- limits and failures

#[tokio::test]
async fn a_loop_stops_at_eight_tool_calls_and_says_so() {
    let app = test_app!();
    let t = app.teacher("T-CHAT-9", "general").await;
    let (id, ws) = session(&app, &t).await;
    let versions = worksheet(&app, &t, ws).await["versions_count"]
        .as_i64()
        .unwrap();
    let turn = chat(&app, &t, id, "一直讀 FAKE_CHAT_LOOP").await;
    assert_eq!(turn["status"], "done", "{turn}");
    let r = &turn["result"];
    assert_eq!(r["tool_calls"], 8);
    assert!(r["answer"].as_str().unwrap().contains("工具次數已經用完"));
    assert_eq!(r["version_no"], Value::Null);
    assert!(calls(&app).await.len() <= 10);
    assert_eq!(
        worksheet(&app, &t, ws).await["versions_count"]
            .as_i64()
            .unwrap(),
        versions
    );
    let tools = usage(&app, "prep_chat_tool").await;
    assert_eq!(tools.len(), 9, "the ninth was refused");
    assert_eq!(tools[8]["reason_code"], "limit");
}

#[tokio::test]
async fn a_worksheet_changed_during_the_turn_fails_the_turn_and_applies_nothing() {
    let app = test_app!();
    let t = app.teacher("T-CHAT-10", "general").await;
    let (id, ws) = session(&app, &t).await;
    let (s, v) = chat_raw(&app, &t, id, "改 FAKE_CHAT_EDIT FAKE_CHAT_PACED").await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    // The teacher edits the worksheet while Manna works.
    let w = worksheet(&app, &t, ws).await;
    let mut content = w["content"].clone();
    content["blocks"][0]["title"] = json!("Edited by hand");
    let (s, v) = app
        .put(
            &format!("/api/worksheets/{ws}"),
            Some(&t),
            json!({ "content": content }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let done = wait(&app, &t, id).await;
    let turn = done["turns"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(turn["status"], "failed", "{turn}");
    assert!(turn["error"].as_str().unwrap().contains("被改過"));
    let after = worksheet(&app, &t, ws).await;
    assert_eq!(after["content"]["blocks"][0]["title"], "Edited by hand");
    assert!(!first_block(&after)["items"][0]["prompt"]
        .as_str()
        .unwrap()
        .contains("(revised)"));
}

#[tokio::test]
async fn only_openai_runs_a_chat() {
    let app = test_app!();
    let t = app.teacher("T-CHAT-11", "general").await;
    let (id, _) = session(&app, &t).await;
    let mut nchc = app.state.ai.active_settings().0;
    nchc.kind = AiProviderKind::OpenAiCompatible;
    app.state
        .ai
        .set_active(nchc, manna_api::ai::AiSource::Admin);
    let turn = chat(&app, &t, id, "問個問題").await;
    assert_eq!(turn["status"], "failed");
    assert!(turn["error"]
        .as_str()
        .unwrap()
        .contains("這個功能需要 OpenAI 模型"));
    assert_eq!(calls(&app).await.len(), 0);
}

// ---------------------------------------------------------------- undo

#[tokio::test]
async fn a_turn_is_undone_as_a_whole_until_a_block_is_edited_again() {
    let app = test_app!();
    let t = app.teacher("T-CHAT-12", "general").await;
    let (id, ws) = session(&app, &t).await;
    let before = worksheet(&app, &t, ws).await;
    let turn = chat(&app, &t, id, "改 FAKE_CHAT_EDIT").await;
    let turn_id = turn["id"].as_i64().unwrap();
    let path = format!("/api/prep-sessions/{id}/turns/{turn_id}/undo");
    // Another teacher cannot.
    let other = app.teacher("T-CHAT-12B", "general").await;
    let (s, _) = app.post(&path, Some(&other), json!({})).await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    let (s, v) = app.post(&path, Some(&t), json!({})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["id"], turn["id"]);
    assert_eq!(v["result"]["undoable"], false);
    let after = worksheet(&app, &t, ws).await;
    assert_eq!(after["content"]["blocks"], before["content"]["blocks"]);
    assert_eq!(
        after["versions_count"].as_i64().unwrap(),
        before["versions_count"].as_i64().unwrap() + 2
    );
    let cause: String = sqlx::query_scalar(
        "SELECT cause FROM worksheet_versions WHERE worksheet_id = $1 ORDER BY version_no DESC LIMIT 1",
    )
    .bind(ws)
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(cause, "undo");
    assert_eq!(
        after["undoable"],
        json!([]),
        "the block history rows are marked undone"
    );
    assert_eq!(usage(&app, "prep_chat_undo").await.len(), 1);
    // Once is enough.
    let (s, v) = app.post(&path, Some(&t), json!({})).await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    assert!(v["error"]["message"]
        .as_str()
        .unwrap()
        .contains("沒有可以復原"));

    // A second turn, then the teacher edits the block by hand: no whole-turn undo.
    let turn2 = chat(&app, &t, id, "再改 FAKE_CHAT_EDIT").await;
    assert_eq!(turn2["result"]["undoable"], true);
    let mut content = worksheet(&app, &t, ws).await["content"].clone();
    let idx = content["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .position(|b| !b["items"].as_array().unwrap().is_empty())
        .unwrap();
    content["blocks"][idx]["title"] = json!("Edited by hand");
    let (s, _) = app
        .put(
            &format!("/api/worksheets/{ws}"),
            Some(&t),
            json!({ "content": content }),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let (_, session_now) = app.get(&format!("/api/prep-sessions/{id}"), Some(&t)).await;
    let shown = session_now["turns"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .clone();
    assert_eq!(
        shown["result"]["undoable"], false,
        "no longer undoable as a whole"
    );
    let path2 = format!("/api/prep-sessions/{id}/turns/{}/undo", turn2["id"]);
    let (s, v) = app.post(&path2, Some(&t), json!({})).await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    assert!(v["error"]["message"].as_str().unwrap().contains("又改過"));
    assert_eq!(
        worksheet(&app, &t, ws).await["content"]["blocks"][idx]["title"],
        "Edited by hand"
    );
}

// ---------------------------------------------------------------- kinds, target and context

#[tokio::test]
async fn ask_and_revise_cannot_be_made_any_more_but_old_turns_still_load() {
    let app = test_app!(|c: &mut Config| c.prep_legacy_turns = false);
    let t = app.teacher("T-CHAT-13", "general").await;
    let (id, ws) = session(&app, &t).await;
    for kind in ["ask", "revise"] {
        let (s, v) = app
            .post(
                &format!("/api/prep-sessions/{id}/turns"),
                Some(&t),
                json!({ "kind": kind, "request": "問題" }),
            )
            .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{kind}: {v}");
        assert!(v["error"]["message"].as_str().unwrap().contains("chat"));
    }
    let (s, v) = app
        .post(
            "/api/prep-sessions",
            Some(&t),
            json!({ "kind": "ask", "request": "怎麼開場？" }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    // A turn without a kind is a chat (the bottom box).
    let (s, v) = app
        .post(
            &format!("/api/prep-sessions/{id}/turns"),
            Some(&t),
            json!({ "request": "怎麼開場？" }),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    assert_eq!(
        v["turns"].as_array().unwrap().last().unwrap()["kind"],
        "chat"
    );
    wait(&app, &t, id).await;
    // An old ask and revise turn are shown as they were.
    sqlx::query(
        "INSERT INTO prep_turns (session_id, seq, kind, request, status, result, finished_at)
         VALUES ($1, 90, 'ask', '舊的問題', 'done', $2, now()),
                ($1, 91, 'revise', '舊的修改', 'done', $3, now())",
    )
    .bind(id)
    .bind(json!({ "answer": "舊的回答", "follow_ups": [] }))
    .bind(
        json!({ "reply": "舊的回覆", "changed_block_ids": [], "version_no": null, "pictures": [] }),
    )
    .execute(&app.db)
    .await
    .unwrap();
    let (_, v) = app.get(&format!("/api/prep-sessions/{id}"), Some(&t)).await;
    let turns = v["turns"].as_array().unwrap();
    assert!(turns
        .iter()
        .any(|t| t["kind"] == "ask" && t["result"]["answer"] == "舊的回答"));
    assert!(turns
        .iter()
        .any(|t| t["kind"] == "revise" && t["result"]["reply"] == "舊的回覆"));
    // The chat sees them in the conversation.
    let turn = chat(&app, &t, id, "再問 FAKE_CHAT_ASK").await;
    assert_eq!(turn["status"], "done");
    let _ = ws;
}

#[tokio::test]
async fn the_target_worksheet_must_be_the_sessions_own() {
    let app = test_app!();
    let t = app.teacher("T-CHAT-14", "general").await;
    let (id, ws) = session(&app, &t).await;
    let (_, other_ws) = session(&app, &t).await;
    let (s, v) = app
        .post(
            &format!("/api/prep-sessions/{id}/turns"),
            Some(&t),
            json!({ "kind": "chat", "request": "改 FAKE_CHAT_EDIT", "worksheet_id": other_ws }),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND, "{v}");
    let (s, v) = app
        .post(
            &format!("/api/prep-sessions/{id}/turns"),
            Some(&t),
            json!({ "kind": "chat", "request": "改 FAKE_CHAT_EDIT", "worksheet_id": ws }),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    let done = wait(&app, &t, id).await;
    assert_eq!(
        done["turns"].as_array().unwrap().last().unwrap()["result"]["tool_calls"],
        1
    );
}

#[tokio::test]
async fn the_model_reads_the_last_turns_and_the_snapshot_hides_the_material() {
    let app = test_app!();
    let t = app.teacher("T-CHAT-15", "general").await;
    let (id, _) = session(&app, &t).await;
    let first = chat(&app, &t, id, "第一個問題 SENTINEL-ONE FAKE_CHAT_ASK").await;
    assert!(first["result"]["answer"]
        .as_str()
        .unwrap()
        .contains("參考了 1 則"));
    // The earlier markers do not drive this turn.
    let second = chat(&app, &t, id, "第二個問題").await;
    assert!(second["result"]["answer"]
        .as_str()
        .unwrap()
        .contains("參考了 2 則"));
    assert_eq!(second["result"]["tool_calls"], 0);
    let rows = calls(&app).await;
    let snapshot = rows.last().unwrap().2.to_string();
    for text in P {
        assert!(
            !snapshot.contains(&text[..30]),
            "the material is logged as a hash"
        );
    }
    assert!(
        !snapshot.contains("SENTINEL-ONE"),
        "earlier turns are redacted"
    );
    assert!(snapshot.contains("備課對話教材"));
    assert!(
        snapshot.contains("第二個問題"),
        "this turn's own words are logged"
    );
    assert!(snapshot.contains("prep_chat"));
}

#[tokio::test]
async fn a_chat_message_can_offer_a_habit_like_a_revision() {
    let app = test_app!();
    let pi = app.pi_token().await;
    let (s, v) = app
        .put(
            "/api/admin/memory-settings",
            Some(&pi),
            json!({ "enabled": true }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let t = app.teacher("T-CHAT-16", "general").await;
    let (id, _) = session(&app, &t).await;
    let turn = chat(&app, &t, id, "之後 選擇題三個選項 FAKE_CHAT_ASK").await;
    let turn_id = turn["id"].as_i64().unwrap();
    let mut found = None;
    for _ in 0..200 {
        let (_, v) = app
            .get(
                &format!("/api/me/memories/candidates?prep_session_id={id}"),
                Some(&t),
            )
            .await;
        if let Some(c) = v["candidates"].as_array().and_then(|a| a.first()) {
            found = Some(c.clone());
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let c = found.expect("a suggestion was offered");
    assert_eq!(c["source"], "prep_turn");
    let stored: Option<i64> = sqlx::query_scalar(
        "SELECT prep_turn_id FROM memory_candidates WHERE teacher_id =
           (SELECT id FROM teachers WHERE research_id = 'T-CHAT-16') LIMIT 1",
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(stored, Some(turn_id));
}
