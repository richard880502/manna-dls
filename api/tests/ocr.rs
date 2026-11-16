//! Phase 2.10 (docs/API.md「掃描檔與 OCR」) with the fake OCR provider: images and scanned or
//! broken PDFs are read in the background after the upload; progress; a failed page; the manual
//! re-run and its refusals; the 409s while a file is being read; the 照我的學習單做 turn that
//! waits for its sample's OCR; and the restart recovery.
//!
//! Every image and PDF here is generated in the test. Rendering PDFs needs `pdftoppm`
//! (poppler-utils): the tests that do are skipped with a message when it is missing locally,
//! and fail in CI (which installs it).

#[macro_use]
mod common;

use std::io::Cursor;
use std::path::PathBuf;
use std::time::Duration;

use axum::http::StatusCode;
use common::Part;
use manna_api::config::Config;
use manna_api::materials::ocr::{self, OcrConfig};
use manna_api::storage::StorageConfig;
use manna_api::AppState;
use serde_json::{json, Value};

// ---------------------------------------------------------------- harness

/// A storage directory that is removed when the test ends.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("manna-ocr-{}", manna_api::auth::new_token()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    /// File storage here and the fake OCR.
    fn configure(&self) -> impl FnOnce(&mut Config) + '_ {
        |c: &mut Config| {
            c.storage = StorageConfig::fs(self.0.clone());
            c.ocr = OcrConfig::fake();
        }
    }

    /// The same, as a second process would see it (restart recovery).
    fn state(&self, db: &sqlx::PgPool, ocr: OcrConfig) -> AppState {
        let mut config = Config::for_tests("postgres://unused");
        config.storage = StorageConfig::fs(self.0.clone());
        config.ocr = ocr;
        AppState::new(db.clone(), config)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Whether PDF pages can be rendered here. Missing locally: the test is skipped (it runs in
/// CI); missing in CI: a failure.
fn poppler() -> bool {
    let has = |bin: &str| std::process::Command::new(bin).arg("-v").output().is_ok();
    if has("pdftoppm") && has("pdfinfo") {
        return true;
    }
    if std::env::var_os("CI").is_some() {
        panic!("pdftoppm and pdfinfo (poppler-utils) must be installed for the OCR tests in CI");
    }
    eprintln!("skipping: pdftoppm (poppler-utils) is not installed here; this test runs in CI");
    false
}

fn err_message(v: &Value) -> &str {
    v["error"]["message"].as_str().unwrap_or_default()
}

/// A real PNG of the given size (a gradient: nothing to read, the fake does not look).
fn png(width: u32, height: u32) -> Vec<u8> {
    let image = image::RgbImage::from_fn(width, height, |x, y| {
        image::Rgb([(x % 251) as u8, (y % 241) as u8, ((x + y) % 239) as u8])
    });
    let mut out = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image)
        .write_to(&mut out, image::ImageFormat::Png)
        .unwrap();
    out.into_inner()
}

/// A minimal PDF: one page per entry, each a list of lines (Helvetica 12pt). An empty page has
/// no text operators at all, like a scanned page.
fn pdf(pages: &[Vec<&str>]) -> Vec<u8> {
    let mut objects: Vec<String> = Vec::new();
    let n = pages.len();
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 4 + i * 2)).collect();
    objects.push("<< /Type /Catalog /Pages 2 0 R >>".into());
    objects.push(format!(
        "<< /Type /Pages /Kids [{}] /Count {n} >>",
        kids.join(" ")
    ));
    objects.push(
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".into(),
    );
    for (i, lines) in pages.iter().enumerate() {
        let mut stream = String::new();
        if !lines.is_empty() {
            stream.push_str("BT\n/F1 12 Tf\n14 TL\n72 720 Td\n");
            for (k, line) in lines.iter().enumerate() {
                if k > 0 {
                    stream.push_str("T*\n");
                }
                stream.push_str(&format!("({line}) Tj\n"));
            }
            stream.push_str("ET\n");
        }
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] \
             /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>",
            5 + i * 2
        ));
        objects.push(format!(
            "<< /Length {} >>\nstream\n{stream}endstream",
            stream.len()
        ));
    }
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// A page of ordinary text: enough for a clean text layer.
const LINES: [&str; 6] = [
    "Mrs. Lin put an umbrella stand outside her shop on a rainy Monday.",
    "A sign said: take one if you need it, and bring it back later.",
    "Her neighbors laughed at the idea and were sure of the result.",
    "In the first month, twelve of the thirty umbrellas disappeared.",
    "However, something unexpected followed in the second month.",
    "Today the stand holds more umbrellas than it did at the start.",
];

async fn upload(app: &common::TestApp, t: &str, filename: &str, data: &[u8]) -> Value {
    let (s, v) = app
        .post_multipart(
            "/api/materials/upload",
            Some(t),
            &[Part::File("file", filename, data)],
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v
}

async fn material(app: &common::TestApp, t: &str, id: i64) -> Value {
    let (s, v) = app.get(&format!("/api/materials/{id}"), Some(t)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v
}

/// Polls the material until its OCR run is over; returns it.
async fn wait_ocr(app: &common::TestApp, t: &str, id: i64) -> Value {
    for _ in 0..600 {
        let v = material(app, t, id).await;
        if !matches!(v["ocr"]["status"].as_str(), Some("queued" | "running")) {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("OCR of material {id} did not finish");
}

fn id_of(v: &Value) -> i64 {
    v["id"].as_i64().unwrap()
}

fn texts(v: &Value) -> Vec<String> {
    v["paragraphs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["text"].as_str().unwrap().to_string())
        .collect()
}

fn pages(v: &Value) -> Vec<i64> {
    v["paragraphs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["page"].as_i64().unwrap_or(0))
        .collect()
}

/// The fake's three paragraphs of page `n` (Section-Header, two Text blocks; the Picture and
/// the Page-Footer are left out).
fn fake_page(n: i64, marker: &str) -> Vec<String> {
    vec![
        format!("Page {n}"),
        format!("Scanned text on page {n}{marker}."),
        format!("第 {n} 頁的第二段：Tom & Jerry <ran>."),
    ]
}

#[derive(Debug, sqlx::FromRow)]
struct Interaction {
    id: i64,
    status: String,
    attempt: i32,
    retry_of: Option<i64>,
    input_snapshot: Value,
    output: Option<Value>,
    provider: String,
}

async fn interactions(app: &common::TestApp, material_id: i64) -> Vec<Interaction> {
    sqlx::query_as(
        "SELECT id, status, attempt, retry_of, input_snapshot, output, provider FROM ai_interactions
          WHERE module = 'material_ocr' AND (input_snapshot->>'material_id')::bigint = $1
          ORDER BY id",
    )
    .bind(material_id)
    .fetch_all(&app.db)
    .await
    .unwrap()
}

async fn stored_pages(app: &common::TestApp, id: i64) -> Value {
    sqlx::query_scalar::<_, Option<Value>>("SELECT ocr_pages FROM materials WHERE id = $1")
        .bind(id)
        .fetch_one(&app.db)
        .await
        .unwrap()
        .unwrap_or(Value::Null)
}

// ---------------------------------------------------------------- images

#[tokio::test]
async fn an_image_is_read_by_ocr_after_the_upload() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure());
    let t = app.teacher("T-OCR-1", "general").await;
    let photo = png(320, 200);

    // The upload answers at once: OCR is queued, the text status is what it was.
    let up = upload(&app, &t, "board OCRMARK1.png", &photo).await;
    let id = id_of(&up);
    assert_eq!(up["kind"], "image");
    assert_eq!(up["text_status"], "not_applicable");
    assert_eq!(up["text_source"], "file");
    assert_eq!(
        up["ocr"],
        json!({ "status": "queued", "done_pages": 0, "total_pages": 1, "error": null })
    );

    let done = wait_ocr(&app, &t, id).await;
    assert_eq!(
        done["ocr"],
        json!({ "status": "done", "done_pages": 1, "total_pages": 1, "error": null })
    );
    assert_eq!(done["text_status"], "ready");
    assert_eq!(done["text_source"], "ocr");
    assert_eq!(texts(&done), fake_page(1, " OCRMARK1"));
    assert_eq!(pages(&done), [1, 1, 1]);
    assert_eq!(done["paragraphs"][0]["id"], "p1");
    assert_eq!(done["text"], fake_page(1, " OCRMARK1").join("\n\n"));
    assert_eq!(done["revision"], up["revision"].as_i64().unwrap() + 1);
    assert_eq!(done["text_truncated"], false);
    assert_eq!(done["looks_like_worksheet"], false);
    // The list says the same.
    let (_, list) = app.get("/api/materials", Some(&t)).await;
    assert_eq!(list["materials"][0]["ocr"]["status"], "done");
    assert_eq!(list["materials"][0]["text_source"], "ocr");

    // The raw blocks are kept, every label with its box; pictures are counted.
    let raw = stored_pages(&app, id).await;
    assert_eq!(raw[0]["page"], 1);
    assert_eq!(raw[0]["pictures"], 1);
    let labels: Vec<&str> = raw[0]["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["label"].as_str().unwrap())
        .collect();
    assert_eq!(
        labels,
        ["Section-Header", "Text", "Text", "Picture", "Page-Footer"]
    );
    assert_eq!(raw[0]["blocks"][0]["bbox"], json!([60, 40, 940, 90]));

    // One logged call: page, image size and hash, output length. Never the image or the text.
    let calls = interactions(&app, id).await;
    assert_eq!(calls.len(), 1);
    let call = &calls[0];
    assert_eq!((call.status.as_str(), call.attempt), ("shown", 1));
    assert_eq!(call.provider, "fake");
    assert_eq!(call.input_snapshot["page"], 1);
    assert_eq!(call.input_snapshot["image"]["bytes"], photo.len());
    assert_eq!(
        call.input_snapshot["image"]["sha256"],
        manna_api::auth::sha256_hex(&photo)
    );
    let output = call.output.clone().unwrap();
    assert!(output["chars"].as_u64().unwrap() > 100, "{output}");
    assert_eq!(output.as_object().unwrap().len(), 1, "{output}");
    let logged = format!("{} {}", call.input_snapshot, output);
    assert!(
        !logged.contains("Scanned") && !logged.contains("base64"),
        "{logged}"
    );
    let finished: Vec<Value> = sqlx::query_scalar(
        "SELECT payload FROM usage_logs WHERE event = 'material_ocr_finished' ORDER BY id",
    )
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert_eq!(finished.len(), 1);
    assert_eq!(finished[0]["status"], "done");

    // DOCX, TXT and a PDF with a good text layer are never read by OCR.
    let txt = upload(&app, &t, "notes.txt", b"First line.\n\nSecond line.").await;
    assert_eq!(txt["ocr"]["status"], "none");
    let clean = upload(&app, &t, "handout.pdf", &pdf(&[LINES.to_vec()])).await;
    assert_eq!(clean["text_status"], "ready", "{clean}");
    assert_eq!(clean["ocr"]["status"], "none");
    assert_eq!(clean["text_source"], "file");
}

#[tokio::test]
async fn progress_and_a_run_where_nothing_can_be_read() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure());
    let t = app.teacher("T-OCR-2", "general").await;

    // Progress: queued in the answer, then running at 0 of 1, then done at 1 of 1.
    let up = upload(&app, &t, "slow FAKE_OCR_SLOW.png", &png(64, 64)).await;
    let id = id_of(&up);
    assert_eq!(up["ocr"]["status"], "queued");
    let mut seen_running = false;
    for _ in 0..200 {
        let v = material(&app, &t, id).await;
        if v["ocr"]["status"] == "running" {
            assert_eq!(v["ocr"]["done_pages"], 0);
            assert_eq!(v["ocr"]["total_pages"], 1);
            assert_eq!(
                v["text_status"], "not_applicable",
                "unchanged while it runs"
            );
            seen_running = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(seen_running, "the run is seen running");
    let done = wait_ocr(&app, &t, id).await;
    assert_eq!(
        (
            done["ocr"]["done_pages"].as_i64(),
            done["ocr"]["total_pages"].as_i64()
        ),
        (Some(1), Some(1))
    );

    // Every page fails (twice: a 5xx is retried once): the run fails, the text stays.
    let down = upload(&app, &t, "broken FAKE_OCR_DOWN.png", &png(48, 48)).await;
    let down_id = id_of(&down);
    let failed = wait_ocr(&app, &t, down_id).await;
    assert_eq!(failed["ocr"]["status"], "failed");
    let error = failed["ocr"]["error"].as_str().unwrap();
    assert!(error.contains("沒有一頁辨識成功"), "{error}");
    assert_eq!(failed["text_status"], "not_applicable");
    assert_eq!(failed["text_source"], "file");
    let calls = interactions(&app, down_id).await;
    assert_eq!(calls.len(), 2, "one retry");
    assert_eq!(
        calls
            .iter()
            .map(|c| (c.status.as_str(), c.attempt))
            .collect::<Vec<_>>(),
        [("error", 1), ("error", 2)]
    );
    assert_eq!(calls[1].retry_of, Some(calls[0].id));
}

// ---------------------------------------------------------------- manual re-run, refusals

#[tokio::test]
async fn ocr_can_be_run_again_by_hand() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure());
    let t = app.teacher("T-OCR-3", "general").await;
    let other = app.teacher("T-OCR-3B", "general").await;

    let up = upload(&app, &t, "page.png", &png(100, 80)).await;
    let id = id_of(&up);
    let first = wait_ocr(&app, &t, id).await;
    assert_eq!(first["ocr"]["status"], "done");

    let (s, again) = app
        .post(&format!("/api/materials/{id}/ocr"), Some(&t), json!({}))
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{again}");
    assert_eq!(
        again["ocr"],
        json!({ "status": "queued", "done_pages": 0, "total_pages": 1, "error": null })
    );
    assert_eq!(
        again["text_status"], "ready",
        "the old text stays meanwhile"
    );
    let second = wait_ocr(&app, &t, id).await;
    assert_eq!(second["ocr"]["status"], "done");
    // The same text: the paragraphs keep their ids and the revision stays.
    assert_eq!(second["paragraphs"], first["paragraphs"]);
    assert_eq!(second["revision"], first["revision"]);
    assert_eq!(interactions(&app, id).await.len(), 2);

    // Only the owner, only uploaded PDFs and images.
    let (s, _) = app
        .post(&format!("/api/materials/{id}/ocr"), Some(&other), json!({}))
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (_, pasted) = app
        .post(
            "/api/materials",
            Some(&t),
            json!({ "kind": "text", "title": "Pasted", "text": "Only text." }),
        )
        .await;
    let (_, link) = app
        .post(
            "/api/materials",
            Some(&t),
            json!({ "kind": "link", "title": "Talk", "url": "https://example.org/talk" }),
        )
        .await;
    let txt = upload(&app, &t, "notes.txt", b"Plain text file.").await;
    for m in [&pasted, &link, &txt] {
        let (s, v) = app
            .post(
                &format!("/api/materials/{}/ocr", m["id"]),
                Some(&t),
                json!({}),
            )
            .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
        assert_eq!(
            err_message(&v),
            "只有上傳的 PDF 與圖片可以用 OCR 重新讀取。"
        );
    }

    // A run already open: 409.
    let slow = upload(&app, &t, "slow FAKE_OCR_SLOW.png", &png(60, 60)).await;
    let (s, v) = app
        .post(
            &format!("/api/materials/{}/ocr", slow["id"]),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    assert_eq!(
        err_message(&v),
        "這份檔案正在辨識文字（第 1／1 頁），完成後再試一次。"
    );
    wait_ocr(&app, &t, id_of(&slow)).await;

    // An environment without OCR says so.
    let off_dir = TempDir::new();
    let off = test_app!(|c: &mut Config| c.storage = StorageConfig::fs(off_dir.0.clone()));
    let t2 = off.teacher("T-OCR-3C", "general").await;
    let image = upload(&off, &t2, "photo.png", &png(30, 30)).await;
    assert_eq!(image["ocr"]["status"], "none", "OCR off: nothing queued");
    let (s, v) = off
        .post(
            &format!("/api/materials/{}/ocr", image["id"]),
            Some(&t2),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE, "{v}");
}

#[tokio::test]
async fn a_file_being_read_is_refused_with_409() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure());
    let t = app.teacher("T-OCR-4", "general").await;
    let up = upload(&app, &t, "worksheet FAKE_OCR_SLOW.png", &png(90, 120)).await;
    let id = id_of(&up);
    let busy = "這份檔案正在辨識文字（第 1／1 頁），完成後再試一次。";

    let (s, v) = app
        .post("/api/worksheets", Some(&t), json!({ "material_id": id }))
        .await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    assert_eq!(err_message(&v), busy);
    let (s, v) = app
        .post(
            "/api/prep-sessions",
            Some(&t),
            json!({ "kind": "worksheet", "material_id": id }),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    assert_eq!(err_message(&v), busy);
    let (s, v) = app
        .post(
            "/api/worksheet-templates",
            Some(&t),
            json!({ "material_id": id }),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    assert_eq!(err_message(&v), busy);

    // Once it is read, the same requests go through.
    let done = wait_ocr(&app, &t, id).await;
    assert_eq!(done["text_status"], "ready");
    let (s, v) = app
        .post(
            "/api/worksheet-templates",
            Some(&t),
            json!({ "material_id": id }),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    let (s, v) = app
        .post("/api/worksheets", Some(&t), json!({ "material_id": id }))
        .await;
    assert!(s.is_success(), "{s} {v}");
}

// ---------------------------------------------------------------- 照我的學習單做 waits

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

const PASSAGE: &str = "Mrs. Lin put an umbrella stand outside her shop on a rainy Monday.\n\n\
    Her neighbors laughed at the idea. They were sure every umbrella would be gone.\n\n\
    In the first month, twelve of the thirty umbrellas did disappear.";

#[tokio::test]
async fn a_scanned_sample_is_read_before_its_template() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure());
    let t = app.teacher("T-OCR-5", "general").await;
    let sample = upload(&app, &t, "my worksheet FAKE_OCR_SLOW.png", &png(200, 280)).await;
    let sample_id = id_of(&sample);

    let (s, started) = app
        .post(
            "/api/prep-sessions",
            Some(&t),
            json!({
                "kind": "worksheet",
                "material": { "title": "The Umbrella Stand", "text": PASSAGE },
                "template_material_id": sample_id,
            }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{started}");
    let turn = started["turns"][0].clone();
    assert_eq!(turn["status"], "running");
    assert_eq!(turn["phase"], "ocr", "{turn}");
    assert_eq!(turn["ocr"], json!({ "done_pages": 0, "total_pages": 1 }));
    assert_eq!(turn["template"]["status"], "extracting");
    assert_eq!(turn["worksheet"], Value::Null);

    let done = wait_session(&app, &t, id_of(&started)).await;
    let turn = done["turns"][0].clone();
    assert_eq!(turn["status"], "done", "{turn}");
    assert!(turn.get("phase").is_none(), "{turn}");
    assert!(turn.get("ocr").is_none(), "{turn}");
    assert_eq!(turn["template"]["status"], "ready");
    assert!(turn["worksheet"]["id"].is_i64(), "{turn}");
    let read = material(&app, &t, sample_id).await;
    assert_eq!(read["text_source"], "ocr");

    // A sample whose OCR fails: the template fails, and so does the turn.
    let bad = upload(
        &app,
        &t,
        "bad FAKE_OCR_SLOW FAKE_OCR_DOWN.png",
        &png(70, 70),
    )
    .await;
    let (s, started) = app
        .post(
            "/api/prep-sessions",
            Some(&t),
            json!({
                "kind": "worksheet",
                "material": { "title": "Again", "text": PASSAGE },
                "template_material_id": bad["id"],
            }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{started}");
    assert_eq!(started["turns"][0]["phase"], "ocr");
    let failed = wait_session(&app, &t, id_of(&started)).await;
    let turn = failed["turns"][0].clone();
    assert_eq!(turn["status"], "failed", "{turn}");
    assert!(
        turn["error"]
            .as_str()
            .unwrap()
            .contains("文字辨識（OCR）沒有成功"),
        "{turn}"
    );
    assert_eq!(turn["template"]["status"], "failed");
}

// ---------------------------------------------------------------- restarts

#[tokio::test]
async fn open_runs_are_resumed_or_failed_after_a_restart() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure());
    let t = app.teacher("T-OCR-6", "general").await;
    let up = upload(&app, &t, "page.png", &png(50, 50)).await;
    let id = id_of(&up);
    wait_ocr(&app, &t, id).await;
    let interrupt = |minutes: i32| {
        sqlx::query(
            "UPDATE materials SET ocr_status = 'running', ocr_done_pages = 0, ocr_pages = NULL,
                    ocr_error = NULL, ocr_started_at = now() - make_interval(mins => $2)
              WHERE id = $1",
        )
        .bind(id)
        .bind(minutes)
    };

    // A run nobody holds, long past `OCR_TIMEOUT_SECS × pages + 120`: reported as failed.
    interrupt(60).execute(&app.db).await.unwrap();
    let stale = material(&app, &t, id).await;
    assert_eq!(stale["ocr"]["status"], "failed");
    assert!(stale["ocr"]["error"].as_str().unwrap().contains("中斷"));
    // A recent one is left alone (its job may still be running elsewhere).
    interrupt(1).execute(&app.db).await.unwrap();
    assert_eq!(material(&app, &t, id).await["ocr"]["status"], "running");

    // At startup a new process takes it over and finishes it.
    let restarted = dir.state(&app.db, OcrConfig::fake());
    assert_eq!(ocr::recover(&restarted).await.unwrap(), 1);
    let resumed = wait_ocr(&app, &t, id).await;
    assert_eq!(resumed["ocr"]["status"], "done");
    assert_eq!(texts(&resumed), fake_page(1, ""));

    // Without OCR after the restart, the run cannot go on: failed.
    interrupt(1).execute(&app.db).await.unwrap();
    let without = dir.state(&app.db, OcrConfig::disabled());
    assert_eq!(ocr::recover(&without).await.unwrap(), 0);
    let v = material(&app, &t, id).await;
    assert_eq!(v["ocr"]["status"], "failed");
    assert!(v["ocr"]["error"].as_str().unwrap().contains("系統更新後"));
}

#[tokio::test]
async fn knowledge_base_files_never_read_are_started_at_startup() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure());
    let pi = app.pi_token().await;
    let (s, v) = app
        .post(
            "/api/admin/invites",
            Some(&pi),
            json!({ "research_id": "KB-OCR-1", "school_track": "general", "role": "kb_manager" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let kb = app.login(v["code"].as_str().unwrap()).await.0;
    let t = app.teacher("T-OCR-7", "general").await;

    // A knowledge-base image and a teacher's own image, both read after their uploads.
    let (s, card) = app
        .post_multipart(
            "/api/admin/library",
            Some(&kb),
            &[
                Part::Text("category", "學測英文"),
                Part::File("file", "kb.png", &png(40, 40)),
            ],
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{card}");
    let platform = id_of(&card);
    wait_ocr(&app, &t, platform).await;
    let own = id_of(&upload(&app, &t, "own.png", &png(40, 40)).await);
    wait_ocr(&app, &t, own).await;

    // As if both had been uploaded before OCR existed.
    sqlx::query(
        "UPDATE materials SET ocr_status = 'none', ocr_pages = NULL, ocr_error = NULL,
                text = '', paragraphs = '[]', text_status = 'not_applicable', text_source = 'file'
          WHERE id = ANY($1)",
    )
    .bind(vec![platform, own])
    .execute(&app.db)
    .await
    .unwrap();

    // Without OCR nothing starts; with it, only the knowledge-base file, once.
    let without = dir.state(&app.db, OcrConfig::disabled());
    assert_eq!(ocr::backfill_platform(&without).await.unwrap(), 0);
    let restarted = dir.state(&app.db, OcrConfig::fake());
    assert_eq!(ocr::backfill_platform(&restarted).await.unwrap(), 1);
    let read = wait_ocr(&app, &t, platform).await;
    assert_eq!(read["ocr"]["status"], "done");
    assert_eq!(texts(&read), fake_page(1, ""));
    assert_eq!(material(&app, &t, own).await["ocr"]["status"], "none");
    assert_eq!(ocr::backfill_platform(&restarted).await.unwrap(), 0);
}

// ---------------------------------------------------------------- PDFs (need pdftoppm)

#[tokio::test]
async fn a_scanned_pdf_is_read_page_by_page() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure());
    if !poppler() {
        return;
    }
    let t = app.teacher("T-OCR-7", "general").await;

    // No text layer, three pages, page 2 fails (twice).
    let scan = pdf(&[vec![], vec![], vec![]]);
    let up = upload(&app, &t, "scan LESSON9 FAKE_OCR_FAIL.pdf", &scan).await;
    let id = id_of(&up);
    assert_eq!(up["text_status"], "no_text");
    assert_eq!(up["page_count"], 3);
    assert_eq!(
        up["ocr"],
        json!({ "status": "queued", "done_pages": 0, "total_pages": 3, "error": null })
    );
    let mut last_done = 0;
    let done = loop {
        let v = material(&app, &t, id).await;
        let n = v["ocr"]["done_pages"].as_i64().unwrap();
        assert!(n >= last_done && n <= 3, "progress only goes up: {v}");
        last_done = n;
        if !matches!(v["ocr"]["status"].as_str(), Some("queued" | "running")) {
            break v;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert_eq!(
        done["ocr"],
        json!({ "status": "done", "done_pages": 3, "total_pages": 3,
                "error": "第 2 頁沒有辨識成功。" })
    );
    assert_eq!(done["text_status"], "ready");
    let mut expected = fake_page(1, " LESSON9");
    expected.extend(fake_page(3, " LESSON9"));
    assert_eq!(texts(&done), expected);
    assert_eq!(pages(&done), [1, 1, 1, 3, 3, 3]);
    let calls = interactions(&app, id).await;
    let page_two: Vec<&Interaction> = calls
        .iter()
        .filter(|c| c.input_snapshot["page"] == 2)
        .collect();
    assert_eq!(page_two.len(), 2, "page 2 was tried twice");
    assert!(page_two.iter().all(|c| c.status == "error"));
    assert_eq!(calls.len(), 4);
    let raw = stored_pages(&app, id).await;
    assert_eq!(raw.as_array().unwrap().len(), 3);
    assert!(raw[1]["error"].is_string(), "{raw}");
    let rendered = &calls
        .iter()
        .find(|c| c.status == "shown")
        .unwrap()
        .input_snapshot;
    assert_eq!(rendered["image"]["content_type"], "image/png");

    // A text layer that is there but broken (a few characters a page) is read again.
    let poor = upload(&app, &t, "thin.pdf", &pdf(&[vec!["Hi"], vec!["p. 2"]])).await;
    assert_eq!(poor["text_status"], "ready", "{poor}");
    assert_eq!(poor["ocr"]["status"], "queued");
    let read = wait_ocr(&app, &t, id_of(&poor)).await;
    assert_eq!(read["text_source"], "ocr");
    assert_eq!(pages(&read), [1, 1, 1, 2, 2, 2]);

    // By hand, on a PDF with a good text layer.
    let clean = upload(&app, &t, "handout.pdf", &pdf(&[LINES.to_vec()])).await;
    assert_eq!(clean["ocr"]["status"], "none");
    let (s, v) = app
        .post(
            &format!("/api/materials/{}/ocr", clean["id"]),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    let reread = wait_ocr(&app, &t, id_of(&clean)).await;
    assert_eq!(reread["text_source"], "ocr");
    assert_eq!(texts(&reread), fake_page(1, ""));
}

#[tokio::test]
async fn long_pdfs_are_cut_at_the_page_limit_and_runs_resume_where_they_stopped() {
    let dir = TempDir::new();
    let app = test_app!(|c: &mut Config| {
        dir.configure()(c);
        c.ocr.max_pages = 2;
    });
    if !poppler() {
        return;
    }
    let t = app.teacher("T-OCR-8", "general").await;
    let up = upload(&app, &t, "long.pdf", &pdf(&[vec![], vec![], vec![]])).await;
    let id = id_of(&up);
    assert_eq!(up["ocr"]["total_pages"], 2);
    let done = wait_ocr(&app, &t, id).await;
    assert_eq!(done["ocr"]["error"], "這份檔案有 3 頁，只辨識了前 2 頁。");
    assert_eq!(done["text_truncated"], true);
    assert_eq!(pages(&done), [1, 1, 1, 2, 2, 2]);

    // Interrupted after page 1: the resumed run reads page 2 only.
    let page_one = json!([{ "page": 1, "pictures": 0,
        "blocks": [{ "label": "Text", "text": "Kept from before the restart.", "bbox": null }] }]);
    sqlx::query(
        "UPDATE materials SET ocr_status = 'running', ocr_done_pages = 1, ocr_pages = $2,
                ocr_started_at = now() - interval '1 minute'
          WHERE id = $1",
    )
    .bind(id)
    .bind(&page_one)
    .execute(&app.db)
    .await
    .unwrap();
    let before = interactions(&app, id).await.len();
    let mut config = OcrConfig::fake();
    config.max_pages = 2;
    assert_eq!(ocr::recover(&dir.state(&app.db, config)).await.unwrap(), 1);
    let resumed = wait_ocr(&app, &t, id).await;
    let mut expected = vec!["Kept from before the restart.".to_string()];
    expected.extend(fake_page(2, ""));
    assert_eq!(texts(&resumed), expected);
    let calls = interactions(&app, id).await;
    assert_eq!(calls.len(), before + 1, "only page 2 was read again");
    assert_eq!(calls.last().unwrap().input_snapshot["page"], 2);
}
