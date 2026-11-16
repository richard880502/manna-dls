//! The 知識庫 gallery's preview images (docs/API.md「知識庫：教材牆與預覽圖」):
//! `GET /api/materials/:id/thumbnail` for uploaded images and PDFs; who may read it; the HTTP
//! validators; the kept copy in the storage and its removal with the last material holding the
//! file.
//!
//! Every image and PDF here is generated in the test. Rendering a PDF needs `pdftoppm`
//! (poppler-utils): without it locally the PDF test checks the 503 instead (CI installs it and
//! checks the picture).

#[macro_use]
mod common;

use std::io::Cursor;
use std::path::{Path, PathBuf};

use axum::http::{Method, StatusCode};
use common::{Part, RawResponse};
use manna_api::config::Config;
use manna_api::materials::ocr::OcrConfig;
use manna_api::storage::StorageConfig;
use serde_json::Value;

/// A storage directory that is removed when the test ends.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let dir =
            std::env::temp_dir().join(format!("manna-thumb-{}", manna_api::auth::new_token()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    /// File storage here; OCR off (it is not what is tested).
    fn configure(&self) -> impl FnOnce(&mut Config) + '_ {
        |c: &mut Config| {
            c.storage = StorageConfig::fs(self.0.clone());
            c.ocr = OcrConfig::disabled();
        }
    }

    fn kept(&self, sha256: &str) -> PathBuf {
        self.0.join("thumbs/w480").join(format!("{sha256}.jpg"))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn has_poppler() -> bool {
    let has = |bin: &str| std::process::Command::new(bin).arg("-v").output().is_ok();
    if has("pdftoppm") {
        return true;
    }
    if std::env::var_os("CI").is_some() {
        panic!("pdftoppm (poppler-utils) must be installed for the preview tests in CI");
    }
    eprintln!("pdftoppm (poppler-utils) is not installed here: checking the 503 instead");
    false
}

/// A PNG with a transparent left half and an opaque blue right half.
fn png(width: u32, height: u32) -> Vec<u8> {
    let image = image::RgbaImage::from_fn(width, height, |x, _| {
        if x < width / 2 {
            image::Rgba([0, 0, 0, 0])
        } else {
            image::Rgba([20, 40, 200, 255])
        }
    });
    let mut out = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut out, image::ImageFormat::Png)
        .unwrap();
    out.into_inner()
}

/// A one-page A4 PDF with a line of text (Helvetica).
fn pdf(line: &str) -> Vec<u8> {
    let stream = format!("BT\n/F1 24 Tf\n72 720 Td\n({line}) Tj\nET\n");
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [4 0 R] /Count 1 >>".to_string(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] \
         /Resources << /Font << /F1 3 0 R >> >> /Contents 5 0 R >>"
            .to_string(),
        format!("<< /Length {} >>\nstream\n{stream}endstream", stream.len()),
    ];
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

/// `POST /api/admin/library`: a platform material with a file.
async fn platform_upload(app: &common::TestApp, t: &str, filename: &str, data: &[u8]) -> Value {
    let (s, v) = app
        .post_multipart(
            "/api/admin/library",
            Some(t),
            &[
                Part::Text("category", "學測英文"),
                Part::File("file", filename, data),
            ],
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v
}

/// `POST /api/materials/upload`: the teacher's own.
async fn own_upload(app: &common::TestApp, t: &str, filename: &str, data: &[u8]) -> Value {
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
        &format!("/api/materials/{id}/thumbnail"),
        &headers,
        Vec::new(),
    )
    .await
}

fn json_of(r: &RawResponse) -> Value {
    serde_json::from_slice(&r.body).unwrap_or(Value::Null)
}

fn jpeg(r: &RawResponse) -> image::RgbImage {
    assert_eq!(r.status, StatusCode::OK, "{}", json_of(r));
    assert_eq!(r.header("content-type"), "image/jpeg");
    image::load_from_memory_with_format(&r.body, image::ImageFormat::Jpeg)
        .expect("a JPEG")
        .to_rgb8()
}

async fn sha_of(app: &common::TestApp, id: i64) -> String {
    sqlx::query_scalar("SELECT sha256 FROM materials WHERE id = $1")
        .bind(id)
        .fetch_one(&app.db)
        .await
        .unwrap()
}

fn id_of(v: &Value) -> i64 {
    v["id"].as_i64().unwrap()
}

fn exists(path: &Path) -> bool {
    path.is_file()
}

#[tokio::test]
async fn an_image_preview_is_small_kept_and_revalidated() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure());
    let pi = app.pi_token().await;
    let teacher = app.teacher("T-THUMB-1", "general").await;

    let card = platform_upload(&app, &pi, "board.png", &png(1200, 800)).await;
    let id = id_of(&card);
    let sha = sha_of(&app, id).await;
    assert!(
        !exists(&dir.kept(&sha)),
        "nothing is rendered before it is asked for"
    );

    // Any teacher reads a platform material's preview: 480 px wide, a JPEG on white.
    let first = thumbnail(&app, id, Some(&teacher), None).await;
    let image = jpeg(&first);
    assert_eq!((image.width(), image.height()), (480, 320));
    let [r, g, b] = image.get_pixel(40, 160).0;
    assert!(
        r > 240 && g > 240 && b > 240,
        "transparent is white: {r} {g} {b}"
    );
    let [r, _, b] = image.get_pixel(440, 160).0;
    assert!(b > 150 && r < 80, "the blue half stays blue");
    let etag = format!("\"{sha}-w480\"");
    assert_eq!(first.header("etag"), etag);
    assert_eq!(first.header("cache-control"), "private, max-age=3600");
    assert_eq!(first.header("vary"), "Authorization");
    assert_eq!(first.header("x-content-type-options"), "nosniff");
    assert!(
        exists(&dir.kept(&sha)),
        "the preview is kept by the file's hash"
    );

    // The kept one is sent again; the browser's copy is still good (304, no body).
    let again = thumbnail(&app, id, Some(&pi), None).await;
    assert_eq!(again.status, StatusCode::OK);
    assert_eq!(again.body, first.body);
    let unchanged = thumbnail(&app, id, Some(&teacher), Some(&etag)).await;
    assert_eq!(unchanged.status, StatusCode::NOT_MODIFIED);
    assert!(unchanged.body.is_empty());
    assert_eq!(unchanged.header("etag"), etag);
    let other = thumbnail(&app, id, Some(&teacher), Some("\"something-else\"")).await;
    assert_eq!(other.status, StatusCode::OK);

    // Signed in only.
    let anonymous = thumbnail(&app, id, None, None).await;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn only_readable_uploaded_pdfs_and_images_have_one() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure());
    let a = app.teacher("T-THUMB-A", "general").await;
    let b = app.teacher("T-THUMB-B", "general").await;

    // Another teacher's own image is a 404, as its file is.
    let own = own_upload(&app, &a, "mine.png", &png(64, 64)).await;
    let id = id_of(&own);
    assert_eq!(
        thumbnail(&app, id, Some(&a), None).await.status,
        StatusCode::OK
    );
    let theirs = thumbnail(&app, id, Some(&b), None).await;
    assert_eq!(theirs.status, StatusCode::NOT_FOUND);
    assert_eq!(json_of(&theirs)["error"]["code"], "not_found");
    assert_eq!(
        thumbnail(&app, 999_999, Some(&a), None).await.status,
        StatusCode::NOT_FOUND
    );

    // A text file and pasted text have no preview (the gallery shows their first lines).
    let txt = own_upload(&app, &a, "notes.txt", b"Plain words in a file.").await;
    let none = thumbnail(&app, id_of(&txt), Some(&a), None).await;
    assert_eq!(none.status, StatusCode::NOT_FOUND);
    assert!(json_of(&none)["error"]["message"]
        .as_str()
        .unwrap()
        .contains("沒有預覽圖"));
    let (s, pasted) = app
        .post(
            "/api/materials",
            Some(&a),
            serde_json::json!({ "kind": "text", "title": "Pasted", "text": "Some pasted words." }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{pasted}");
    let none = thumbnail(&app, id_of(&pasted), Some(&a), None).await;
    assert_eq!(none.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_kept_preview_goes_with_the_last_material_holding_the_file() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure());
    let pi = app.pi_token().await;
    let teacher = app.teacher("T-THUMB-D", "general").await;
    let photo = png(600, 300);

    // The same file in the platform library and in a teacher's own: one kept preview.
    let platform = id_of(&platform_upload(&app, &pi, "photo.png", &photo).await);
    let own = id_of(&own_upload(&app, &teacher, "photo.png", &photo).await);
    let sha = sha_of(&app, own).await;
    assert_eq!(sha, sha_of(&app, platform).await);
    let mine = thumbnail(&app, own, Some(&teacher), None).await;
    jpeg(&mine);
    assert!(exists(&dir.kept(&sha)));
    let shared = thumbnail(&app, platform, Some(&teacher), None).await;
    assert_eq!(shared.body, mine.body);

    // Deleting one keeps it for the other; deleting the last removes it.
    let (s, v) = app
        .delete(&format!("/api/materials/{own}"), Some(&teacher))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert!(exists(&dir.kept(&sha)), "the platform copy still uses it");
    let (s, v) = app
        .delete(&format!("/api/admin/library/{platform}"), Some(&pi))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert!(
        !exists(&dir.kept(&sha)),
        "the preview went with the last file"
    );
}

#[tokio::test]
async fn a_pdf_preview_is_its_first_page() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure());
    let pi = app.pi_token().await;
    let card = platform_upload(&app, &pi, "114 學測英文.pdf", &pdf("Reading Test")).await;
    let id = id_of(&card);
    let response = thumbnail(&app, id, Some(&pi), None).await;

    if !has_poppler() {
        assert_eq!(response.status, StatusCode::SERVICE_UNAVAILABLE);
        assert!(json_of(&response)["error"]["message"]
            .as_str()
            .unwrap()
            .contains("pdftoppm"));
        return;
    }
    // A4 at 480 px wide: 679 px tall; the page is white with dark text near the top.
    let image = jpeg(&response);
    assert_eq!(image.width(), 480);
    assert!((675..=683).contains(&image.height()), "{}", image.height());
    let [r, g, b] = image.get_pixel(470, 670).0;
    assert!(r > 240 && g > 240 && b > 240, "the page is white");
    let dark = (0..image.height() / 4)
        .flat_map(|y| (0..image.width()).map(move |x| (x, y)))
        .filter(|&(x, y)| image.get_pixel(x, y).0[0] < 100)
        .count();
    assert!(dark > 50, "the line of text is drawn ({dark} dark pixels)");
    assert!(exists(&dir.kept(&sha_of(&app, id).await)));
}
