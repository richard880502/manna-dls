//! 版本紀錄 (docs/API.md「版本紀錄與還原整份學習單」): every version is listed, any one can be read
//! back as a sheet, and 還原成這一版 writes the old content as a NEW version (text and pictures),
//! keeps each changed block in its own 修改歷程, and waits while Manna or a chat turn works.

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

fn block_json(state: &Value, id: &str) -> Value {
    state["content"]["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["id"] == id)
        .unwrap()
        .clone()
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

fn current_image(images: &Value, slot_id: &str) -> Value {
    images["slots"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["slot_id"] == slot_id)
        .cloned()
        .unwrap_or(Value::Null)
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

async fn restore(
    app: &common::TestApp,
    t: &str,
    ws: i64,
    no: i64,
    base: i64,
) -> (StatusCode, Value) {
    app.post(
        &format!("/api/worksheets/{ws}/versions/{no}/restore"),
        Some(t),
        json!({ "base_version_no": base }),
    )
    .await
}

async fn usage(app: &common::TestApp, event: &str) -> Vec<Value> {
    sqlx::query_scalar("SELECT payload FROM usage_logs WHERE event = $1 ORDER BY id")
        .bind(event)
        .fetch_all(&app.db)
        .await
        .unwrap()
}

#[tokio::test]
async fn versions_are_listed_readable_and_restorable_as_a_new_version() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure());
    let t = app.teacher("T-VER-1", "general").await;
    let other = app.teacher("T-VER-1B", "general").await;
    let ws = session_with_worksheet(&app, &t).await;
    let state = worksheet(&app, &t, ws).await;
    let (block, slot) = first_slot(&state);
    let original_block = block_json(&state, &block);
    let image_before = current_image(&wait_images(&app, &t, ws).await, &slot);
    let first_no = state["version_no"].as_i64().unwrap();

    edit_title(&app, &t, ws, &block, "Title two").await;
    edit_title(&app, &t, ws, &block, "Title three").await;
    let latest = worksheet(&app, &t, ws).await;
    let latest_no = latest["version_no"].as_i64().unwrap();
    assert_eq!(latest_no, first_no + 2);

    // The list: newest first, with cause and source.
    let (s, list) = app
        .get(&format!("/api/worksheets/{ws}/versions"), Some(&t))
        .await;
    assert_eq!(s, StatusCode::OK, "{list}");
    let nos: Vec<i64> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["version_no"].as_i64().unwrap())
        .collect();
    assert_eq!(nos[0], latest_no);
    assert_eq!(*nos.last().unwrap(), 1);
    assert!(nos.windows(2).all(|w| w[0] > w[1]));
    assert_eq!(list[0]["cause"], "edit");

    // An earlier version as a sheet (the title it had), the other teacher gets 404.
    let q = "level=standard&audience=teacher";
    let (s, old) = app
        .get(
            &format!("/api/worksheets/{ws}/versions/{}/sheet?{q}", first_no + 1),
            Some(&t),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{old}");
    assert!(old.to_string().contains("Title two"), "{old}");
    assert!(!old.to_string().contains("Title three"));
    let (s, cur) = app
        .get(&format!("/api/worksheets/{ws}/sheet?{q}"), Some(&t))
        .await;
    assert_eq!(s, StatusCode::OK);
    assert!(cur.to_string().contains("Title three"));
    let (s, _) = app
        .get(
            &format!("/api/worksheets/{ws}/versions/{}/sheet?{q}", first_no),
            Some(&other),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = app
        .get(
            &format!("/api/worksheets/{ws}/versions/999/sheet?{q}"),
            Some(&t),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = restore(&app, &other, ws, first_no, latest_no).await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // 409s: the latest one, a stale base.
    let (s, v) = restore(&app, &t, ws, latest_no, latest_no).await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    let (s, v) = restore(&app, &t, ws, first_no, latest_no - 1).await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    assert!(v["error"].to_string().contains("新的版本") || v.to_string().contains("新的版本"));

    // Restoring writes a new version with the old content.
    let (s, done) = restore(&app, &t, ws, first_no, latest_no).await;
    assert_eq!(s, StatusCode::OK, "{done}");
    assert_eq!(done["version_no"], latest_no + 1);
    assert_eq!(done["from_version_no"], first_no);
    assert_eq!(block_json(&done["worksheet"], &block), original_block);
    assert_eq!(done["worksheet"]["version_no"], latest_no + 1);
    assert_eq!(
        done["worksheet"]["versions_count"].as_i64().unwrap(),
        latest_no + 1
    );
    let (cause, source): (String, Value) = sqlx::query_as(
        "SELECT cause, source FROM worksheet_versions WHERE worksheet_id = $1 AND version_no = $2",
    )
    .bind(ws)
    .bind((latest_no + 1) as i32)
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(cause, "restore");
    assert_eq!(source["adopted_kind"], "worksheet");
    assert_eq!(source["adopted_version_no"], first_no);
    assert_eq!(source["from_version_no"], latest_no);
    // Nothing was lost: the old versions are all still there and unchanged.
    let (_, list) = app
        .get(&format!("/api/worksheets/{ws}/versions"), Some(&t))
        .await;
    assert_eq!(list.as_array().unwrap().len() as i64, latest_no + 1);
    let (_, old) = app
        .get(
            &format!("/api/worksheets/{ws}/versions/{}/sheet?{q}", latest_no),
            Some(&t),
        )
        .await;
    assert!(old.to_string().contains("Title three"));
    // The picture of the version is the current one again.
    assert_eq!(
        current_image(&wait_images(&app, &t, ws).await, &slot)["image_id"],
        image_before["image_id"]
    );

    // The block's own 修改歷程 knows the step and can still go back one step.
    let (s, h) = app
        .get(
            &format!("/api/worksheets/{ws}/blocks/{block}/history"),
            Some(&t),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{h}");
    assert_eq!(h["steps"][0]["kind"], "restore", "{h}");
    let rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM worksheet_block_history
          WHERE worksheet_id = $1 AND block_id = $2 AND version_no = $3 AND after_block IS NOT NULL",
    )
    .bind(ws)
    .bind(&block)
    .bind((latest_no + 1) as i32)
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(rows, 1);
    let (s, st) = app
        .post(
            &format!("/api/worksheets/{ws}/blocks/{block}/undo"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{st}");
    assert_eq!(block_json(&st, &block)["title"], "Title three");

    let logged = usage(&app, "worksheet_version_restored").await;
    assert_eq!(logged.len(), 1);
    assert_eq!(logged[0]["from_version_no"], first_no);
    assert_eq!(logged[0]["previous_version_no"], latest_no);
    assert_eq!(logged[0]["new_version_no"], latest_no + 1);
    assert!(logged[0].as_object().unwrap().len() == 4);
}

#[tokio::test]
async fn restoring_waits_while_manna_or_a_chat_turn_works() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure());
    let t = app.teacher("T-VER-2", "general").await;
    let ws = session_with_worksheet(&app, &t).await;
    let state = worksheet(&app, &t, ws).await;
    let (block, _) = first_slot(&state);
    edit_title(&app, &t, ws, &block, "Title two").await;
    let latest = worksheet(&app, &t, ws).await["version_no"]
        .as_i64()
        .unwrap();

    // A prep chat turn on the worksheet is running.
    sqlx::query(
        "UPDATE prep_turns SET status = 'running', chat_worksheet_id = $1 WHERE worksheet_id = $1",
    )
    .bind(ws)
    .execute(&app.db)
    .await
    .unwrap();
    let (s, v) = restore(&app, &t, ws, 1, latest).await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    assert!(v.to_string().contains("備課對話"), "{v}");
    sqlx::query("UPDATE prep_turns SET status = 'done' WHERE worksheet_id = $1")
        .bind(ws)
        .execute(&app.db)
        .await
        .unwrap();

    // The worksheet is generating.
    sqlx::query(
        "INSERT INTO worksheet_jobs (worksheet_id, teacher_id, run_no, step, status)
         SELECT id, teacher_id, 99, 'tasks', 'running' FROM worksheets WHERE id = $1",
    )
    .bind(ws)
    .execute(&app.db)
    .await
    .unwrap();
    let (s, v) = restore(&app, &t, ws, 1, latest).await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    assert!(v.to_string().contains("正在產生"), "{v}");
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM worksheet_versions WHERE worksheet_id = $1")
            .bind(ws)
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(count, latest);
}
