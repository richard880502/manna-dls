//! Pictures on worksheets (docs/API.md「學習單的圖片」) with the fake text model and the fake
//! picture provider: slots written with the blocks, pictures made in the background and served
//! to the owner only, failures and skipped slots as empty boxes, the teacher's 重畫 / upload /
//! 拿掉 with their caps, pictures in the DOCX and PDF exports, template worksheets, the logs,
//! and the restart recovery. With pictures off nothing changes.

#[macro_use]
mod common;

use std::io::Cursor;
use std::path::PathBuf;
use std::time::Duration;

use axum::http::StatusCode;
use common::Part;
use manna_api::config::Config;
use manna_api::storage::StorageConfig;
use manna_api::worksheet::images::{self, ImageConfig};
use manna_api::AppState;
use serde_json::{json, Value};

// ---------------------------------------------------------------- harness

/// A storage directory that is removed when the test ends.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("manna-img-{}", manna_api::auth::new_token()));
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

    /// The same, as a second process would see it (restart recovery).
    fn state(&self, db: &sqlx::PgPool, images: ImageConfig) -> AppState {
        let mut config = Config::for_tests("postgres://unused");
        config.storage = StorageConfig::fs(self.0.clone());
        config.images = images;
        AppState::new(db.clone(), config)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const P1: &str = "Mrs. Lin put an umbrella stand outside her shop on a rainy Monday. A sign said, “Take one if you need it.”";
const P2: &str = "Her neighbors laughed at the idea. They were sure that every umbrella would be gone within a week.";
const P3: &str = "In the first month, twelve of the thirty umbrellas did disappear. However, something unexpected followed.";
const P4: &str = "Today the stand holds more umbrellas than it did at the start. Mrs. Lin says trust is like an umbrella.";

fn passage(marker: &str) -> String {
    format!("{P1}\n\n{P2}\n\n{P3}\n\n{P4} {marker}")
}

fn err_message(v: &Value) -> &str {
    v["error"]["message"].as_str().unwrap_or_default()
}

async fn direct(app: &common::TestApp, t: &str, text: &str) -> i64 {
    let (s, v) = app
        .post(
            "/api/worksheets",
            Some(t),
            json!({ "material": { "title": "The Umbrella Stand", "text": text } }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    v["id"].as_i64().unwrap()
}

async fn wait_jobs(app: &common::TestApp, t: &str, id: i64) -> Value {
    for _ in 0..800 {
        let (s, v) = app
            .get(&format!("/api/worksheets/{id}/jobs"), Some(t))
            .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        if v["running"] == false {
            let (s, state) = app.get(&format!("/api/worksheets/{id}"), Some(t)).await;
            assert_eq!(s, StatusCode::OK, "{state}");
            return state;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("worksheet {id} did not finish generating");
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
    wait_jobs(app, t, id).await
}

/// The pictures once none is queued or running any more.
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

fn slot<'a>(images: &'a Value, slot_id: &str) -> &'a Value {
    images["slots"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["slot_id"] == slot_id)
        .unwrap_or_else(|| panic!("no slot {slot_id} in {images}"))
}

/// Every slot of the content, in order: (block id, slot).
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

async fn sheet(app: &common::TestApp, t: &str, id: i64, audience: &str) -> Value {
    let (s, v) = app
        .get(
            &format!("/api/worksheets/{id}/sheet?audience={audience}"),
            Some(t),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v
}

/// The pictures a sheet prints, in order.
fn printed_images(sheet: &Value) -> Vec<Value> {
    sheet["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|b| b["images"].as_array().cloned().unwrap_or_default())
        .collect()
}

async fn export(app: &common::TestApp, t: &str, id: i64, query: &str) -> common::RawResponse {
    let r = app
        .send_raw(
            axum::http::Method::GET,
            &format!("/api/worksheets/{id}/export?{query}"),
            &[("authorization", &format!("Bearer {t}"))],
            Vec::new(),
        )
        .await;
    assert_eq!(
        r.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&r.body)
    );
    r
}

fn zip_names(bytes: &[u8]) -> Vec<String> {
    let zip = zip::ZipArchive::new(Cursor::new(bytes.to_vec())).unwrap();
    (0..zip.len())
        .map(|i| zip.name_for_index(i).unwrap().unwrap().to_string())
        .collect()
}

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

#[derive(sqlx::FromRow, Debug)]
struct Interaction {
    attempt: i32,
    retry_of: Option<i64>,
    status: String,
    input_snapshot: Value,
    output: Option<Value>,
    error_detail: Option<String>,
}

async fn picture_calls(app: &common::TestApp) -> Vec<Interaction> {
    sqlx::query_as(
        "SELECT attempt, retry_of, status, input_snapshot, output, error_detail
           FROM ai_interactions WHERE module = 'worksheet_image' ORDER BY id",
    )
    .fetch_all(&app.db)
    .await
    .unwrap()
}

// ---------------------------------------------------------------- generation

#[tokio::test]
async fn a_generated_worksheet_gets_its_pictures_in_the_background() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure(|_| {}));
    let t = app.teacher("T-IMG-1", "general").await;
    let other = app.teacher("T-IMG-1B", "general").await;
    let id = direct(&app, &t, &passage("FAKE_IMG_PHOTO")).await;
    let state = generate(&app, &t, id).await;

    // The tasks step wrote one slot for the first block, and asked for a map in the second: a
    // real photo, left for the teacher.
    let slots = slots_of(&state);
    assert_eq!(slots.len(), 2, "{slots:?}");
    let (first_block, first) = &slots[0];
    let first_id = first["id"].as_str().unwrap().to_string();
    assert_eq!(first_id, format!("{first_block}-p1"));
    assert_eq!(first["kind"], "scene");
    assert!(first["prompt_en"].as_str().unwrap().contains("paragraph 1"));
    assert!(!first["alt_en"].as_str().unwrap().is_empty());
    assert_eq!(first["caption"], "Look at the picture of paragraph 1.");
    let photo_id = slots[1].1["id"].as_str().unwrap().to_string();
    assert_eq!(slots[1].1["kind"], "photo_needed");
    assert_eq!(state["images"]["enabled"], true);
    assert_eq!(state["images"]["regenerations_left"], 10);

    let images = wait_images(&app, &t, id).await;
    let done = slot(&images, &first_id);
    assert_eq!(done["status"], "done", "{images}");
    assert_eq!(done["source"], "generated");
    assert_eq!(done["block_id"], first_block.as_str());
    assert_eq!(
        (done["width"].as_i64(), done["height"].as_i64()),
        (Some(48), Some(32))
    );
    let photo = slot(&images, &photo_id);
    assert_eq!(photo["status"], "skipped");
    assert_eq!(photo["image_id"], Value::Null);
    assert!(photo["message"].as_str().unwrap().contains("真實"));

    // Served to the owner only, as JPEG with a validator.
    let image_id = done["image_id"].as_i64().unwrap();
    let path = format!("/api/worksheets/{id}/images/{image_id}");
    let r = app
        .send_raw(
            axum::http::Method::GET,
            &path,
            &[("authorization", &format!("Bearer {t}"))],
            Vec::new(),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.header("content-type"), "image/jpeg");
    assert!(r.body.starts_with(b"\xFF\xD8\xFF"));
    assert!(r.header("cache-control").starts_with("private"));
    assert_eq!(r.header("vary"), "Authorization");
    assert_eq!(r.header("x-content-type-options"), "nosniff");
    let etag = r.header("etag").to_string();
    assert_eq!(etag.len(), 66, "the SHA-256, quoted");
    let again = app
        .send_raw(
            axum::http::Method::GET,
            &path,
            &[
                ("authorization", &format!("Bearer {t}")),
                ("if-none-match", &etag),
            ],
            Vec::new(),
        )
        .await;
    assert_eq!(again.status, StatusCode::NOT_MODIFIED);
    assert!(again.body.is_empty());
    let (s, _) = app.get(&path, Some(&other)).await;
    assert_eq!(s, StatusCode::NOT_FOUND, "another teacher");
    let (s, _) = app.get(&path, None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _) = app
        .get(&format!("/api/worksheets/{id}/images/999999"), Some(&t))
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = app
        .get(&format!("/api/worksheets/{id}/images"), Some(&other))
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // The sheet: the picture after the instructions; the photo slot as an empty box. Only the
    // teacher version says what the box is for.
    let student = sheet(&app, &t, id, "student").await;
    let printed: Vec<&Value> = student["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|b| b["images"].as_array().into_iter().flatten())
        .collect();
    assert_eq!(printed.len(), 2, "{student}");
    assert_eq!(printed[0]["image_id"], image_id);
    assert_eq!(printed[0]["status"], "done");
    assert_eq!(printed[0]["ai_generated"], true);
    assert_eq!(printed[0]["caption"], "Look at the picture of paragraph 1.");
    assert!(printed[0].get("purpose").is_none() && printed[1].get("message").is_none());
    assert_eq!(printed[1]["status"], "skipped");
    let teacher = sheet(&app, &t, id, "teacher").await;
    let boxes: Vec<&Value> = teacher["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|b| b["images"].as_array().into_iter().flatten())
        .collect();
    assert!(boxes[0]["purpose"]
        .as_str()
        .unwrap()
        .contains("paragraph 1"));
    assert!(boxes[1]["message"].as_str().unwrap().contains("真實"));

    // The exports carry the picture (and the box).
    let docx = export(&app, &t, id, "format=docx&audience=student").await;
    let names = zip_names(&docx.body);
    assert!(
        names.contains(&"word/media/image1.jpeg".to_string()),
        "{names:?}"
    );
    assert!(!names.contains(&"word/media/image2.jpeg".to_string()));
    let pdf = export(&app, &t, id, "format=pdf&audience=student").await;
    assert!(pdf.body.starts_with(b"%PDF-"));
    assert!(pdf
        .body
        .windows(13)
        .any(|w| w == b"/Subtype /Ima" || w == b"/Subtype/Imag"));
    let exported: Value = sqlx::query_scalar(
        "SELECT payload FROM usage_logs WHERE event = 'worksheet_exported' ORDER BY id LIMIT 1",
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(
        (
            exported["pictures"].as_i64(),
            exported["pictures_embedded"].as_i64()
        ),
        (Some(2), Some(1))
    );

    // One logged call: counts and hashes, never the prompt.
    let calls = picture_calls(&app).await;
    assert_eq!(calls.len(), 1);
    let call = &calls[0];
    assert_eq!((call.attempt, call.status.as_str()), (1, "shown"));
    let snapshot = call.input_snapshot.to_string();
    assert!(
        !snapshot.contains("umbrella") && !snapshot.contains("Simple flat vector"),
        "{snapshot}"
    );
    assert_eq!(
        call.input_snapshot["prompt"]["sha256"]
            .as_str()
            .unwrap()
            .len(),
        64
    );
    assert!(call.input_snapshot["prompt"]["chars"].as_i64().unwrap() > 100);
    assert_eq!(call.input_snapshot["prompt"]["style_version_no"], 1);
    assert_eq!(call.input_snapshot["request"]["quality"], "medium");
    assert_eq!(call.input_snapshot["request"]["output_format"], "jpeg");
    let output = call.output.as_ref().unwrap();
    assert_eq!(output["usage"]["total_tokens"], 216);
    assert_eq!(output["usage"]["output_image_tokens"], 196);
    assert_eq!(output["sha256"].as_str().unwrap().len(), 64);
    // The text model got the slot rules, and its logged prompt keeps them.
    let user: String = sqlx::query_scalar(
        "SELECT input_snapshot->'prompt'->>'user' FROM ai_interactions
          WHERE module = 'worksheet_tasks' ORDER BY id LIMIT 1",
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert!(user.contains("圖片位置：image_slots"), "{user}");
    let row: (i32, String, String, i32) = sqlx::query_as(
        "SELECT prompt_chars, prompt_sha256, model, bytes FROM worksheet_images
          WHERE slot_id = $1 AND current",
    )
    .bind(&first_id)
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert!(row.0 > 100 && row.1.len() == 64 && row.2 == "fake-image" && row.3 > 0);
}

#[tokio::test]
async fn with_pictures_off_nothing_changes() {
    let app = test_app!();
    let t = app.teacher("T-IMG-OFF", "general").await;
    let id = direct(&app, &t, &passage("FAKE_IMG_PHOTO")).await;
    let state = generate(&app, &t, id).await;
    assert!(slots_of(&state).is_empty());
    assert_eq!(
        state["images"],
        json!({ "enabled": false, "regenerations_left": 10, "slots": [] })
    );
    let user: String = sqlx::query_scalar(
        "SELECT input_snapshot->'prompt'->>'user' FROM ai_interactions
          WHERE module = 'worksheet_tasks' ORDER BY id LIMIT 1",
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert!(!user.contains("image_slots"), "the request is what it was");
    // No `images` key on a sheet without pictures.
    let s = sheet(&app, &t, id, "student").await;
    assert!(s["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .all(|b| b.get("images").is_none()));
    // The JSON of a block without slots has no `image_slots` either.
    assert!(state["content"]["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .all(|b| b.get("image_slots").is_none()));
}

#[tokio::test]
async fn a_failed_picture_leaves_a_box_and_is_retried_once() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure(|_| {}));
    let t = app.teacher("T-IMG-2", "general").await;
    let id = direct(&app, &t, &format!("FAKE_IMG_FAIL {}", passage(""))).await;
    let state = generate(&app, &t, id).await;
    let slot_id = slots_of(&state)[0].1["id"].as_str().unwrap().to_string();
    let images = wait_images(&app, &t, id).await;
    let failed = slot(&images, &slot_id);
    assert_eq!(failed["status"], "failed");
    assert_eq!(failed["image_id"], Value::Null);
    assert!(failed["message"].as_str().unwrap().contains("HTTP 503"));
    // Tried twice (a 5xx), the second pointing at the first; the error keeps only the status.
    let calls = picture_calls(&app).await;
    assert_eq!(calls.len(), 2);
    assert_eq!((calls[0].attempt, calls[1].attempt), (1, 2));
    assert!(calls[1].retry_of.is_some());
    assert_eq!(calls[1].status, "error");
    let detail = calls[1].error_detail.clone().unwrap();
    assert!(
        detail.contains("503") && !detail.contains("secret"),
        "{detail}"
    );
    // The sheet and the export show the empty box.
    let teacher = sheet(&app, &t, id, "teacher").await;
    let img = &printed_images(&teacher)[0];
    assert_eq!(img["status"], "failed");
    assert!(img["message"].as_str().unwrap().contains("HTTP 503"));
    let docx = export(&app, &t, id, "format=docx&audience=student").await;
    assert!(!zip_names(&docx.body)
        .iter()
        .any(|n| n.starts_with("word/media/")));

    // A refusal by the provider's safety system: not retried, a generic message.
    let id2 = direct(&app, &t, &format!("FAKE_IMG_BLOCKED {}", passage(""))).await;
    let state = generate(&app, &t, id2).await;
    let slot_id = slots_of(&state)[0].1["id"].as_str().unwrap().to_string();
    let images = wait_images(&app, &t, id2).await;
    let blocked = slot(&images, &slot_id);
    assert_eq!(blocked["status"], "failed");
    assert!(blocked["message"].as_str().unwrap().contains("內容檢查"));
    let calls = picture_calls(&app).await;
    assert_eq!(calls.len(), 3, "no retry after a refusal");
    assert_eq!(calls[2].status, "blocked");
}

// ---------------------------------------------------------------- the teacher's actions

#[tokio::test]
async fn regenerate_upload_and_remove_are_the_owners() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure(|c| c.max_regenerations = 2));
    let t = app.teacher("T-IMG-3", "general").await;
    let other = app.teacher("T-IMG-3B", "general").await;
    let id = direct(&app, &t, &passage("FAKE_IMG_PHOTO")).await;
    let state = generate(&app, &t, id).await;
    let slots = slots_of(&state);
    let slot_id = slots[0].1["id"].as_str().unwrap().to_string();
    let photo_id = slots[1].1["id"].as_str().unwrap().to_string();
    let first = wait_images(&app, &t, id).await;
    let first_image = slot(&first, &slot_id)["image_id"].as_i64().unwrap();
    let base = format!("/api/worksheets/{id}/image-slots/{slot_id}");

    // Someone else: 404 for every action.
    let (s, _) = app
        .post(&format!("{base}/regenerate"), Some(&other), json!({}))
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = app.delete(&base, Some(&other)).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = app
        .post_multipart(
            &format!("{base}/upload"),
            Some(&other),
            &[Part::File("file", "mine.png", &png(20, 20))],
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // 重畫 with a note: 202, a new picture, one 重畫 used.
    let (s, v) = app
        .post(
            &format!("{base}/regenerate"),
            Some(&t),
            json!({ "hint": "  make it sunny  " }),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    let status = slot(&v, &slot_id)["status"].as_str().unwrap();
    assert!(["queued", "running", "done"].contains(&status), "{status}");
    assert_eq!(v["regenerations_left"], 1);
    let after = wait_images(&app, &t, id).await;
    let redrawn = slot(&after, &slot_id);
    assert_eq!(redrawn["status"], "done");
    assert_ne!(redrawn["image_id"].as_i64().unwrap(), first_image);
    // The note went into the prompt (a different picture), and only its length into the log.
    let calls = picture_calls(&app).await;
    let last = &calls.last().unwrap().input_snapshot;
    assert_eq!(last["regeneration"], true);
    assert_eq!(last["teacher_note"]["chars"], 13);
    assert!(!last.to_string().contains("sunny"));
    let hint: String =
        sqlx::query_scalar("SELECT hint FROM worksheet_images WHERE slot_id = $1 AND current")
            .bind(&slot_id)
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(hint, "make it sunny");
    // The old picture is kept (not current) and still served.
    let (s, _) = app
        .get(
            &format!("/api/worksheets/{id}/images/{first_image}"),
            Some(&t),
        )
        .await;
    assert_eq!(s, StatusCode::OK);

    // Bad requests: a slot that needs a real photo, an unknown slot, a long note, a link.
    let (s, v) = app
        .post(
            &format!("/api/worksheets/{id}/image-slots/{photo_id}/regenerate"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    assert!(err_message(&v).contains("真實照片"));
    let (s, _) = app
        .post(
            &format!("/api/worksheets/{id}/image-slots/nope/regenerate"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = app
        .post(
            &format!("{base}/regenerate"),
            Some(&t),
            json!({ "hint": "x".repeat(201) }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = app
        .post(
            &format!("{base}/regenerate"),
            Some(&t),
            json!({ "hint": "like https://example.com" }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // While its picture is being made: 409. Then the cap.
    let (s, v) = app
        .post(
            &format!("{base}/regenerate"),
            Some(&t),
            json!({ "hint": "FAKE_IMG_SLOW" }),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    assert_eq!(v["regenerations_left"], 0);
    let (s, v) = app
        .post(&format!("{base}/regenerate"), Some(&t), json!({}))
        .await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    assert!(err_message(&v).contains("正在產生"));
    wait_images(&app, &t, id).await;
    let (s, v) = app
        .post(&format!("{base}/regenerate"), Some(&t), json!({}))
        .await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    assert!(err_message(&v).contains("上限"));

    // 換成我的圖: any picture format, stored as JPEG; other files are refused.
    let (s, v) = app
        .post_multipart(
            &format!("{base}/upload"),
            Some(&t),
            &[Part::File("file", "mine.pdf", b"%PDF-1.4 x")],
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    let (s, v) = app
        .post_multipart(
            &format!("{base}/upload"),
            Some(&t),
            &[Part::File("file", "fake.png", b"GIF89a not a png")],
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    let (s, v) = app
        .post_multipart(
            &format!("/api/worksheets/{id}/image-slots/{photo_id}/upload"),
            Some(&t),
            &[Part::File("file", "Map.PNG", &png(2400, 600))],
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let uploaded = slot(&v, &photo_id);
    assert_eq!(
        (uploaded["status"].as_str(), uploaded["source"].as_str()),
        (Some("done"), Some("upload"))
    );
    assert_eq!(
        (uploaded["width"].as_i64(), uploaded["height"].as_i64()),
        (Some(1536), Some(384))
    );
    let r = app
        .send_raw(
            axum::http::Method::GET,
            &format!("/api/worksheets/{id}/images/{}", uploaded["image_id"]),
            &[("authorization", &format!("Bearer {t}"))],
            Vec::new(),
        )
        .await;
    assert!(r.body.starts_with(b"\xFF\xD8\xFF"), "stored as JPEG");
    // The teacher's picture is not labelled as AI-made.
    let teacher = sheet(&app, &t, id, "teacher").await;
    let printed: Vec<&Value> = teacher["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|b| b["images"].as_array().into_iter().flatten())
        .collect();
    let mine = printed
        .iter()
        .find(|i| i["slot_id"] == photo_id.as_str())
        .unwrap();
    assert_eq!(mine["ai_generated"], false);
    assert_eq!(mine["status"], "done");

    // 拿掉: nothing printed; an upload brings a picture back.
    let (s, v) = app.delete(&base, Some(&t)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(slot(&v, &slot_id)["status"], "removed");
    let student = sheet(&app, &t, id, "student").await;
    assert!(!student.to_string().contains(&slot_id));
    let (s, v) = app
        .post_multipart(
            &format!("{base}/upload"),
            Some(&t),
            &[Part::File("file", "back.webp", &{
                let mut out = Cursor::new(Vec::new());
                image::DynamicImage::ImageRgb8(image::RgbImage::new(10, 10))
                    .write_to(&mut out, image::ImageFormat::WebP)
                    .unwrap();
                out.into_inner()
            })],
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(slot(&v, &slot_id)["status"], "done");
    let events: Vec<String> = sqlx::query_scalar(
        "SELECT event FROM usage_logs WHERE event LIKE 'worksheet_image%' ORDER BY id",
    )
    .fetch_all(&app.db)
    .await
    .unwrap();
    for e in [
        "worksheet_images_queued",
        "worksheet_image_finished",
        "worksheet_image_regenerated",
        "worksheet_image_uploaded",
        "worksheet_image_removed",
    ] {
        assert!(events.iter().any(|x| x == e), "{e} in {events:?}");
    }
}

#[tokio::test]
async fn without_pictures_or_storage_the_actions_say_so() {
    let app = test_app!(|c: &mut Config| {
        c.images = ImageConfig::disabled();
    });
    let t = app.teacher("T-IMG-4", "general").await;
    let id = direct(&app, &t, &passage("")).await;
    // A slot the teacher's editor kept (pictures were on when the worksheet was made).
    let (s, state) = app.get(&format!("/api/worksheets/{id}"), Some(&t)).await;
    assert_eq!(s, StatusCode::OK);
    let mut content = state["content"].clone();
    content["blocks"] = json!([{
        "id": "b1", "kind": "activity", "title": "Look", "instructions": "Look at it.",
        "image_slots": [{ "id": "b1-p1", "kind": "scene", "purpose_zh": "情境", "prompt_en": "A cat.",
                          "alt_en": "A cat.", "caption": "" }],
    }]);
    let (s, v) = app
        .put(
            &format!("/api/worksheets/{id}"),
            Some(&t),
            json!({ "content": content }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(
        v["content"]["blocks"][0]["image_slots"][0]["id"], "b1-p1",
        "kept by a save"
    );
    let base = format!("/api/worksheets/{id}/image-slots/b1-p1");
    let (s, v) = app
        .post(&format!("{base}/regenerate"), Some(&t), json!({}))
        .await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE, "{v}");
    let (s, _) = app
        .post_multipart(
            &format!("{base}/upload"),
            Some(&t),
            &[Part::File("file", "a.png", &png(10, 10))],
        )
        .await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE, "no storage");
    // A slot without a row prints as an empty box.
    let s = sheet(&app, &t, id, "student").await;
    assert_eq!(printed_images(&s)[0]["status"], "skipped");

    // Saved slots are checked like everything else.
    content["blocks"][0]["image_slots"][0]["caption"] = json!("c".repeat(81));
    let (s, v) = app
        .put(
            &format!("/api/worksheets/{id}"),
            Some(&t),
            json!({ "content": content }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    assert!(err_message(&v).contains("圖說"));
    content["blocks"][0]["image_slots"][0]["caption"] = json!("");
    content["blocks"][0]["image_slots"][0]["kind"] = json!("hologram");
    let (s, _) = app
        .put(
            &format!("/api/worksheets/{id}"),
            Some(&t),
            json!({ "content": content }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------- regenerating a block

#[tokio::test]
async fn a_regenerated_block_gets_new_slots_and_undo_brings_the_old_picture_back() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure(|_| {}));
    let t = app.teacher("T-IMG-5", "general").await;
    let id = direct(&app, &t, &passage("")).await;
    let state = generate(&app, &t, id).await;
    let (block_id, old_slot) = slots_of(&state)[0].clone();
    let old_id = old_slot["id"].as_str().unwrap().to_string();
    wait_images(&app, &t, id).await;

    let (s, v) = app
        .post(
            &format!("/api/worksheets/{id}/blocks/{block_id}/regenerate"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    let state = wait_jobs(&app, &t, id).await;
    let slots = slots_of(&state);
    let new_id = slots[0].1["id"].as_str().unwrap().to_string();
    assert_ne!(new_id, old_id);
    assert!(new_id.starts_with(&format!("{block_id}-r")), "{new_id}");
    let images = wait_images(&app, &t, id).await;
    assert_eq!(slot(&images, &new_id)["status"], "done");
    // The old picture is still the old slot's.
    assert_eq!(slot(&images, &old_id)["status"], "done");

    let (s, v) = app
        .post(
            &format!("/api/worksheets/{id}/blocks/{block_id}/undo"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(slots_of(&v)[0].1["id"], old_id.as_str());
    let s = sheet(&app, &t, id, "student").await;
    let printed = printed_images(&s);
    assert_eq!(printed.len(), 1);
    assert_eq!(printed[0]["slot_id"], old_id.as_str());
    assert_eq!(printed[0]["status"], "done");
}

// ---------------------------------------------------------------- 照我的版型

async fn paste(app: &common::TestApp, t: &str, title: &str, text: &str) -> i64 {
    let (s, v) = app
        .post(
            "/api/materials",
            Some(t),
            json!({ "kind": "text", "title": title, "text": text }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v["id"].as_i64().unwrap()
}

#[tokio::test]
async fn template_blocks_ask_for_pictures_within_the_cap() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure(|c| c.max_per_worksheet = 3));
    let t = app.teacher("T-IMG-6", "general").await;
    let sample = "Name: ______ No: ______\n\n\
        ## Warm-up: [主題]\nWhat do you know about umbrellas?\n\n\
        ## Paragraph [n]: Details\n1. What does she put outside?\n\n\
        ## Watch the video and compare\nWatch the clip and write two differences.\n\n\
        ## Writing: [主題]\nWrite 80 words about trust.";
    let m = paste(&app, &t, "L2 Reading Worksheet", sample).await;
    let (s, v) = app
        .post(
            "/api/worksheet-templates",
            Some(&t),
            json!({ "material_id": m }),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    let tpl = v["id"].as_i64().unwrap();
    for _ in 0..400 {
        let (_, v) = app
            .get(&format!("/api/worksheet-templates/{tpl}"), Some(&t))
            .await;
        if v["status"] != "extracting" {
            assert_eq!(v["status"], "ready", "{v}");
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    // As if the sample had been scanned: OCR counted two pictures on it.
    sqlx::query(
        r#"UPDATE materials SET ocr_pages = '[{"page": 1, "blocks": [], "pictures": 2}]' WHERE id = $1"#,
    )
    .bind(m)
    .execute(&app.db)
    .await
    .unwrap();
    let (s, v) = app
        .post(
            "/api/worksheets",
            Some(&t),
            json!({ "template_id": tpl, "material": { "text": passage("") } }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    let id = v["id"].as_i64().unwrap();
    let state = generate(&app, &t, id).await;
    let blocks = state["content"]["blocks"].as_array().unwrap();
    // 1 warm-up + 4 paragraphs + the video block + writing; the video block needs the
    // teacher's material and gets no picture.
    assert_eq!(blocks.len(), 7);
    let slots = slots_of(&state);
    assert_eq!(slots.len(), 6, "{slots:?}");
    let video = blocks
        .iter()
        .find(|b| b["title"].as_str().unwrap().contains("video"))
        .unwrap();
    assert!(video.get("image_slots").is_none());
    let images = wait_images(&app, &t, id).await;
    let statuses: Vec<&str> = slots
        .iter()
        .map(|(_, s)| {
            slot(&images, s["id"].as_str().unwrap())["status"]
                .as_str()
                .unwrap()
        })
        .collect();
    // In worksheet order: the first three are made, the rest wait for the teacher.
    assert_eq!(
        statuses,
        ["done", "done", "done", "skipped", "skipped", "skipped"]
    );
    let capped = slot(&images, slots[3].1["id"].as_str().unwrap());
    assert!(capped["message"].as_str().unwrap().contains("上限"));
    // The section schema asked for at most two slots per block, after the items.
    let user: String = sqlx::query_scalar(
        "SELECT input_snapshot->'prompt'->>'user' FROM ai_interactions
          WHERE module = 'worksheet_template_section' ORDER BY id LIMIT 1",
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert!(user.contains("圖片位置："));
    assert!(
        user.contains("範例學習單裡有 2 張圖"),
        "the sample's pictures are a hint"
    );
    // A capped slot can still be drawn by hand (重畫 has its own cap).
    let (s, v) = app
        .post(
            &format!(
                "/api/worksheets/{id}/image-slots/{}/regenerate",
                slots[3].1["id"].as_str().unwrap()
            ),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    // The DOCX holds the three pictures made so far.
    wait_images(&app, &t, id).await;
    let docx = export(&app, &t, id, "format=docx&audience=teacher").await;
    let media = zip_names(&docx.body)
        .into_iter()
        .filter(|n| n.starts_with("word/media/"))
        .count();
    assert_eq!(media, 4);
}

// ---------------------------------------------------------------- restarts

#[tokio::test]
async fn open_pictures_are_resumed_or_failed_after_a_restart() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure(|_| {}));
    let t = app.teacher("T-IMG-7", "general").await;
    let id = direct(&app, &t, &passage("")).await;
    let state = generate(&app, &t, id).await;
    let slot_id = slots_of(&state)[0].1["id"].as_str().unwrap().to_string();
    wait_images(&app, &t, id).await;
    let interrupt = |minutes: i32| {
        sqlx::query(
            "UPDATE worksheet_images
                SET status = 'running', storage_key = NULL, sha256 = NULL, message = NULL,
                    started_at = now() - make_interval(mins => $2), finished_at = NULL
              WHERE slot_id = $1 AND current",
        )
        .bind(slot_id.clone())
        .bind(minutes)
    };

    // A row nobody holds, long past its time: reported as failed on the next read.
    interrupt(60).execute(&app.db).await.unwrap();
    let (_, v) = app
        .get(&format!("/api/worksheets/{id}/images"), Some(&t))
        .await;
    assert_eq!(slot(&v, &slot_id)["status"], "failed");
    assert!(slot(&v, &slot_id)["message"]
        .as_str()
        .unwrap()
        .contains("中斷"));
    // A recent one is left alone.
    interrupt(1).execute(&app.db).await.unwrap();
    let (_, v) = app
        .get(&format!("/api/worksheets/{id}/images"), Some(&t))
        .await;
    assert_eq!(slot(&v, &slot_id)["status"], "running");

    // At startup a new process takes it over and finishes it.
    let restarted = dir.state(&app.db, ImageConfig::fake());
    assert_eq!(images::recover(&restarted).await.unwrap(), 1);
    let v = wait_images(&app, &t, id).await;
    assert_eq!(slot(&v, &slot_id)["status"], "done");

    // Without pictures after the restart, it cannot go on: failed.
    interrupt(1).execute(&app.db).await.unwrap();
    let without = dir.state(&app.db, ImageConfig::disabled());
    assert_eq!(images::recover(&without).await.unwrap(), 0);
    let (_, v) = app
        .get(&format!("/api/worksheets/{id}/images"), Some(&t))
        .await;
    assert_eq!(slot(&v, &slot_id)["status"], "failed");
    assert!(slot(&v, &slot_id)["message"]
        .as_str()
        .unwrap()
        .contains("系統更新後"));
}
