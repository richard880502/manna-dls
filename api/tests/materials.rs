//! Phase 2.2 (docs/API.md「教材與注記」): the material library on the filesystem backend:
//! upload rules, text extraction, ownership, the hard delete, the lesson SOP on top of the
//! library (including the data migration), annotations, and `STORAGE_BACKEND=none`.

#[macro_use]
mod common;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use axum::http::{Method, StatusCode};
use common::Part;
use manna_api::config::Config;
use manna_api::storage::StorageConfig;
use serde_json::{json, Value};

const P1: &str = "Mrs. Lin put an umbrella stand outside her shop on a rainy Monday. A sign said, “Take one if you need it.”";
const P2: &str = "Her neighbors laughed at the idea. They were sure that every umbrella would be gone within a week.";
const P3: &str = "In the first month, twelve of the thirty umbrellas did disappear. However, something unexpected followed.";

fn passage() -> String {
    format!("{P1}\n\n{P2}\n\n{P3}")
}

// ---------------------------------------------------------------- harness

/// A storage directory that is removed when the test ends.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let dir =
            std::env::temp_dir().join(format!("manna-materials-{}", manna_api::auth::new_token()));
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

fn err_code(v: &Value) -> &str {
    v["error"]["code"].as_str().unwrap_or("<none>")
}

fn err_message(v: &Value) -> &str {
    v["error"]["message"].as_str().unwrap_or_default()
}

async fn upload(
    app: &common::TestApp,
    t: &str,
    filename: &str,
    data: &[u8],
) -> (StatusCode, Value) {
    app.post_multipart(
        "/api/materials/upload",
        Some(t),
        &[
            Part::Text("source_note", "測試用"),
            Part::File("file", filename, data),
        ],
    )
    .await
}

async fn upload_ok(app: &common::TestApp, t: &str, filename: &str, data: &[u8]) -> Value {
    let (s, v) = upload(app, t, filename, data).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v
}

async fn paste(app: &common::TestApp, t: &str, title: &str, text: &str) -> Value {
    let (s, v) = app
        .post(
            "/api/materials",
            Some(t),
            json!({ "kind": "text", "title": title, "source_note": "課本第三課", "text": text }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v
}

async fn lesson(app: &common::TestApp, t: &str) -> i64 {
    let unit = app.create_unit(t, "L3 The Umbrella Stand").await;
    app.create_lesson(t, unit, "第 2 節 精讀與提問").await
}

async fn pick(app: &common::TestApp, t: &str, lesson: i64, material: Value) -> Value {
    let (s, v) = app
        .put(
            &format!("/api/lessons/{lesson}/sop/material"),
            Some(t),
            json!({ "material_id": material }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v
}

async fn annotate(app: &common::TestApp, t: &str, material: i64, body: Value) -> Value {
    let (s, v) = app
        .post(
            &format!("/api/materials/{material}/annotations"),
            Some(t),
            body,
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v
}

/// UTF-16 offsets of `quote` in `text` (what the browser would send).
fn span(text: &str, quote: &str) -> (usize, usize) {
    let byte = text.find(quote).expect("quote is in the text");
    let start = text[..byte].encode_utf16().count();
    (start, start + quote.encode_utf16().count())
}

// ---------------------------------------------------------------- synthetic documents

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

/// A minimal text PDF: one page per entry, each a list of paragraphs, each a list of lines
/// (Helvetica 12pt; a blank line's worth of space between paragraphs). An empty page has no
/// text operators at all, like a scanned page.
fn pdf(pages: &[Vec<Vec<&str>>]) -> Vec<u8> {
    let mut objects: Vec<String> = Vec::new();
    let page_count = pages.len();
    // 1: catalog, 2: pages, 3: font, then (page, content) pairs.
    let kids: Vec<String> = (0..page_count)
        .map(|i| format!("{} 0 R", 4 + i * 2))
        .collect();
    objects.push("<< /Type /Catalog /Pages 2 0 R >>".into());
    objects.push(format!(
        "<< /Type /Pages /Kids [{}] /Count {page_count} >>",
        kids.join(" ")
    ));
    objects.push(
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".into(),
    );
    for (i, page) in pages.iter().enumerate() {
        let mut stream = String::new();
        if !page.is_empty() {
            stream.push_str("BT\n/F1 12 Tf\n14 TL\n72 720 Td\n");
            for (n, paragraph) in page.iter().enumerate() {
                if n > 0 {
                    stream.push_str("0 -28 Td\n");
                }
                for (k, line) in paragraph.iter().enumerate() {
                    let escaped = line
                        .replace('\\', "\\\\")
                        .replace('(', "\\(")
                        .replace(')', "\\)");
                    if k > 0 {
                        stream.push_str("T*\n");
                    }
                    stream.push_str(&format!("({escaped}) Tj\n"));
                }
            }
            stream.push_str("ET\n");
        }
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
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

fn png(extra: usize) -> Vec<u8> {
    let mut data = b"\x89PNG\r\n\x1a\n".to_vec();
    data.extend(std::iter::repeat_n(0u8, extra));
    data
}

fn paragraph_texts(v: &Value) -> Vec<&str> {
    v["paragraphs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["text"].as_str().unwrap())
        .collect()
}

// ---------------------------------------------------------------- pasted text, links, ownership

#[tokio::test]
async fn pasted_text_and_links_live_in_the_teachers_own_library() {
    let app = test_app!();
    let t = app.teacher("T-MAT-01", "general").await;
    let other = app.teacher("T-MAT-02", "general").await;

    let (s, me) = app.get("/api/me", Some(&t)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(me["capabilities"], json!(["use_materials"]));

    let m = paste(&app, &t, "The Umbrella Stand", &passage()).await;
    let id = m["id"].as_i64().unwrap();
    assert_eq!(
        (m["kind"].as_str(), m["origin"].as_str()),
        (Some("text"), Some("paste"))
    );
    assert_eq!(m["text_status"], "ready");
    assert_eq!(m["has_file"], false);
    assert_eq!(m["revision"], 1);
    assert_eq!(m["paragraph_count"], 3);
    assert_eq!(
        m["paragraphs"][0],
        json!({ "id": "p1", "no": 1, "text": P1 })
    );
    // SHA-256 and size of the pasted text are recorded.
    assert_eq!(m["sha256"], manna_api::auth::sha256_hex(passage()));
    assert_eq!(m["size_bytes"], passage().len());
    assert_eq!(m["source_note"], "課本第三課");

    // A link: only the address, a title and a note. Nothing is fetched.
    let (s, link) = app
        .post(
            "/api/materials",
            Some(&t),
            json!({ "kind": "link", "title": "TED talk", "url": "https://example.org/talks/trust?lang=en" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{link}");
    assert_eq!(
        (link["kind"].as_str(), link["text_status"].as_str()),
        (Some("link"), Some("not_applicable"))
    );
    assert_eq!(link["url"], "https://example.org/talks/trust?lang=en");
    for bad in [
        json!({ "kind": "link", "url": "javascript:alert(1)" }),
        json!({ "kind": "link", "url": "example.org" }),
        json!({ "kind": "text", "text": "  " }),
        json!({ "kind": "text", "text": "字".repeat(50_001) }),
        json!({ "kind": "video", "text": "x" }),
        json!({ "kind": "text", "text": "ok", "title": "題".repeat(301) }),
    ] {
        let (s, v) = app.post("/api/materials", Some(&t), bad.clone()).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{bad} → {v}");
    }

    // The library lists both, newest first, with usage, limits and whether uploads work.
    let (s, library) = app.get("/api/materials", Some(&t)).await;
    assert_eq!(s, StatusCode::OK, "{library}");
    let titles: Vec<&str> = library["materials"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["TED talk", "The Umbrella Stand"]);
    assert!(
        library["materials"][1].get("text").is_none(),
        "the list carries no text"
    );
    assert_eq!(library["usage"]["count"], 2);
    assert_eq!(
        library["usage"]["used_bytes"], 0,
        "pasted text does not count against the file quota"
    );
    assert_eq!(library["usage"]["quota_bytes"], 1024 * 1024 * 1024);
    assert_eq!(library["uploads_enabled"], false);
    assert_eq!(library["storage"], "none");
    let pdf_rule = library["limits"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["ext"] == "pdf")
        .unwrap();
    assert_eq!(pdf_rule["max_bytes"], 20 * 1024 * 1024);

    // Renaming, and editing the text in place: unchanged paragraphs keep their ids.
    let edited = format!("A new opening line.\n\n{P2}\n\n{P3}");
    let (s, v) = app
        .put(
            &format!("/api/materials/{id}"),
            Some(&t),
            json!({ "title": "L3 Umbrella", "text": edited }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["title"], "L3 Umbrella");
    assert_eq!(v["revision"], 2);
    let ids: Vec<&str> = v["paragraphs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["p4", "p2", "p3"]);
    // The earlier text is not kept anywhere.
    let stored: String = sqlx::query_scalar("SELECT text FROM materials WHERE id = $1")
        .bind(id)
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert!(!stored.contains("rainy Monday"));
    // A link has no text to edit.
    let (s, v) = app
        .put(
            &format!("/api/materials/{}", link["id"]),
            Some(&t),
            json!({ "text": "x" }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");

    // Nobody else reaches any of it: every path answers 404, the list is empty.
    let (s, theirs) = app.get("/api/materials", Some(&other)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(theirs["materials"], json!([]));
    for (method, path, body) in [
        (Method::GET, format!("/api/materials/{id}"), None),
        (
            Method::PUT,
            format!("/api/materials/{id}"),
            Some(json!({ "title": "mine now" })),
        ),
        (Method::DELETE, format!("/api/materials/{id}"), None),
        (
            Method::POST,
            format!("/api/materials/{id}/file-link"),
            Some(json!({})),
        ),
        (Method::GET, format!("/api/materials/{id}/file"), None),
        (
            Method::GET,
            format!("/api/materials/{id}/annotations"),
            None,
        ),
        (
            Method::GET,
            format!("/api/materials/{id}/annotations/ai"),
            None,
        ),
        (
            Method::POST,
            format!("/api/materials/{id}/annotations"),
            Some(json!({ "paragraph_id": "p2", "start": 0, "end": 3 })),
        ),
    ] {
        let (s, v) = app
            .send(
                method.clone(),
                &path,
                Some(&other),
                body.as_ref().map(|_| "application/json"),
                body.map(|b| b.to_string().into_bytes()).unwrap_or_default(),
            )
            .await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{method} {path} → {v}");
    }
    // …and their own lesson cannot use it either.
    let their_lesson = lesson(&app, &other).await;
    let (s, v) = app
        .put(
            &format!("/api/lessons/{their_lesson}/sop/material"),
            Some(&other),
            json!({ "material_id": id }),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND, "{v}");
    let (s, _) = app
        .put(
            &format!("/api/lessons/{their_lesson}/materials/{id}"),
            Some(&other),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    // Not signed in: 401.
    let (s, _) = app.get("/api/materials", None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _) = app.get(&format!("/api/materials/{id}/file"), None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

// ---------------------------------------------------------------- storage `none`

#[tokio::test]
async fn without_storage_uploads_are_refused_clearly_and_text_still_works() {
    // The default test configuration is STORAGE_BACKEND=none.
    let app = test_app!();
    let t = app.teacher("T-MAT-03", "general").await;
    let (s, config) = app.get("/api/config", None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(config["uploads_enabled"], false);

    let (s, v) = upload(&app, &t, "reading.txt", b"Some text.").await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE, "{v}");
    assert_eq!(err_code(&v), "unavailable");
    assert!(err_message(&v).contains("還沒有設定檔案儲存空間"), "{v}");

    // Pasting works, in the library and in the lesson SOP.
    let m = paste(&app, &t, "Pasted", &passage()).await;
    assert_eq!(m["paragraph_count"], 3);
    let id = lesson(&app, &t).await;
    let (s, sop) = app
        .put(
            &format!("/api/lessons/{id}/sop/material"),
            Some(&t),
            json!({ "title": "In the SOP", "text": passage() }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{sop}");
    assert_eq!(sop["material"]["paragraphs"].as_array().unwrap().len(), 3);
    // …and so does deleting a pasted material.
    let (s, v) = app
        .delete(&format!("/api/materials/{}", m["id"]), Some(&t))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
}

// ---------------------------------------------------------------- uploads: rules

#[tokio::test]
async fn uploads_enforce_type_content_size_and_quota() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure());
    let t = app.teacher("T-MAT-04", "general").await;
    let (_, config) = app.get("/api/config", None).await;
    assert_eq!(config["uploads_enabled"], true);

    // Extensions outside the list.
    for name in [
        "tool.exe",
        "page.html",
        "icon.svg",
        "old.doc",
        "notes",
        "archive.pdf.zip",
    ] {
        let (s, v) = upload(&app, &t, name, b"whatever").await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{name}: {v}");
        assert!(err_message(&v).contains("不支援這種檔案"), "{v}");
    }
    // The right extension on the wrong content.
    for (name, data) in [
        (
            "fake.png",
            b"<html><script>alert(1)</script></html>".as_slice(),
        ),
        ("fake.pdf", b"MZ\x90\x00 this is an executable".as_slice()),
        ("fake.docx", b"plain text pretending".as_slice()),
        ("fake.mp4", b"\x00\x00\x00\x00nope".as_slice()),
        ("fake.txt", b"\x00\x01\x02\x03\xff\xfe\x00binary".as_slice()),
    ] {
        let (s, v) = upload(&app, &t, name, data).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{name}: {v}");
        assert!(err_message(&v).contains("檔案內容不是"), "{v}");
    }
    let (s, v) = upload(&app, &t, "empty.txt", b"").await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    let (s, v) = app
        .post_multipart(
            "/api/materials/upload",
            Some(&t),
            &[Part::Text("title", "no file")],
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");

    // Per-kind size limit: an image may be 10 MB, not one byte more.
    let (s, v) = upload(&app, &t, "big.png", &png(10 * 1024 * 1024)).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    assert!(err_message(&v).contains("圖片檔案最大 10 MB"), "{v}");
    assert!(dir.files().is_empty(), "nothing refused was stored");

    // A good image is stored, described, and not analysed.
    let image = upload_ok(&app, &t, "board photo.PNG", &png(2000)).await;
    assert_eq!(
        (image["kind"].as_str(), image["text_status"].as_str()),
        (Some("image"), Some("not_applicable"))
    );
    assert_eq!(image["content_type"], "image/png");
    assert_eq!(
        image["title"], "board photo",
        "the title defaults to the file name"
    );
    assert_eq!(image["filename"], "board photo.PNG");
    assert_eq!(image["size_bytes"], 2008);
    assert_eq!(image["sha256"], manna_api::auth::sha256_hex(png(2000)));
    assert_eq!(image["has_file"], true);
    assert_eq!(image["paragraphs"], json!([]));
    assert_eq!(dir.files().len(), 1);
    let (key, teacher_id): (String, i64) =
        sqlx::query_as("SELECT storage_key, teacher_id FROM materials WHERE id = $1")
            .bind(image["id"].as_i64().unwrap())
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert!(
        key.starts_with(&format!("t{teacher_id}/")) && key.ends_with(".png"),
        "{key}"
    );
    assert!(
        !key.contains("board"),
        "the object name says nothing about the file"
    );

    // The same file again is recognised.
    let (s, v) = upload(&app, &t, "copy.png", &png(2000)).await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    assert_eq!(dir.files().len(), 1);

    // Quota: pretend the teacher has used all but 1,000 bytes of the gigabyte.
    sqlx::query(
        "INSERT INTO materials (teacher_id, kind, origin, title, size_bytes, storage_key, text_status)
         VALUES ($1, 'video', 'upload', 'filler', $2, 'filler-key', 'not_applicable')",
    )
    .bind(teacher_id)
    .bind(1024_i64 * 1024 * 1024 - 2008 - 1000)
    .execute(&app.db)
    .await
    .unwrap();
    let (s, v) = upload(&app, &t, "more.png", &png(5000)).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    assert!(err_message(&v).contains("空間不夠"), "{v}");
    upload_ok(&app, &t, "small.png", &png(500)).await;

    // Number of materials.
    sqlx::query(
        "INSERT INTO materials (teacher_id, kind, origin, title, text_status)
         SELECT $1, 'link', 'url', 'filler ' || n, 'not_applicable' FROM generate_series(1, 297) n",
    )
    .bind(teacher_id)
    .execute(&app.db)
    .await
    .unwrap();
    let (s, v) = upload(&app, &t, "one-too-many.png", &png(100)).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    assert!(err_message(&v).contains("最多 300 份"), "{v}");
    let (s, v) = app
        .post(
            "/api/materials",
            Some(&t),
            json!({ "kind": "text", "text": "One more." }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
}

// ---------------------------------------------------------------- uploads: extraction, download

#[tokio::test]
async fn documents_are_extracted_with_positions_and_served_through_the_api() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure());
    let t = app.teacher("T-MAT-05", "general").await;

    // DOCX: paragraphs in document order.
    let docx_bytes = docx(&[P1, "", "Tom & Jerry <ran>.", P3]);
    let d = upload_ok(&app, &t, "L3 reading.docx", &docx_bytes).await;
    assert_eq!(
        (d["kind"].as_str(), d["text_status"].as_str()),
        (Some("document"), Some("ready"))
    );
    assert_eq!(paragraph_texts(&d), [P1, "Tom & Jerry <ran>.", P3]);
    assert_eq!(d["paragraphs"][2]["id"], "p3");
    assert_eq!(d["page_count"], Value::Null);
    assert_eq!(d["text_truncated"], false);

    // PDF: page and paragraph positions; lines of a paragraph are joined.
    let pdf_bytes = pdf(&[
        vec![
            vec![
                "Mrs. Lin put an umbrella stand outside her shop",
                "on a rainy Monday.",
            ],
            vec!["Her neighbors laughed at the idea."],
        ],
        vec![vec!["In the first month, twelve umbrellas did disappear."]],
    ]);
    let p = upload_ok(&app, &t, "L3 handout.pdf", &pdf_bytes).await;
    assert_eq!(p["text_status"], "ready", "{p}");
    assert_eq!(p["page_count"], 2);
    assert_eq!(
        paragraph_texts(&p),
        [
            "Mrs. Lin put an umbrella stand outside her shop on a rainy Monday.",
            "Her neighbors laughed at the idea.",
            "In the first month, twelve umbrellas did disappear.",
        ]
    );
    let pages: Vec<i64> = p["paragraphs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["page"].as_i64().unwrap())
        .collect();
    assert_eq!(pages, [1, 1, 2]);
    assert_eq!(p["paragraphs"][2]["no"], 3);

    // A scanned PDF (no text layer): stored, previewable, 「no text found」. No OCR.
    let scan = upload_ok(&app, &t, "scan.pdf", &pdf(&[vec![], vec![]])).await;
    assert_eq!(scan["text_status"], "no_text", "{scan}");
    assert_eq!(scan["paragraphs"], json!([]));
    assert_eq!(scan["page_count"], 2);
    assert_eq!(scan["has_file"], true);

    // TXT in Big5 (what older Windows editors save) is decoded.
    let txt = upload_ok(&app, &t, "note.txt", b"First line.\n\n\xA7\x41\xA6\x6E").await;
    assert_eq!(paragraph_texts(&txt), ["First line.", "你好"]);

    // ---- the original comes back byte for byte, only through the API
    let token = format!("Bearer {t}");
    let id = d["id"].as_i64().unwrap();
    let path = format!("/api/materials/{id}/file");
    let whole = app
        .send_raw(Method::GET, &path, &[("authorization", &token)], vec![])
        .await;
    assert_eq!(whole.status, StatusCode::OK);
    assert_eq!(whole.body, docx_bytes);
    assert_eq!(
        manna_api::auth::sha256_hex(&whole.body),
        d["sha256"].as_str().unwrap()
    );
    assert_eq!(
        whole.header("content-type"),
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
    );
    assert_eq!(whole.header("content-length"), docx_bytes.len().to_string());
    assert_eq!(whole.header("accept-ranges"), "bytes");
    // It can never run as a page of this site.
    assert_eq!(whole.header("x-content-type-options"), "nosniff");
    assert!(whole.header("content-security-policy").contains("sandbox"));
    assert!(
        whole
            .header("content-disposition")
            .starts_with("attachment;"),
        "documents download"
    );
    assert_eq!(whole.header("cache-control"), "private, no-store");

    // Range requests (audio and video seek with these).
    let part = app
        .send_raw(
            Method::GET,
            &path,
            &[("authorization", &token), ("range", "bytes=2-11")],
            vec![],
        )
        .await;
    assert_eq!(part.status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(part.body, &docx_bytes[2..12]);
    assert_eq!(
        part.header("content-range"),
        format!("bytes 2-11/{}", docx_bytes.len())
    );
    assert_eq!(part.header("content-length"), "10");
    let tail = app
        .send_raw(
            Method::GET,
            &path,
            &[("authorization", &token), ("range", "bytes=-4")],
            vec![],
        )
        .await;
    assert_eq!(tail.body, &docx_bytes[docx_bytes.len() - 4..]);
    let beyond = app
        .send_raw(
            Method::GET,
            &path,
            &[("authorization", &token), ("range", "bytes=999999-")],
            vec![],
        )
        .await;
    assert_eq!(beyond.status, StatusCode::RANGE_NOT_SATISFIABLE);

    // A short-lived link for <img>/<audio>/<video>, which cannot send the bearer header.
    let image = upload_ok(&app, &t, "photo.png", &png(64)).await;
    let image_id = image["id"].as_i64().unwrap();
    let (s, link) = app
        .post(
            &format!("/api/materials/{image_id}/file-link"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{link}");
    let url = link["url"].as_str().unwrap();
    assert!(
        url.starts_with(&format!("/api/materials/{image_id}/file?t=")),
        "{url}"
    );
    let shown = app.send_raw(Method::GET, url, &[], vec![]).await;
    assert_eq!(shown.status, StatusCode::OK);
    assert_eq!(shown.body, png(64));
    assert!(
        shown.header("content-disposition").starts_with("inline;"),
        "images show in place"
    );
    // The link opens that one material only, and not once it has been tampered with.
    let elsewhere = url.replace(
        &format!("/materials/{image_id}/"),
        &format!("/materials/{id}/"),
    );
    assert_eq!(
        app.send_raw(Method::GET, &elsewhere, &[], vec![])
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    let tampered = format!("{}0", url);
    assert_eq!(
        app.send_raw(Method::GET, &tampered, &[], vec![])
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    // A material without a file has no link.
    let pasted = paste(&app, &t, "Pasted", "Only text.").await;
    let (s, _) = app
        .post(
            &format!("/api/materials/{}/file-link", pasted["id"]),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    // A link dies with the material.
    let (s, _) = app
        .delete(&format!("/api/materials/{image_id}"), Some(&t))
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        app.send_raw(Method::GET, url, &[], vec![]).await.status,
        StatusCode::NOT_FOUND
    );
}

// ---------------------------------------------------------------- annotations

#[tokio::test]
async fn annotations_are_separate_anchored_and_only_shared_with_ai_when_switched_on() {
    let app = test_app!();
    let t = app.teacher("T-MAT-06", "general").await;
    let m = paste(&app, &t, "The Umbrella Stand", &passage()).await;
    let id = m["id"].as_i64().unwrap();

    // The server takes the quote from the paragraph: the client only sends where.
    let (start, end) = span(P1, "umbrella stand");
    let a = annotate(
        &app,
        &t,
        id,
        json!({ "paragraph_id": "p1", "start": start, "end": end, "note": "先確認學生知道這是什麼" }),
    )
    .await;
    assert_eq!(a["quote"], "umbrella stand");
    assert_eq!(
        a["share_with_ai"], false,
        "off unless the teacher turns it on"
    );
    assert_eq!(a["hidden"], false);
    assert_eq!(
        a["anchor"],
        json!({ "status": "ok", "paragraph_id": "p1", "paragraph_no": 1, "start": start, "end": end })
    );
    // A curly quote is one UTF-16 unit, three bytes: offsets follow the browser, not bytes.
    let (qs, qe) = span(P1, "“Take one if you need it.”");
    let b = annotate(
        &app,
        &t,
        id,
        json!({ "paragraph_id": "p1", "start": qs, "end": qe, "note": "指示句", "share_with_ai": true }),
    )
    .await;
    assert_eq!(b["quote"], "“Take one if you need it.”");
    let (hs, he) = span(P3, "However");
    let c = annotate(
        &app,
        &t,
        id,
        json!({ "paragraph_id": "p3", "start": hs, "end": he, "note": "學生常忽略這個轉折", "share_with_ai": true }),
    )
    .await;

    for bad in [
        json!({ "paragraph_id": "p9", "start": 0, "end": 3 }),
        json!({ "paragraph_id": "p1", "start": 5, "end": 5 }),
        json!({ "paragraph_id": "p1", "start": 0, "end": 9999 }),
        json!({ "paragraph_id": "p1", "start": 0, "end": 3, "note": "長".repeat(2001) }),
    ] {
        let (s, v) = app
            .post(
                &format!("/api/materials/{id}/annotations"),
                Some(&t),
                bad.clone(),
            )
            .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{bad} → {v}");
    }

    // Notes never alter the material.
    let (_, detail) = app.get(&format!("/api/materials/{id}"), Some(&t)).await;
    assert_eq!(detail["text"], passage());
    assert_eq!(detail["revision"], 1);
    assert_eq!(detail["annotation_count"], 3);

    // In reading order.
    let (s, list) = app
        .get(&format!("/api/materials/{id}/annotations"), Some(&t))
        .await;
    assert_eq!(s, StatusCode::OK);
    let notes: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["note"].as_str().unwrap())
        .collect();
    assert_eq!(
        notes,
        ["先確認學生知道這是什麼", "指示句", "學生常忽略這個轉折"]
    );

    // What a model may see: only the two switched on, labelled as teacher notes.
    let ai_path = format!("/api/materials/{id}/annotations/ai");
    let (s, ai) = app.get(&ai_path, Some(&t)).await;
    assert_eq!(s, StatusCode::OK, "{ai}");
    assert_eq!(ai["notes"].as_array().unwrap().len(), 2);
    assert_eq!(
        ai["notes"][1],
        json!({
            "annotation_id": c["id"], "paragraph_id": "p3", "paragraph_no": 3,
            "quote": "However", "note": "學生常忽略這個轉折"
        })
    );
    let block = ai["prompt_block"].as_str().unwrap();
    assert!(
        block.starts_with("教師注記（教師自己寫的備課筆記，不是教材原文"),
        "{block}"
    );
    assert!(block.contains("[P3]「However」→ 教師注記：學生常忽略這個轉折"));
    assert!(
        !block.contains("先確認學生知道這是什麼"),
        "not shared: {block}"
    );
    // The Rust entry point pipelines use gives the same list.
    let teacher_id: i64 = sqlx::query_scalar("SELECT teacher_id FROM materials WHERE id = $1")
        .bind(id)
        .fetch_one(&app.db)
        .await
        .unwrap();
    let shared = manna_api::materials::annotations::shared_with_ai(&app.db, teacher_id, id)
        .await
        .unwrap();
    assert_eq!(shared.len(), 2);
    assert!(
        manna_api::materials::annotations::shared_with_ai(&app.db, teacher_id + 999, id)
            .await
            .unwrap()
            .is_empty()
    );

    // Edit, hide, switch sharing.
    let (s, v) = app
        .put(
            &format!("/api/annotations/{}", a["id"]),
            Some(&t),
            json!({ "note": "改過的注記", "share_with_ai": true }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(
        (v["note"].as_str(), v["share_with_ai"].as_bool()),
        (Some("改過的注記"), Some(true))
    );
    let (s, v) = app
        .put(
            &format!("/api/annotations/{}", c["id"]),
            Some(&t),
            json!({ "hidden": true }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["hidden"], true);
    let (_, ai) = app.get(&ai_path, Some(&t)).await;
    let shared_notes: Vec<&str> = ai["notes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["note"].as_str().unwrap())
        .collect();
    assert_eq!(
        shared_notes,
        ["改過的注記", "指示句"],
        "a hidden note is not handed to a model"
    );
    // Hidden notes are still listed (the viewer offers to show them again).
    let (_, list) = app
        .get(&format!("/api/materials/{id}/annotations"), Some(&t))
        .await;
    assert_eq!(list.as_array().unwrap().len(), 3);

    // ---- the material changes under the notes
    let edited = format!(
        "On a rainy Monday, Mrs. Lin put an umbrella stand outside her shop. A sign said, “Take one if you need it.”\n\n{P2}\n\nSomething unexpected followed in the second month."
    );
    let (s, v) = app
        .put(
            &format!("/api/materials/{id}"),
            Some(&t),
            json!({ "text": edited }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, list) = app
        .get(&format!("/api/materials/{id}/annotations"), Some(&t))
        .await;
    let by_id = |want: &Value| {
        list.as_array()
            .unwrap()
            .iter()
            .find(|x| x["id"] == want["id"])
            .unwrap()
            .clone()
    };
    // P1 was rewritten (new paragraph id): both quotes are found again in it.
    let moved = by_id(&a);
    assert_eq!(moved["anchor"]["status"], "moved");
    let new_p1 = v["paragraphs"][0].clone();
    assert_eq!(moved["anchor"]["paragraph_id"], new_p1["id"]);
    let (ns, ne) = span(new_p1["text"].as_str().unwrap(), "umbrella stand");
    assert_eq!(
        (
            moved["anchor"]["start"].as_u64(),
            moved["anchor"]["end"].as_u64()
        ),
        (Some(ns as u64), Some(ne as u64))
    );
    assert_eq!(by_id(&b)["anchor"]["status"], "moved");
    // 「However」 is gone: the note is kept and asks to be placed again.
    let lost = by_id(&c);
    assert_eq!(
        lost["anchor"],
        json!({ "status": "lost", "paragraph_id": null, "paragraph_no": null, "start": null, "end": null })
    );
    assert_eq!(lost["note"], "學生常忽略這個轉折");
    assert_eq!(
        list.as_array().unwrap().last().unwrap()["id"],
        c["id"],
        "lost notes come last"
    );
    // A lost note is not handed to a model even when shared and visible.
    app.put(
        &format!("/api/annotations/{}", c["id"]),
        Some(&t),
        json!({ "hidden": false }),
    )
    .await;
    let (_, ai) = app.get(&ai_path, Some(&t)).await;
    assert_eq!(ai["notes"].as_array().unwrap().len(), 2);

    // The teacher confirms the new position.
    let last = v["paragraphs"][2].clone();
    let (rs, re) = span(last["text"].as_str().unwrap(), "Something unexpected");
    let (s, placed) = app
        .put(
            &format!("/api/annotations/{}", c["id"]),
            Some(&t),
            json!({ "anchor": { "paragraph_id": last["id"], "start": rs, "end": re } }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{placed}");
    assert_eq!(placed["anchor"]["status"], "ok");
    assert_eq!(placed["quote"], "Something unexpected");

    // Deleting a note deletes the note only.
    let (s, v) = app
        .delete(&format!("/api/annotations/{}", a["id"]), Some(&t))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, detail) = app.get(&format!("/api/materials/{id}"), Some(&t)).await;
    assert_eq!(detail["text"], edited);
    assert_eq!(detail["annotation_count"], 2);
    // Someone else's annotation is a 404.
    let other = app.teacher("T-MAT-07", "general").await;
    for method in [Method::PUT, Method::DELETE] {
        let (s, _) = app
            .send(
                method,
                &format!("/api/annotations/{}", b["id"]),
                Some(&other),
                Some("application/json"),
                b"{}".to_vec(),
            )
            .await;
        assert_eq!(s, StatusCode::NOT_FOUND);
    }
}

// ---------------------------------------------------------------- lesson SOP on the library

#[tokio::test]
async fn the_sop_picks_library_materials_and_keeps_their_paragraphs_apart() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure());
    let t = app.teacher("T-MAT-08", "general").await;
    let lesson_a = lesson(&app, &t).await;
    let a = paste(&app, &t, "Material A", &passage()).await;
    let b = upload_ok(&app, &t, "b.docx", &docx(&["First of B.", "Second of B."])).await;
    let (a_id, b_id) = (a["id"].as_i64().unwrap(), b["id"].as_i64().unwrap());

    // Pick A: the SOP shows it, and the library says where it is used.
    let v1 = pick(&app, &t, lesson_a, json!(a_id)).await;
    assert_eq!(v1["version_no"], 1);
    assert_eq!(v1["material"]["id"], a_id);
    assert_eq!(v1["material"]["paragraphs"].as_array().unwrap().len(), 3);
    assert_eq!(v1["material_deleted"], false);
    let (_, detail) = app.get(&format!("/api/materials/{a_id}"), Some(&t)).await;
    assert_eq!(detail["lessons"][0]["lesson_id"], lesson_a);
    assert_eq!(detail["lessons"][0]["lesson_title"], "第 2 節 精讀與提問");
    // Picking it again changes nothing.
    assert_eq!(
        pick(&app, &t, lesson_a, json!(a_id)).await["versions_count"],
        1
    );

    // The teacher writes about paragraph p1 of A.
    let (s, saved) = app
        .put(
            &format!("/api/lessons/{lesson_a}/sop"),
            Some(&t),
            json!({ "content": {
                "analysis": { "paragraphs": [{ "paragraph_id": "p1", "function": "A 的開場" }] },
                "task_blocks": [{ "id": "b1", "paragraph_ids": ["p1", "p2"], "tasks": [] }]
            } }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{saved}");

    // Switch to B, whose paragraphs are also p1, p2: A's references are set aside, not reused.
    let v3 = pick(&app, &t, lesson_a, json!(b_id)).await;
    assert_eq!(v3["material"]["id"], b_id);
    assert_eq!(v3["material"]["kind"], "document");
    assert_eq!(
        v3["content"]["analysis"]["paragraphs"][0]["paragraph_id"],
        format!("m{a_id}-p1")
    );
    assert_eq!(
        v3["content"]["task_blocks"][0]["paragraph_ids"],
        json!([format!("m{a_id}-p1"), format!("m{a_id}-p2")])
    );
    assert_eq!(
        v3["content"]["analysis"]["paragraphs"][0]["function"], "A 的開場",
        "nothing is lost"
    );
    // The link moved with it.
    let (_, in_lesson) = app
        .get(&format!("/api/materials?lesson_id={lesson_a}"), Some(&t))
        .await;
    let ids: Vec<i64> = in_lesson["materials"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_i64().unwrap())
        .collect();
    assert_eq!(ids, [b_id]);
    // …and back to A: they return.
    let v4 = pick(&app, &t, lesson_a, json!(a_id)).await;
    assert_eq!(
        v4["content"]["analysis"]["paragraphs"][0]["paragraph_id"],
        "p1"
    );
    assert_eq!(
        v4["content"]["task_blocks"][0]["paragraph_ids"],
        json!(["p1", "p2"])
    );

    // A second lesson uses A too. Pasting a change in the first lesson must not rewrite it
    // under the second: the first lesson gets its own copy, with the same paragraph ids.
    let lesson_b = app
        .create_lesson(
            &t,
            {
                let (_, l) = app.get(&format!("/api/lessons/{lesson_a}"), Some(&t)).await;
                l["unit_id"].as_i64().unwrap()
            },
            "第 3 節",
        )
        .await;
    pick(&app, &t, lesson_b, json!(a_id)).await;
    let edited = format!("{P1}\n\n{P2}\n\nA different ending.");
    let (s, v5) = app
        .put(
            &format!("/api/lessons/{lesson_a}/sop/material"),
            Some(&t),
            json!({ "title": "Material A", "source_note": "課本第三課", "text": edited }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v5}");
    let copy_id = v5["material"]["id"].as_i64().unwrap();
    assert_ne!(copy_id, a_id);
    let copy_ids: Vec<&str> = v5["material"]["paragraphs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap())
        .collect();
    assert_eq!(copy_ids, ["p1", "p2", "p4"]);
    assert_eq!(
        v5["content"]["analysis"]["paragraphs"][0]["paragraph_id"],
        "p1"
    );
    let (_, other_lesson) = app
        .get(&format!("/api/lessons/{lesson_b}/sop"), Some(&t))
        .await;
    assert_eq!(other_lesson["material"]["id"], a_id);
    assert_eq!(
        other_lesson["material"]["paragraphs"][2]["text"], P3,
        "untouched"
    );
    // Now only this lesson uses the copy: the next paste edits it in place.
    let (s, v6) = app
        .put(
            &format!("/api/lessons/{lesson_a}/sop/material"),
            Some(&t),
            json!({ "title": "Material A", "source_note": "課本第三課", "text": format!("{edited}\n\nOne more.") }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v6}");
    assert_eq!(v6["material"]["id"], copy_id);
    assert_eq!(v6["material"]["version_no"], 2);

    // Taking the material off the lesson keeps it in the library.
    let off = pick(&app, &t, lesson_a, Value::Null).await;
    assert_eq!(off["material"], Value::Null);
    assert_eq!(off["material_deleted"], false);
    let (s, _) = app
        .get(&format!("/api/materials/{copy_id}"), Some(&t))
        .await;
    assert_eq!(s, StatusCode::OK);

    // A material without text can be the lesson's material, but there is nothing for Manna to read.
    let image = upload_ok(&app, &t, "board.png", &png(32)).await;
    let with_image = pick(&app, &t, lesson_a, image["id"].clone()).await;
    assert_eq!(with_image["material"]["kind"], "image");
    assert_eq!(with_image["material"]["has_file"], true);
    let (s, v) = app
        .post(
            &format!("/api/lessons/{lesson_a}/sop/analysis"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    assert!(err_message(&v).contains("沒有可以讀的文字"), "{v}");

    // Extra links by hand; the SOP's own material cannot be unlinked from outside step 1.
    let (s, _) = app
        .put(
            &format!("/api/lessons/{lesson_a}/materials/{b_id}"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let (s, v) = app
        .delete(
            &format!("/api/lessons/{lesson_a}/materials/{}", image["id"]),
            Some(&t),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    let (s, _) = app
        .delete(
            &format!("/api/lessons/{lesson_a}/materials/{b_id}"),
            Some(&t),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
}

// ---------------------------------------------------------------- hard delete

async fn wait_job(app: &common::TestApp, t: &str, id: i64) -> Value {
    for _ in 0..200 {
        let (s, job) = app.get(&format!("/api/sop-jobs/{id}"), Some(t)).await;
        assert_eq!(s, StatusCode::OK, "{job}");
        if job["status"] == "done" {
            return job;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("job {id} did not finish");
}

/// How many rows anywhere in the research and content tables still contain `needle`.
async fn traces(app: &common::TestApp, needle: &str) -> Vec<(String, i64)> {
    let mut found = Vec::new();
    for (table, expr) in [
        (
            "materials",
            "text || paragraphs::text || title || source_note || COALESCE(filename, '')",
        ),
        ("material_annotations", "quote || prefix || suffix || note"),
        ("lesson_sop_versions", "content::text"),
        ("lesson_sop_jobs", "COALESCE(result::text, '')"),
        (
            "ai_interactions",
            "input_snapshot::text || COALESCE(output::text, '')",
        ),
        ("retrieval_logs", "query_text"),
        ("usage_logs", "payload::text"),
        ("material_deletions", "kind"),
    ] {
        let n: i64 = sqlx::query_scalar(&format!(
            "SELECT COUNT(*) FROM {table} WHERE {expr} LIKE '%' || $1 || '%'"
        ))
        .bind(needle)
        .fetch_one(&app.db)
        .await
        .unwrap();
        if n > 0 {
            found.push((table.to_string(), n));
        }
    }
    found
}

#[tokio::test]
async fn deleting_a_material_removes_the_file_the_text_the_notes_and_the_quotes() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure());
    let t = app.teacher("T-MAT-09", "general").await;
    let lesson_id = lesson(&app, &t).await;

    // An uploaded document, used in a lesson, analysed by Manna, annotated.
    let m = upload_ok(&app, &t, "Umbrella handout.docx", &docx(&[P1, P2, P3])).await;
    let id = m["id"].as_i64().unwrap();
    pick(&app, &t, lesson_id, json!(id)).await;
    let (s, job) = app
        .post(
            &format!("/api/lessons/{lesson_id}/sop/analysis"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{job}");
    let job = wait_job(&app, &t, job["id"].as_i64().unwrap()).await;
    let suggestion = &job["result"]["output"]["paragraphs"][0];
    let quote = suggestion["features"][0]["evidence"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        P1.contains(&quote),
        "the model quotes the material verbatim: {quote}"
    );
    // The teacher adopts it (quote included) and adds an own task.
    let (s, saved) = app
        .put(
            &format!("/api/lessons/{lesson_id}/sop"),
            Some(&t),
            json!({ "content": {
                "analysis": {
                    "main_idea": "信任會帶來信任",
                    "language_focus": [{ "id": "f1", "item": "however", "paragraph_id": "p3", "evidence": "However, something unexpected followed." }],
                    "paragraphs": [{ "paragraph_id": "p1", "function": "以故事開場", "origin": "ai_adopted",
                                     "features": [{ "id": "x", "feature": "引用", "evidence": quote, "explanation": "用招牌上的話帶出主題" }] }]
                },
                "task_blocks": [{ "id": "b1", "paragraph_ids": ["p1"],
                                  "tasks": [{ "id": "t1", "text": "Why did she put the stand outside?" }] }]
            } }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{saved}");
    let (s, objectives) = app
        .post(
            &format!("/api/lessons/{lesson_id}/sop/objectives"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{objectives}");
    wait_job(&app, &t, objectives["id"].as_i64().unwrap()).await;
    let (hs, he) = span(P3, "However");
    annotate(&app, &t, id, json!({ "paragraph_id": "p3", "start": hs, "end": he, "note": "轉折", "share_with_ai": true })).await;

    // Before: the file is on disk, and the AI logs already hold no material text except the
    // verbatim quotes in the analysis output.
    assert_eq!(dir.files().len(), 1);
    assert!(traces(&app, "rainy Monday")
        .await
        .iter()
        .any(|(t, _)| t == "materials"));
    let logged: Vec<Value> =
        sqlx::query_scalar("SELECT input_snapshot FROM ai_interactions ORDER BY id")
            .fetch_all(&app.db)
            .await
            .unwrap();
    assert!(logged.len() >= 2);
    for snapshot in &logged {
        let text = snapshot.to_string();
        assert!(
            !text.contains("rainy Monday") && !text.contains("Her neighbors"),
            "{text}"
        );
    }
    let queries: Vec<String> = sqlx::query_scalar("SELECT query_text FROM retrieval_logs")
        .fetch_all(&app.db)
        .await
        .unwrap();
    assert!(!queries.is_empty());
    assert!(
        queries
            .iter()
            .all(|q| !q.contains("rainy Monday") && q.contains("sha256")),
        "{queries:?}"
    );

    // ---- delete
    let (s, deleted) = app.delete(&format!("/api/materials/{id}"), Some(&t)).await;
    assert_eq!(s, StatusCode::OK, "{deleted}");
    assert_eq!(
        deleted,
        json!({ "deleted": true, "annotations": 1, "lessons": [lesson_id] })
    );

    // Storage: the object is gone.
    assert!(dir.files().is_empty(), "{:?}", dir.files());
    // Database: no row, no annotation, no link.
    for table in ["materials", "material_annotations", "material_links"] {
        let n: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(&app.db)
            .await
            .unwrap();
        assert_eq!(n, 0, "{table}");
    }
    // Nothing of the text is left anywhere: not in SOP versions, job results or AI logs.
    for needle in [
        "rainy Monday",
        "Her neighbors",
        "twelve of the thirty",
        "However",
        "Umbrella handout",
    ] {
        assert_eq!(traces(&app, needle).await, vec![], "{needle}");
    }
    // What remains: that a material was deleted.
    let record: (i64, String, bool, i32) =
        sqlx::query_as("SELECT material_id, kind, had_file, annotations FROM material_deletions")
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(record, (id, "document".to_string(), true, 1));

    // The API answers 404 for it now.
    for path in [
        format!("/api/materials/{id}"),
        format!("/api/materials/{id}/file"),
        format!("/api/materials/{id}/annotations"),
    ] {
        let (s, _) = app.get(&path, Some(&t)).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{path}");
    }

    // The lesson: a tombstone, and the teacher's own work intact.
    let (s, sop) = app
        .get(&format!("/api/lessons/{lesson_id}/sop"), Some(&t))
        .await;
    assert_eq!(s, StatusCode::OK, "{sop}");
    assert_eq!(sop["material"], Value::Null);
    assert_eq!(sop["material_deleted"], true);
    let analysis = &sop["content"]["analysis"];
    assert_eq!(analysis["main_idea"], "信任會帶來信任");
    assert_eq!(analysis["paragraphs"][0]["function"], "以故事開場");
    assert_eq!(
        analysis["paragraphs"][0]["features"][0]["explanation"],
        "用招牌上的話帶出主題"
    );
    assert_eq!(
        analysis["paragraphs"][0]["features"][0]["evidence"],
        "［教材已刪除］"
    );
    assert_eq!(analysis["language_focus"][0]["evidence"], "［教材已刪除］");
    assert_eq!(
        analysis["paragraphs"][0]["paragraph_id"],
        format!("m{id}-p1")
    );
    assert_eq!(
        sop["content"]["task_blocks"][0]["tasks"][0]["text"],
        "Why did she put the stand outside?"
    );
    // Older versions show the tombstone too.
    let (_, versions) = app
        .get(&format!("/api/lessons/{lesson_id}/sop/versions"), Some(&t))
        .await;
    assert!(
        versions
            .as_array()
            .unwrap()
            .iter()
            .all(|v| v["material_deleted"] == true && v["material_version_no"].is_null()),
        "{versions}"
    );
    let (_, first) = app
        .get(
            &format!("/api/lessons/{lesson_id}/sop/versions/1"),
            Some(&t),
        )
        .await;
    assert_eq!(
        (first["material"].clone(), first["material_deleted"].clone()),
        (Value::Null, json!(true))
    );
    // The job result is still readable, without the quotes.
    let (_, job_after) = app
        .get(&format!("/api/sop-jobs/{}", job["id"]), Some(&t))
        .await;
    assert_eq!(
        job_after["result"]["output"]["paragraphs"][0]["features"][0]["evidence"],
        "［教材已刪除］"
    );
    assert_eq!(
        job_after["result"]["output"]["paragraphs"][0]["summary"],
        suggestion["summary"]
    );

    // The lesson goes on: a new material starts clean, old references cannot attach to it.
    let (s, next) = app
        .put(
            &format!("/api/lessons/{lesson_id}/sop/material"),
            Some(&t),
            json!({ "title": "New", "text": "A brand new first paragraph.\n\nAnd a second." }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{next}");
    assert_eq!(next["material_deleted"], false);
    assert_eq!(next["material"]["paragraphs"][0]["id"], "p1");
    assert_eq!(
        next["content"]["analysis"]["paragraphs"][0]["paragraph_id"],
        format!("m{id}-p1")
    );

    // Deleting again: 404. The same file can be uploaded again afterwards.
    let (s, _) = app.delete(&format!("/api/materials/{id}"), Some(&t)).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    upload_ok(&app, &t, "Umbrella handout.docx", &docx(&[P1, P2, P3])).await;
}

#[tokio::test]
async fn a_delete_that_cannot_remove_the_file_deletes_nothing() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure());
    let t = app.teacher("T-MAT-10", "general").await;
    let m = upload_ok(
        &app,
        &t,
        "note.txt",
        b"Keep me until the file is really gone.",
    )
    .await;
    let id = m["id"].as_i64().unwrap();
    // Make the object undeletable: replace it with a non-empty directory.
    let file = dir.files().pop().unwrap();
    std::fs::remove_file(&file).unwrap();
    std::fs::create_dir(&file).unwrap();
    std::fs::write(file.join("blocker"), b"x").unwrap();

    let (s, v) = app.delete(&format!("/api/materials/{id}"), Some(&t)).await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE, "{v}");
    assert!(err_message(&v).contains("還沒有刪除"), "{v}");
    let (s, still) = app.get(&format!("/api/materials/{id}"), Some(&t)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(still["text"], "Keep me until the file is really gone.");

    // Once the store works again the same request succeeds.
    std::fs::remove_dir_all(&file).unwrap();
    let (s, v) = app.delete(&format!("/api/materials/{id}"), Some(&t)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
}

// ---------------------------------------------------------------- migration of lesson_sop_materials

#[tokio::test]
async fn legacy_sop_materials_move_into_the_library_and_old_texts_are_dropped() {
    let seed = |db: sqlx::PgPool| async move {
        sqlx::raw_sql(
            r#"
            INSERT INTO teachers (id, research_id, school_track) VALUES (501, 'T-LEGACY', 'general');
            INSERT INTO units (id, teacher_id, title) VALUES (601, 501, 'L3');
            INSERT INTO lessons (id, unit_id, title) VALUES (701, 601, '有教材'), (702, 601, '教材已移除'), (703, 601, '沒有 SOP');

            -- Lesson 701: pasted, then edited (two material rows), SOP content written against p2.
            INSERT INTO lesson_sop_materials (id, lesson_id, version_no, title, source_note, text, paragraphs, paragraph_seq, created_at) VALUES
              (801, 701, 1, 'Umbrella', '課本', E'OLD FIRST DRAFT sentence.\n\nKept paragraph.',
               '[{"id":"p1","no":1,"text":"OLD FIRST DRAFT sentence."},{"id":"p2","no":2,"text":"Kept paragraph."}]', 2, '2026-11-01T00:00:00Z'),
              (802, 701, 2, 'Umbrella', '課本', E'Rewritten opening.\n\nKept paragraph.',
               '[{"id":"p3","no":1,"text":"Rewritten opening."},{"id":"p2","no":2,"text":"Kept paragraph."}]', 3, '2026-11-02T00:00:00Z');
            INSERT INTO lesson_sop_versions (id, lesson_id, version_no, material_id, content) VALUES
              (901, 701, 1, 801, '{}'),
              (902, 701, 2, 802, '{}'),
              (903, 701, 3, 802, '{"analysis":{"paragraphs":[{"paragraph_id":"p2","function":"延續"}]}}');

            -- Lesson 702: pasted, then removed by the teacher.
            INSERT INTO lesson_sop_materials (id, lesson_id, version_no, text, paragraphs, paragraph_seq) VALUES
              (803, 702, 1, 'REMOVED BY THE TEACHER text.', '[{"id":"p1","no":1,"text":"REMOVED BY THE TEACHER text."}]', 1);
            INSERT INTO lesson_sop_versions (id, lesson_id, version_no, material_id, content) VALUES
              (904, 702, 1, 803, '{}'),
              (905, 702, 2, NULL, '{}');

            -- What the objectives stage logged before: the query carried material text.
            INSERT INTO ai_interactions (id, teacher_id, module, lesson_id, input_snapshot, provider, model, status) VALUES
              (1001, 501, 'sop_objectives', 701,
               '{"retrieval":{"query":"L3\nRewritten opening. Kept paragraph.","limit":150}}', 'fake', 'fake-model', 'shown'),
              (1002, 501, 'plan_suggest', 701, '{"retrieval":{"query":"untouched"}}', 'fake', 'fake-model', 'shown');
            INSERT INTO retrieval_logs (ai_interaction_id, query_text, school_track) VALUES
              (1001, E'L3\nRewritten opening. Kept paragraph.', 'general'),
              (1002, 'untouched', 'general');
            INSERT INTO lesson_sop_jobs (lesson_id, teacher_id, kind, status, sop_version_id) VALUES
              (701, 501, 'analysis', 'done', 903);
            SELECT setval('teachers_id_seq', 600);
            "#,
        )
        .execute(&db)
        .await
        .expect("legacy rows");
    };
    let Some(app) = common::TestApp::new_seeded(|_| {}, Some(8), seed).await else {
        return;
    };
    let t = app.teacher("T-LEGACY", "general").await;

    // The old table is gone; one library material per lesson that still had one.
    let old_table: Option<String> =
        sqlx::query_scalar("SELECT to_regclass('lesson_sop_materials')::text")
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(old_table, None);
    let (s, library) = app.get("/api/materials", Some(&t)).await;
    assert_eq!(s, StatusCode::OK, "{library}");
    let materials = library["materials"].as_array().unwrap();
    assert_eq!(materials.len(), 1, "{library}");
    let m = &materials[0];
    assert_eq!(
        (
            m["kind"].as_str(),
            m["origin"].as_str(),
            m["title"].as_str()
        ),
        (Some("text"), Some("paste"), Some("Umbrella"))
    );
    assert_eq!(m["source_note"], "課本");
    assert_eq!(m["revision"], 2);
    assert_eq!(
        m["sha256"],
        manna_api::auth::sha256_hex("Rewritten opening.\n\nKept paragraph.")
    );
    assert_eq!(m["lessons"][0]["lesson_id"], 701);

    // The SOP reads as before: same paragraphs and ids, same content, all versions.
    let (s, sop) = app.get("/api/lessons/701/sop", Some(&t)).await;
    assert_eq!(s, StatusCode::OK, "{sop}");
    assert_eq!(sop["version_no"], 3);
    assert_eq!(sop["material"]["id"], m["id"]);
    assert_eq!(sop["material"]["version_no"], 2);
    assert_eq!(
        sop["material"]["paragraphs"],
        json!([
            { "id": "p3", "no": 1, "text": "Rewritten opening." },
            { "id": "p2", "no": 2, "text": "Kept paragraph." }
        ])
    );
    assert_eq!(
        sop["content"]["analysis"]["paragraphs"][0]["paragraph_id"],
        "p2"
    );
    let (_, versions) = app.get("/api/lessons/701/sop/versions", Some(&t)).await;
    let revisions: Vec<Value> = versions
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["material_version_no"].clone())
        .collect();
    assert_eq!(revisions, [json!(2), json!(2), json!(1)]);
    // Editing continues the paragraph ids where the old table left off.
    let (s, edited) = app
        .put(
            "/api/lessons/701/sop/material",
            Some(&t),
            json!({ "title": "Umbrella", "source_note": "課本", "text": "Rewritten opening.\n\nKept paragraph.\n\nA new ending." }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{edited}");
    assert_eq!(edited["material"]["id"], m["id"], "edited in place");
    assert_eq!(edited["material"]["paragraphs"][2]["id"], "p4");

    // The removed material is not resurrected: a tombstone on the version that had it.
    let (_, removed) = app.get("/api/lessons/702/sop", Some(&t)).await;
    assert_eq!(removed["material"], Value::Null);
    assert_eq!(
        removed["material_deleted"], false,
        "the current version never had one"
    );
    let (_, had_one) = app.get("/api/lessons/702/sop/versions/1", Some(&t)).await;
    assert_eq!(
        (
            had_one["material"].clone(),
            had_one["material_deleted"].clone()
        ),
        (Value::Null, json!(true))
    );
    // A lesson without SOP is untouched.
    let (_, none) = app.get("/api/lessons/703/sop", Some(&t)).await;
    assert_eq!(none["version_no"], Value::Null);

    // Texts the teacher had rewritten or removed are gone, and the logged query is redacted.
    for needle in ["OLD FIRST DRAFT", "REMOVED BY THE TEACHER"] {
        assert_eq!(traces(&app, needle).await, vec![], "{needle}");
    }
    let queries: Vec<(i64, String)> =
        sqlx::query_as("SELECT ai_interaction_id, query_text FROM retrieval_logs ORDER BY id")
            .fetch_all(&app.db)
            .await
            .unwrap();
    assert!(
        !queries[0].1.contains("Rewritten opening") && queries[0].1.contains("sha256"),
        "{queries:?}"
    );
    assert_eq!(queries[1].1, "untouched");
    let snapshot: Value =
        sqlx::query_scalar("SELECT input_snapshot FROM ai_interactions WHERE id = 1001")
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert!(
        !snapshot.to_string().contains("Rewritten opening"),
        "{snapshot}"
    );
    assert_eq!(snapshot["retrieval"]["limit"], 150);
    let job_material: Option<i64> = sqlx::query_scalar("SELECT material_id FROM lesson_sop_jobs")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(job_material, m["id"].as_i64());
}
