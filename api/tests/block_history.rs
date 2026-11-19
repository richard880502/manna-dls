//! 修改歷程 (docs/API.md「修改歷程與採用過往的版本」): the steps of one block are derived from the
//! versions and the proposals, a past version or a once-rejected proposal can be adopted as a new
//! version (text and pictures), the picture buttons are steps, and nothing is ever deleted.

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

async fn revise_block(
    app: &common::TestApp,
    t: &str,
    ws: i64,
    block: &str,
    instruction: &str,
) -> Value {
    let (s, v) = app
        .post(
            &format!("/api/worksheets/{ws}/blocks/{block}/revise"),
            Some(t),
            json!({ "instruction": instruction }),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    let pid = v["id"].as_i64().unwrap();
    for _ in 0..800 {
        let (_, v) = app
            .get(&format!("/api/worksheets/{ws}/proposals/{pid}"), Some(t))
            .await;
        if v["status"] != "running" {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("proposal {pid} did not finish");
}

async fn decide(app: &common::TestApp, t: &str, ws: i64, pid: i64, what: &str) -> Value {
    let (s, v) = app
        .post(
            &format!("/api/worksheets/{ws}/proposals/{pid}/{what}"),
            Some(t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v
}

async fn history(app: &common::TestApp, t: &str, ws: i64, block: &str) -> Value {
    let (s, v) = app
        .get(
            &format!("/api/worksheets/{ws}/blocks/{block}/history"),
            Some(t),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v
}

async fn adopt(
    app: &common::TestApp,
    t: &str,
    ws: i64,
    block: &str,
    body: Value,
) -> (StatusCode, Value) {
    app.post(
        &format!("/api/worksheets/{ws}/blocks/{block}/adopt"),
        Some(t),
        body,
    )
    .await
}

fn kinds(h: &Value) -> Vec<String> {
    h["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["kind"].as_str().unwrap().to_string())
        .collect()
}

async fn count(app: &common::TestApp, table: &str) -> i64 {
    sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
        .fetch_one(&app.db)
        .await
        .unwrap()
}

async fn usage(app: &common::TestApp, event: &str) -> Vec<Value> {
    sqlx::query_scalar("SELECT payload FROM usage_logs WHERE event = $1 ORDER BY id")
        .bind(event)
        .fetch_all(&app.db)
        .await
        .unwrap()
}

/// The editor saving the block with another title (a manual edit).
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

// ---------------------------------------------------------------- tests

#[tokio::test]
async fn the_steps_of_a_block_and_adopting_a_past_version_with_its_pictures() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure());
    let t = app.teacher("T-BH-1", "general").await;
    let other = app.teacher("T-BH-1B", "general").await;
    let ws = session_with_worksheet(&app, &t).await;
    let state = worksheet(&app, &t, ws).await;
    let (block, slot) = first_slot(&state);
    let block_before = block_json(&state, &block);
    let image_before = current_image(&wait_images(&app, &t, ws).await, &slot);
    assert_eq!(image_before["status"], "done");

    // Only the original so far.
    let h = history(&app, &t, ws, &block).await;
    assert_eq!(kinds(&h), ["original"], "{h}");
    let original = &h["steps"][0];
    assert_eq!(original["label"], "原本");
    assert_eq!(original["current"], true);
    assert_eq!(original["adopt"]["possible"], false);
    assert_eq!(
        original["pictures"][0]["image_id"],
        image_before["image_id"]
    );
    let original_no = original["version_no"].as_i64().unwrap();
    let viewed = usage(&app, "worksheet_block_history_viewed").await;
    assert_eq!(viewed.len(), 1);
    assert_eq!(viewed[0]["block_id"], block.as_str());
    assert_eq!(viewed[0]["steps"], 1);

    // 採用 a proposal (text, a new picture, a redraw).
    let p = revise_block(
        &app,
        &t,
        ws,
        &block,
        "改一題，加蜜蜂的圖，原本那張重畫 FAKE_REVISE_TOUCH_ALL FAKE_REVISE_PIC_ADD FAKE_REVISE_PIC_REDRAW",
    )
    .await;
    assert_eq!(p["status"], "proposed", "{p}");
    let pid = p["id"].as_i64().unwrap();
    decide(&app, &t, ws, pid, "accept").await;
    let after_images = wait_images(&app, &t, ws).await;
    let redrawn = current_image(&after_images, &slot);
    assert_ne!(redrawn["image_id"], image_before["image_id"]);
    let accepted_block = block_json(&worksheet(&app, &t, ws).await, &block);
    let h = history(&app, &t, ws, &block).await;
    assert_eq!(kinds(&h), ["proposal_adopted", "original"], "{h}");
    let adopted_step = h["steps"][0].clone();
    assert_eq!(adopted_step["label"], "Manna 的提案（你採用）");
    assert_eq!(adopted_step["proposal_id"], pid);
    assert_eq!(adopted_step["current"], true);
    assert_eq!(adopted_step["pictures"].as_array().unwrap().len(), 2);
    assert_eq!(adopted_step["pictures"][0]["image_id"], redrawn["image_id"]);
    // The step before keeps its picture: the row that was current then.
    assert_eq!(
        h["steps"][1]["pictures"][0]["image_id"],
        image_before["image_id"]
    );
    assert_eq!(h["steps"][1]["adopt"]["possible"], true);
    assert_eq!(h["steps"][1]["adopt"]["clean"], true);
    // An adopted proposal has no second way in.
    let (s, v) = adopt(&app, &t, ws, &block, json!({ "proposal_id": pid })).await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");

    // The teacher's own edit, then her own 重畫 button.
    edit_title(&app, &t, ws, &block, "My own title").await;
    let (s, v) = app
        .post(
            &format!("/api/worksheets/{ws}/image-slots/{slot}/regenerate"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    let button_images = wait_images(&app, &t, ws).await;
    let button_image = current_image(&button_images, &slot);
    assert_ne!(button_image["image_id"], redrawn["image_id"]);
    let h = history(&app, &t, ws, &block).await;
    assert_eq!(
        kinds(&h),
        ["pictures", "manual", "proposal_adopted", "original"],
        "{h}"
    );
    assert_eq!(h["steps"][0]["label"], "改圖片");
    assert_eq!(h["steps"][0]["current"], true);
    assert_eq!(h["steps"][1]["label"], "你手動修改");
    assert_eq!(
        h["steps"][0]["pictures"][0]["image_id"],
        button_image["image_id"]
    );

    // Adopt the original: text and the original picture are back, nothing drawn, nothing deleted.
    let before_adopt = worksheet(&app, &t, ws).await;
    let versions = count(&app, "worksheet_versions").await;
    let images = count(&app, "worksheet_images").await;
    let proposals = count(&app, "worksheet_block_proposals").await;
    let (s, done) = adopt(&app, &t, ws, &block, json!({ "version_no": original_no })).await;
    assert_eq!(s, StatusCode::OK, "{done}");
    assert_eq!(done["kind"], "version");
    assert_eq!(done["pictures"], json!([]));
    assert_eq!(block_json(&done["worksheet"], &block), block_before);
    let back = current_image(&done["worksheet"]["images"], &slot);
    assert_eq!(back["image_id"], image_before["image_id"]);
    assert_eq!(back["status"], "done");
    assert_eq!(count(&app, "worksheet_versions").await, versions + 1);
    assert_eq!(
        count(&app, "worksheet_images").await,
        images,
        "no picture was drawn"
    );
    assert_eq!(count(&app, "worksheet_block_proposals").await, proposals);
    let (cause, source): (String, Value) = sqlx::query_as(
        "SELECT cause, source FROM worksheet_versions WHERE worksheet_id = $1
          ORDER BY version_no DESC LIMIT 1",
    )
    .bind(ws)
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(cause, "restore");
    assert_eq!(source["adopted_kind"], "version");
    assert_eq!(source["adopted_version_no"], original_no);
    assert_eq!(done["version_no"], done["worksheet"]["version_no"]);
    let logged = usage(&app, "worksheet_block_version_adopted").await;
    assert_eq!(logged.len(), 1);
    assert_eq!(logged[0]["block_id"], block.as_str());
    assert_eq!(logged[0]["adopted_version_no"], original_no);
    assert_eq!(logged[0]["kind"], "version");
    assert_eq!(logged[0]["from_version_no"], before_adopt["version_no"]);
    let h = history(&app, &t, ws, &block).await;
    assert_eq!(h["steps"][0]["kind"], "restore", "{h}");
    assert_eq!(h["steps"][0]["label"], "回到這一步");
    assert_eq!(h["steps"][0]["current"], true);
    assert_eq!(h["steps"].as_array().unwrap().len(), 5);

    // And forward again: the step of the adopted proposal brings back its text and both pictures.
    let (s, done) = adopt(
        &app,
        &t,
        ws,
        &block,
        json!({ "version_no": adopted_step["version_no"] }),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{done}");
    assert_eq!(block_json(&done["worksheet"], &block), accepted_block);
    let slots = done["worksheet"]["images"]["slots"].as_array().unwrap();
    assert_eq!(
        slots
            .iter()
            .filter(|s| s["block_id"] == block.as_str())
            .count(),
        2
    );
    assert_eq!(
        current_image(&done["worksheet"]["images"], &slot)["image_id"],
        redrawn["image_id"]
    );
    assert_eq!(
        count(&app, "worksheet_images").await,
        images,
        "still nothing drawn"
    );

    // 復原上一版 takes the restore back (the stack works for restore steps too).
    let (s, st) = app
        .post(
            &format!("/api/worksheets/{ws}/blocks/{block}/undo"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{st}");
    assert_eq!(block_json(&st, &block), block_before);
    assert_eq!(
        current_image(&st["images"], &slot)["image_id"],
        image_before["image_id"]
    );

    // Refusals.
    let h = history(&app, &t, ws, &block).await;
    let top = h["steps"][0]["version_no"].clone();
    let (s, _) = adopt(&app, &t, ws, &block, json!({ "version_no": top })).await;
    assert_eq!(s, StatusCode::CONFLICT, "the version the block is in");
    let (s, _) = adopt(&app, &t, ws, &block, json!({})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = adopt(
        &app,
        &t,
        ws,
        &block,
        json!({ "version_no": 1, "proposal_id": pid }),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = adopt(&app, &t, ws, &block, json!({ "version_no": 9999 })).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = adopt(
        &app,
        &other,
        ws,
        &block,
        json!({ "version_no": original_no }),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = app
        .get(
            &format!("/api/worksheets/{ws}/blocks/{block}/history"),
            Some(&other),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = app
        .get(
            &format!("/api/worksheets/{ws}/blocks/nope/history"),
            Some(&t),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_rejected_proposal_can_be_adopted_later_and_runs_its_picture_actions() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure());
    let t = app.teacher("T-BH-2", "general").await;
    let ws = session_with_worksheet(&app, &t).await;
    let state = worksheet(&app, &t, ws).await;
    let (block, _slot) = first_slot(&state);
    let slots_before = state["images"]["slots"].as_array().unwrap().len();

    let p = revise_block(
        &app,
        &t,
        ws,
        &block,
        "改一題，加蜜蜂的圖，原本那張重畫 FAKE_REVISE_TOUCH_ALL FAKE_REVISE_PIC_ADD FAKE_REVISE_PIC_REDRAW",
    )
    .await;
    let pid = p["id"].as_i64().unwrap();
    let interaction = p["interaction_id"]
        .as_i64()
        .expect("the proposal has an interaction");
    decide(&app, &t, ws, pid, "reject").await;
    let rows = count(&app, "worksheet_images").await;

    // It shows as a step, with the picture actions that never ran.
    let h = history(&app, &t, ws, &block).await;
    assert_eq!(kinds(&h), ["proposal_rejected", "original"], "{h}");
    let step = &h["steps"][0];
    assert_eq!(step["label"], "Manna 的提案（沒採用）");
    assert_eq!(step["proposal_id"], pid);
    assert_eq!(step["picture_actions"].as_array().unwrap().len(), 2);
    assert_eq!(step["adopt"]["draws_pictures"], true);
    assert_eq!(step["adopt"]["warn_older"], false);
    assert_eq!(step["adopt"]["clean"], false);
    assert!(step["block"]["title"]
        .as_str()
        .unwrap()
        .contains("(touched)"));
    assert_eq!(
        count(&app, "worksheet_images").await,
        rows,
        "looking draws nothing"
    );

    // The teacher changes the block meanwhile: the proposal is now from an older state.
    edit_title(&app, &t, ws, &block, "Changed since").await;
    let h = history(&app, &t, ws, &block).await;
    let step = h["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["kind"] == "proposal_rejected")
        .unwrap();
    assert_eq!(step["adopt"]["warn_older"], true);
    let before_adopt_block = block_json(&worksheet(&app, &t, ws).await, &block);

    let (s, done) = adopt(&app, &t, ws, &block, json!({ "proposal_id": pid })).await;
    assert_eq!(s, StatusCode::OK, "{done}");
    assert_eq!(done["kind"], "proposal");
    let outcomes = done["pictures"].as_array().unwrap();
    assert_eq!(outcomes.len(), 2, "{done}");
    assert!(outcomes.iter().all(|o| o["status"] == "queued"));
    let adopted = block_json(&done["worksheet"], &block);
    assert!(adopted["title"].as_str().unwrap().contains("(touched)"));
    assert_eq!(adopted["image_slots"].as_array().unwrap().len(), 2);
    wait_images(&app, &t, ws).await;
    assert!(
        count(&app, "worksheet_images").await > rows,
        "its pictures were drawn now"
    );

    // The decision of the time stays; the adoption is a new row; the proposal is marked.
    let decisions: Vec<String> = sqlx::query_scalar(
        "SELECT decision FROM teacher_decisions WHERE ai_interaction_id = $1 ORDER BY id",
    )
    .bind(interaction)
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert_eq!(decisions, ["reject", "adopt"]);
    let (status, later): (String, Option<i32>) = sqlx::query_as(
        "SELECT status, adopted_later_version_no FROM worksheet_block_proposals WHERE id = $1",
    )
    .bind(pid)
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(status, "rejected");
    assert_eq!(later.map(i64::from), done["version_no"].as_i64());
    let logged = usage(&app, "worksheet_block_version_adopted").await;
    assert_eq!(logged.len(), 1);
    assert_eq!(logged[0]["proposal_id"], pid);
    assert_eq!(logged[0]["kind"], "proposal");
    assert_eq!(logged[0]["warned_older"], true);
    let source: Value = sqlx::query_scalar(
        "SELECT source FROM worksheet_versions WHERE worksheet_id = $1 ORDER BY version_no DESC LIMIT 1",
    )
    .bind(ws)
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(source["cause"], "restore");
    assert_eq!(source["proposal_id"], pid);
    let h = history(&app, &t, ws, &block).await;
    assert_eq!(h["steps"][0]["kind"], "restore");
    assert_eq!(h["steps"][0]["current"], true);
    let step = h["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["kind"] == "proposal_rejected")
        .unwrap();
    assert_eq!(step["adopted_later_version_no"], done["version_no"]);

    // 復原上一版 gives back the text and the pictures (a slot of the proposal goes away).
    let (s, st) = app
        .post(
            &format!("/api/worksheets/{ws}/blocks/{block}/undo"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{st}");
    assert_eq!(block_json(&st, &block), before_adopt_block);
    assert_eq!(
        st["images"]["slots"].as_array().unwrap().len(),
        slots_before
    );
}

#[tokio::test]
async fn picture_buttons_are_steps_and_do_not_stop_the_editor_from_saving() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure());
    let t = app.teacher("T-BH-3", "general").await;
    let ws = session_with_worksheet(&app, &t).await;
    let state = worksheet(&app, &t, ws).await;
    let (block, slot) = first_slot(&state);
    let block_before = block_json(&state, &block);
    let image_before = current_image(&wait_images(&app, &t, ws).await, &slot);
    let seen = state["version_no"].as_i64().unwrap();

    let (s, v) = app
        .post(
            &format!("/api/worksheets/{ws}/image-slots/{slot}/regenerate"),
            Some(&t),
            json!({ "hint": "改成晴天" }),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    wait_images(&app, &t, ws).await;
    let (s, v) = app
        .delete(
            &format!("/api/worksheets/{ws}/image-slots/{slot}"),
            Some(&t),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let now = worksheet(&app, &t, ws).await;
    assert_eq!(now["version_no"].as_i64().unwrap(), seen + 2);
    assert_eq!(now["content"], state["content"], "the text did not change");
    let h = history(&app, &t, ws, &block).await;
    assert_eq!(kinds(&h), ["pictures", "pictures", "original"], "{h}");
    assert_eq!(h["steps"][0]["detail"], "拿掉了一張圖");
    assert_eq!(h["steps"][0]["pictures"][0]["status"], "removed");
    assert_eq!(h["steps"][1]["detail"], "重畫了一張圖");
    let src: Value = sqlx::query_scalar(
        "SELECT source FROM worksheet_versions WHERE worksheet_id = $1 AND version_no = $2",
    )
    .bind(ws)
    .bind((seen + 1) as i32)
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(src["cause"], "pictures");
    assert_eq!(src["action"], "regenerate");
    assert_eq!(src["slot_id"], slot.as_str());

    // An editor that still holds the version before the picture buttons saves its edit.
    let mut content = state["content"].clone();
    for b in content["blocks"].as_array_mut().unwrap() {
        if b["id"] == block.as_str() {
            b["title"] = json!("Typed meanwhile");
        }
    }
    let (s, v) = app
        .put(
            &format!("/api/worksheets/{ws}"),
            Some(&t),
            json!({ "content": content, "base_version_no": seen }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");

    // The original brings its picture back, though it was redrawn and removed since.
    let h = history(&app, &t, ws, &block).await;
    let original = h["steps"].as_array().unwrap().last().unwrap().clone();
    let (s, done) = adopt(
        &app,
        &t,
        ws,
        &block,
        json!({ "version_no": original["version_no"] }),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{done}");
    assert_eq!(block_json(&done["worksheet"], &block), block_before);
    let back = current_image(&done["worksheet"]["images"], &slot);
    assert_eq!(back["image_id"], image_before["image_id"]);
    assert_eq!(back["status"], "done");
}

#[tokio::test]
async fn regeneration_and_undo_are_steps_and_adopting_waits_while_manna_works() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure());
    let t = app.teacher("T-BH-4", "general").await;
    let ws = session_with_worksheet(&app, &t).await;
    let state = worksheet(&app, &t, ws).await;
    let (block, _slot) = first_slot(&state);
    let block_before = block_json(&state, &block);
    let (s, v) = app
        .post(
            &format!("/api/worksheets/{ws}/blocks/{block}/regenerate"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    for _ in 0..800 {
        let (_, v) = app
            .get(&format!("/api/worksheets/{ws}/jobs"), Some(&t))
            .await;
        if v["running"] == false {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    wait_images(&app, &t, ws).await;
    let h = history(&app, &t, ws, &block).await;
    assert_eq!(kinds(&h)[0], "regenerate", "{h}");
    assert_eq!(h["steps"][0]["label"], "Manna 重新產生");
    let original_no = h["steps"].as_array().unwrap().last().unwrap()["version_no"].clone();

    let (s, st) = app
        .post(
            &format!("/api/worksheets/{ws}/blocks/{block}/undo"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{st}");
    assert_eq!(block_json(&st, &block), block_before);
    let h = history(&app, &t, ws, &block).await;
    assert_eq!(kinds(&h)[0], "undo", "{h}");
    assert_eq!(h["steps"][0]["label"], "復原");

    // While a revision of the block runs nothing is adopted.
    sqlx::query(
        "INSERT INTO worksheet_block_proposals
           (worksheet_id, teacher_id, block_id, base_version_no, base_block, instruction,
            comment_ids, whole_block, round, status)
         SELECT $1, teacher_id, $2, 1, '{}'::jsonb, '', '{}', FALSE, 1, 'running'
           FROM worksheets WHERE id = $1",
    )
    .bind(ws)
    .bind(&block)
    .execute(&app.db)
    .await
    .unwrap();
    let (s, v) = adopt(&app, &t, ws, &block, json!({ "version_no": original_no })).await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    let h = history(&app, &t, ws, &block).await;
    assert_eq!(h["busy"], true);
}
