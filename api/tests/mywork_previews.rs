//! 我的作品 cards (docs/API.md「我的作品：卡片預覽」): the preview fields of the session list
//! and `GET /api/worksheets/:id/thumbnail` (owner only, a JPEG of page 1 of the student PDF,
//! `ETag` / 304, 409 while the worksheet is made, kept in the storage and removed with the
//! worksheet, never logged as an export).
//!
//! Rendering needs `pdftoppm` (poppler-utils): without it locally the tests that need the picture
//! check the 503 instead (CI installs it and checks the picture).

#[macro_use]
mod common;

use std::path::PathBuf;
use std::time::Duration;

use axum::http::{Method, StatusCode};
use common::RawResponse;
use manna_api::config::Config;
use manna_api::storage::StorageConfig;
use manna_api::worksheet::{store, thumbnail};
use serde_json::{json, Value};

const P: [&str; 4] = [
    "Mrs. Lin put an umbrella stand outside her shop on a rainy Monday. A sign said, “Take one if you need it.”",
    "Her neighbors laughed at the idea. They were sure that every umbrella would be gone within a week.",
    "In the first month, twelve of the thirty umbrellas did disappear. However, something unexpected followed.",
    "Today the stand holds more umbrellas than it did at the start. Mrs. Lin says trust is like an umbrella.",
];

/// A storage directory that is removed when the test ends.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("manna-card-{}", manna_api::auth::new_token()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn configure(&self) -> impl FnOnce(&mut Config) + '_ {
        |c: &mut Config| c.storage = StorageConfig::fs(self.0.clone())
    }

    /// The preview files kept for a worksheet.
    fn kept(&self, worksheet_id: i64) -> Vec<String> {
        std::fs::read_dir(self.0.join(format!("thumbs/worksheets/{worksheet_id}")))
            .map(|d| {
                d.filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn has_poppler() -> bool {
    if std::process::Command::new("pdftoppm")
        .arg("-v")
        .output()
        .is_ok()
    {
        return true;
    }
    if std::env::var_os("CI").is_some() {
        panic!("pdftoppm (poppler-utils) must be installed for the preview tests in CI");
    }
    eprintln!("pdftoppm (poppler-utils) is not installed here: checking the 503 instead");
    false
}

fn passage() -> String {
    P.join("\n\n")
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

async fn wait_worksheet(app: &common::TestApp, t: &str, id: i64) {
    for _ in 0..800 {
        let (s, v) = app
            .get(&format!("/api/worksheets/{id}/jobs"), Some(t))
            .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        if v["running"] == false {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("worksheet {id} did not finish generating");
}

/// A session from pasted material whose first turn is `kind`, finished.
async fn session(app: &common::TestApp, t: &str, body: Value) -> Value {
    let (s, v) = app.post("/api/prep-sessions", Some(t), body).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    wait_session(app, t, v["id"].as_i64().unwrap()).await
}

fn material() -> Value {
    json!({ "title": "The Umbrella Stand", "text": passage() })
}

/// A standalone worksheet, generated.
async fn worksheet(app: &common::TestApp, t: &str) -> i64 {
    let (s, v) = app
        .post(
            "/api/worksheets",
            Some(t),
            json!({ "material": material() }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    let id = v["id"].as_i64().unwrap();
    let (s, v) = app
        .post(
            &format!("/api/worksheets/{id}/generate"),
            Some(t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    wait_worksheet(app, t, id).await;
    id
}

async fn thumbnail(
    app: &common::TestApp,
    id: i64,
    token: Option<&str>,
    if_none_match: Option<&str>,
) -> RawResponse {
    let bearer = token.map(|t| format!("Bearer {t}"));
    let mut headers: Vec<(&str, &str)> = Vec::new();
    if let Some(b) = &bearer {
        headers.push(("authorization", b));
    }
    if let Some(tag) = if_none_match {
        headers.push(("if-none-match", tag));
    }
    app.send_raw(
        Method::GET,
        &format!("/api/worksheets/{id}/thumbnail"),
        &headers,
        Vec::new(),
    )
    .await
}

fn json_of(r: &RawResponse) -> Value {
    serde_json::from_slice(&r.body).unwrap_or(Value::Null)
}

async fn exports_logged(app: &common::TestApp) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM usage_logs WHERE event = 'worksheet_exported'")
        .fetch_one(&app.db)
        .await
        .unwrap()
}

// ---------------------------------------------------------------- the list

#[tokio::test]
async fn the_list_carries_what_a_card_shows() {
    let app = test_app!();
    let t = app.teacher("T-CARD-1", "general").await;

    let ws = session(
        &app,
        &t,
        json!({ "kind": "worksheet", "material": material() }),
    )
    .await;
    let activity = session(
        &app,
        &t,
        json!({ "kind": "activity", "material": material() }),
    )
    .await;
    let ask = session(
        &app,
        &t,
        json!({ "kind": "ask", "request": "How do I open the lesson?", "material": material() }),
    )
    .await;
    let found = session(&app, &t, json!({ "kind": "resources", "topic": "rain" })).await;

    let (s, list) = app.get("/api/prep-sessions", Some(&t)).await;
    assert_eq!(s, StatusCode::OK, "{list}");
    let by_id = |id: &Value| {
        list["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"] == *id)
            .cloned()
            .unwrap()
    };

    // A worksheet session: its worksheet and the version it is at; no text needed.
    let card = by_id(&ws["id"]);
    let turn_ws = ws["turns"][0]["worksheet"]["id"].as_i64().unwrap();
    assert_eq!(card["preview_worksheet"]["id"], turn_ws);
    assert_eq!(
        card["preview_worksheet"]["version_no"].as_i64().unwrap(),
        ws["turns"][0]["worksheet"]["version_no"].as_i64().unwrap()
    );
    assert!(card["preview_text"].as_str().is_some());

    // An activity: no picture, the plan's title and first steps.
    let card = by_id(&activity["id"]);
    assert_eq!(card["preview_worksheet"], Value::Null);
    let text = card["preview_text"].as_str().unwrap();
    let plan = &activity["turns"][0]["result"];
    let title: String = plan["title"]
        .as_str()
        .unwrap()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(text.starts_with(&title[..title.len().min(40)]), "{text}");
    assert!(text.contains("\n1. "), "{text}");
    assert!(text.chars().count() <= 200);

    // 問 Manna: the question, then the start of the answer.
    let card = by_id(&ask["id"]);
    assert_eq!(card["preview_worksheet"], Value::Null);
    let text = card["preview_text"].as_str().unwrap();
    assert!(text.starts_with("How do I open the lesson?\n"), "{text}");
    assert!(text.chars().count() <= 200);

    // Anything else: the request.
    let card = by_id(&found["id"]);
    assert_eq!(card["preview_worksheet"], Value::Null);
    assert!(!card["preview_text"].as_str().unwrap().is_empty());

    // A deleted worksheet is no preview.
    let (s, _) = app
        .delete(&format!("/api/worksheets/{turn_ws}"), Some(&t))
        .await;
    assert_eq!(s, StatusCode::OK);
    let (_, list) = app.get("/api/prep-sessions", Some(&t)).await;
    let card = list["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == ws["id"])
        .unwrap();
    assert_eq!(card["preview_worksheet"], Value::Null);
}

// ---------------------------------------------------------------- the thumbnail

#[tokio::test]
async fn only_the_owner_gets_a_thumbnail_and_it_is_not_an_export() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure());
    let t = app.teacher("T-CARD-2", "general").await;
    let other = app.teacher("T-CARD-3", "general").await;
    let id = worksheet(&app, &t).await;

    assert_eq!(
        thumbnail(&app, id, Some(&other), None).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        thumbnail(&app, id, None, None).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        thumbnail(&app, 999_999, Some(&t), None).await.status,
        StatusCode::NOT_FOUND
    );

    let first = thumbnail(&app, id, Some(&t), None).await;
    if has_poppler() {
        assert_eq!(first.status, StatusCode::OK, "{}", json_of(&first));
        assert_eq!(first.header("content-type"), "image/jpeg");
        assert_eq!(
            first.header("cache-control"),
            "private, max-age=0, must-revalidate"
        );
        assert_eq!(first.header("vary"), "Authorization");
        let image = image::load_from_memory_with_format(&first.body, image::ImageFormat::Jpeg)
            .expect("a JPEG")
            .to_rgb8();
        assert_eq!(image.width(), 480);
        assert!(image.height() > 600, "an A4 page, not a strip");
        assert_eq!(dir.kept(id).len(), 1, "kept under the worksheet's folder");

        // Asked again: the kept one; the browser's copy is still good.
        let etag = first.header("etag").to_string();
        assert!(etag.starts_with('"'));
        let again = thumbnail(&app, id, Some(&t), None).await;
        assert_eq!(again.body, first.body);
        let fresh = thumbnail(&app, id, Some(&t), Some(&etag)).await;
        assert_eq!(fresh.status, StatusCode::NOT_MODIFIED);
        assert!(fresh.body.is_empty());
        assert_eq!(fresh.header("etag"), etag);
        let other_tag = thumbnail(&app, id, Some(&t), Some("\"nope\"")).await;
        assert_eq!(other_tag.status, StatusCode::OK);

        // A new version is a new picture (a new key and validator); the old key stays.
        let (_, state) = app.get(&format!("/api/worksheets/{id}"), Some(&t)).await;
        let mut content = state["content"].clone();
        content["blocks"][0]["title"] = json!("A changed first block");
        let (s, v) = app
            .send(
                Method::PUT,
                &format!("/api/worksheets/{id}"),
                Some(&t),
                Some("application/json"),
                json!({ "content": content }).to_string().into_bytes(),
            )
            .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        let next = thumbnail(&app, id, Some(&t), Some(&etag)).await;
        assert_eq!(next.status, StatusCode::OK, "{}", json_of(&next));
        assert_ne!(next.header("etag"), etag);
        assert_eq!(dir.kept(id).len(), 2);

        // Deleting the worksheet removes the picture of its current state.
        let (s, _) = app.delete(&format!("/api/worksheets/{id}"), Some(&t)).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(dir.kept(id).len(), 1, "only the older version's is left");
        assert_eq!(
            thumbnail(&app, id, Some(&t), None).await.status,
            StatusCode::NOT_FOUND
        );
    } else {
        assert_eq!(
            first.status,
            StatusCode::SERVICE_UNAVAILABLE,
            "{}",
            json_of(&first)
        );
        assert!(json_of(&first)["error"]["message"]
            .as_str()
            .unwrap()
            .contains("pdftoppm"));
        // Deleting still works with nothing kept.
        let (s, _) = app.delete(&format!("/api/worksheets/{id}"), Some(&t)).await;
        assert_eq!(s, StatusCode::OK);
    }
    assert_eq!(exports_logged(&app).await, 0, "a preview is not an export");
}

#[tokio::test]
async fn a_matching_etag_is_answered_without_rendering() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure());
    let t = app.teacher("T-CARD-4", "general").await;
    let id = worksheet(&app, &t).await;

    // The validator is a function of the worksheet's state, so it is known without a picture
    // (and without poppler): the answer is 304 and nothing is rendered or kept.
    let ws = store::by_id(&app.db, id).await.unwrap().unwrap();
    let latest = store::latest(&app.db, id).await.unwrap();
    let fp = thumbnail::fingerprint(&app.db, &ws).await.unwrap();
    let tag = thumbnail::etag(id, latest.version_no, &fp);
    let r = thumbnail(&app, id, Some(&t), Some(&tag)).await;
    assert_eq!(r.status, StatusCode::NOT_MODIFIED, "{}", json_of(&r));
    assert!(r.body.is_empty());
    assert_eq!(r.header("etag"), tag);
    assert_eq!(
        r.header("cache-control"),
        "private, max-age=0, must-revalidate"
    );
    assert!(dir.kept(id).is_empty());
    // A list of tags works too; a tag of another state does not match.
    let list = format!("\"x\", {tag}");
    assert_eq!(
        thumbnail(&app, id, Some(&t), Some(&list)).await.status,
        StatusCode::NOT_MODIFIED
    );
    let stale = thumbnail::etag(id, latest.version_no + 1, &fp);
    assert_ne!(
        thumbnail(&app, id, Some(&t), Some(&stale)).await.status,
        StatusCode::NOT_MODIFIED
    );
    assert_eq!(exports_logged(&app).await, 0);
}

#[tokio::test]
async fn a_worksheet_being_made_has_no_thumbnail_yet() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure());
    let t = app.teacher("T-CARD-5", "general").await;

    // Never generated.
    let (s, v) = app
        .post(
            "/api/worksheets",
            Some(&t),
            json!({ "material": material() }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    let fresh = v["id"].as_i64().unwrap();
    let r = thumbnail(&app, fresh, Some(&t), None).await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{}", json_of(&r));
    assert!(!json_of(&r)["error"]["message"].as_str().unwrap().is_empty());

    // Generated, then a new run in progress.
    let id = worksheet(&app, &t).await;
    sqlx::query(
        "INSERT INTO worksheet_jobs (worksheet_id, teacher_id, run_no, step, status, started_at)
         SELECT $1, teacher_id,
                (SELECT COALESCE(MAX(run_no), 0) + 1 FROM worksheet_jobs WHERE worksheet_id = $1),
                'tasks', 'running', now()
           FROM worksheets WHERE id = $1",
    )
    .bind(id)
    .execute(&app.db)
    .await
    .unwrap();
    let r = thumbnail(&app, id, Some(&t), None).await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{}", json_of(&r));
    assert!(dir.kept(id).is_empty(), "nothing is rendered meanwhile");
    assert_eq!(exports_logged(&app).await, 0);
}
