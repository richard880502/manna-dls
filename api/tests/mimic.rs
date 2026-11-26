//! Phase 2.8 (docs/API.md「照我的學習單做」), with the fake provider: a prep session started
//! from a worksheet sample (`template_material_id`) extracts a template and the first turn
//! waits for it; a ready template of the sample is reused; a failed extraction fails the turn
//! (and 「再試一次」 extracts again); the sample is only for the full worksheet; template blocks
//! carry Bloom levels (`worksheet_template_section` version 2); materials say whether they look
//! like a worksheet; and a refused worksheet-like material points to 照我的學習單做.

#[macro_use]
mod common;

use std::path::PathBuf;
use std::time::Duration;

use axum::http::StatusCode;
use common::Part;
use manna_api::config::Config;
use manna_api::storage::StorageConfig;
use serde_json::{json, Value};

const BLOOMS: [&str; 6] = [
    "remember",
    "understand",
    "apply",
    "analyze",
    "evaluate",
    "create",
];
const MSG_SAMPLE_KIND: &str = "範本只適用於完整學習單";
const MSG_UNREADABLE: &str = "Manna 這次沒有讀出這份學習單的設計（輸出不完整）。請按「重試」再試一次；一直失敗的話，可能是檔案太長或版面太亂。";

/// A teacher's past worksheet, as the text extractor would read it. The fake provider takes
/// every `## ` line as one block.
fn worksheet_text(marker: &str) -> String {
    format!(
        "Name: ______ No: ______ Class: ______\n\nObjective: I can find details in each paragraph.\n\n\
         ## Warm-up: [主題]\nWhat do you know about the shop?\n\n\
         ## Paragraph [n]: Details\n1. What does the owner put outside? 2. Why?\n\n\
         ## Watch the video and compare\nWatch the clip and write two differences.\n\n\
         ## Writing: [主題]\nWrite 80 words about trust. {marker}"
    )
}

const P: [&str; 4] = [
    "Mrs. Lin put an umbrella stand outside her shop on a rainy Monday. A sign said, “Take one if you need it.”",
    "Her neighbors laughed at the idea. They were sure that every umbrella would be gone within a week.",
    "In the first month, twelve of the thirty umbrellas did disappear. However, something unexpected followed.",
    "Today the stand holds more umbrellas than it did at the start. Mrs. Lin says trust is like an umbrella.",
];

fn passage(marker: &str) -> String {
    format!("{}\n\n{}\n\n{}\n\n{} {marker}", P[0], P[1], P[2], P[3])
}

fn err_message(v: &Value) -> &str {
    v["error"]["message"].as_str().unwrap_or_default()
}

async fn paste(app: &common::TestApp, t: &str, title: &str, text: &str) -> Value {
    let (s, v) = app
        .post(
            "/api/materials",
            Some(t),
            json!({ "kind": "text", "title": title, "text": text }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v
}

async fn create(app: &common::TestApp, t: &str, body: Value) -> Value {
    let (s, v) = app.post("/api/prep-sessions", Some(t), body).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    v
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

async fn wait_template(app: &common::TestApp, t: &str, id: i64) -> Value {
    for _ in 0..400 {
        let (s, v) = app
            .get(&format!("/api/worksheet-templates/{id}"), Some(t))
            .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        if v["status"] != "extracting" {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("template {id} did not finish extracting");
}

fn last(session: &Value) -> Value {
    session["turns"].as_array().unwrap().last().unwrap().clone()
}

fn sid(session: &Value) -> i64 {
    session["id"].as_i64().unwrap()
}

async fn count(app: &common::TestApp, sql: &str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(&app.db).await.unwrap()
}

async fn usage(app: &common::TestApp, event: &str) -> Vec<Value> {
    sqlx::query_scalar("SELECT payload FROM usage_logs WHERE event = $1 ORDER BY id")
        .bind(event)
        .fetch_all(&app.db)
        .await
        .unwrap()
}

// ---------------------------------------------------------------- one step from a sample

#[tokio::test]
async fn a_sample_becomes_a_template_and_the_first_turn_follows_it() {
    let app = test_app!();
    let t = app.teacher("T-MIMIC-1", "general").await;
    let sample = paste(&app, &t, "L2 Reading Worksheet", &worksheet_text("")).await;
    let sample_id = sample["id"].as_i64().unwrap();
    assert_eq!(sample["looks_like_worksheet"], true);

    let started = create(
        &app,
        &t,
        json!({
            "kind": "worksheet",
            "material": { "title": "The Umbrella Stand", "text": passage("") },
            "template_material_id": sample_id,
        }),
    )
    .await;
    let id = sid(&started);
    // A new template, the session's template from the start.
    let template_id = started["template"]["id"]
        .as_i64()
        .expect("session template");
    assert_eq!(started["template"]["title"], "L2 Reading Worksheet版型");
    assert_eq!(started["material"]["title"], "The Umbrella Stand");
    // The answer is read before the extraction starts: the turn waits, without a worksheet.
    let first = last(&started);
    assert_eq!(first["status"], "running");
    assert_eq!(first["worksheet"], Value::Null);
    assert_eq!(
        first["template"],
        json!({ "id": template_id, "title": "L2 Reading Worksheet版型", "status": "extracting" })
    );

    let done = wait(&app, &t, id).await;
    let turn = last(&done);
    assert_eq!(turn["status"], "done", "{turn}");
    assert_eq!(turn["error"], Value::Null);
    assert_eq!(turn["template"]["status"], "ready");
    let ws_id = turn["worksheet"]["id"].as_i64().expect("worksheet made");

    // An ordinary template in 我的版型, made from the sample.
    let (_, list) = app.get("/api/worksheet-templates", Some(&t)).await;
    let templates = list["templates"].as_array().unwrap();
    assert_eq!(templates.len(), 1);
    assert_eq!(templates[0]["id"], template_id);
    assert_eq!(templates[0]["status"], "ready");
    assert_eq!(templates[0]["source_material"]["id"], sample_id);
    let created = usage(&app, "worksheet_template_created").await;
    assert_eq!(
        created,
        [json!({ "template_id": template_id, "material_id": sample_id, "prep_session_id": id })]
    );

    // The worksheet follows the template, on the session's material (not the sample).
    let (_, ws) = app.get(&format!("/api/worksheets/{ws_id}"), Some(&t)).await;
    assert_eq!(ws["template"]["id"], template_id);
    assert_eq!(ws["prep_session_id"], id);
    assert_eq!(
        ws["material"]["source"]["material_id"],
        done["material"]["id"]
    );
    let blocks = ws["content"]["blocks"].as_array().unwrap();
    assert_eq!(blocks.len(), 7, "warm-up, 4 paragraphs, video, writing");
    assert!(blocks.iter().all(|b| b["template_section_id"].is_string()));

    // Version 2 of the section prompt: every item carries a Bloom level.
    let mut items = 0;
    for b in blocks {
        for it in b["items"].as_array().unwrap() {
            items += 1;
            let bloom = it["bloom"].as_str().unwrap_or_else(|| panic!("{it}"));
            assert!(BLOOMS.contains(&bloom), "{it}");
        }
    }
    assert!(items > 7);
    let writing = blocks.last().unwrap();
    assert_eq!(writing["items"][0]["bloom"], "create");
    let calls: Vec<Value> = sqlx::query_scalar(
        "SELECT input_snapshot FROM ai_interactions
          WHERE module = 'worksheet_template_section' ORDER BY id",
    )
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert_eq!(calls.len(), 7);
    for c in &calls {
        // Version 3 (0029_graphic_organizers.sql) is version 2 with the organizer section;
        // version 4 (0030_fill_in_blanks.sql) adds the 填空題 blank; version 5
        // (0036_template_supports.sql) the support boxes, 是非題 and one term per sheet; version 6
        // (0051_table_organizer_headers.sql) the table's headings and row labels; version 7
        // (0060_objectives_not_items.sql) that the objectives are not questions.
        assert_eq!(c["prompt"]["version_no"], 7);
        assert!(c["prompt"]["system"]
            .as_str()
            .unwrap()
            .contains("8. bloom："));
    }
    // The teacher version shows the levels; the student version never does.
    let level = ws["content"]["levels"][0].as_str().unwrap().to_string();
    let (s, teacher) = app
        .get(
            &format!("/api/worksheets/{ws_id}/sheet?level={level}&audience=teacher"),
            Some(&t),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{teacher}");
    assert!(teacher["blocks"][0]["items"][0]["bloom"].is_string());
    assert!(teacher["blocks"][0]["bloom_summary"]
        .as_str()
        .unwrap()
        .starts_with("Bloom："));
    let (_, student) = app
        .get(
            &format!("/api/worksheets/{ws_id}/sheet?level={level}&audience=student"),
            Some(&t),
        )
        .await;
    assert!(!student.to_string().contains("\"bloom\""));

    // Other kinds of turns follow no template.
    let (s, v) = app
        .post(
            &format!("/api/prep-sessions/{id}/turns"),
            Some(&t),
            json!({ "kind": "comprehension" }),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    assert_eq!(last(&v)["template"], Value::Null);
    let done = wait(&app, &t, id).await;
    assert_eq!(last(&done)["status"], "done");
    assert_eq!(done["turns"][0]["template"]["id"], template_id);
}

#[tokio::test]
async fn a_ready_template_of_the_sample_is_reused() {
    let app = test_app!();
    let t = app.teacher("T-MIMIC-2", "general").await;
    let sample = paste(&app, &t, "L2 Reading Worksheet", &worksheet_text("")).await;
    let sample_id = sample["id"].as_i64().unwrap();
    let (s, v) = app
        .post(
            "/api/worksheet-templates",
            Some(&t),
            json!({ "material_id": sample_id, "title": "我的閱讀學習單" }),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    let template_id = v["id"].as_i64().unwrap();
    assert_eq!(
        wait_template(&app, &t, template_id).await["status"],
        "ready"
    );
    let extractions = count(
        &app,
        "SELECT COUNT(*) FROM ai_interactions WHERE module = 'worksheet_template_outline'",
    )
    .await;
    assert_eq!(extractions, 1);

    // The article carries FAKE_TPL_BAD_BLOOM: the first item of each block gets a level outside
    // the six, which the server drops.
    let started = create(
        &app,
        &t,
        json!({
            "kind": "worksheet",
            "material": { "title": "The Umbrella Stand", "text": passage("FAKE_TPL_BAD_BLOOM") },
            "template_material_id": sample_id,
        }),
    )
    .await;
    assert_eq!(started["template"]["id"], template_id);
    assert_eq!(started["template"]["title"], "我的閱讀學習單");
    // A ready template: the worksheet is made at once, as with `template_id`.
    let first = last(&started);
    assert_eq!(first["template"]["status"], "ready");
    let ws_id = first["worksheet"]["id"]
        .as_i64()
        .expect("worksheet at once");
    let done = wait(&app, &t, sid(&started)).await;
    assert_eq!(last(&done)["status"], "done");

    // No second template, no second extraction.
    assert_eq!(
        count(&app, "SELECT COUNT(*) FROM worksheet_templates").await,
        1
    );
    assert_eq!(
        count(
            &app,
            "SELECT COUNT(*) FROM ai_interactions WHERE module = 'worksheet_template_outline'",
        )
        .await,
        extractions
    );
    assert_eq!(usage(&app, "worksheet_template_created").await.len(), 1);
    let (_, ws) = app.get(&format!("/api/worksheets/{ws_id}"), Some(&t)).await;
    assert_eq!(ws["template"]["id"], template_id);
    for b in ws["content"]["blocks"].as_array().unwrap() {
        let items = b["items"].as_array().unwrap();
        if b["message"].is_null() {
            assert_eq!(items[0]["bloom"], Value::Null, "{b}");
        }
        for it in items.iter().skip(1) {
            assert!(it["bloom"].is_string(), "{it}");
        }
    }

    // `template_id` wins: the sample is then ignored (even one that could not be used).
    let other = app.teacher("T-MIMIC-3", "general").await;
    let theirs = paste(&app, &other, "Theirs", &worksheet_text("")).await;
    let s2 = create(
        &app,
        &t,
        json!({
            "kind": "worksheet",
            "material_id": done["material"]["id"],
            "template_id": template_id,
            "template_material_id": theirs["id"],
        }),
    )
    .await;
    assert_eq!(s2["template"]["id"], template_id);
    wait(&app, &t, sid(&s2)).await;
    assert_eq!(
        count(&app, "SELECT COUNT(*) FROM worksheet_templates").await,
        1
    );
}

#[tokio::test]
async fn a_failed_extraction_fails_the_turn_and_try_again_extracts_again() {
    let app = test_app!();
    let t = app.teacher("T-MIMIC-4", "general").await;
    // FAKE_BLOCK in the sample: both extraction calls are unreadable.
    let sample = paste(&app, &t, "Broken Worksheet", &worksheet_text("FAKE_BLOCK")).await;
    let sample_id = sample["id"].as_i64().unwrap();
    let started = create(
        &app,
        &t,
        json!({
            "kind": "worksheet",
            "material": { "title": "The Umbrella Stand", "text": passage("") },
            "template_material_id": sample_id,
        }),
    )
    .await;
    let id = sid(&started);
    let template_id = started["template"]["id"].as_i64().unwrap();
    let failed = wait(&app, &t, id).await;
    let turn = last(&failed);
    assert_eq!(turn["status"], "failed", "{turn}");
    // The turn's error is the template's error.
    assert_eq!(turn["error"], MSG_UNREADABLE);
    assert_eq!(turn["worksheet"], Value::Null);
    assert_eq!(turn["template"]["status"], "failed");
    // The template stays failed, retryable on its page.
    let (_, tpl) = app
        .get(&format!("/api/worksheet-templates/{template_id}"), Some(&t))
        .await;
    assert_eq!(tpl["status"], "failed");
    assert_eq!(tpl["error"], MSG_UNREADABLE);
    assert_eq!(
        count(
            &app,
            "SELECT COUNT(*) FROM worksheets WHERE deleted_at IS NULL"
        )
        .await,
        0
    );

    // 「再試一次」 (the same kind again) extracts the template again; with a readable sample the
    // turn waits for it and makes the worksheet.
    let (s, v) = app
        .put(
            &format!("/api/materials/{sample_id}"),
            Some(&t),
            json!({ "text": worksheet_text("") }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, v) = app
        .post(
            &format!("/api/prep-sessions/{id}/turns"),
            Some(&t),
            json!({ "kind": "worksheet" }),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    let again = last(&v);
    assert_eq!(again["template"]["status"], "extracting");
    assert_eq!(again["worksheet"], Value::Null);
    let done = wait(&app, &t, id).await;
    let turn = last(&done);
    assert_eq!(turn["status"], "done", "{turn}");
    assert!(turn["worksheet"]["id"].is_i64());
    let tpl = wait_template(&app, &t, template_id).await;
    assert_eq!(tpl["status"], "ready");
    assert_eq!(
        count(&app, "SELECT COUNT(*) FROM worksheet_templates").await,
        1,
        "the same template, extracted again"
    );
    let retried = usage(&app, "worksheet_template_retried").await;
    assert_eq!(retried.len(), 1);
    assert_eq!(retried[0]["run"], 2);
}

#[tokio::test]
async fn a_sample_is_only_for_the_full_worksheet_and_must_be_usable() {
    let app = test_app!();
    let t = app.teacher("T-MIMIC-5", "general").await;
    let sample = paste(&app, &t, "L2 Reading Worksheet", &worksheet_text("")).await;
    for kind in ["comprehension", "tiered", "kit", "activity", "assessment"] {
        let (s, v) = app
            .post(
                "/api/prep-sessions",
                Some(&t),
                json!({
                    "kind": kind,
                    "material": { "text": passage("") },
                    "template_material_id": sample["id"],
                }),
            )
            .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{kind}: {v}");
        assert_eq!(err_message(&v), MSG_SAMPLE_KIND);
    }
    // Someone else's material, a missing one, a link: 400, as for POST /api/worksheet-templates.
    let other = app.teacher("T-MIMIC-6", "general").await;
    let theirs = paste(&app, &other, "Theirs", &worksheet_text("")).await;
    let (_, link) = app
        .post(
            "/api/materials",
            Some(&t),
            json!({ "kind": "link", "url": "https://example.com/worksheet" }),
        )
        .await;
    for (bad, message) in [
        (theirs["id"].clone(), "找不到這份教材。"),
        (json!(999999), "找不到這份教材。"),
        (
            link["id"].clone(),
            "這份教材沒有文字可以讀。請選一份學習單文件（DOCX、PDF 或貼上的文字）。",
        ),
    ] {
        let (s, v) = app
            .post(
                "/api/prep-sessions",
                Some(&t),
                json!({
                    "kind": "worksheet",
                    "material": { "text": passage("") },
                    "template_material_id": bad,
                }),
            )
            .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
        assert_eq!(err_message(&v), message);
    }
    // Nothing was made by the refused requests.
    assert_eq!(count(&app, "SELECT COUNT(*) FROM prep_sessions").await, 0);
    assert_eq!(
        count(&app, "SELECT COUNT(*) FROM worksheet_templates").await,
        0
    );
}

// ---------------------------------------------------------------- detection

/// A storage directory that is removed when the test ends.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let dir =
            std::env::temp_dir().join(format!("manna-mimic-{}", manna_api::auth::new_token()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn configure(&self) -> impl FnOnce(&mut Config) + '_ {
        |c: &mut Config| c.storage = StorageConfig::fs(self.0.clone())
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn materials_say_whether_they_look_like_a_worksheet() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure());
    let t = app.teacher("T-MIMIC-7", "general").await;

    // Paste.
    let ws = paste(&app, &t, "Worksheet", &worksheet_text("")).await;
    assert_eq!(ws["looks_like_worksheet"], true);
    let text = paste(&app, &t, "Story", &passage("")).await;
    assert_eq!(text["looks_like_worksheet"], false);
    // Editing the text computes it again.
    let (s, edited) = app
        .put(
            &format!("/api/materials/{}", text["id"]),
            Some(&t),
            json!({ "text": format!("Name: ____ Class: ____\n\n{}\n\n1. ____\n2. ____", passage("")) }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{edited}");
    assert_eq!(edited["looks_like_worksheet"], true);
    let (_, link) = app
        .post(
            "/api/materials",
            Some(&t),
            json!({ "kind": "link", "url": "https://example.com/a" }),
        )
        .await;
    assert_eq!(link["looks_like_worksheet"], false);

    // Upload: computed from the extracted text.
    let upload = |name: &'static str, data: String| {
        let app = &app;
        let t = &t;
        async move {
            let (s, v) = app
                .post_multipart(
                    "/api/materials/upload",
                    Some(t),
                    &[Part::File("file", name, data.as_bytes())],
                )
                .await;
            assert_eq!(s, StatusCode::OK, "{v}");
            v
        }
    };
    let up_ws = upload(
        "exit-ticket.txt",
        "Exit Ticket\n1. What is the main idea?\n2. Who helped her?\n3. Why did they laugh?\n4. What would you do?\n".to_string(),
    )
    .await;
    assert_eq!(up_ws["looks_like_worksheet"], true);
    let up_text = upload("reading.txt", passage("")).await;
    assert_eq!(up_text["looks_like_worksheet"], false);

    // The list and the detail say it too.
    let (_, library) = app.get("/api/materials", Some(&t)).await;
    let flags: Vec<(i64, bool)> = library["materials"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| {
            (
                m["id"].as_i64().unwrap(),
                m["looks_like_worksheet"].as_bool().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        flags,
        [
            (up_text["id"].as_i64().unwrap(), false),
            (up_ws["id"].as_i64().unwrap(), true),
            (link["id"].as_i64().unwrap(), false),
            (text["id"].as_i64().unwrap(), true),
            (ws["id"].as_i64().unwrap(), true),
        ]
    );
    let (_, one) = app
        .get(&format!("/api/materials/{}", up_ws["id"]), Some(&t))
        .await;
    assert_eq!(one["looks_like_worksheet"], true);

    // A row from before the column existed: computed on its first read and stored then,
    // without touching updated_at.
    sqlx::query("UPDATE materials SET looks_like_worksheet = NULL WHERE id = $1")
        .bind(ws["id"].as_i64().unwrap())
        .execute(&app.db)
        .await
        .unwrap();
    let (_, library) = app.get("/api/materials", Some(&t)).await;
    let row = library["materials"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == ws["id"])
        .unwrap()
        .clone();
    assert_eq!(row["looks_like_worksheet"], true);
    assert_eq!(row["updated_at"], ws["updated_at"]);
    let stored: Option<bool> =
        sqlx::query_scalar("SELECT looks_like_worksheet FROM materials WHERE id = $1")
            .bind(ws["id"].as_i64().unwrap())
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(stored, Some(true));
}

// ---------------------------------------------------------------- the refusal's hint

/// More paragraphs than Manna designs at once: a worksheet (header, blanks, numbered questions)
/// or a plain text.
fn long_worksheet() -> String {
    let questions: Vec<String> = (1..=45)
        .map(|i| format!("{i}. What happens in part {i} of the story?"))
        .collect();
    format!(
        "Name: ______ Class: ______ No.: ______\n\n{}",
        questions.join("\n\n")
    )
}

fn long_passage() -> String {
    (1..=45)
        .map(|i| format!("This is paragraph {i} of a long story about a quiet harbor town."))
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[tokio::test]
async fn a_long_worksheet_like_text_is_no_longer_refused() {
    let app = test_app!();
    let t = app.teacher("T-MIMIC-8", "general").await;

    // 46 paragraphs: no 40-paragraph refusal, whatever the text looks like. (The web app says
    // "looks like a worksheet" before generating, from the material's own flag.)
    let s1 = create(
        &app,
        &t,
        json!({ "kind": "worksheet", "material": { "title": "L3 Worksheet", "text": long_worksheet() } }),
    )
    .await;
    assert!(s1["material"]["id"].is_i64());
    let done = wait(&app, &t, sid(&s1)).await;
    let error = last(&done)["error"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    assert!(!error.contains("超過"), "{error}");

    let s2 = create(
        &app,
        &t,
        json!({ "kind": "worksheet", "material": { "title": "Harbor", "text": long_passage() } }),
    )
    .await;
    let done = wait(&app, &t, sid(&s2)).await;
    let error = last(&done)["error"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    assert!(!error.contains("超過"), "{error}");

    // The worksheet generator starts as well.
    let (s, v) = app
        .post(
            "/api/worksheets",
            Some(&t),
            json!({ "material": { "title": "L3 Worksheet", "text": long_worksheet() } }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    let (s, v) = app
        .post(
            &format!("/api/worksheets/{}/generate", v["id"]),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
}
