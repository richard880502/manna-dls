//! 快照 (docs/API.md「快照」): naming the latest version, renaming, removing the name only, the
//! list, ownership, and waiting while Manna or a chat turn works.

#[macro_use]
mod common;

use std::path::PathBuf;
use std::time::Duration;

use axum::http::StatusCode;
use manna_api::config::Config;
use manna_api::storage::StorageConfig;
use manna_api::worksheet::images::ImageConfig;
use serde_json::{json, Value};

// ---------------------------------------------------------------- harness

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("manna-hist-{}", manna_api::auth::new_token()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn configure(&self) -> impl FnOnce(&mut Config) + '_ {
        move |c: &mut Config| {
            c.storage = StorageConfig::fs(self.0.clone());
            c.images = ImageConfig::fake();
        }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const P: [&str; 4] = [
    "Mrs. Lin put an umbrella stand outside her shop on a rainy Monday. A sign said, “Take one if you need it.”",
    "Her neighbors laughed at the idea. They were sure that every umbrella would be gone within a week.",
    "In the first month, twelve of the thirty umbrellas did disappear. However, something unexpected followed.",
    "Today the stand holds more umbrellas than it did at the start. Mrs. Lin says trust is like an umbrella.",
];

async fn session_with_worksheet(app: &common::TestApp, t: &str) -> i64 {
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
    let mut ws = None;
    for _ in 0..800 {
        let (_, v) = app.get(&format!("/api/prep-sessions/{id}"), Some(t)).await;
        if v["running"] == false {
            ws = v["turns"][0]["worksheet"]["id"].as_i64();
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let ws = ws.expect("the worksheet was made");
    wait_images(app, t, ws).await;
    ws
}

async fn wait_images(app: &common::TestApp, t: &str, id: i64) -> Value {
    for _ in 0..800 {
        let (s, v) = app
            .get(&format!("/api/worksheets/{id}/images"), Some(t))
            .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        let open = v["slots"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["status"] == "queued" || s["status"] == "running");
        if !open {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("the pictures of worksheet {id} did not finish");
}

async fn worksheet(app: &common::TestApp, t: &str, id: i64) -> Value {
    let (s, v) = app.get(&format!("/api/worksheets/{id}"), Some(t)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v
}

/// The first block with a picture slot: (block id, slot id).
fn first_slot(state: &Value) -> (String, String) {
    state["content"]["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|b| {
            b["image_slots"]
                .as_array()
                .and_then(|s| s.first())
                .map(|s| {
                    (
                        b["id"].as_str().unwrap().to_string(),
                        s["id"].as_str().unwrap().to_string(),
                    )
                })
        })
        .expect("a block with a picture slot")
}

async fn edit_title(app: &common::TestApp, t: &str, ws: i64, block: &str, title: &str) -> Value {
    let st = worksheet(app, t, ws).await;
    let mut content = st["content"].clone();
    for b in content["blocks"].as_array_mut().unwrap() {
        if b["id"] == block {
            b["title"] = json!(title);
        }
    }
    let (s, v) = app
        .put(
            &format!("/api/worksheets/{ws}"),
            Some(t),
            json!({ "content": content, "base_version_no": st["version_no"] }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v
}

async fn usage(app: &common::TestApp, event: &str) -> Vec<Value> {
    sqlx::query_scalar("SELECT payload FROM usage_logs WHERE event = $1 ORDER BY id")
        .bind(event)
        .fetch_all(&app.db)
        .await
        .unwrap()
}

async fn snap(
    app: &common::TestApp,
    t: &str,
    ws: i64,
    name: Option<&str>,
    base: i64,
) -> (StatusCode, Value) {
    let mut body = json!({ "base_version_no": base });
    if let Some(n) = name {
        body["name"] = json!(n);
    }
    app.post(&format!("/api/worksheets/{ws}/snapshots"), Some(t), body)
        .await
}

async fn versions(app: &common::TestApp, t: &str, ws: i64) -> Value {
    let (s, v) = app
        .get(&format!("/api/worksheets/{ws}/versions"), Some(t))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v
}

#[tokio::test]
async fn snapshots_label_the_latest_version_and_show_in_the_list() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure());
    let t = app.teacher("T-SNAP-1", "general").await;
    let other = app.teacher("T-SNAP-1B", "general").await;
    let ws = session_with_worksheet(&app, &t).await;
    let state = worksheet(&app, &t, ws).await;
    let (block, _) = first_slot(&state);
    let first_no = state["version_no"].as_i64().unwrap();

    // Named, trimmed.
    let (s, a) = snap(&app, &t, ws, Some("  Before the quiz  "), first_no).await;
    assert_eq!(s, StatusCode::OK, "{a}");
    assert_eq!(a["name"], "Before the quiz");
    assert_eq!(a["version_no"], first_no);
    assert!(a["id"].as_i64().is_some() && a["created_at"].is_string());

    // The same version again: 409 saying it already is a snapshot.
    let (s, v) = snap(&app, &t, ws, Some("again"), first_no).await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    assert!(v.to_string().contains("已經是快照"), "{v}");

    // A stale base: 409.
    edit_title(&app, &t, ws, &block, "Title two").await;
    let latest = first_no + 1;
    let (s, v) = snap(&app, &t, ws, None, first_no).await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    assert!(v.to_string().contains("新的版本"), "{v}");

    // A blank name: the default 「快照 MM/DD HH:MM」.
    let (s, b) = snap(&app, &t, ws, Some("   "), latest).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let default = b["name"].as_str().unwrap().to_string();
    assert!(
        default.starts_with("快照 ")
            && default.chars().count() == "快照 11/25 17:30".chars().count()
            && default.contains(':')
            && default.contains('/'),
        "{default}"
    );

    // Too long: 400 (60 characters is fine).
    edit_title(&app, &t, ws, &block, "Title three").await;
    let (s, v) = snap(&app, &t, ws, Some(&"字".repeat(61)), latest + 1).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    let (s, _) = snap(&app, &t, ws, Some(&"字".repeat(60)), latest + 1).await;
    assert_eq!(s, StatusCode::OK);

    // Listed on its version, null on the others.
    let list = versions(&app, &t, ws).await;
    let by_no = |n: i64| {
        list.as_array()
            .unwrap()
            .iter()
            .find(|r| r["version_no"] == n)
            .unwrap()
            .clone()
    };
    assert_eq!(by_no(first_no)["snapshot"]["name"], "Before the quiz");
    assert_eq!(by_no(first_no)["snapshot"]["id"], a["id"]);
    assert_eq!(by_no(latest)["snapshot"]["name"], default.as_str());

    // Rename.
    let id = a["id"].as_i64().unwrap();
    let (s, r) = app
        .patch(
            &format!("/api/worksheets/{ws}/snapshots/{id}"),
            Some(&t),
            json!({ "name": " Final " }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["name"], "Final");
    let (s, _) = app
        .patch(
            &format!("/api/worksheets/{ws}/snapshots/{id}"),
            Some(&t),
            json!({ "name": "  " }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // Another teacher: 404 on everything.
    let (s, _) = snap(&app, &other, ws, None, latest + 1).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = app
        .patch(
            &format!("/api/worksheets/{ws}/snapshots/{id}"),
            Some(&other),
            json!({ "name": "x" }),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = app
        .delete(
            &format!("/api/worksheets/{ws}/snapshots/{id}"),
            Some(&other),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // Delete removes only the label: the version stays and can still be read.
    let before = versions(&app, &t, ws).await.as_array().unwrap().len();
    let (s, v) = app
        .delete(&format!("/api/worksheets/{ws}/snapshots/{id}"), Some(&t))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let list = versions(&app, &t, ws).await;
    assert_eq!(list.as_array().unwrap().len(), before);
    let gone = list
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["version_no"] == first_no)
        .unwrap();
    assert!(gone["snapshot"].is_null());
    let (s, _) = app
        .delete(&format!("/api/worksheets/{ws}/snapshots/{id}"), Some(&t))
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let q = "level=standard&audience=teacher";
    let (s, _) = app
        .get(
            &format!("/api/worksheets/{ws}/versions/{first_no}/sheet?{q}"),
            Some(&t),
        )
        .await;
    assert_eq!(s, StatusCode::OK);

    // Usage: ids and a flag only, never the name.
    let saved = usage(&app, "worksheet_snapshot_saved").await;
    assert_eq!(saved.len(), 3);
    assert_eq!(saved[0]["named"], true);
    assert_eq!(saved[1]["named"], false);
    let renamed = usage(&app, "worksheet_snapshot_renamed").await;
    let removed = usage(&app, "worksheet_snapshot_removed").await;
    assert_eq!(renamed.len(), 1);
    assert_eq!(removed.len(), 1);
    for p in saved.iter().chain(&renamed).chain(&removed) {
        let o = p.as_object().unwrap();
        assert_eq!(o.len(), 3, "{p}");
        assert!(o.contains_key("worksheet_id") && o.contains_key("version_no"));
        let text = p.to_string();
        assert!(!text.contains("Final") && !text.contains("Before the quiz"));
    }
}

#[tokio::test]
async fn snapshots_wait_while_manna_or_a_chat_turn_works() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure());
    let t = app.teacher("T-SNAP-2", "general").await;
    let ws = session_with_worksheet(&app, &t).await;
    let no = worksheet(&app, &t, ws).await["version_no"]
        .as_i64()
        .unwrap();

    sqlx::query(
        "UPDATE prep_turns SET status = 'running', chat_worksheet_id = $1 WHERE worksheet_id = $1",
    )
    .bind(ws)
    .execute(&app.db)
    .await
    .unwrap();
    let (s, v) = snap(&app, &t, ws, Some("x"), no).await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    assert!(v.to_string().contains("備課對話"), "{v}");
    sqlx::query("UPDATE prep_turns SET status = 'done' WHERE worksheet_id = $1")
        .bind(ws)
        .execute(&app.db)
        .await
        .unwrap();

    sqlx::query(
        "INSERT INTO worksheet_jobs (worksheet_id, teacher_id, run_no, step, status)
         SELECT id, teacher_id, 99, 'tasks', 'running' FROM worksheets WHERE id = $1",
    )
    .bind(ws)
    .execute(&app.db)
    .await
    .unwrap();
    let (s, v) = snap(&app, &t, ws, Some("x"), no).await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    assert!(v.to_string().contains("正在產生"), "{v}");
    let n: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM worksheet_snapshots WHERE worksheet_id = $1")
            .bind(ws)
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(n, 0);
}
