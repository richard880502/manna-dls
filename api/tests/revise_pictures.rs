//! Pictures in revisions (docs/API.md「改圖片」) and pictures attached to a revise request
//! (「附上圖片」): the fake model proposes picture actions (`FAKE_PREP_PIC_*`, `FAKE_REVISE_PIC_*`),
//! the server checks and runs them through the picture machinery (the turn-level revise applies
//! them with its text, a block revision only when the teacher accepts); uploads are validated,
//! re-encoded, owner-only and removed with the session; the model gets them as image parts.

#[macro_use]
mod common;

use std::io::Cursor;
use std::path::PathBuf;
use std::time::Duration;

use axum::http::{Method, StatusCode};
use common::Part;
use manna_api::config::Config;
use manna_api::storage::StorageConfig;
use manna_api::worksheet::images::ImageConfig;
use serde_json::{json, Value};

// ---------------------------------------------------------------- harness

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("manna-rev-{}", manna_api::auth::new_token()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    /// File storage here and the fake picture provider (with `change` applied).
    fn configure(&self, change: impl FnOnce(&mut ImageConfig)) -> impl FnOnce(&mut Config) + '_ {
        let mut images = ImageConfig::fake();
        change(&mut images);
        move |c: &mut Config| {
            c.storage = StorageConfig::fs(self.0.clone());
            c.images = images;
        }
    }

    /// Storage only (pictures off).
    fn storage_only(&self) -> impl FnOnce(&mut Config) + '_ {
        move |c: &mut Config| {
            c.storage = StorageConfig::fs(self.0.clone());
        }
    }

    /// The stored files under `prefix`.
    fn files(&self, prefix: &str) -> usize {
        fn walk(dir: &std::path::Path) -> usize {
            std::fs::read_dir(dir)
                .map(|rd| {
                    rd.flatten()
                        .map(|e| {
                            if e.path().is_dir() {
                                walk(&e.path())
                            } else {
                                1
                            }
                        })
                        .sum()
                })
                .unwrap_or(0)
        }
        walk(&self.0.join(prefix))
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

fn png(width: u32, height: u32) -> Vec<u8> {
    let image = image::RgbImage::from_fn(width, height, |x, y| {
        image::Rgb([(x % 251) as u8, (y % 241) as u8, 90])
    });
    let mut out = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image)
        .write_to(&mut out, image::ImageFormat::Png)
        .unwrap();
    out.into_inner()
}

/// A prep session whose first turn made a worksheet (with the fake model): (session id,
/// worksheet id). The pictures it asked for are drawn before this returns.
async fn session_with_worksheet(app: &common::TestApp, t: &str) -> (i64, i64) {
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
    let v = wait_session(app, t, id).await;
    let ws = v["turns"][0]["worksheet"]["id"].as_i64().unwrap();
    wait_images(app, t, ws).await;
    (id, ws)
}

async fn wait_session(app: &common::TestApp, t: &str, id: i64) -> Value {
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

/// Every slot of the content: (block id, slot).
fn slots_of(state: &Value) -> Vec<(String, Value)> {
    state["content"]["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|b| {
            let id = b["id"].as_str().unwrap().to_string();
            b["image_slots"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(move |s| (id.clone(), s))
        })
        .collect()
}

/// A revise turn; the finished turn.
async fn revise_turn(app: &common::TestApp, t: &str, session: i64, body: Value) -> Value {
    let (s, v) = app
        .post(
            &format!("/api/prep-sessions/{session}/turns"),
            Some(t),
            body,
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    let v = wait_session(app, t, session).await;
    v["turns"].as_array().unwrap().last().unwrap().clone()
}

async fn usage(app: &common::TestApp, event: &str) -> Vec<Value> {
    sqlx::query_scalar("SELECT payload FROM usage_logs WHERE event = $1 ORDER BY id")
        .bind(event)
        .fetch_all(&app.db)
        .await
        .unwrap()
}

async fn upload(
    app: &common::TestApp,
    t: &str,
    session: i64,
    name: &str,
    data: &[u8],
) -> (StatusCode, Value) {
    app.post_multipart(
        &format!("/api/prep-sessions/{session}/images"),
        Some(t),
        &[Part::File("file", name, data)],
    )
    .await
}

fn err_message(v: &Value) -> &str {
    v["error"]["message"].as_str().unwrap_or_default()
}

// ---------------------------------------------------------------- turn-level revise

#[tokio::test]
async fn a_revise_turn_adds_and_redraws_pictures_with_its_text() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure(|_| {}));
    let t = app.teacher("T-RP-1", "general").await;
    let (session, ws) = session_with_worksheet(&app, &t).await;
    let before = worksheet(&app, &t, ws).await;
    let slots_before = slots_of(&before);
    assert!(!slots_before.is_empty(), "the fake model writes a slot");
    let left_before = before["images"]["regenerations_left"].as_i64().unwrap();

    let turn = revise_turn(
        &app,
        &t,
        session,
        json!({ "kind": "revise",
                "request": "B 區加一張蜜蜂在花上的圖，第一張圖重畫不要有文字 FAKE_PREP_PIC_ADD FAKE_PREP_PIC_REDRAW" }),
    )
    .await;
    assert_eq!(turn["status"], "done", "{turn}");
    let result = &turn["result"];
    let pictures = result["pictures"].as_array().unwrap();
    assert_eq!(pictures.len(), 2, "{result}");
    assert_eq!(pictures[0]["action"], "add");
    assert!(pictures[0]["text"]
        .as_str()
        .unwrap()
        .starts_with("加一張圖："));
    assert_eq!(pictures[0]["status"], "queued");
    assert_eq!(pictures[1]["action"], "redraw");
    assert!(pictures[1]["text"]
        .as_str()
        .unwrap()
        .starts_with("重畫第 1 張圖："));
    assert_eq!(pictures[1]["status"], "queued");
    assert!(result["reply"].as_str().unwrap().contains("圖片："));
    // The new slot is part of the saved version, with the model's description.
    let after = worksheet(&app, &t, ws).await;
    let slots_after = slots_of(&after);
    assert_eq!(slots_after.len(), slots_before.len() + 1);
    let new = slots_after
        .iter()
        .find(|(_, s)| s["prompt_en"].as_str().unwrap().contains("honey bee"))
        .expect("the new slot");
    assert_eq!(new.1["purpose_zh"], "蜜蜂停在花上");
    assert!(result["changed_block_ids"]
        .as_array()
        .unwrap()
        .iter()
        .any(|b| b == new.0.as_str()));
    // Both pictures are drawn with the picture machinery; the redraw used one of the 重畫.
    let images = wait_images(&app, &t, ws).await;
    let drawn = images["slots"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["slot_id"] == new.1["id"])
        .unwrap();
    assert_eq!(drawn["status"], "done", "{images}");
    assert_eq!(
        images["regenerations_left"].as_i64().unwrap(),
        left_before - 1
    );
    // The research data: counts, not texts.
    let logged = usage(&app, "worksheet_picture_actions_applied").await;
    assert_eq!(logged.len(), 1);
    assert_eq!(logged[0]["source"], "prep_turn");
    assert_eq!(logged[0]["add"], 1);
    assert_eq!(logged[0]["redraw"], 1);
    assert_eq!(logged[0]["queued"], 2);
    assert_eq!(logged[0]["skipped"], 0);
    // The raw output (with its picture_actions) is kept like any other revise outcome.
    let output: Value = sqlx::query_scalar(
        "SELECT output FROM ai_interactions WHERE module = 'prep_revise' ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(output["picture_actions"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn a_revise_turn_removes_a_picture_and_drops_invalid_actions() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure(|_| {}));
    let t = app.teacher("T-RP-2", "general").await;
    let (session, ws) = session_with_worksheet(&app, &t).await;
    let slots = slots_of(&worksheet(&app, &t, ws).await);
    let last_slot = slots.last().unwrap().1["id"].as_str().unwrap().to_string();

    let turn = revise_turn(
        &app,
        &t,
        session,
        json!({ "kind": "revise", "request": "拿掉最後一張圖 FAKE_PREP_PIC_REMOVE FAKE_PREP_PIC_BAD" }),
    )
    .await;
    let result = &turn["result"];
    let pictures = result["pictures"].as_array().unwrap();
    // Only the removal is valid: an unknown block, an unknown slot (twice) and a web address
    // are dropped by the server, and the teacher is told.
    assert_eq!(pictures.len(), 1, "{result}");
    assert_eq!(pictures[0]["action"], "remove");
    assert_eq!(pictures[0]["status"], "removed");
    assert!(result["reply"].as_str().unwrap().contains("沒有通過檢查"));
    let images = wait_images(&app, &t, ws).await;
    let removed = images["slots"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["slot_id"] == last_slot.as_str())
        .unwrap();
    assert_eq!(removed["status"], "removed");
    // Nothing was added.
    assert_eq!(slots_of(&worksheet(&app, &t, ws).await).len(), slots.len());
}

#[tokio::test]
async fn too_many_picture_actions_are_trimmed_and_a_picture_only_revise_saves_a_version() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure(|_| {}));
    let t = app.teacher("T-RP-3", "general").await;
    let (session, ws) = session_with_worksheet(&app, &t).await;
    let before = worksheet(&app, &t, ws).await;
    let turn = revise_turn(
        &app,
        &t,
        session,
        json!({ "kind": "revise", "request": "每一區都加圖 FAKE_PREP_PIC_MANY FAKE_PREP_PIC_ONLY" }),
    )
    .await;
    let result = &turn["result"];
    assert_eq!(result["pictures"].as_array().unwrap().len(), 3, "{result}");
    let after = worksheet(&app, &t, ws).await;
    assert!(after["version_no"].as_i64() > before["version_no"].as_i64());
    assert_eq!(slots_of(&after).len(), slots_of(&before).len() + 3);
    assert!(!result["changed_block_ids"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn with_pictures_off_the_actions_are_skipped_and_said_so() {
    let dir = TempDir::new();
    let app = test_app!(dir.storage_only());
    let t = app.teacher("T-RP-4", "general").await;
    let (session, ws) = session_with_worksheet(&app, &t).await;
    let before = slots_of(&worksheet(&app, &t, ws).await).len();
    let turn = revise_turn(
        &app,
        &t,
        session,
        json!({ "kind": "revise", "request": "加一張圖 FAKE_PREP_PIC_ADD" }),
    )
    .await;
    assert_eq!(turn["status"], "done", "{turn}");
    assert!(turn["result"]["pictures"].as_array().unwrap().is_empty());
    assert!(turn["result"]["reply"]
        .as_str()
        .unwrap()
        .contains("沒有開啟 AI 產圖"));
    assert_eq!(slots_of(&worksheet(&app, &t, ws).await).len(), before);
    // The text change still went through.
    assert_eq!(
        turn["result"]["changed_block_ids"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn a_redraw_beyond_the_cap_is_skipped_with_the_reason() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure(|c| c.max_regenerations = 0));
    let t = app.teacher("T-RP-5", "general").await;
    let (session, ws) = session_with_worksheet(&app, &t).await;
    let turn = revise_turn(
        &app,
        &t,
        session,
        json!({ "kind": "revise", "request": "第一張圖重畫 FAKE_PREP_PIC_REDRAW" }),
    )
    .await;
    let pictures = turn["result"]["pictures"].as_array().unwrap();
    assert_eq!(pictures.len(), 1, "{turn}");
    assert_eq!(pictures[0]["status"], "skipped");
    assert!(pictures[0]["message"].as_str().unwrap().contains("上限"));
    let logged = usage(&app, "worksheet_picture_actions_applied").await;
    assert_eq!(logged[0]["skipped"], 1);
    let _ = ws;
}

// ---------------------------------------------------------------- block revise (proposals)

async fn revise_block(app: &common::TestApp, t: &str, ws: i64, block: &str, body: Value) -> Value {
    let (s, v) = app
        .post(
            &format!("/api/worksheets/{ws}/blocks/{block}/revise"),
            Some(t),
            body,
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    let pid = v["id"].as_i64().unwrap();
    for _ in 0..800 {
        let (s, v) = app
            .get(&format!("/api/worksheets/{ws}/proposals/{pid}"), Some(t))
            .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        if v["status"] != "running" {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("proposal {pid} did not finish");
}

#[tokio::test]
async fn a_block_proposal_lists_picture_actions_and_draws_nothing_until_accepted() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure(|_| {}));
    let t = app.teacher("T-RP-6", "general").await;
    let (_, ws) = session_with_worksheet(&app, &t).await;
    let state = worksheet(&app, &t, ws).await;
    let slots = slots_of(&state);
    let (block, slot) = slots[0].clone();
    let slot_id = slot["id"].as_str().unwrap().to_string();
    let images_before = wait_images(&app, &t, ws).await;
    let rows_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM worksheet_images")
        .fetch_one(&app.db)
        .await
        .unwrap();

    let p = revise_block(
        &app,
        &t,
        ws,
        &block,
        json!({ "instruction": "這一區加一張蜜蜂的圖，原本那張重畫不要有文字 FAKE_REVISE_PIC_ADD FAKE_REVISE_PIC_REDRAW FAKE_REVISE_PIC_BAD" }),
    )
    .await;
    assert_eq!(p["status"], "proposed", "{p}");
    let actions = p["picture_actions"].as_array().unwrap();
    // The two invalid ones (an unknown slot, another block) are not in the proposal.
    assert_eq!(actions.len(), 2, "{p}");
    assert!(actions[0]["text"]
        .as_str()
        .unwrap()
        .starts_with("加一張圖："));
    assert!(actions[1]["text"]
        .as_str()
        .unwrap()
        .starts_with("重畫第 1 張圖："));
    assert_eq!(actions[1]["slot_id"], slot_id.as_str());
    // Proposed only: nothing drawn, nothing added, no extra 重畫 used.
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM worksheet_images")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(rows, rows_before);
    assert_eq!(slots_of(&worksheet(&app, &t, ws).await).len(), slots.len());

    let pid = p["id"].as_i64().unwrap();
    let (s, accepted) = app
        .post(
            &format!("/api/worksheets/{ws}/proposals/{pid}/accept"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{accepted}");
    let outcomes = accepted["pictures"].as_array().unwrap();
    assert_eq!(outcomes.len(), 2, "{accepted}");
    assert!(outcomes.iter().all(|o| o["status"] == "queued"));
    assert_eq!(
        slots_of(&worksheet(&app, &t, ws).await).len(),
        slots.len() + 1
    );
    let images = wait_images(&app, &t, ws).await;
    assert_eq!(
        images["regenerations_left"].as_i64().unwrap(),
        images_before["regenerations_left"].as_i64().unwrap() - 1
    );
    let logged = usage(&app, "worksheet_picture_actions_applied").await;
    assert_eq!(logged.len(), 1);
    assert_eq!(logged[0]["source"], "block_revise");
}

#[tokio::test]
async fn a_picture_only_proposal_can_be_accepted_or_rejected_and_rejecting_draws_nothing() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure(|_| {}));
    let t = app.teacher("T-RP-7", "general").await;
    let (_, ws) = session_with_worksheet(&app, &t).await;
    let slots = slots_of(&worksheet(&app, &t, ws).await);
    let (block, slot) = slots[0].clone();
    let slot_id = slot["id"].as_str().unwrap().to_string();

    let p = revise_block(
        &app,
        &t,
        ws,
        &block,
        json!({ "instruction": "把這張圖拿掉 FAKE_REVISE_PIC_ONLY FAKE_REVISE_PIC_REMOVE" }),
    )
    .await;
    assert_eq!(p["status"], "proposed", "{p}");
    assert_eq!(p["picture_actions"].as_array().unwrap().len(), 1);
    let pid = p["id"].as_i64().unwrap();
    let (s, r) = app
        .post(
            &format!("/api/worksheets/{ws}/proposals/{pid}/reject"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let images = wait_images(&app, &t, ws).await;
    let kept = images["slots"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["slot_id"] == slot_id.as_str())
        .unwrap();
    assert_eq!(kept["status"], "done", "rejecting removes nothing");

    let version_before = worksheet(&app, &t, ws).await["version_no"].clone();
    let p = revise_block(
        &app,
        &t,
        ws,
        &block,
        json!({ "instruction": "把這張圖拿掉 FAKE_REVISE_PIC_ONLY FAKE_REVISE_PIC_REMOVE" }),
    )
    .await;
    let pid = p["id"].as_i64().unwrap();
    let (s, accepted) = app
        .post(
            &format!("/api/worksheets/{ws}/proposals/{pid}/accept"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{accepted}");
    assert_eq!(accepted["pictures"][0]["status"], "removed");
    let images = wait_images(&app, &t, ws).await;
    let gone = images["slots"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["slot_id"] == slot_id.as_str())
        .unwrap();
    assert_eq!(gone["status"], "removed");
    // No text and no slot changed: no new version.
    assert_eq!(worksheet(&app, &t, ws).await["version_no"], version_before);
}

#[tokio::test]
async fn a_block_proposal_with_pictures_off_says_so_and_proposes_none() {
    let dir = TempDir::new();
    let app = test_app!(dir.storage_only());
    let t = app.teacher("T-RP-8", "general").await;
    let (_, ws) = session_with_worksheet(&app, &t).await;
    let state = worksheet(&app, &t, ws).await;
    let block = state["content"]["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["kind"] == "activity")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let p = revise_block(
        &app,
        &t,
        ws,
        &block,
        json!({ "instruction": "加一張圖，第一題改簡單 FAKE_REVISE_PIC_ADD" }),
    )
    .await;
    assert_eq!(p["status"], "proposed", "{p}");
    assert!(p["picture_actions"].as_array().unwrap().is_empty());
    assert!(p["reply"].as_str().unwrap().contains("沒有開啟 AI 產圖"));
}

// ---------------------------------------------------------------- attached pictures

#[tokio::test]
async fn an_upload_is_validated_reencoded_and_owner_only() {
    let dir = TempDir::new();
    let app = test_app!(dir.storage_only());
    let t = app.teacher("T-RP-9", "general").await;
    let other = app.teacher("T-RP-9B", "general").await;
    let (session, _) = session_with_worksheet(&app, &t).await;

    // A wide screenshot is scaled to 1568 px on its long side and becomes a JPEG.
    let (s, img) = upload(&app, &t, session, "shot.PNG", &png(3136, 1000)).await;
    assert_eq!(s, StatusCode::CREATED, "{img}");
    assert_eq!(img["width"], 1568);
    assert_eq!(img["height"], 500);
    assert_eq!(img["session_id"], session);
    let id = img["id"].as_i64().unwrap();
    let path = format!("/api/prep-sessions/{session}/images/{id}");
    let r = app
        .send_raw(
            Method::GET,
            &path,
            &[("authorization", &format!("Bearer {t}"))],
            Vec::new(),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.header("content-type"), "image/jpeg");
    assert!(r.body.starts_with(b"\xFF\xD8\xFF"));
    assert!(r.header("cache-control").starts_with("private"));
    // Another teacher gets a 404 for the image, the upload and the delete.
    let (s, _) = app.get(&path, Some(&other)).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = upload(&app, &other, session, "x.png", &png(10, 10)).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = app.delete(&path, Some(&other)).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = app.get(&path, None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // Type, content, size.
    let (s, v) = upload(&app, &t, session, "a.gif", b"GIF89a....").await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(err_message(&v).contains("PNG"), "{v}");
    let (s, v) = upload(&app, &t, session, "a.pdf", b"%PDF-1.4").await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    let (s, v) = upload(&app, &t, session, "fake.png", b"<html>not a png</html>").await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    let mut broken = b"\x89PNG\r\n\x1a\n".to_vec();
    broken.extend_from_slice(&[0u8; 64]);
    let (s, v) = upload(&app, &t, session, "broken.png", &broken).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    assert!(err_message(&v).contains("讀不出來"), "{v}");
    let mut big = b"\x89PNG\r\n\x1a\n".to_vec();
    big.resize(5 * 1024 * 1024 + 1, 0);
    let (s, v) = upload(&app, &t, session, "big.png", &big).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    assert!(err_message(&v).contains("5 MB"), "{v}");
    let (s, v) = app
        .post_multipart(
            &format!("/api/prep-sessions/{session}/images"),
            Some(&t),
            &[Part::Text("note", "no file")],
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");

    // Only so many wait to be sent; a waiting one can be removed.
    let mut ids = vec![id];
    for _ in 0..3 {
        let (s, v) = upload(&app, &t, session, "p.jpg", &jpeg_of(&png(40, 30))).await;
        assert_eq!(s, StatusCode::CREATED, "{v}");
        ids.push(v["id"].as_i64().unwrap());
    }
    let (s, v) = upload(&app, &t, session, "p.png", &png(10, 10)).await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    let (s, _) = app
        .delete(
            &format!("/api/prep-sessions/{session}/images/{}", ids[3]),
            Some(&t),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = app
        .get(
            &format!("/api/prep-sessions/{session}/images/{}", ids[3]),
            Some(&t),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

fn jpeg_of(png_bytes: &[u8]) -> Vec<u8> {
    let img = image::load_from_memory(png_bytes).unwrap();
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Jpeg).unwrap();
    out.into_inner()
}

#[tokio::test]
async fn without_storage_an_upload_is_a_503() {
    let app = test_app!();
    let t = app.teacher("T-RP-10", "general").await;
    let (session, _) = {
        let text = P.join("\n\n");
        let (s, v) = app
            .post(
                "/api/prep-sessions",
                Some(&t),
                json!({ "kind": "worksheet", "material": { "title": "U", "text": text } }),
            )
            .await;
        assert_eq!(s, StatusCode::CREATED, "{v}");
        let id = v["id"].as_i64().unwrap();
        wait_session(&app, &t, id).await;
        (id, 0)
    };
    let (s, v) = upload(&app, &t, session, "a.png", &png(10, 10)).await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE, "{v}");
}

#[tokio::test]
async fn images_sent_with_a_revise_reach_the_model_and_are_kept_for_the_session() {
    let dir = TempDir::new();
    let app = test_app!(dir.storage_only());
    let t = app.teacher("T-RP-11", "general").await;
    let other = app.teacher("T-RP-11B", "general").await;
    let (session, ws) = session_with_worksheet(&app, &t).await;
    let (_, a) = upload(&app, &t, session, "a.png", &png(200, 100)).await;
    let (_, b) = upload(&app, &t, session, "b.jpg", &jpeg_of(&png(50, 50))).await;
    let (_, c) = upload(&app, &t, session, "c.png", &png(20, 20)).await;
    let (a, b, c) = (
        a["id"].as_i64().unwrap(),
        b["id"].as_i64().unwrap(),
        c["id"].as_i64().unwrap(),
    );
    let turns_path = format!("/api/prep-sessions/{session}/turns");

    // At most two per message; another session's or a missing id; not for questions.
    let (s, v) = app
        .post(
            &turns_path,
            Some(&t),
            json!({ "kind": "revise", "request": "看圖改", "image_ids": [a, b, c] }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    assert!(err_message(&v).contains("最多附 2 張"), "{v}");
    let (s, v) = app
        .post(
            &turns_path,
            Some(&t),
            json!({ "kind": "revise", "request": "看圖改", "image_ids": [999999] }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    let (s, v) = app
        .post(
            &turns_path,
            Some(&t),
            json!({ "kind": "ask", "request": "這是什麼", "image_ids": [a] }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    // Another teacher cannot use them.
    let (_, other_session) = {
        let text = P.join("\n\n");
        let (_, v) = app
            .post(
                "/api/prep-sessions",
                Some(&other),
                json!({ "kind": "worksheet", "material": { "title": "U", "text": text } }),
            )
            .await;
        let id = v["id"].as_i64().unwrap();
        wait_session(&app, &other, id).await;
        ((), id)
    };
    let (s, v) = app
        .post(
            &format!("/api/prep-sessions/{other_session}/turns"),
            Some(&other),
            json!({ "kind": "revise", "request": "看圖改", "image_ids": [a] }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");

    // Two images go with the request.
    let turn = revise_turn(
        &app,
        &t,
        session,
        json!({ "kind": "revise", "request": "照這張截圖改第 2 題", "image_ids": [a, b] }),
    )
    .await;
    assert_eq!(turn["status"], "done", "{turn}");
    assert_eq!(turn["images"].as_array().unwrap().len(), 2);
    assert_eq!(turn["images"][0]["id"], a);
    assert!(turn["result"]["reply"]
        .as_str()
        .unwrap()
        .contains("收到 2 張圖片"));
    // The record keeps ids, sizes and hashes, never the bytes.
    let snapshot: Value = sqlx::query_scalar(
        "SELECT input_snapshot FROM ai_interactions WHERE module = 'prep_revise'
          ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    let record = snapshot["prep"]["attached_images"].as_array().unwrap();
    assert_eq!(record.len(), 2);
    assert_eq!(record[0]["id"], a);
    assert!(record[0]["bytes"].as_i64().unwrap() > 0);
    assert_eq!(record[0]["sha256"].as_str().unwrap().len(), 64);
    assert!(!snapshot.to_string().contains("base64"));
    assert!(snapshot["prompt"]["user"]
        .as_str()
        .unwrap()
        .contains("教師附上了 2 張圖片"));
    // A sent image cannot be sent again or removed; the leftover one can still be used.
    let (s, v) = app
        .post(
            &turns_path,
            Some(&t),
            json!({ "kind": "revise", "request": "再看一次", "image_ids": [a] }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    let (s, _) = app
        .delete(
            &format!("/api/prep-sessions/{session}/images/{a}"),
            Some(&t),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT);

    // The block revision takes them too, and shows them with the proposal.
    let state = worksheet(&app, &t, ws).await;
    let block = state["content"]["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|bl| bl["kind"] == "activity")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let p = revise_block(
        &app,
        &t,
        ws,
        &block,
        json!({ "instruction": "看圖改簡單", "image_ids": [c] }),
    )
    .await;
    assert_eq!(p["images"].as_array().unwrap().len(), 1, "{p}");
    assert!(
        p["reply"].as_str().unwrap().contains("收到 1 張圖片"),
        "{p}"
    );
    // The session shows only its turns' pictures; the picture is served to its owner.
    let r = app
        .send_raw(
            Method::GET,
            &format!("/api/prep-sessions/{session}/images/{c}"),
            &[("authorization", &format!("Bearer {t}"))],
            Vec::new(),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);

    // Deleting the session removes the stored pictures and their rows.
    assert_eq!(dir.files("prep-images"), 3);
    let (s, _) = app
        .delete(&format!("/api/prep-sessions/{session}"), Some(&t))
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(dir.files("prep-images"), 0);
    let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM prep_images WHERE session_id = $1")
        .bind(session)
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(left, 0);
}
