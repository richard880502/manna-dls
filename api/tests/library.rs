//! Phase 2.9 (docs/API.md「教材參考、我的教材、學習單範本」): platform materials and templates
//! (read and used by every teacher, written only by ManageKnowledge), personal annotations on
//! platform materials, the template copy, worksheet samples leaving 我的教材 (backfill and on
//! template creation), the browse APIs (filters, facets, cursor), the admin uploads, edits and
//! deletes (text and a DOCX built here), the category suggestion and the 403s.
//!
//! Every text and file is synthetic; the fake model reads the templates.

#[macro_use]
mod common;

use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use axum::http::StatusCode;
use common::Part;
use manna_api::config::Config;
use manna_api::storage::StorageConfig;
use serde_json::{json, Value};

const P: [&str; 4] = [
    "Mrs. Lin put an umbrella stand outside her shop on a rainy Monday. A sign said, “Take one if you need it.”",
    "Her neighbors laughed at the idea. They were sure that every umbrella would be gone within a week.",
    "In the first month, twelve of the thirty umbrellas did disappear. However, something unexpected followed.",
    "Today the stand holds more umbrellas than it did at the start. Mrs. Lin says trust is like an umbrella.",
];

fn passage() -> String {
    P.join("\n\n")
}

/// A past worksheet as the extractor reads it; the fake model takes every `## ` line as a block.
fn worksheet_text(marker: &str) -> String {
    format!(
        "Name: ______ No: ______ Class: ______\n\n\
         ## Warm-up: [主題]\nWhat do you know about umbrellas?\n\n\
         ## Paragraph [n]: Details\n1. What does she put outside? 2. Why?\n\n\
         ## Writing: [主題]\nWrite 80 words about trust. {marker}"
    )
}

// ---------------------------------------------------------------- harness

/// A storage directory that is removed when the test ends.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let dir =
            std::env::temp_dir().join(format!("manna-library-{}", manna_api::auth::new_token()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn files(&self) -> Vec<PathBuf> {
        fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
            for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out);
                } else {
                    out.push(path);
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.0, &mut out);
        out
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

/// A minimal DOCX: a zip with `word/document.xml` holding the given paragraphs.
fn docx(paragraphs: &[&str]) -> Vec<u8> {
    let body: String = paragraphs
        .iter()
        .map(|p| {
            let escaped = p.replace('&', "&amp;").replace('<', "&lt;");
            format!("<w:p><w:r><w:t xml:space=\"preserve\">{escaped}</w:t></w:r></w:p>")
        })
        .collect();
    let document = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">\
         <w:body>{body}</w:body></w:document>"
    );
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file("[Content_Types].xml", options).unwrap();
    zip.write_all(b"<?xml version=\"1.0\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"/>")
        .unwrap();
    zip.start_file("word/document.xml", options).unwrap();
    zip.write_all(document.as_bytes()).unwrap();
    zip.finish().unwrap().into_inner()
}

fn png() -> Vec<u8> {
    let mut data = b"\x89PNG\r\n\x1a\n".to_vec();
    data.extend_from_slice(&[0u8; 64]);
    data
}

/// Percent-encodes a query value (Chinese text cannot go into a URI as it is).
fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

fn err_message(v: &Value) -> &str {
    v["error"]["message"].as_str().unwrap_or_default()
}

fn id(v: &Value) -> i64 {
    v["id"].as_i64().unwrap_or_else(|| panic!("no id in {v}"))
}

fn ids(list: &Value) -> Vec<i64> {
    list.as_array().unwrap().iter().map(id).collect()
}

fn keys(facets: &Value) -> Vec<(String, i64)> {
    facets
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            (
                f["key"].as_str().unwrap().to_string(),
                f["count"].as_i64().unwrap(),
            )
        })
        .collect()
}

fn kv(pairs: &[(&str, i64)]) -> Vec<(String, i64)> {
    pairs.iter().map(|(k, n)| (k.to_string(), *n)).collect()
}

/// A 知識庫管理員 (`kb_manager`) account and its research id.
async fn kb_manager(app: &common::TestApp, research_id: &str) -> String {
    let pi = app.pi_token().await;
    let (s, v) = app
        .post(
            "/api/admin/invites",
            Some(&pi),
            json!({ "research_id": research_id, "school_track": "general", "role": "kb_manager" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    app.login(v["code"].as_str().unwrap()).await.0
}

/// `POST /api/admin/library` with pasted text.
async fn admin_text(
    app: &common::TestApp,
    t: &str,
    title: &str,
    category: &str,
    tags: &str,
    text: &str,
) -> (StatusCode, Value) {
    app.post_multipart(
        "/api/admin/library",
        Some(t),
        &[
            Part::Text("title", title),
            Part::Text("category", category),
            Part::Text("tags", tags),
            Part::Text("source_note", "測試用"),
            Part::Text("text", text),
        ],
    )
    .await
}

async fn admin_text_ok(
    app: &common::TestApp,
    t: &str,
    title: &str,
    category: &str,
    tags: &str,
    text: &str,
) -> Value {
    let (s, v) = admin_text(app, t, title, category, tags, text).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v
}

/// `POST /api/admin/library/templates` with pasted text; the 202 says `extracting`.
async fn admin_template(
    app: &common::TestApp,
    t: &str,
    title: &str,
    category: &str,
    text: &str,
) -> Value {
    let (s, v) = app
        .post_multipart(
            "/api/admin/library/templates",
            Some(t),
            &[
                Part::Text("title", title),
                Part::Text("category", category),
                Part::Text("tags", "三民"),
                Part::Text("text", text),
            ],
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    assert_eq!(v["status"], "extracting", "{v}");
    v
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

async fn browse(app: &common::TestApp, t: &str, path: &str) -> Value {
    let (s, v) = app.get(path, Some(t)).await;
    assert_eq!(s, StatusCode::OK, "{path}: {v}");
    v
}

/// UTF-16 offsets of `quote` in `text`.
fn span(text: &str, quote: &str) -> (usize, usize) {
    let byte = text.find(quote).expect("quote is in the text");
    let start = text[..byte].encode_utf16().count();
    (start, start + quote.encode_utf16().count())
}

// ---------------------------------------------------------------- scope and access

#[tokio::test]
async fn platform_materials_are_read_by_every_teacher_and_written_only_by_managers() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure());
    let kb = kb_manager(&app, "KB-001").await;
    let a = app.teacher("T-LIB-A", "general").await;
    let b = app.teacher("T-LIB-B", "general").await;

    // A platform DOCX uploaded by the 知識庫管理員.
    let file = docx(&[P[0], P[1], P[2]]);
    let (s, card) = app
        .post_multipart(
            "/api/admin/library",
            Some(&kb),
            &[
                Part::Text("category", "學測英文"),
                Part::Text("tags", "114, 試題，114"),
                Part::Text("source_note", "大考中心"),
                Part::File("file", "114 學測英文.docx", &file),
            ],
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{card}");
    let platform = id(&card);
    assert_eq!(card["title"], "114 學測英文");
    assert_eq!(card["scope"], "platform");
    assert_eq!(card["category"], "學測英文");
    assert_eq!(card["tags"], json!(["114", "試題"]));
    assert_eq!(card["kind"], "document");
    assert_eq!(card["file_type"], "docx");
    assert_eq!(card["paragraph_count"], 3);
    assert!(card["word_count"].as_i64().unwrap() > 40, "{card}");
    assert!(
        card["excerpt"]
            .as_str()
            .unwrap()
            .starts_with("Mrs. Lin put an umbrella stand outside her shop"),
        "{card}"
    );
    assert!(card["excerpt"].as_str().unwrap().chars().count() <= 280);
    assert_eq!(card["text_status"], "ready");
    assert_eq!(card["looks_like_worksheet"], false);
    assert_eq!(card["owner_label"], "Manna 知識庫");
    assert_eq!(card["uploaded_by"], "KB-001");
    assert_eq!(card["size_bytes"], file.len() as i64);
    assert!(card["created_at"].is_string());

    // The admin's own library does not hold it (and her quota does not count it).
    let (_, mine) = app.get("/api/materials", Some(&kb)).await;
    assert_eq!(mine["materials"], json!([]));
    assert_eq!(mine["usage"]["count"], 0);
    assert_eq!(mine["usage"]["used_bytes"], 0);

    // Every teacher reads it: detail, the original file, a file link.
    let mine_a = paste(&app, &a, "A's own text", &passage()).await;
    for t in [&a, &b] {
        let (s, v) = app
            .get(&format!("/api/materials/{platform}"), Some(t))
            .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        assert_eq!(v["scope"], "platform");
        assert_eq!(v["purpose"], "material");
        assert_eq!(v["tags"], json!(["114", "試題"]));
        assert_eq!(v["paragraphs"].as_array().unwrap().len(), 3);
        let raw = app
            .send_raw(
                axum::http::Method::GET,
                &format!("/api/materials/{platform}/file"),
                &[("authorization", &format!("Bearer {t}"))],
                Vec::new(),
            )
            .await;
        assert_eq!(raw.status, StatusCode::OK);
        assert_eq!(raw.body, file);
        let (s, link) = app
            .post(
                &format!("/api/materials/{platform}/file-link"),
                Some(t),
                json!({}),
            )
            .await;
        assert_eq!(s, StatusCode::OK, "{link}");
        let by_link = app
            .send_raw(
                axum::http::Method::GET,
                link["url"].as_str().unwrap(),
                &[],
                Vec::new(),
            )
            .await;
        assert_eq!(by_link.status, StatusCode::OK);
    }
    // …but never another teacher's personal material.
    let theirs = id(&mine_a);
    for path in [
        format!("/api/materials/{theirs}"),
        format!("/api/materials/{theirs}/annotations"),
    ] {
        let (s, _) = app.get(&path, Some(&b)).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{path}");
    }
    let (s, _) = app
        .post(
            &format!("/api/materials/{theirs}/file-link"),
            Some(&b),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    // A teacher's own lists never show platform rows.
    let (_, list_b) = app.get("/api/materials", Some(&b)).await;
    assert_eq!(list_b["materials"], json!([]));

    // Teachers cannot change or delete it: 403, through the teacher endpoints and the admin ones.
    let path = format!("/api/materials/{platform}");
    let (s, v) = app
        .put(&path, Some(&b), json!({ "title": "mine now" }))
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN, "{v}");
    let (s, _) = app.delete(&path, Some(&b)).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let admin_path = format!("/api/admin/library/{platform}");
    let (s, _) = app
        .send(
            axum::http::Method::PATCH,
            &admin_path,
            Some(&b),
            Some("application/json"),
            json!({ "title": "x" }).to_string().into_bytes(),
        )
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = app.delete(&admin_path, Some(&b)).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, v) = app.get(&path, Some(&b)).await;
    assert_eq!(
        (s, v["title"].as_str()),
        (StatusCode::OK, Some("114 學測英文"))
    );
    // A manager may, through the teacher endpoint too.
    let (s, v) = app
        .put(&path, Some(&kb), json!({ "title": "114 學測英文（定稿）" }))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["title"], "114 學測英文（定稿）");

    // Annotations stay personal: each teacher sees and shares only her own notes.
    let text = v["paragraphs"][0]["text"].as_str().unwrap().to_string();
    let pid = v["paragraphs"][0]["id"].as_str().unwrap().to_string();
    let (start, end) = span(&text, "umbrella stand");
    let mut notes = Vec::new();
    for (t, note) in [(&a, "A 的筆記"), (&b, "B 的筆記")] {
        let (s, n) = app
            .post(
                &format!("/api/materials/{platform}/annotations"),
                Some(t),
                json!({ "paragraph_id": pid, "start": start, "end": end, "note": note,
                        "share_with_ai": true }),
            )
            .await;
        assert_eq!(s, StatusCode::OK, "{n}");
        notes.push(id(&n));
    }
    for (t, note) in [(&a, "A 的筆記"), (&b, "B 的筆記")] {
        let (_, list) = app
            .get(&format!("/api/materials/{platform}/annotations"), Some(t))
            .await;
        let list = list.as_array().unwrap();
        assert_eq!(list.len(), 1, "{list:?}");
        assert_eq!(list[0]["note"], note);
        let (_, ai) = app
            .get(
                &format!("/api/materials/{platform}/annotations/ai"),
                Some(t),
            )
            .await;
        assert_eq!(ai["notes"].as_array().unwrap().len(), 1, "{ai}");
        assert!(ai["prompt_block"].as_str().unwrap().contains(note));
        let (_, detail) = app
            .get(&format!("/api/materials/{platform}"), Some(t))
            .await;
        assert_eq!(detail["annotation_count"], 1);
    }
    // B cannot touch A's note.
    let a_note = format!("/api/annotations/{}", notes[0]);
    let (s, _) = app.put(&a_note, Some(&b), json!({ "note": "改掉" })).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = app.delete(&a_note, Some(&b)).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, v) = app.put(&a_note, Some(&a), json!({ "hidden": true })).await;
    assert_eq!((s, v["hidden"].as_bool()), (StatusCode::OK, Some(true)));

    // A platform material cannot be linked to a lesson (owner-only, as before).
    let unit = app.create_unit(&b, "Unit").await;
    let lesson = app.create_lesson(&b, unit, "Lesson").await;
    let (s, _) = app
        .put(
            &format!("/api/lessons/{lesson}/materials/{platform}"),
            Some(&b),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // The manager deletes it: the file and every teacher's notes go.
    let before = dir.files().len();
    let (s, v) = app.delete(&admin_path, Some(&kb)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["deleted"], true);
    assert_eq!(v["annotations"], 2);
    assert_eq!(dir.files().len(), before - 1);
    let (s, _) = app.get(&path, Some(&a)).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_admin_endpoints_answer_403_to_everyone_but_managers() {
    let app = test_app!();
    let pi = app.pi_token().await;
    let kb = kb_manager(&app, "KB-002").await;
    let t = app.teacher("T-LIB-403", "general").await;
    let platform = id(&admin_text_ok(&app, &kb, "課綱", "課綱", "", &passage()).await);
    let tpl = id(&admin_template(&app, &kb, "閱讀單", "閱讀理解", &worksheet_text("")).await);
    wait_template(&app, &kb, tpl).await;

    let json_body = |v: Value| Some(v.to_string().into_bytes());
    let calls: Vec<(axum::http::Method, String, Option<Vec<u8>>)> = vec![
        (axum::http::Method::GET, "/api/admin/library".into(), None),
        (
            axum::http::Method::GET,
            "/api/admin/library?kind=templates".into(),
            None,
        ),
        (
            axum::http::Method::PATCH,
            format!("/api/admin/library/{platform}"),
            json_body(json!({ "title": "x" })),
        ),
        (
            axum::http::Method::DELETE,
            format!("/api/admin/library/{platform}"),
            None,
        ),
        (
            axum::http::Method::PATCH,
            format!("/api/admin/library/templates/{tpl}"),
            json_body(json!({ "title": "x" })),
        ),
        (
            axum::http::Method::DELETE,
            format!("/api/admin/library/templates/{tpl}"),
            None,
        ),
        (
            axum::http::Method::POST,
            format!("/api/admin/library/templates/{tpl}/retry"),
            None,
        ),
    ];
    for (method, path, body) in &calls {
        let (s, v) = app
            .send(
                method.clone(),
                path,
                Some(&t),
                body.as_ref().map(|_| "application/json"),
                body.clone().unwrap_or_default(),
            )
            .await;
        assert_eq!(s, StatusCode::FORBIDDEN, "{method} {path}: {v}");
    }
    for path in ["/api/admin/library", "/api/admin/library/templates"] {
        let (s, v) = app
            .post_multipart(
                path,
                Some(&t),
                &[
                    Part::Text("category", "課綱"),
                    Part::Text("text", "Some text."),
                ],
            )
            .await;
        assert_eq!(s, StatusCode::FORBIDDEN, "{path}: {v}");
    }
    // Teachers cannot change, retry or delete a platform template through the teacher endpoints.
    let path = format!("/api/worksheet-templates/{tpl}");
    let (s, v) = app.put(&path, Some(&t), json!({ "title": "x" })).await;
    assert_eq!(s, StatusCode::FORBIDDEN, "{v}");
    assert!(err_message(&v).contains("另存成我的範本"), "{v}");
    let (s, _) = app
        .send(
            axum::http::Method::PATCH,
            &path,
            Some(&t),
            Some("application/json"),
            json!({ "category": "寫作" }).to_string().into_bytes(),
        )
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = app
        .post(&format!("{path}/retry"), Some(&t), json!({}))
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = app.delete(&path, Some(&t)).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, v) = app.get(&path, Some(&t)).await;
    assert_eq!((s, v["scope"].as_str()), (StatusCode::OK, Some("platform")));
    // Without a login: 401.
    let (s, _) = app.get("/api/admin/library", None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _) = app.get("/api/browse/materials", None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    // PI and 知識庫管理員 both may.
    for token in [&pi, &kb] {
        let (s, v) = app.get("/api/admin/library", Some(token)).await;
        assert_eq!(s, StatusCode::OK, "{v}");
        assert_eq!(ids(&v["items"]), [platform]);
    }
}

// ---------------------------------------------------------------- using platform rows

#[tokio::test]
async fn teachers_use_platform_materials_and_templates_in_worksheets_and_prep_sessions() {
    let app = test_app!();
    let kb = kb_manager(&app, "KB-003").await;
    let t = app.teacher("T-LIB-USE", "general").await;
    let other = app.teacher("T-LIB-OTHER", "general").await;
    let reference =
        id(&admin_text_ok(&app, &kb, "The Umbrella Stand", "其他", "", &passage()).await);
    let tpl_card = admin_template(&app, &kb, "逐段閱讀單", "", &worksheet_text("")).await;
    let tpl = id(&tpl_card);
    assert_eq!(tpl_card["scope"], "platform");
    assert_eq!(tpl_card["owner_label"], "Manna 範本");
    let sample = tpl_card["source_material"]["id"].as_i64().unwrap();
    let ready = wait_template(&app, &t, tpl).await;
    assert_eq!(ready["status"], "ready", "{ready}");
    assert_eq!(ready["scope"], "platform");
    // Nobody chose a category: suggested from what was read (課文深度閱讀 → 閱讀理解).
    assert_eq!(ready["category"], "閱讀理解");
    assert_eq!(ready["tags"], json!(["三民"]));

    // A worksheet from the platform material and the platform template.
    let (s, ws) = app
        .post(
            "/api/worksheets",
            Some(&t),
            json!({ "material_id": reference, "template_id": tpl }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{ws}");
    let ws_id = id(&ws);
    assert_eq!(ws["material"]["source"]["kind"], "library");
    assert_eq!(ws["material"]["source"]["material_id"], reference);
    assert_eq!(ws["template"]["id"], tpl);
    let (s, v) = app
        .post(
            &format!("/api/worksheets/{ws_id}/generate"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    let generated = loop {
        let (_, jobs) = app
            .get(&format!("/api/worksheets/{ws_id}/jobs"), Some(&t))
            .await;
        if jobs["running"] == false {
            break app
                .get(&format!("/api/worksheets/{ws_id}"), Some(&t))
                .await
                .1;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    let blocks = generated["content"]["blocks"].as_array().unwrap();
    assert!(
        blocks.iter().any(|b| b["template_section_id"].is_string()),
        "{generated}"
    );

    // The material's counts are the viewer's own.
    let (_, seen) = app
        .get(&format!("/api/materials/{reference}"), Some(&t))
        .await;
    assert_eq!(seen["worksheet_count"], 1);
    let (_, seen) = app
        .get(&format!("/api/materials/{reference}"), Some(&other))
        .await;
    assert_eq!(seen["worksheet_count"], 0);

    // A prep session on the platform material with the platform template.
    let (s, session) = app
        .post(
            "/api/prep-sessions",
            Some(&t),
            json!({ "kind": "worksheet", "material_id": reference, "template_id": tpl }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{session}");
    assert_eq!(session["material"]["id"], reference);
    assert_eq!(session["template"]["id"], tpl);
    let sid = id(&session);
    let done = loop {
        let (_, v) = app
            .get(&format!("/api/prep-sessions/{sid}"), Some(&t))
            .await;
        if v["running"] == false {
            break v;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    let turn = &done["turns"][0];
    assert_eq!(turn["status"], "done", "{done}");
    assert!(turn["worksheet"]["id"].is_i64());
    // 照我的學習單做 with the platform sample: its ready platform template is reused.
    let (s, session) = app
        .post(
            "/api/prep-sessions",
            Some(&t),
            json!({ "kind": "worksheet", "material_id": reference,
                    "template_material_id": sample }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{session}");
    assert_eq!(session["template"]["id"], tpl);
    let templates: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM worksheet_templates")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(templates, 1, "no new extraction");
    let sid = id(&session);
    loop {
        let (_, v) = app
            .get(&format!("/api/prep-sessions/{sid}"), Some(&t))
            .await;
        if v["running"] == false {
            assert_eq!(v["turns"][0]["status"], "done", "{v}");
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    // The platform sample stays a platform sample.
    let (_, s_row) = app.get(&format!("/api/materials/{sample}"), Some(&t)).await;
    assert_eq!(
        (s_row["scope"].as_str(), s_row["purpose"].as_str()),
        (Some("platform"), Some("worksheet_sample"))
    );

    // The admin deletes the platform material: worksheets made from it keep their snapshot.
    let before = &generated["material"];
    let (s, _) = app
        .delete(&format!("/api/admin/library/{reference}"), Some(&kb))
        .await;
    assert_eq!(s, StatusCode::OK);
    let (s, after) = app.get(&format!("/api/worksheets/{ws_id}"), Some(&t)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(&after["material"]["paragraphs"], &before["paragraphs"]);
    assert_eq!(after["content"]["blocks"], generated["content"]["blocks"]);
    // Deleting the sample: the template keeps working without its source.
    let (s, _) = app
        .delete(&format!("/api/admin/library/{sample}"), Some(&kb))
        .await;
    assert_eq!(s, StatusCode::OK);
    let (_, v) = app
        .get(&format!("/api/worksheet-templates/{tpl}"), Some(&t))
        .await;
    assert_eq!(
        (v["status"].as_str(), &v["source_material"]),
        (Some("ready"), &Value::Null)
    );
    let (s, v) = app
        .post(
            "/api/worksheets",
            Some(&t),
            json!({ "template_id": tpl, "material": { "text": passage() } }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    // Deleting the template: the worksheet keeps its content.
    let (s, _) = app
        .delete(&format!("/api/admin/library/templates/{tpl}"), Some(&kb))
        .await;
    assert_eq!(s, StatusCode::OK);
    let (s, after) = app.get(&format!("/api/worksheets/{ws_id}"), Some(&t)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(after["template"], Value::Null);
    assert_eq!(after["content"]["blocks"], generated["content"]["blocks"]);
}

#[tokio::test]
async fn a_platform_template_is_copied_into_my_templates() {
    let app = test_app!();
    let kb = kb_manager(&app, "KB-004").await;
    let t = app.teacher("T-LIB-COPY", "general").await;
    let other = app.teacher("T-LIB-COPY-2", "general").await;
    let tpl = id(&admin_template(&app, &kb, "寫作引導單", "寫作", &worksheet_text("")).await);
    let platform = wait_template(&app, &t, tpl).await;
    assert_eq!(platform["category"], "寫作", "a chosen category stays");

    let (s, copy) = app
        .post(
            &format!("/api/worksheet-templates/{tpl}/copy"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{copy}");
    let copy_id = id(&copy);
    assert_ne!(copy_id, tpl);
    assert_eq!(copy["title"], "寫作引導單（我的版本）");
    assert_eq!(copy["scope"], "personal");
    assert_eq!(copy["status"], "ready");
    assert_eq!(copy["version_no"], 1);
    assert_eq!(copy["content"], platform["content"]);
    assert_eq!(copy["category"], "寫作");
    assert_eq!(copy["tags"], json!(["三民"]));
    assert_eq!(
        copy["source_material"]["id"],
        platform["source_material"]["id"]
    );
    let origins: Vec<String> =
        sqlx::query_scalar("SELECT origin FROM worksheet_template_versions WHERE template_id = $1")
            .bind(copy_id)
            .fetch_all(&app.db)
            .await
            .unwrap();
    assert_eq!(origins, ["teacher"]);

    // The copy is hers: listed, editable; the platform template is unchanged.
    let (_, list) = app.get("/api/worksheet-templates", Some(&t)).await;
    assert_eq!(ids(&list["templates"]), [copy_id]);
    let mut content = copy["content"].clone();
    content["style_notes"] = json!(["我自己的習慣"]);
    let (s, v) = app
        .put(
            &format!("/api/worksheet-templates/{copy_id}"),
            Some(&t),
            json!({ "content": content, "category": "字彙", "tags": ["我的"] }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(
        (v["version_no"].as_i64(), v["category"].as_str()),
        (Some(2), Some("字彙"))
    );
    let (_, again) = app
        .get(&format!("/api/worksheet-templates/{tpl}"), Some(&t))
        .await;
    assert_eq!(again["content"], platform["content"]);
    assert_eq!(again["version_no"], 1);
    let mine = browse(&app, &t, "/api/browse/templates?area=mine").await;
    assert_eq!(ids(&mine["items"]), [copy_id]);
    assert_eq!(mine["items"][0]["owner_label"], "我");

    // Nobody else sees the copy or copies it; a copy of one's own template works too.
    let path = format!("/api/worksheet-templates/{copy_id}");
    let (s, _) = app.get(&path, Some(&other)).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = app
        .post(&format!("{path}/copy"), Some(&other), json!({}))
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, v) = app.post(&format!("{path}/copy"), Some(&t), json!({})).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    assert_eq!(v["title"], "寫作引導單（我的版本）（我的版本）");
    let copy_of_copy = id(&v);

    // Deleting her copies never takes the platform's sample: the platform template keeps it.
    let sample = platform["source_material"]["id"].as_i64().unwrap();
    for (who, template) in [(&t, copy_id), (&t, copy_of_copy), (&kb, tpl)] {
        let (_, v) = app
            .get(&format!("/api/worksheet-templates/{template}"), Some(who))
            .await;
        assert_eq!(v["deletes_source_material"], false, "{v}");
    }
    for template in [copy_id, copy_of_copy] {
        let (s, v) = app
            .delete(&format!("/api/worksheet-templates/{template}"), Some(&t))
            .await;
        assert_eq!((s, v), (StatusCode::OK, json!({ "ok": true })));
    }
    let (s, v) = app.get(&format!("/api/materials/{sample}"), Some(&t)).await;
    assert_eq!(
        (s, v["scope"].as_str(), v["purpose"].as_str()),
        (StatusCode::OK, Some("platform"), Some("worksheet_sample"))
    );
    let (_, v) = app
        .get(&format!("/api/worksheet-templates/{tpl}"), Some(&kb))
        .await;
    assert_eq!(
        (
            v["source_material"]["id"].as_i64(),
            &v["deletes_source_material"]
        ),
        (Some(sample), &json!(true))
    );
    // A teacher cannot delete the platform's sample either.
    let (s, _) = app
        .delete(&format!("/api/materials/{sample}"), Some(&t))
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    // A template that is not ready cannot be copied.
    let failed = id(&admin_template(&app, &kb, "壞掉的", "", &worksheet_text("FAKE_BLOCK")).await);
    let failed_v = wait_template(&app, &t, failed).await;
    assert_eq!(failed_v["status"], "failed");
    let (s, _) = app
        .post(
            &format!("/api/worksheet-templates/{failed}/copy"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
}

// ---------------------------------------------------------------- samples leave 我的教材

#[tokio::test]
async fn materials_with_a_template_become_samples_by_backfill_and_on_creation() {
    // Before 0017: a teacher with a material that a template was made from, and another one.
    let app = match common::TestApp::new_seeded(
        |_c: &mut Config| {},
        Some(16),
        |db: sqlx::PgPool| async move {
            let teacher: i64 = sqlx::query_scalar(
                "INSERT INTO teachers (research_id, school_track, role)
                 VALUES ('T-OLD', 'general', 'teacher') RETURNING id",
            )
            .fetch_one(&db)
            .await
            .unwrap();
            let mut ids = Vec::new();
            for title in ["Old worksheet", "Old reading"] {
                let id: i64 = sqlx::query_scalar(
                    "INSERT INTO materials (teacher_id, kind, origin, title, text_status, text)
                     VALUES ($1, 'text', 'paste', $2, 'ready', 'Some text.') RETURNING id",
                )
                .bind(teacher)
                .bind(title)
                .fetch_one(&db)
                .await
                .unwrap();
                ids.push(id);
            }
            sqlx::query(
                "INSERT INTO worksheet_templates (teacher_id, title, status, source_material_id)
                 VALUES ($1, 'Old template', 'ready', $2)",
            )
            .bind(teacher)
            .bind(ids[0])
            .execute(&db)
            .await
            .unwrap();
        },
    )
    .await
    {
        Some(app) => app,
        None => return,
    };
    let rows: Vec<(String, String, String, String)> =
        sqlx::query_as("SELECT title, scope, purpose, category FROM materials ORDER BY id")
            .fetch_all(&app.db)
            .await
            .unwrap();
    assert_eq!(
        rows,
        [
            (
                "Old worksheet".to_string(),
                "personal".to_string(),
                "worksheet_sample".to_string(),
                String::new()
            ),
            (
                "Old reading".to_string(),
                "personal".to_string(),
                "material".to_string(),
                String::new()
            ),
        ]
    );
    let scope: String = sqlx::query_scalar("SELECT scope FROM worksheet_templates")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(scope, "personal");

    // Now: a template made from a pasted material moves it to the samples.
    let t = app.teacher("T-LIB-MOVE", "general").await;
    let reading = id(&paste(&app, &t, "Reading", &passage()).await);
    let ws = id(&paste(&app, &t, "My worksheet", &worksheet_text("")).await);
    let list = |purpose: &'static str| {
        let app = &app;
        let t = &t;
        async move {
            let (s, v) = app.get(&format!("/api/materials{purpose}"), Some(t)).await;
            assert_eq!(s, StatusCode::OK, "{v}");
            ids(&v["materials"])
        }
    };
    assert_eq!(list("").await, [ws, reading]);
    let (s, v) = app
        .post(
            "/api/worksheet-templates",
            Some(&t),
            json!({ "material_id": ws }),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    assert_eq!(v["status"], "extracting");
    let tpl = wait_template(&app, &t, id(&v)).await;
    assert_eq!(tpl["category"], "閱讀理解", "suggested at extraction");
    assert_eq!(list("").await, [reading]);
    assert_eq!(list("?purpose=material").await, [reading]);
    assert_eq!(list("?purpose=worksheet_sample").await, [ws]);
    let (_, one) = app.get(&format!("/api/materials/{ws}"), Some(&t)).await;
    assert_eq!(one["purpose"], "worksheet_sample");
    let mine = browse(&app, &t, "/api/browse/materials?area=mine").await;
    assert_eq!(ids(&mine["items"]), [reading]);
    let (s, _) = app.get("/api/materials?purpose=sample", Some(&t)).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // Uploads and pastes can say they are samples straight away (the mimic sample upload).
    let (s, sample) = app
        .post(
            "/api/materials",
            Some(&t),
            json!({ "kind": "text", "title": "Sample", "text": worksheet_text(""),
                    "purpose": "worksheet_sample" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{sample}");
    assert_eq!(sample["purpose"], "worksheet_sample");
    assert_eq!(list("").await, [reading]);
    let (s, v) = app
        .post(
            "/api/materials",
            Some(&t),
            json!({ "kind": "text", "text": "x", "purpose": "exam" }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");

    // 照我的學習單做 from a pasted material moves it too.
    let another = id(&paste(&app, &t, "Another worksheet", &worksheet_text("")).await);
    let (s, v) = app
        .post(
            "/api/prep-sessions",
            Some(&t),
            json!({ "kind": "worksheet", "material_id": reading,
                    "template_material_id": another }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    assert_eq!(list("").await, [reading]);
    let samples = list("?purpose=worksheet_sample").await;
    assert!(samples.contains(&another) && samples.contains(&id(&sample)));
    // Waiting for the session's extraction keeps the test from ending mid-job.
    let sid = id(&v);
    for _ in 0..400 {
        let (_, v) = app
            .get(&format!("/api/prep-sessions/{sid}"), Some(&t))
            .await;
        if v["running"] == false {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

// ---------------------------------------------------------------- browsing

#[tokio::test]
async fn browsing_filters_counts_facets_and_pages_with_a_cursor() {
    let app = test_app!();
    let kb = kb_manager(&app, "KB-005").await;
    let t = app.teacher("T-LIB-BROWSE", "general").await;
    let long: String = (1..=45)
        .map(|i| format!("Line {i} of the curriculum."))
        .collect::<Vec<_>>()
        .join("\n\n");
    let m114 = id(&admin_text_ok(
        &app,
        &kb,
        "114 學測英文",
        "學測英文",
        "114, 試題",
        &passage(),
    )
    .await);
    let m113 = id(&admin_text_ok(
        &app,
        &kb,
        "113 學測英文",
        "學測英文",
        "113, 試題",
        "A different reading about bicycles.\n\nSecond paragraph.",
    )
    .await);
    let tong = id(&admin_text_ok(
        &app,
        &kb,
        "114 統測英文",
        "統測英文",
        "114",
        "Trains run on time.",
    )
    .await);
    let kegang = id(&admin_text_ok(&app, &kb, "英語文課綱", "課綱", "", &long).await);
    // A platform sample is never a 教材參考 row.
    let tpl = id(&admin_template(&app, &kb, "字彙學習單", "字彙", &worksheet_text("")).await);
    let own = id(&paste(&app, &t, "My umbrella notes", &passage()).await);

    let v = browse(&app, &t, "/api/browse/materials?area=reference").await;
    assert_eq!(v["total"], 4);
    assert_eq!(ids(&v["items"]), [kegang, tong, m113, m114], "newest first");
    assert_eq!(v["next_cursor"], Value::Null);
    assert_eq!(
        keys(&v["facets"]["categories"]),
        kv(&[("學測英文", 2), ("統測英文", 1), ("課綱", 1)])
    );
    assert_eq!(
        keys(&v["facets"]["tags"]),
        kv(&[("114", 2), ("試題", 2), ("113", 1)])
    );
    assert_eq!(keys(&v["facets"]["file_types"]), kv(&[("text", 4)]));
    let first = &v["items"][3];
    assert_eq!(first["owner_label"], "Manna 知識庫");
    assert_eq!(first["file_type"], "text");
    assert_eq!(first["kind"], "text");
    assert_eq!(first["paragraph_count"], 4);
    assert_eq!(first["page_count"], Value::Null);
    assert!(first.get("uploaded_by").is_none(), "admin fields stay out");
    assert!(first.get("size_bytes").is_none());

    // Filters narrow the rows; the facets still count over area + search.
    let v = browse(
        &app,
        &t,
        &format!(
            "/api/browse/materials?area=reference&category={}",
            enc("學測英文")
        ),
    )
    .await;
    assert_eq!(
        (v["total"].as_i64(), ids(&v["items"])),
        (Some(2), vec![m113, m114])
    );
    assert_eq!(keys(&v["facets"]["categories"]).len(), 3);
    let v = browse(
        &app,
        &t,
        "/api/browse/materials?tag=114&tag=%E8%A9%A6%E9%A1%8C",
    )
    .await;
    assert_eq!(ids(&v["items"]), [m114], "every tag must match");
    let v = browse(&app, &t, "/api/browse/materials?tag=114").await;
    assert_eq!(ids(&v["items"]), [tong, m114]);
    let v = browse(&app, &t, "/api/browse/materials?fits_lesson=true").await;
    assert_eq!(
        ids(&v["items"]),
        [kegang, tong, m113, m114],
        "any material with text counts, however many paragraphs"
    );
    let v = browse(&app, &t, "/api/browse/materials?file_type=pdf").await;
    assert_eq!(
        (v["total"].as_i64(), v["items"].as_array().unwrap().len()),
        (Some(0), 0)
    );
    // The search: title, tags and text, case-insensitive; facets follow it.
    let v = browse(&app, &t, "/api/browse/materials?q=UMBRELLA").await;
    assert_eq!(ids(&v["items"]), [m114]);
    assert_eq!(keys(&v["facets"]["categories"]), kv(&[("學測英文", 1)]));
    assert_eq!(keys(&v["facets"]["tags"]), kv(&[("114", 1), ("試題", 1)]));
    let v = browse(&app, &t, "/api/browse/materials?q=113").await;
    assert_eq!(ids(&v["items"]), [m113]);
    let v = browse(&app, &t, "/api/browse/materials?q=%E7%B5%B1%E6%B8%AC").await;
    assert_eq!(ids(&v["items"]), [tong], "統測 in the title");

    // Pages: a stable keyset cursor, newest first and by title.
    let mut seen = Vec::new();
    let mut path = "/api/browse/materials?limit=3".to_string();
    loop {
        let v = browse(&app, &t, &path).await;
        assert_eq!(v["total"], 4);
        seen.extend(ids(&v["items"]));
        match v["next_cursor"].as_str() {
            Some(c) => path = format!("/api/browse/materials?limit=3&cursor={c}"),
            None => break,
        }
    }
    assert_eq!(seen, [kegang, tong, m113, m114]);
    let v = browse(&app, &t, "/api/browse/materials?sort=title&limit=2").await;
    assert_eq!(ids(&v["items"]), [m113, m114]);
    let cursor = v["next_cursor"].as_str().unwrap().to_string();
    let v = browse(
        &app,
        &t,
        &format!("/api/browse/materials?sort=title&limit=2&cursor={cursor}"),
    )
    .await;
    assert_eq!(ids(&v["items"]), [tong, kegang]);
    assert_eq!(v["next_cursor"], Value::Null);
    // A cursor of one sort is refused by the other; bad values are 400.
    for bad in [
        format!("/api/browse/materials?cursor={cursor}"),
        "/api/browse/materials?area=everything".to_string(),
        "/api/browse/materials?file_type=doc".to_string(),
        "/api/browse/materials?limit=many".to_string(),
        format!("/api/browse/templates?lang={}", enc("日文")),
        "/api/browse/templates?area=reference".to_string(),
    ] {
        let (s, v) = app.get(&bad, Some(&t)).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{bad}: {v}");
    }

    // 我的教材: only the teacher's own.
    let v = browse(&app, &t, "/api/browse/materials?area=mine").await;
    assert_eq!(ids(&v["items"]), [own]);
    assert_eq!(v["items"][0]["owner_label"], "我");
    assert_eq!(v["items"][0]["scope"], "personal");
    assert_eq!(v["facets"]["categories"], json!([]));
    let v = browse(&app, &kb, "/api/browse/materials?area=mine").await;
    assert_eq!(v["total"], 0, "the admin's uploads are not hers");

    // Templates: 平台範本 shows ready ones only; 我的範本 shows every status.
    let platform_failed =
        id(&admin_template(&app, &kb, "壞掉的範本", "", &worksheet_text("FAKE_BLOCK")).await);
    wait_template(&app, &kb, tpl).await;
    wait_template(&app, &kb, platform_failed).await;
    let mine_ws = id(&paste(&app, &t, "My worksheet", &worksheet_text("")).await);
    let (s, v) = app
        .post(
            "/api/worksheet-templates",
            Some(&t),
            json!({ "material_id": mine_ws, "category": "寫作", "tags": ["三民"] }),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    let mine_tpl = id(&v);
    assert_eq!(
        (v["category"].as_str(), v["tags"].clone()),
        (Some("寫作"), json!(["三民"]))
    );
    wait_template(&app, &t, mine_tpl).await;

    let v = browse(&app, &t, "/api/browse/templates").await;
    assert_eq!(ids(&v["items"]), [tpl]);
    let card = &v["items"][0];
    assert_eq!(card["scope"], "platform");
    assert_eq!(card["status"], "ready");
    assert_eq!(card["category"], "字彙");
    assert_eq!(card["tags"], json!(["三民"]));
    assert_eq!(card["worksheet_kind"], "課文深度閱讀");
    assert_eq!(card["instruction_language"], "英文");
    assert_eq!(card["section_count"], 3);
    assert_eq!(
        card["section_titles"],
        json!([
            "Warm-up: [主題]",
            "Paragraph [n]: Details",
            "Writing: [主題]"
        ])
    );
    assert_eq!(card["source_material"]["file_type"], "text");
    assert_eq!(card["source_material"]["page_count"], Value::Null);
    assert_eq!(card["owner_label"], "Manna 範本");
    assert!(card.get("uploaded_by").is_none());
    assert_eq!(keys(&v["facets"]["langs"]), kv(&[("英文", 1)]));
    assert_eq!(keys(&v["facets"]["categories"]), kv(&[("字彙", 1)]));
    let v = browse(&app, &t, "/api/browse/templates?lang=%E4%B8%AD%E6%96%87").await;
    assert_eq!(v["total"], 0);
    let v = browse(&app, &t, "/api/browse/templates?q=%E5%AD%97%E5%BD%99").await;
    assert_eq!(ids(&v["items"]), [tpl], "字彙 in the title");
    let v = browse(&app, &t, "/api/browse/templates?area=mine").await;
    assert_eq!(ids(&v["items"]), [mine_tpl]);
    assert_eq!(v["items"][0]["category"], "寫作", "a chosen category stays");
    assert_eq!(v["items"][0]["source_material"]["id"], mine_ws);

    // The admin list: every platform template, with who uploaded it and why it failed.
    let v = browse(&app, &kb, "/api/admin/library?kind=templates").await;
    assert_eq!(ids(&v["items"]), [platform_failed, tpl]);
    assert_eq!(v["items"][0]["status"], "failed");
    assert!(v["items"][0]["error"].is_string());
    assert_eq!(v["items"][1]["error"], Value::Null);
    assert_eq!(v["items"][1]["uploaded_by"], "KB-005");
    assert!(v["items"][1]["size_bytes"].as_i64().unwrap() > 0);
    let v = browse(
        &app,
        &kb,
        &format!(
            "/api/admin/library?kind=materials&category={}&q=114",
            enc("學測英文")
        ),
    )
    .await;
    assert_eq!(ids(&v["items"]), [m114]);
    assert_eq!(v["items"][0]["uploaded_by"], "KB-005");
    let (s, _) = app.get("/api/admin/library?kind=lessons", Some(&kb)).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------- admin uploads, edits, deletes

#[tokio::test]
async fn admins_upload_edit_and_delete_platform_materials_and_templates() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure());
    let kb = kb_manager(&app, "KB-006").await;
    let t = app.teacher("T-LIB-ADMIN", "general").await;

    // Validation: category required (教材參考 list), tags bounded, a file or text.
    let thirteen: String = (0..13).map(|i| format!("t{i},")).collect();
    let too_long = "字".repeat(21);
    for (category, tags, text, needle) in [
        ("", "", "Some text.", "請選擇分類"),
        ("字彙", "", "Some text.", "不是分類的選項"),
        ("課綱", thirteen.as_str(), "Some text.", "標籤最多 12 個"),
        ("課綱", too_long.as_str(), "Some text.", "太長了"),
        ("課綱", "", "  ", "請選擇要上傳的檔案"),
    ] {
        let (s, v) = admin_text(&app, &kb, "", category, tags, text).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
        assert!(err_message(&v).contains(needle), "{needle}: {v}");
    }

    // A DOCX, then the same file again (409), then the text path.
    let file = docx(&[
        "Unit 1 Vocabulary",
        "apple banana cherry",
        "dog elephant fox",
    ]);
    let (s, card) = app
        .post_multipart(
            "/api/admin/library",
            Some(&kb),
            &[
                Part::Text("title", "第一冊詞彙"),
                Part::Text("category", "詞彙表"),
                Part::File("file", "vocab.docx", &file),
            ],
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{card}");
    let vocab = id(&card);
    assert_eq!(
        (card["title"].as_str(), card["file_type"].as_str()),
        (Some("第一冊詞彙"), Some("docx"))
    );
    assert_eq!(dir.files().len(), 1);
    assert!(dir.files()[0].to_string_lossy().contains("platform"));
    let (s, v) = app
        .post_multipart(
            "/api/admin/library",
            Some(&kb),
            &[
                Part::Text("category", "詞彙表"),
                Part::File("file", "again.docx", &file),
            ],
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    assert!(err_message(&v).contains("平台教材庫"), "{v}");
    // A teacher may still upload the same file to her own library.
    let (s, v) = app
        .post_multipart(
            "/api/materials/upload",
            Some(&t),
            &[Part::File("file", "vocab.docx", &file)],
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let text_card =
        admin_text_ok(&app, &kb, "", "考試說明", "", "Exam notes.\n\nSecond part.").await;
    assert_eq!(
        text_card["title"], "Exam notes.",
        "named after its first line"
    );

    // Edit: title, category, tags (a list or a comma string).
    let path = format!("/api/admin/library/{vocab}");
    let patch = |body: Value| {
        let app = &app;
        let kb = &kb;
        let path = path.clone();
        async move {
            app.send(
                axum::http::Method::PATCH,
                &path,
                Some(kb),
                Some("application/json"),
                body.to_string().into_bytes(),
            )
            .await
        }
    };
    let (s, v) =
        patch(json!({ "title": " 第一冊詞彙表 ", "category": "其他", "tags": "高一, 三民" })).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["title"], "第一冊詞彙表");
    assert_eq!(v["category"], "其他");
    assert_eq!(v["tags"], json!(["高一", "三民"]));
    let (s, v) = patch(json!({ "tags": ["高二"] })).await;
    assert_eq!(
        (s, v["tags"].clone(), v["title"].as_str()),
        (StatusCode::OK, json!(["高二"]), Some("第一冊詞彙表"))
    );
    for bad in [
        json!({ "category": "寫作" }),
        json!({ "title": "  " }),
        json!({ "tags": (0..13).map(|i| i.to_string()).collect::<Vec<_>>() }),
    ] {
        let (s, v) = patch(bad.clone()).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{bad}: {v}");
    }
    // A personal material is not reachable through the admin endpoints.
    let (_, personal) = app.get("/api/materials", Some(&t)).await;
    let personal = personal["materials"][0]["id"].as_i64().unwrap();
    let (s, _) = app
        .delete(&format!("/api/admin/library/{personal}"), Some(&kb))
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // A template from a DOCX sample: 202 extracting, then ready, with the file to look at.
    let sample = docx(&[
        "Name: ______ Class: ______",
        "## Vocabulary: [主題]",
        "1. ______ 2. ______",
        "## Reading: [主題]",
        "What is the main idea?",
    ]);
    let (s, card) = app
        .post_multipart(
            "/api/admin/library/templates",
            Some(&kb),
            &[
                Part::Text("title", "龍騰 字彙單"),
                Part::Text("tags", "龍騰"),
                Part::File("file", "vocab-sheet.docx", &sample),
            ],
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{card}");
    assert_eq!(card["status"], "extracting");
    assert_eq!(card["uploaded_by"], "KB-006");
    assert_eq!(card["size_bytes"], sample.len() as i64);
    assert_eq!(card["source_material"]["file_type"], "docx");
    let tpl = id(&card);
    let sample_id = card["source_material"]["id"].as_i64().unwrap();
    let ready = wait_template(&app, &t, tpl).await;
    assert_eq!(ready["status"], "ready", "{ready}");
    assert_eq!(
        ready["category"], "字彙",
        "suggested from the title 「龍騰 字彙單」 before the extracted kind 課文深度閱讀"
    );
    // Teachers read the sample's original file.
    let raw = app
        .send_raw(
            axum::http::Method::GET,
            &format!("/api/materials/{sample_id}/file"),
            &[("authorization", &format!("Bearer {t}"))],
            Vec::new(),
        )
        .await;
    assert_eq!((raw.status, raw.body.len()), (StatusCode::OK, sample.len()));
    // The sample is not a 教材參考 row nor in the admin's material list.
    let v = browse(&app, &kb, "/api/admin/library?kind=materials").await;
    let listed: HashSet<i64> = ids(&v["items"]).into_iter().collect();
    assert!(
        listed.contains(&vocab) && !listed.contains(&sample_id),
        "{v}"
    );

    // Edit the template: title, tags, category; a blank category is suggested again.
    let tpath = format!("/api/admin/library/templates/{tpl}");
    let tpatch = |body: Value| {
        let app = &app;
        let kb = &kb;
        let tpath = tpath.clone();
        async move {
            app.send(
                axum::http::Method::PATCH,
                &tpath,
                Some(kb),
                Some("application/json"),
                body.to_string().into_bytes(),
            )
            .await
        }
    };
    let (s, v) =
        tpatch(json!({ "title": "字彙單 A", "category": "字彙", "tags": ["龍騰", "高一"] })).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(
        (
            v["title"].as_str(),
            v["category"].as_str(),
            v["tags"].clone()
        ),
        (Some("字彙單 A"), Some("字彙"), json!(["龍騰", "高一"]))
    );
    // Clearing the category suggests again, from the title 「字彙單 A」 first.
    let (s, v) = tpatch(json!({ "category": "" })).await;
    assert_eq!((s, v["category"].as_str()), (StatusCode::OK, Some("字彙")));
    let (s, v) = tpatch(json!({ "category": "學測英文" })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    let (s, _) = app
        .post(&format!("{tpath}/retry"), Some(&kb), json!({}))
        .await;
    assert_eq!(s, StatusCode::CONFLICT, "only failed templates are retried");

    // A sample without text is refused before anything is stored.
    let files_before = dir.files().len();
    let rows_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM materials")
        .fetch_one(&app.db)
        .await
        .unwrap();
    let (s, v) = app
        .post_multipart(
            "/api/admin/library/templates",
            Some(&kb),
            &[Part::File("file", "photo.png", &png())],
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    assert!(err_message(&v).contains("沒有文字"), "{v}");
    let (s, v) = app
        .post_multipart(
            "/api/admin/library/templates",
            Some(&kb),
            &[Part::Text("text", &"word ".repeat(7000))],
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    let rows_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM materials")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!((dir.files().len(), rows_after), (files_before, rows_before));

    // A failed platform template is retried by the admin: 202 says extracting.
    let failed = admin_template(&app, &kb, "壞掉的", "", &worksheet_text("FAKE_BLOCK")).await;
    let failed = id(&failed);
    assert_eq!(wait_template(&app, &kb, failed).await["status"], "failed");
    let (s, v) = app
        .post(
            &format!("/api/admin/library/templates/{failed}/retry"),
            Some(&kb),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    assert_eq!(v["status"], "extracting");
    assert_eq!(wait_template(&app, &kb, failed).await["status"], "failed");

    // Delete the template: its sample (and the sample's file) go with it.
    let files_before = dir.files().len();
    let (s, v) = app.delete(&tpath, Some(&kb)).await;
    assert_eq!((s, v), (StatusCode::OK, json!({ "ok": true })));
    assert_eq!(dir.files().len(), files_before - 1);
    let (s, _) = app
        .get(&format!("/api/materials/{sample_id}"), Some(&t))
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = app
        .get(&format!("/api/worksheet-templates/{tpl}"), Some(&t))
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    // Delete a material: the file goes.
    let (s, v) = app.delete(&path, Some(&kb)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(dir.files().len(), files_before - 2);
    let (s, _) = app.get(&format!("/api/materials/{vocab}"), Some(&t)).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_platform_library_has_its_own_storage_cap() {
    let dir = TempDir::new();
    let app = test_app!(|c: &mut Config| {
        c.storage = StorageConfig::fs(dir.0.clone());
        c.platform_library_bytes = 200;
    });
    let kb = kb_manager(&app, "KB-007").await;
    let file = docx(&["A paragraph that makes the file bigger than a thousand bytes."]);
    assert!(file.len() > 200);
    let (s, v) = app
        .post_multipart(
            "/api/admin/library",
            Some(&kb),
            &[
                Part::Text("category", "其他"),
                Part::File("file", "big.docx", &file),
            ],
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    assert!(err_message(&v).contains("平台教材庫的空間不夠"), "{v}");
    assert!(dir.files().is_empty());
    // The admin's personal quota is separate.
    let (s, v) = app
        .post_multipart(
            "/api/materials/upload",
            Some(&kb),
            &[Part::File("file", "big.docx", &file)],
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
}

#[tokio::test]
async fn teachers_set_category_and_tags_on_their_own_templates() {
    let app = test_app!();
    let t = app.teacher("T-LIB-CAT", "general").await;
    let m = id(&paste(&app, &t, "Grammar worksheet", &worksheet_text("")).await);
    let (s, v) = app
        .post(
            "/api/worksheet-templates",
            Some(&t),
            json!({ "material_id": m }),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    assert_eq!(v["category"], "", "nothing chosen yet");
    let tid = id(&v);
    let path = format!("/api/worksheet-templates/{tid}");
    // Category and tags may be set while it is still being read.
    let (s, v) = app
        .send(
            axum::http::Method::PATCH,
            &path,
            Some(&t),
            Some("application/json"),
            json!({ "category": "文法句型", "tags": "高二, 三民" })
                .to_string()
                .into_bytes(),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let ready = wait_template(&app, &t, tid).await;
    assert_eq!(ready["category"], "文法句型", "a chosen category is kept");
    assert_eq!(ready["tags"], json!(["高二", "三民"]));
    for bad in [
        json!({ "category": "學測英文" }),
        json!({ "tags": ["字".repeat(21)] }),
    ] {
        let (s, v) = app.put(&path, Some(&t), bad.clone()).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{bad}: {v}");
    }
    let (s, v) = app.put(&path, Some(&t), json!({ "category": "" })).await;
    assert_eq!((s, v["category"].as_str()), (StatusCode::OK, Some("")));
    let (s, v) = app
        .post(
            "/api/worksheet-templates",
            Some(&t),
            json!({ "material_id": m, "category": "電影" }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
}
