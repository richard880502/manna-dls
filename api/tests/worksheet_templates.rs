//! Phase 2.5 (docs/API.md「我的學習單版型」): extracting a template from a teacher's worksheet as
//! a background job (with the fake provider), retry and failure, validation of edits, owner
//! isolation, hard delete (with the uploaded sample, on the filesystem backend), the samples no
//! template uses, worksheets made from a template and a library material, generation
//! (per-paragraph expansion, the 16-block cap, blocks that need the teacher's material, a failed
//! block) and single-block regeneration.

#[macro_use]
mod common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use axum::http::StatusCode;
use common::Part;
use manna_api::config::Config;
use manna_api::storage::StorageConfig;
use serde_json::{json, Value};

const MSG_EXTERNAL: &str = "這一區需要你提供素材（例如影片或另一篇文章），Manna 不會自己編造。";

/// A teacher's past worksheet, as the text extractor would read it. The fake provider takes
/// every `## ` line as one block.
fn worksheet_text(marker: &str) -> String {
    format!(
        "Name: ______ No: ______ Class: ______\n\nObjective: I can find details in each paragraph.\n\n\
         ## Warm-up: [主題]\nWhat do you know about Ms. Chen's umbrella shop in Hsinchu?\n\n\
         ## Paragraph [n]: Details\n1. What does Ms. Chen put outside? 2. Why?\n\n\
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

fn passage() -> String {
    P.join("\n\n")
}

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

async fn extract(app: &common::TestApp, t: &str, material_id: i64) -> Value {
    let (s, v) = app
        .post(
            "/api/worksheet-templates",
            Some(t),
            json!({ "material_id": material_id }),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    assert_eq!(v["status"], "extracting");
    wait_template(app, t, v["id"].as_i64().unwrap()).await
}

async fn ready_template(app: &common::TestApp, t: &str) -> Value {
    let m = paste(app, t, "L2 Reading Worksheet", &worksheet_text("")).await;
    let v = extract(app, t, m).await;
    assert_eq!(v["status"], "ready", "{v}");
    v
}

fn section(id: &str, title: &str, kind: &str, formats: &[&str], count: i64) -> Value {
    json!({
        "id": id, "title_pattern": title, "purpose": "測試用途", "block_kind": kind,
        "per_paragraph": false, "student_tasks": ["理解內容"], "answer_formats": formats,
        "item_count": count, "text_scope": "全文", "scaffolds": ["引導問題"],
        "instruction_style": "英文祈使句", "item_patterns": [], "needs_external_material": false,
    })
}

fn content(sections: Vec<Value>) -> Value {
    json!({
        "worksheet_kind": "課文深度閱讀", "instruction_language": "英文",
        "header_fields": ["Name", "No"], "shows_objectives": false,
        "includes_passage_text": false, "sections": sections,
        "generation_options": ["提供句型框架"], "style_notes": ["每段先問細節"],
    })
}

async fn put_content(app: &common::TestApp, t: &str, id: i64, c: Value) -> (StatusCode, Value) {
    app.put(
        &format!("/api/worksheet-templates/{id}"),
        Some(t),
        json!({ "content": c }),
    )
    .await
}

async fn wait_worksheet(app: &common::TestApp, t: &str, id: i64) -> Value {
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
    wait_worksheet(app, t, id).await
}

fn blocks(state: &Value) -> Vec<Value> {
    state["content"]["blocks"].as_array().cloned().unwrap()
}

async fn snapshots(app: &common::TestApp, module: &str) -> Vec<(i32, Option<i64>, Value, String)> {
    sqlx::query_as(
        "SELECT attempt, retry_of, input_snapshot, status FROM ai_interactions
          WHERE module = $1 ORDER BY id",
    )
    .bind(module)
    .fetch_all(&app.db)
    .await
    .unwrap()
}

#[tokio::test]
async fn extraction_makes_a_template_the_teacher_can_edit() {
    let app = test_app!();
    let t = app.teacher("T-TPL-1", "general").await;
    let m = paste(&app, &t, "L2 Reading Worksheet", &worksheet_text("")).await;
    let (s, started) = app
        .post(
            "/api/worksheet-templates",
            Some(&t),
            json!({ "material_id": m }),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{started}");
    assert_eq!(started["status"], "extracting");
    assert_eq!(started["content"], Value::Null);
    assert_eq!(started["version_no"], Value::Null);
    assert_eq!(started["section_count"], 0);
    assert_eq!(
        started["source_material"],
        json!({ "id": m, "title": "L2 Reading Worksheet" })
    );
    let id = started["id"].as_i64().unwrap();
    let v = wait_template(&app, &t, id).await;
    assert_eq!(v["status"], "ready", "{v}");
    assert_eq!(v["error"], Value::Null);
    assert_eq!(v["title"], "L2 Reading Worksheet版型");
    assert_eq!(v["version_no"], 1);
    assert_eq!(v["section_count"], 4);
    let c = &v["content"];
    assert_eq!(c["shows_objectives"], true);
    let sections = c["sections"].as_array().unwrap();
    let ids: Vec<&str> = sections.iter().map(|s| s["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["s1", "s2", "s3", "s4"]);
    assert_eq!(sections[0]["block_kind"], "warmup");
    assert_eq!(sections[1]["per_paragraph"], true);
    assert_eq!(sections[1]["title_pattern"], "Paragraph [n]: Details");
    assert_eq!(sections[2]["needs_external_material"], true);
    assert_eq!(sections[3]["answer_formats"], json!(["段落寫作"]));
    assert_eq!(sections[0]["per_paragraph"], false);

    // Two calls, logged without the worksheet's text.
    for module in ["worksheet_template_outline", "worksheet_template_details"] {
        let rows = snapshots(&app, module).await;
        assert_eq!(rows.len(), 1, "{module}");
        let (attempt, _, snapshot, status) = &rows[0];
        assert_eq!((*attempt, status.as_str()), (1, "shown"));
        let logged = snapshot.to_string();
        assert!(!logged.contains("umbrella shop"), "{module}: {logged}");
        assert!(logged.contains("sha256"), "{module}");
    }

    // The list, newest first.
    let (s, list) = app.get("/api/worksheet-templates", Some(&t)).await;
    assert_eq!(s, StatusCode::OK, "{list}");
    let list = list["templates"].as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["id"], id);
    assert_eq!(list[0]["section_count"], 4);
    assert!(list[0].get("content").is_none());

    // An edit is a new version by the teacher; the title alone is not.
    let mut edited = c.clone();
    edited["sections"][1]["per_paragraph"] = json!(false);
    edited["sections"][1]["item_count"] = json!(5);
    edited["sections"].as_array_mut().unwrap().remove(2);
    let (s, v) = put_content(&app, &t, id, edited.clone()).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["version_no"], 2);
    assert_eq!(v["section_count"], 3);
    assert_eq!(v["content"]["sections"][1]["item_count"], 5);
    assert_eq!(v["content"]["sections"][2]["id"], "s4", "ids stay");
    let (s, v) = app
        .put(
            &format!("/api/worksheet-templates/{id}"),
            Some(&t),
            json!({ "title": "  我的閱讀版型 " }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(
        (v["title"].as_str(), v["version_no"].as_i64()),
        (Some("我的閱讀版型"), Some(2))
    );
    // Saving the same content again adds nothing.
    let (_, v) = put_content(&app, &t, id, edited).await;
    assert_eq!(v["version_no"], 2);
    let origins: Vec<String> = sqlx::query_scalar(
        "SELECT origin FROM worksheet_template_versions WHERE template_id = $1 ORDER BY version_no",
    )
    .bind(id)
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert_eq!(origins, ["ai", "teacher"]);

    // Labels outside the fixed sets and impossible counts from the model are cleaned up.
    let m2 = paste(&app, &t, "Other", &worksheet_text("FAKE_TPL_BAD_LABEL")).await;
    let v = extract(&app, &t, m2).await;
    assert_eq!(v["status"], "ready", "{v}");
    let s1 = &v["content"]["sections"][1];
    assert_eq!(s1["student_tasks"], json!(["理解內容"]));
    assert_eq!(s1["answer_formats"], json!(["簡答題"]));
    assert_eq!(s1["scaffolds"], json!(["引導問題"]));
    assert_eq!(s1["item_count"], 12);
    assert_eq!(v["content"]["generation_options"], json!(["提供引導問題"]));
}

#[tokio::test]
async fn extraction_retries_once_and_a_failed_one_can_be_retried() {
    let app = test_app!();
    let t = app.teacher("T-TPL-2", "general").await;

    // A runaway on the first attempt of each call: retried once with a higher temperature.
    let m = paste(&app, &t, "Runaway", &worksheet_text("FAKE_RUNAWAY")).await;
    let v = extract(&app, &t, m).await;
    assert_eq!(v["status"], "ready", "{v}");
    for module in ["worksheet_template_outline", "worksheet_template_details"] {
        let rows = snapshots(&app, module).await;
        assert_eq!(rows.len(), 2, "{module}");
        assert_eq!(rows[0].3, "blocked");
        assert_eq!((rows[1].0, rows[1].3.as_str()), (2, "shown"));
        assert!(rows[1].1.is_some(), "the retry points at the first call");
        let t1 = rows[0].2["request"]["temperature"].as_f64().unwrap();
        let t2 = rows[1].2["request"]["temperature"].as_f64().unwrap();
        assert!((t2 - t1 - 0.4).abs() < 1e-3, "{t1} → {t2}");
        assert_eq!(
            rows[0].2["request"]["max_tokens"],
            if module.ends_with("outline") {
                2000
            } else {
                5000
            }
        );
    }

    // Unreadable twice: failed, with a Chinese message.
    let m = paste(&app, &t, "Broken", &worksheet_text("FAKE_BLOCK")).await;
    let failed = extract(&app, &t, m).await;
    assert_eq!(failed["status"], "failed", "{failed}");
    let id = failed["id"].as_i64().unwrap();
    assert!(
        failed["error"].as_str().unwrap().contains("重試"),
        "{failed}"
    );
    assert_eq!(failed["content"], Value::Null);
    assert_eq!(failed["version_no"], Value::Null);
    // Its content cannot be saved; its title can.
    let (s, v) = put_content(
        &app,
        &t,
        id,
        content(vec![section("s1", "A", "activity", &["簡答題"], 2)]),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    let (s, _) = app
        .put(
            &format!("/api/worksheet-templates/{id}"),
            Some(&t),
            json!({ "title": "壞掉的" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    // A failed template cannot be used for a worksheet.
    let (s, v) = app
        .post(
            "/api/worksheets",
            Some(&t),
            json!({ "template_id": id, "material": { "text": passage() } }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");

    // The teacher fixes the text and retries.
    let (s, v) = app
        .put(
            &format!("/api/materials/{m}"),
            Some(&t),
            json!({ "text": worksheet_text("") }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, v) = app
        .post(
            &format!("/api/worksheet-templates/{id}/retry"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    assert_eq!(
        (v["status"].as_str(), v["error"].clone()),
        (Some("extracting"), Value::Null)
    );
    let v = wait_template(&app, &t, id).await;
    assert_eq!(v["status"], "ready", "{v}");
    assert_eq!(v["title"], "壞掉的");
    assert_eq!(v["version_no"], 1);
    // Only a failed template is retried.
    let (s, v) = app
        .post(
            &format!("/api/worksheet-templates/{id}/retry"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");

    // A provider error fails at once, without a retry.
    let m = paste(&app, &t, "Error", &worksheet_text("FAKE_ERROR")).await;
    let before = snapshots(&app, "worksheet_template_outline").await.len();
    let v = extract(&app, &t, m).await;
    assert_eq!(v["status"], "failed");
    assert!(
        v["error"].as_str().unwrap().contains("AI 暫時無法使用"),
        "{v}"
    );
    assert_eq!(
        snapshots(&app, "worksheet_template_outline").await.len(),
        before + 1
    );
}

#[tokio::test]
async fn invalid_requests_and_edits_get_chinese_400s() {
    let app = test_app!();
    let t = app.teacher("T-TPL-3", "general").await;
    let has_cjk = |v: &Value| {
        v["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .chars()
            .any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
    };

    // The source must be one of the teacher's materials with text.
    let (s, link) = app
        .post(
            "/api/materials",
            Some(&t),
            json!({ "kind": "link", "title": "Video", "url": "https://example.org/v" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let m = paste(&app, &t, "W", &worksheet_text("")).await;
    for (body, expect) in [
        (json!({ "material_id": link["id"] }), "沒有文字"),
        (json!({ "material_id": 999_999 }), "找不到這份教材"),
        (
            json!({ "material_id": m, "title": "題".repeat(201) }),
            "200",
        ),
        (json!({}), ""),
    ] {
        let (s, v) = app
            .post("/api/worksheet-templates", Some(&t), body.clone())
            .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{body} → {v}");
        assert!(has_cjk(&v), "{v}");
        assert!(
            v["error"]["message"].as_str().unwrap().contains(expect),
            "{v}"
        );
    }
    let long = paste(&app, &t, "Long", &"word ".repeat(7_000)).await;
    let (s, v) = app
        .post(
            "/api/worksheet-templates",
            Some(&t),
            json!({ "material_id": long }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");

    let tpl = extract(&app, &t, m).await;
    let id = tpl["id"].as_i64().unwrap();
    let good = tpl["content"].clone();
    let set = |path: &[&str], value: Value| {
        let mut c = good.clone();
        let mut at = &mut c;
        for p in path {
            at = match p.parse::<usize>() {
                Ok(i) => &mut at[i],
                Err(_) => &mut at[*p],
            };
        }
        *at = value;
        c
    };
    let many: Vec<Value> = (1..=13)
        .map(|i| section(&format!("s{i}"), "A", "activity", &["簡答題"], 2))
        .collect();
    let cases = vec![
        (
            "作答方式",
            set(&["sections", "0", "answer_formats"], json!(["探究"])),
        ),
        (
            "學生任務",
            set(&["sections", "0", "student_tasks"], json!(["SDGs"])),
        ),
        (
            "鷹架",
            set(&["sections", "0", "scaffolds"], json!(["高層次思考"])),
        ),
        ("產生設定", set(&["generation_options"], json!(["SEL"]))),
        ("題數", set(&["sections", "1", "item_count"], json!(0))),
        ("題數", set(&["sections", "1", "item_count"], json!(13))),
        ("1 到 12 個區塊", set(&["sections"], json!([]))),
        ("1 到 12 個區塊", set(&["sections"], json!(many))),
        ("指示語言", set(&["instruction_language"], json!("日文"))),
        ("重複", set(&["sections", "1", "id"], json!("s1"))),
        (
            "標題",
            set(&["sections", "0", "title_pattern"], json!("  ")),
        ),
        (
            "標題",
            set(&["sections", "0", "title_pattern"], json!("x".repeat(201))),
        ),
        (
            "用途說明",
            set(&["sections", "0", "purpose"], json!("x".repeat(301))),
        ),
        (
            "課文範圍",
            set(&["sections", "0", "text_scope"], json!("x".repeat(101))),
        ),
        (
            "題型樣式",
            set(
                &["sections", "0", "item_patterns"],
                json!(["a", "b", "c", "d", "e", "f", "g"]),
            ),
        ),
        ("設計習慣", set(&["style_notes"], json!(vec!["n"; 9]))),
        ("設計習慣", set(&["style_notes"], json!(["x".repeat(201)]))),
        (
            "格式",
            set(&["sections", "0", "block_kind"], json!("nonsense")),
        ),
    ];
    for (expect, c) in cases {
        let (s, v) = put_content(&app, &t, id, c).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{expect} → {v}");
        assert!(has_cjk(&v), "{v}");
        assert!(
            v["error"]["message"].as_str().unwrap().contains(expect),
            "{expect} → {v}"
        );
    }
    for title in [json!(""), json!("x".repeat(201))] {
        let (s, v) = app
            .put(
                &format!("/api/worksheet-templates/{id}"),
                Some(&t),
                json!({ "title": title }),
            )
            .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
        assert!(has_cjk(&v));
    }
    // Nothing of that was saved; a new section without an id gets one.
    let mut added = good.clone();
    added["sections"].as_array_mut().unwrap().push(section(
        "",
        "New one",
        "activity",
        &["填空題", "填空題"],
        2,
    ));
    let (s, v) = put_content(&app, &t, id, added).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["version_no"], 2);
    assert_eq!(v["content"]["sections"][4]["id"], "s5");
    assert_eq!(
        v["content"]["sections"][4]["answer_formats"],
        json!(["填空題"])
    );
}

#[tokio::test]
async fn templates_are_the_owners_only_and_delete_is_for_good() {
    let app = test_app!();
    let a = app.teacher("T-TPL-4A", "general").await;
    let b = app.teacher("T-TPL-4B", "general").await;
    let tpl = ready_template(&app, &a).await;
    let id = tpl["id"].as_i64().unwrap();
    let material = tpl["source_material"]["id"].as_i64().unwrap();
    let lesson_material = paste(&app, &a, "Umbrella", &passage()).await;

    let path = format!("/api/worksheet-templates/{id}");
    let (s, _) = app.get(&path, Some(&b)).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = app.put(&path, Some(&b), json!({ "title": "mine" })).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = app
        .post(&format!("{path}/retry"), Some(&b), json!({}))
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = app.delete(&path, Some(&b)).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, v) = app.get("/api/worksheet-templates", Some(&b)).await;
    assert_eq!(
        (s, v["templates"].as_array().unwrap().len()),
        (StatusCode::OK, 0)
    );
    // Nor can B use A's template or A's materials.
    let (s, _) = app
        .post(
            "/api/worksheet-templates",
            Some(&b),
            json!({ "material_id": material }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = app
        .post("/api/worksheets", Some(&b), json!({ "template_id": id }))
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = app
        .post(
            "/api/worksheets",
            Some(&b),
            json!({ "material_id": lesson_material }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = app.get(&path, None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // A worksheet made from it keeps its content when the template goes.
    let (s, ws) = app
        .post(
            "/api/worksheets",
            Some(&a),
            json!({ "material_id": lesson_material, "template_id": id }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{ws}");
    let ws_id = ws["id"].as_i64().unwrap();
    assert_eq!(ws["template"]["id"], id);
    let (s, v) = app.delete(&path, Some(&a)).await;
    assert_eq!((s, v), (StatusCode::OK, json!({ "ok": true })));
    let (s, _) = app.get(&path, Some(&a)).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let left: i64 = sqlx::query_scalar(
        "SELECT (SELECT COUNT(*) FROM worksheet_templates WHERE id = $1)
              + (SELECT COUNT(*) FROM worksheet_template_versions WHERE template_id = $1)",
    )
    .bind(id)
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(left, 0, "template and versions are gone");
    let (s, ws) = app.get(&format!("/api/worksheets/{ws_id}"), Some(&a)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(ws["template"], Value::Null);
    // Its sample (pasted, so no file) went with it; the lesson's material stays.
    let (s, _) = app
        .get(&format!("/api/materials/{material}"), Some(&a))
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = app
        .get(&format!("/api/materials/{lesson_material}"), Some(&a))
        .await;
    assert_eq!(s, StatusCode::OK);

    // The sample of a template is not deleted on its own: the template would lose its original.
    let tpl = ready_template(&app, &a).await;
    let m = tpl["source_material"]["id"].as_i64().unwrap();
    let (s, v) = app.delete(&format!("/api/materials/{m}"), Some(&a)).await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    assert_eq!(
        v["error"]["message"],
        format!(
            "這份檔案是範本「{}」的原檔，範本還在用它。要刪除的話，請刪除那個範本，原檔會一起刪除。",
            tpl["title"].as_str().unwrap()
        )
    );
    let (_, v) = app
        .get(&format!("/api/worksheet-templates/{}", tpl["id"]), Some(&a))
        .await;
    assert_eq!(
        (v["status"].as_str(), v["source_material"]["id"].as_i64()),
        (Some("ready"), Some(m))
    );
}

// ---------------------------------------------------------------- the uploaded sample goes too

/// A storage directory that is removed when the test ends.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let dir =
            std::env::temp_dir().join(format!("manna-templates-{}", manna_api::auth::new_token()));
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

/// 上傳我的學習單: a worksheet file as a sample (never in 我的教材). `marker` keeps the files apart
/// (the same file twice is the same material).
async fn upload_sample(app: &common::TestApp, t: &str, filename: &str, marker: &str) -> i64 {
    let text = worksheet_text(marker);
    let (s, v) = app
        .post_multipart(
            "/api/materials/upload",
            Some(t),
            &[
                Part::Text("purpose", "worksheet_sample"),
                Part::File("file", filename, text.as_bytes()),
            ],
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["purpose"], "worksheet_sample");
    v["id"].as_i64().unwrap()
}

/// 我的範本's 「沒有範本在用的原檔」.
async fn orphans(app: &common::TestApp, t: &str) -> Vec<i64> {
    let (s, v) = app
        .get(
            "/api/materials?purpose=worksheet_sample&orphaned=true",
            Some(t),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v["materials"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_i64().unwrap())
        .collect()
}

async fn count(app: &common::TestApp, sql: &str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(&app.db).await.unwrap()
}

/// The deletion events written so far, oldest first.
async fn deletion_events(app: &common::TestApp) -> Vec<(String, Value)> {
    sqlx::query_as(
        "SELECT event, payload FROM usage_logs
          WHERE event IN ('material_deleted', 'worksheet_template_deleted') ORDER BY id",
    )
    .fetch_all(&app.db)
    .await
    .unwrap()
}

#[tokio::test]
async fn deleting_my_template_deletes_the_worksheet_i_uploaded_for_it() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure());
    let t = app.teacher("T-TPL-DEL-1", "general").await;
    let m = upload_sample(&app, &t, "Midterm worksheet.txt", "MIDTERM").await;
    assert_eq!(dir.files().len(), 1);
    let tpl = extract(&app, &t, m).await;
    assert_eq!(tpl["status"], "ready", "{tpl}");
    assert_eq!(tpl["deletes_source_material"], true);
    let id = tpl["id"].as_i64().unwrap();
    // A note on the sample goes with it.
    let (_, sample) = app.get(&format!("/api/materials/{m}"), Some(&t)).await;
    let (s, v) = app
        .post(
            &format!("/api/materials/{m}/annotations"),
            Some(&t),
            json!({ "paragraph_id": sample["paragraphs"][0]["id"], "start": 0, "end": 4,
                    "note": "抬頭" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");

    let (s, v) = app
        .delete(&format!("/api/worksheet-templates/{id}"), Some(&t))
        .await;
    assert_eq!((s, v), (StatusCode::OK, json!({ "ok": true })));

    // The file, the template, the sample's row (its text and paragraphs) and its note are gone.
    assert!(dir.files().is_empty(), "{:?}", dir.files());
    for table in [
        "worksheet_templates",
        "worksheet_template_versions",
        "materials",
        "material_annotations",
    ] {
        assert_eq!(
            count(&app, &format!("SELECT COUNT(*) FROM {table}")).await,
            0,
            "{table}"
        );
    }
    for path in [
        format!("/api/materials/{m}"),
        format!("/api/materials/{m}/file"),
        format!("/api/worksheet-templates/{id}"),
    ] {
        let (s, _) = app.get(&path, Some(&t)).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{path}");
    }
    // What remains: that a material was deleted (no title, no text) and the two events.
    let record: (i64, String, bool, i32) =
        sqlx::query_as("SELECT material_id, kind, had_file, annotations FROM material_deletions")
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(record, (m, "document".to_string(), true, 1));
    assert_eq!(
        deletion_events(&app).await,
        [
            (
                "material_deleted".to_string(),
                json!({ "material_id": m, "kind": "document", "lessons": 0, "scope": "personal" })
            ),
            (
                "worksheet_template_deleted".to_string(),
                json!({ "template_id": id, "scope": "personal", "deleted_sample_id": m })
            ),
        ]
    );
    let named = count(
        &app,
        "SELECT COUNT(*) FROM usage_logs WHERE payload::text LIKE '%Midterm%' OR payload::text LIKE '%MIDTERM%'",
    )
    .await;
    assert_eq!(named, 0);
    assert!(orphans(&app, &t).await.is_empty());
}

#[tokio::test]
async fn a_sample_two_templates_use_goes_with_the_last_of_them() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure());
    let t = app.teacher("T-TPL-DEL-2", "general").await;
    let m = upload_sample(&app, &t, "Shared worksheet.txt", "SHARED").await;
    let first = extract(&app, &t, m).await;
    let first_id = first["id"].as_i64().unwrap();
    let (s, second) = app
        .post(
            &format!("/api/worksheet-templates/{first_id}/copy"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{second}");
    let second_id = second["id"].as_i64().unwrap();
    assert_eq!(second["source_material"]["id"], m);
    for id in [first_id, second_id] {
        let (_, v) = app
            .get(&format!("/api/worksheet-templates/{id}"), Some(&t))
            .await;
        assert_eq!(v["deletes_source_material"], false, "{v}");
    }
    // While templates use it, the sample is not deleted on its own, nor listed as left over.
    let (s, v) = app.delete(&format!("/api/materials/{m}"), Some(&t)).await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    let message = v["error"]["message"].as_str().unwrap();
    assert!(message.starts_with("這份檔案是 2 個範本（"), "{message}");
    for title in [&first["title"], &second["title"]] {
        assert!(message.contains(title.as_str().unwrap()), "{message}");
    }
    assert!(orphans(&app, &t).await.is_empty());

    // The first one goes: the sample stays for the other.
    let (s, _) = app
        .delete(&format!("/api/worksheet-templates/{first_id}"), Some(&t))
        .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = app.get(&format!("/api/materials/{m}"), Some(&t)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(dir.files().len(), 1);
    assert_eq!(
        count(&app, "SELECT COUNT(*) FROM material_deletions").await,
        0
    );
    let (_, v) = app
        .get(&format!("/api/worksheet-templates/{second_id}"), Some(&t))
        .await;
    assert_eq!(
        (
            v["source_material"]["id"].as_i64(),
            &v["deletes_source_material"]
        ),
        (Some(m), &json!(true))
    );

    // The last one takes it along.
    let (s, _) = app
        .delete(&format!("/api/worksheet-templates/{second_id}"), Some(&t))
        .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = app.get(&format!("/api/materials/{m}"), Some(&t)).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert!(dir.files().is_empty(), "{:?}", dir.files());
    assert_eq!(
        count(&app, "SELECT COUNT(*) FROM material_deletions").await,
        1
    );
}

#[tokio::test]
async fn a_template_delete_that_cannot_remove_the_file_deletes_nothing() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure());
    let t = app.teacher("T-TPL-DEL-3", "general").await;
    let m = upload_sample(&app, &t, "Keep me.txt", "KEEPME").await;
    let id = extract(&app, &t, m).await["id"].as_i64().unwrap();
    let path = format!("/api/worksheet-templates/{id}");
    // Make the object undeletable: replace it with a non-empty directory.
    let file = dir.files().pop().unwrap();
    std::fs::remove_file(&file).unwrap();
    std::fs::create_dir(&file).unwrap();
    std::fs::write(file.join("blocker"), b"x").unwrap();

    let (s, v) = app.delete(&path, Some(&t)).await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE, "{v}");
    assert_eq!(
        v["error"]["message"],
        "原檔沒有刪除成功（儲存空間暫時無法使用），所以這份教材還沒有刪除。請稍後再試一次。"
    );
    // Nothing went: the template still has its sample, the sample its text.
    let (s, tpl) = app.get(&path, Some(&t)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        (
            tpl["status"].as_str(),
            tpl["source_material"]["id"].as_i64()
        ),
        (Some("ready"), Some(m))
    );
    let (s, sample) = app.get(&format!("/api/materials/{m}"), Some(&t)).await;
    assert_eq!(s, StatusCode::OK);
    assert!(sample["text"].as_str().unwrap().contains("KEEPME"));
    assert_eq!(
        count(&app, "SELECT COUNT(*) FROM material_deletions").await,
        0
    );
    assert!(deletion_events(&app).await.is_empty());

    // Once the store works again the same request deletes both.
    std::fs::remove_dir_all(&file).unwrap();
    let (s, v) = app.delete(&path, Some(&t)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    for path in [path, format!("/api/materials/{m}")] {
        let (s, _) = app.get(&path, Some(&t)).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{path}");
    }
}

#[tokio::test]
async fn my_samples_no_template_uses_are_listed_for_me_alone_and_deleted_for_good() {
    let dir = TempDir::new();
    let app = test_app!(dir.configure());
    let a = app.teacher("T-TPL-DEL-4A", "general").await;
    let b = app.teacher("T-TPL-DEL-4B", "general").await;
    // Left behind by a template deleted under the old rule (the template went, the sample stayed).
    let old = upload_sample(&app, &a, "Old worksheet.txt", "OLDONE").await;
    let old_tpl = extract(&app, &a, old).await["id"].as_i64().unwrap();
    sqlx::query("DELETE FROM worksheet_templates WHERE id = $1")
        .bind(old_tpl)
        .execute(&app.db)
        .await
        .unwrap();
    // An upload that never became a template, one still in use, and a lesson material.
    let never = upload_sample(&app, &a, "Never read.txt", "NEVER").await;
    let used = upload_sample(&app, &a, "In use.txt", "INUSE").await;
    extract(&app, &a, used).await;
    paste(&app, &a, "Umbrella", &passage()).await;
    let theirs = upload_sample(&app, &b, "B worksheet.txt", "THEIRS").await;

    assert_eq!(orphans(&app, &a).await, [never, old], "newest first");
    assert_eq!(orphans(&app, &b).await, [theirs]);
    for query in ["?orphaned=true", "?purpose=material&orphaned=true"] {
        let (s, v) = app.get(&format!("/api/materials{query}"), Some(&a)).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{query}: {v}");
    }

    // Another teacher can neither read nor delete them.
    let files = dir.files().len();
    for m in [old, never] {
        let (s, _) = app.get(&format!("/api/materials/{m}"), Some(&b)).await;
        assert_eq!(s, StatusCode::NOT_FOUND);
        let (s, _) = app.delete(&format!("/api/materials/{m}"), Some(&b)).await;
        assert_eq!(s, StatusCode::NOT_FOUND);
    }
    assert_eq!(dir.files().len(), files);

    // Their owner deletes them for good, through the material's own delete.
    for m in [old, never] {
        let (s, v) = app.delete(&format!("/api/materials/{m}"), Some(&a)).await;
        assert_eq!(s, StatusCode::OK, "{v}");
    }
    assert_eq!(dir.files().len(), files - 2);
    assert!(orphans(&app, &a).await.is_empty());
    assert_eq!(orphans(&app, &b).await, [theirs]);
    let deleted: Vec<i64> =
        sqlx::query_scalar("SELECT material_id FROM material_deletions ORDER BY material_id")
            .fetch_all(&app.db)
            .await
            .unwrap();
    assert_eq!(deleted, [old, never]);
}

/// A template with a warm-up, a per-paragraph block, a block that needs the teacher's own
/// video and an exit ticket.
fn lesson_template() -> Value {
    let mut per = section("s2", "Paragraph [n]: Details", "activity", &["選擇題"], 2);
    per["per_paragraph"] = json!(true);
    per["text_scope"] = json!("逐段");
    let mut video = section(
        "s3",
        "Watch the video and compare",
        "activity",
        &["簡答題"],
        2,
    );
    video["needs_external_material"] = json!(true);
    video["text_scope"] = json!("課文外");
    let mut c = content(vec![
        section("s1", "Warm-up: [主題]", "warmup", &["簡答題"], 2),
        per,
        video,
        section("s4", "Exit ticket", "exit_ticket", &["是非題"], 1),
    ]);
    c["shows_objectives"] = json!(true);
    c
}

#[tokio::test]
async fn a_worksheet_follows_the_template_on_a_library_material() {
    let app = test_app!();
    let t = app.teacher("T-TPL-5", "general").await;
    let tpl = ready_template(&app, &t).await;
    let id = tpl["id"].as_i64().unwrap();
    let (s, v) = put_content(&app, &t, id, lesson_template()).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let lesson_material = paste(&app, &t, "The Umbrella Stand", &passage()).await;
    let materials_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM materials")
        .fetch_one(&app.db)
        .await
        .unwrap();

    // The library material wins over pasted text, which is then not stored.
    let (s, ws) = app
        .post(
            "/api/worksheets",
            Some(&t),
            json!({ "material_id": lesson_material, "template_id": id,
                    "material": { "text": "Ignored text." } }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{ws}");
    let ws_id = ws["id"].as_i64().unwrap();
    let materials_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM materials")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(materials_before, materials_after);
    assert_eq!(ws["material"]["source"]["kind"], "library");
    assert_eq!(ws["material"]["source"]["material_id"], lesson_material);
    assert_eq!(ws["material"]["paragraphs"].as_array().unwrap().len(), 4);
    assert_eq!(ws["material"]["title"], "The Umbrella Stand");
    assert_eq!(
        ws["template"],
        json!({ "id": id, "title": tpl["title"], "version_no": 2 })
    );
    // Settings come from the template.
    assert_eq!(ws["settings"]["student_tasks"], json!(["理解內容"]));
    assert_eq!(
        ws["settings"]["answer_formats"],
        json!(["選擇題", "是非題", "簡答題"])
    );
    assert_eq!(
        ws["settings"]["generation"],
        json!(["顯示學習目標", "提供句型框架"])
    );
    let (s, m) = app
        .get(&format!("/api/materials/{lesson_material}"), Some(&t))
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        m["worksheet_count"], 1,
        "the worksheet is linked to the material"
    );
    // A material without text cannot be chosen.
    let (_, link) = app
        .post(
            "/api/materials",
            Some(&t),
            json!({ "kind": "link", "url": "https://example.org" }),
        )
        .await;
    let (s, v) = app
        .post(
            "/api/worksheets",
            Some(&t),
            json!({ "material_id": link["id"] }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");

    let state = generate(&app, &t, ws_id).await;
    let jobs = state["jobs"].as_array().unwrap();
    assert_eq!(jobs.len(), 1, "{jobs:?}");
    assert_eq!(
        (
            jobs[0]["step"].as_str(),
            jobs[0]["outcome"].as_str(),
            jobs[0]["status"].as_str()
        ),
        (Some("template"), Some("shown"), Some("done"))
    );
    let bs = blocks(&state);
    let kinds: Vec<&str> = bs.iter().map(|b| b["kind"].as_str().unwrap()).collect();
    assert_eq!(
        kinds,
        [
            "warmup",
            "activity",
            "activity",
            "activity",
            "activity",
            "activity",
            "exit_ticket"
        ]
    );
    let sections: Vec<&str> = bs
        .iter()
        .map(|b| b["template_section_id"].as_str().unwrap())
        .collect();
    assert_eq!(sections, ["s1", "s2", "s2", "s2", "s2", "s3", "s4"]);
    // One block per paragraph, numbered.
    for (i, b) in bs[1..5].iter().enumerate() {
        let n = i + 1;
        assert_eq!(b["title"], format!("Paragraph {n}: Details"));
        assert_eq!(b["paragraph_ids"], json!([format!("p{n}")]));
        assert_eq!(b["message"], Value::Null);
        let items = b["items"].as_array().unwrap();
        assert_eq!(items.len(), 2, "capped at item_count");
        for it in items {
            assert_eq!(it["answer_format"], "選擇題");
            assert_eq!(it["origin"], "ai");
            let options: Vec<&str> = it["options"]
                .as_array()
                .unwrap()
                .iter()
                .map(|o| o.as_str().unwrap())
                .collect();
            assert!(options.contains(&it["answer"].as_str().unwrap()));
            // The answer is quoted from its paragraph: that is its evidence.
            assert!(P[i].contains(it["evidence"].as_str().unwrap()), "{it}");
            assert!(!it["evidence"].as_str().unwrap().is_empty());
        }
        // The section's scaffold text, as its first scaffold kind.
        let sc = b["scaffolds"].as_array().unwrap();
        assert_eq!(sc.len(), 1);
        assert_eq!(
            (sc[0]["type"].as_str(), sc[0]["kind"].as_str()),
            (Some("verbal"), Some("引導問題"))
        );
        assert_eq!(b["student_tasks"], json!(["理解內容"]));
        assert_eq!(b["teacher_notes"], "測試用途");
    }
    // The block that needs the teacher's video: placeholder items and the message.
    let video = &bs[5];
    assert_eq!(video["message"], MSG_EXTERNAL);
    assert!(!video["items"].as_array().unwrap().is_empty());
    for it in video["items"].as_array().unwrap() {
        assert!(it["prompt"].as_str().unwrap().contains("______"));
        assert!(!it["prompt"].as_str().unwrap().contains("http"));
        assert_eq!(it["evidence"], "");
    }
    assert_eq!(video["paragraph_ids"], json!([]));
    assert_eq!(bs[6]["items"].as_array().unwrap().len(), 1);
    assert_eq!(bs[6]["items"][0]["answer"], "False");
    let chosen = &state["content"]["chosen"];
    assert_eq!(
        chosen["reasons"][0],
        format!("依版型：{}", tpl["title"].as_str().unwrap())
    );
    assert_eq!(state["content"]["show_objectives"], true);
    assert_eq!(state["content"]["notices"], json!([]));
    assert_eq!(state["generated"], true);

    // One call per block, the material never in the log.
    let rows = snapshots(&app, "worksheet_template_section").await;
    assert_eq!(rows.len(), 7);
    for (_, _, snapshot, status) in &rows {
        assert_eq!(status, "shown");
        assert!(!snapshot.to_string().contains("umbrella stand outside"));
        assert_eq!(snapshot["request"]["max_tokens"], 2500);
    }

    // The student sheet has no answers.
    let (s, sheet) = app
        .get(
            &format!("/api/worksheets/{ws_id}/sheet?level=standard&audience=student"),
            Some(&t),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{sheet}");
    assert!(!sheet.to_string().contains("\"answer\""));

    // PUT template_id: null removes the template and keeps the settings; a number sets it
    // again (current version) and re-derives them.
    let path = format!("/api/worksheets/{ws_id}");
    let (s, v) = app
        .put(
            &path,
            Some(&t),
            json!({ "template_id": null, "settings": {
            "material_types": [], "student_tasks": ["寫作"], "answer_formats": [], "generation": [],
            "levels": ["standard"], "minutes": null } }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["template"], Value::Null);
    assert_eq!(v["settings"]["student_tasks"], json!(["寫作"]));
    let (s, v) = app.put(&path, Some(&t), json!({ "template_id": id })).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["template"]["version_no"], 2);
    assert_eq!(v["settings"]["student_tasks"], json!(["理解內容"]));
    assert_eq!(
        v["settings"]["answer_formats"],
        json!(["選擇題", "是非題", "簡答題"])
    );
    let (s, v) = app
        .put(&path, Some(&t), json!({ "template_id": 999_999 }))
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
}

#[tokio::test]
async fn expansion_is_capped_and_a_failed_block_keeps_a_message() {
    let app = test_app!();
    let t = app.teacher("T-TPL-6", "general").await;
    let tpl = ready_template(&app, &t).await;
    let id = tpl["id"].as_i64().unwrap();
    let mut first = section("s2", "Paragraph [n]: Find", "activity", &["簡答題"], 1);
    first["per_paragraph"] = json!(true);
    let mut second = section("s3", "Details [n]", "activity", &["填空題"], 1);
    second["per_paragraph"] = json!(true);
    let c = content(vec![
        section("s1", "Before you read", "warmup", &["簡答題"], 1),
        first,
        second,
        section("s4", "Broken FAKE_BLOCK", "activity", &["簡答題"], 2),
    ]);
    let (s, v) = put_content(&app, &t, id, c).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let text: Vec<String> = (1..=10)
        .map(|i| {
            format!("This is paragraph number {i} of the story, and it has a few words in it.")
        })
        .collect();
    let (s, ws) = app
        .post(
            "/api/worksheets",
            Some(&t),
            json!({ "template_id": id, "material": { "title": "Ten", "text": text.join("\n\n") } }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{ws}");
    assert_eq!(ws["material"]["source"]["kind"], "pasted");
    let state = generate(&app, &t, ws["id"].as_i64().unwrap()).await;
    let bs = blocks(&state);
    // 4 sections; the first per-paragraph one expands to 10 (13 blocks), the second would make
    // 22 > 16, so it is done once over the whole text.
    assert_eq!(bs.len(), 13);
    let notices = state["content"]["notices"].as_array().unwrap();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert!(notices[0].as_str().unwrap().contains("Details [n]"));
    assert!(notices[0].as_str().unwrap().contains("16"));
    let collapsed = &bs[11];
    assert_eq!(collapsed["template_section_id"], "s3");
    assert_eq!(collapsed["paragraph_ids"], json!([]));
    assert!(!collapsed["items"].as_array().unwrap().is_empty());
    // The failed block says so; the others were generated.
    let broken = &bs[12];
    assert_eq!(broken["items"], json!([]));
    assert!(
        broken["message"].as_str().unwrap().contains("沒有產生成功"),
        "{broken}"
    );
    assert_eq!(broken["title"], "Broken FAKE_BLOCK");
    assert!(bs[..12]
        .iter()
        .all(|b| !b["items"].as_array().unwrap().is_empty()));
    let job = &state["jobs"][0];
    assert_eq!(job["outcome"], "shown");
    assert!(
        job["message"].as_str().unwrap().contains("1 個區塊"),
        "{job}"
    );
    // The failed block was retried once.
    let rows = snapshots(&app, "worksheet_template_section").await;
    assert_eq!(rows.len(), 13 + 1);
}

#[tokio::test]
async fn a_template_block_is_regenerated_on_its_own() {
    let app = test_app!();
    let t = app.teacher("T-TPL-7", "general").await;
    let tpl = ready_template(&app, &t).await;
    let id = tpl["id"].as_i64().unwrap();
    let (s, _) = put_content(&app, &t, id, lesson_template()).await;
    assert_eq!(s, StatusCode::OK);
    let lesson_material = paste(&app, &t, "The Umbrella Stand", &passage()).await;
    let (_, ws) = app
        .post(
            "/api/worksheets",
            Some(&t),
            json!({ "material_id": lesson_material, "template_id": id }),
        )
        .await;
    let ws_id = ws["id"].as_i64().unwrap();
    let before = generate(&app, &t, ws_id).await;
    let old = blocks(&before)[3].clone();
    assert_eq!(old["title"], "Paragraph 3: Details");
    let block_id = old["id"].as_str().unwrap().to_string();

    // Editing the template afterwards does not change the version this worksheet uses.
    let mut edited = lesson_template();
    edited["sections"][1]["item_count"] = json!(1);
    let (_, v) = put_content(&app, &t, id, edited).await;
    assert_eq!(v["version_no"], 3);

    let (s, v) = app
        .post(
            &format!("/api/worksheets/{ws_id}/blocks/{block_id}/regenerate"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    let after = wait_worksheet(&app, &t, ws_id).await;
    let jobs = after["jobs"].as_array().unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(
        (jobs[0]["step"].as_str(), jobs[0]["block_id"].as_str()),
        (Some("template"), Some(block_id.as_str()))
    );
    assert_eq!(jobs[0]["outcome"], "shown");
    let new_blocks = blocks(&after);
    let new = &new_blocks[3];
    assert_eq!(new["id"], block_id);
    assert_eq!(new["title"], "Paragraph 3: Details");
    assert_eq!(new["paragraph_ids"], json!(["p3"]));
    assert_eq!(new["template_section_id"], "s2");
    assert_eq!(
        new["items"].as_array().unwrap().len(),
        2,
        "the worksheet's template version"
    );
    assert_ne!(new["items"], old["items"]);
    for it in new["items"].as_array().unwrap() {
        assert!(
            it["id"]
                .as_str()
                .unwrap()
                .starts_with(&format!("{block_id}-r2")),
            "{it}"
        );
    }
    // The other blocks are untouched.
    for (i, b) in blocks(&before).iter().enumerate() {
        if i != 3 {
            assert_eq!(b, &new_blocks[i]);
        }
    }
    assert_eq!(after["template"]["version_no"], 2);
    assert_eq!(snapshots(&app, "worksheet_template_section").await.len(), 8);
    assert!(after["undoable"]
        .as_array()
        .unwrap()
        .iter()
        .any(|b| b == &json!(block_id)));

    // Undo brings the previous block back.
    let (s, v) = app
        .post(
            &format!("/api/worksheets/{ws_id}/blocks/{block_id}/undo"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(blocks(&v)[3], old);

    // A new generation uses the template's latest version.
    let again = generate(&app, &t, ws_id).await;
    assert_eq!(again["template"]["version_no"], 3);
    assert_eq!(blocks(&again)[1]["items"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn the_template_cannot_change_while_generating() {
    let app = test_app!();
    let t = app.teacher("T-TPL-8", "general").await;
    let tpl = ready_template(&app, &t).await;
    let id = tpl["id"].as_i64().unwrap();
    let (s, _) = put_content(
        &app,
        &t,
        id,
        content(vec![section("s1", "Only one", "activity", &["簡答題"], 1)]),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (_, ws) = app
        .post(
            "/api/worksheets",
            Some(&t),
            json!({ "template_id": id, "material": { "text": format!("{} FAKE_SLOW", passage()) } }),
        )
        .await;
    let ws_id = ws["id"].as_i64().unwrap();
    let (s, _) = app
        .post(
            &format!("/api/worksheets/{ws_id}/generate"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED);
    let (s, v) = app
        .put(
            &format!("/api/worksheets/{ws_id}"),
            Some(&t),
            json!({ "template_id": null }),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    let state = wait_worksheet(&app, &t, ws_id).await;
    assert_eq!(blocks(&state).len(), 1);
    assert_eq!(state["template"]["id"], id);
}

// ---------------------------------------------------------------- support boxes (docs/API.md「提示框」)

/// A sample like the teacher's: a per-paragraph analysis block with a Directions part and a
/// Sentence Frame for Sharing part, each under its own heading.
fn sample_with_boxes(marker: &str) -> String {
    format!(
        "Name: ______ No: ______\n\n\
         ## [n] Paragraph Analysis\n\
         1. What is the common idea about bees in the first sentence?\n\
         2. Which sentence is the topic sentence? (A) True (B) False\n\
         Directions: Identify [關鍵概念] in the first sentence.\n\
         Sentence Frame for Sharing: Hi! In Paragraph [n], the author describes _____. I identified _____ as one of the main characteristics of informative writing.\n\n\
         ## Writing: [主題]\nWrite 80 words. {marker}"
    )
}

#[tokio::test]
async fn extraction_keeps_the_samples_support_boxes() {
    let app = test_app!();
    let t = app.teacher("T-TPL-SUP-1", "general").await;
    let m = paste(&app, &t, "Bees Analysis Worksheet", &sample_with_boxes("")).await;
    let v = extract(&app, &t, m).await;
    assert_eq!(v["status"], "ready", "{v}");
    let s = &v["content"]["sections"][0];
    assert_eq!(s["per_paragraph"], true);
    assert_eq!(
        s["supports"],
        json!([
            { "heading": "Directions", "role": "directions",
              "pattern": "Identify [關鍵概念] in the first sentence." },
            { "heading": "Sentence Frame for Sharing", "role": "sharing_frame",
              "pattern": "Hi! In Paragraph [n], the author describes _____. I identified _____ as one of the main characteristics of informative writing." },
        ])
    );
    assert_eq!(
        s["scaffolds"],
        json!(["清楚的分步指示及完成順序", "句型框架"])
    );
    // A section without boxes has none in its JSON.
    assert!(v["content"]["sections"][1].get("supports").is_none());
    // The model's call asked for the boxes before the question patterns.
    let rows = snapshots(&app, "worksheet_template_details").await;
    assert!(rows[0].2["prompt"]["system"]
        .as_str()
        .unwrap()
        .contains("提示框（supports）"));

    // Read the way it was before supports: the sharing frame comes back as a question pattern;
    // the server makes it a box again and counts one question less.
    let m = paste(
        &app,
        &t,
        "Bees Analysis Worksheet 2",
        &sample_with_boxes("FAKE_TPL_FRAME_AS_ITEM"),
    )
    .await;
    let v = extract(&app, &t, m).await;
    let s = &v["content"]["sections"][0];
    assert_eq!(s["item_count"], 3, "{s}");
    assert!(s["item_patterns"]
        .as_array()
        .unwrap()
        .iter()
        .all(|p| !p.as_str().unwrap().starts_with("Hi!")));
    let boxes: Vec<(&str, &str)> = s["supports"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| (b["heading"].as_str().unwrap(), b["role"].as_str().unwrap()))
        .collect();
    assert_eq!(
        boxes,
        [
            ("Directions", "directions"),
            ("Sentence Frame for Sharing", "sharing_frame")
        ]
    );
}

/// The teacher's per-paragraph analysis block: a 是非題 and short answers, 3 questions, and
/// (with `boxes`) the sample's three support boxes.
fn analysis_template(boxes: bool) -> Value {
    let mut s = section(
        "s1",
        "[n] Paragraph Analysis",
        "activity",
        &["是非題", "簡答題"],
        3,
    );
    s["per_paragraph"] = json!(true);
    s["text_scope"] = json!("逐段");
    s["scaffolds"] = json!(["句型框架", "提示與追問"]);
    if boxes {
        s["supports"] = json!([
            { "heading": "Directions", "role": "directions", "pattern": "Identify [關鍵概念]." },
            { "heading": "Sentence Frames", "role": "sentence_frame",
              "pattern": "The author describes _____ to show _____." },
            { "heading": "Sentence Frame for Sharing", "role": "sharing_frame",
              "pattern": "Hi! In Paragraph [n], the author describes _____." },
        ]);
    }
    content(vec![s])
}

async fn worksheet_from(app: &common::TestApp, t: &str, template: i64, text: &str) -> i64 {
    let (s, ws) = app
        .post(
            "/api/worksheets",
            Some(t),
            json!({ "template_id": template, "material": { "title": "Bees", "text": text } }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{ws}");
    ws["id"].as_i64().unwrap()
}

async fn student_sheet(app: &common::TestApp, t: &str, id: i64) -> Value {
    let (s, sheet) = app
        .get(
            &format!("/api/worksheets/{id}/sheet?level=standard&audience=student"),
            Some(t),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{sheet}");
    sheet
}

fn headings(b: &Value) -> Vec<String> {
    b["supports"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["heading"].as_str().unwrap().to_string())
        .collect()
}

/// No printed question is a greeting-led frame.
fn no_numbered_greeting(sheet: &Value) {
    for b in sheet["blocks"].as_array().unwrap() {
        for it in b["items"].as_array().unwrap() {
            let p = it["prompt"].as_str().unwrap();
            assert!(!p.contains("Hi!"), "{p}");
            assert!(!p.starts_with("Do you think"), "{p}");
        }
    }
}

#[tokio::test]
async fn each_box_prints_under_the_samples_heading_and_frames_are_never_numbered() {
    let app = test_app!();
    let t = app.teacher("T-TPL-SUP-2", "general").await;
    let tpl = ready_template(&app, &t).await;
    let id = tpl["id"].as_i64().unwrap();
    let (s, v) = put_content(&app, &t, id, analysis_template(true)).await;
    assert_eq!(s, StatusCode::OK, "{v}");

    // The model writes the reported shapes in both calls: the server sorts them out.
    let ws = worksheet_from(
        &app,
        &t,
        id,
        &format!("{} FAKE_TPL_BAD_SHAPES_ALWAYS", passage()),
    )
    .await;
    let state = generate(&app, &t, ws).await;
    let bs = blocks(&state);
    assert_eq!(bs.len(), 4, "one per paragraph");
    for (i, b) in bs.iter().enumerate() {
        let n = i + 1;
        let sc = b["scaffolds"].as_array().unwrap();
        let shape: Vec<(&str, &str, &str)> = sc
            .iter()
            .map(|s| {
                (
                    s["type"].as_str().unwrap(),
                    s["kind"].as_str().unwrap(),
                    s["heading"].as_str().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            shape,
            [
                ("procedural", "清楚的分步指示及完成順序", "Directions"),
                ("verbal", "句型框架", "Sentence Frames"),
                ("verbal", "句型框架", "Sentence Frame for Sharing"),
            ],
            "{b}"
        );
        assert_eq!(
            sc[0]["steps"],
            json!([
                "Identify the common idea in the first sentence.",
                "Look for the sentence that introduces the paragraph's main point."
            ])
        );
        assert_eq!(
            sc[1]["content"],
            "The author describes ______ to show ______."
        );
        assert!(sc[2]["content"].as_str().unwrap().starts_with(&format!(
            "Hi! In Paragraph {n}, the author describes ______."
        )));
        let items = b["items"].as_array().unwrap();
        assert_eq!(items.len(), 3, "{b}");
        assert_eq!(
            (
                items[0]["answer_format"].as_str(),
                items[0]["prompt"].as_str()
            ),
            (
                Some("是非題"),
                Some("The first sentence serves as a topic sentence.")
            )
        );
    }
    // Every call was sent back once, with what was wrong.
    let rows: Vec<(i32, String, Value)> = sqlx::query_as(
        "SELECT attempt, status, check_results FROM ai_interactions
          WHERE module = 'worksheet_template_section' ORDER BY id",
    )
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert_eq!(rows.len(), 8);
    let first = rows.iter().find(|r| r.0 == 1).unwrap();
    assert_eq!(first.1, "blocked");
    let failed: Vec<&str> = first.2["checks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["passed"] == false)
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert_eq!(failed, ["items_wellformed", "supports_wellformed"]);
    // On paper: the sample's headings, in its order; the greeting frame in its box only.
    let sheet = student_sheet(&app, &t, ws).await;
    for b in sheet["blocks"].as_array().unwrap() {
        assert_eq!(
            headings(b),
            [
                "Directions",
                "Sentence Frames",
                "Sentence Frame for Sharing"
            ]
        );
        assert_eq!(b["steps"], json!([]));
        assert_eq!(b["items"].as_array().unwrap().len(), 3);
    }
    no_numbered_greeting(&sheet);
    let nos: Vec<u64> = sheet["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|b| b["items"].as_array().unwrap().clone())
        .map(|i| i["no"].as_u64().unwrap())
        .collect();
    assert_eq!(nos, (1..=12).collect::<Vec<u64>>());

    // Repaired by the second call: its boxes are used as written.
    let ws = worksheet_from(&app, &t, id, &format!("{} FAKE_TPL_BAD_SHAPES", passage())).await;
    let state = generate(&app, &t, ws).await;
    let b = &blocks(&state)[1];
    let sc = b["scaffolds"].as_array().unwrap();
    assert_eq!(
        sc[0]["steps"],
        json!([
            "Underline the sentence that tells the main idea.",
            "Circle two details that support it."
        ])
    );
    assert_eq!(
        sc[2]["content"],
        "Hi! In Paragraph 2, the author describes ______. I identified ______ as the main idea."
    );
    no_numbered_greeting(&student_sheet(&app, &t, ws).await);
}

#[tokio::test]
async fn a_template_read_before_supports_prints_directions_and_the_sharing_frame_apart() {
    let app = test_app!();
    let t = app.teacher("T-TPL-SUP-3", "general").await;
    let tpl = ready_template(&app, &t).await;
    let id = tpl["id"].as_i64().unwrap();
    let (s, v) = put_content(&app, &t, id, analysis_template(false)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let ws = worksheet_from(
        &app,
        &t,
        id,
        &format!("{} FAKE_TPL_BAD_SHAPES_ALWAYS", passage()),
    )
    .await;
    let state = generate(&app, &t, ws).await;
    for b in blocks(&state) {
        let sc = b["scaffolds"].as_array().unwrap();
        let shape: Vec<(&str, Option<&str>)> = sc
            .iter()
            .map(|s| (s["kind"].as_str().unwrap(), s["heading"].as_str()))
            .collect();
        assert_eq!(
            shape,
            [
                ("清楚的分步指示及完成順序", Some("Directions")),
                ("句型框架", None),
                ("句型框架", Some("Sentence Frame for Sharing")),
            ],
            "{b}"
        );
    }
    let sheet = student_sheet(&app, &t, ws).await;
    for b in sheet["blocks"].as_array().unwrap() {
        assert_eq!(
            headings(b),
            [
                "Directions",
                "Sentence frames",
                "Sentence Frame for Sharing"
            ]
        );
    }
    no_numbered_greeting(&sheet);
}

#[tokio::test]
async fn one_sheet_uses_the_templates_name_for_the_genre() {
    let app = test_app!();
    let t = app.teacher("T-TPL-SUP-4", "general").await;
    let tpl = ready_template(&app, &t).await;
    let id = tpl["id"].as_i64().unwrap();
    let mut c = analysis_template(true);
    c["style_notes"] = json!(["每段分析 informative writing 的特徵"]);
    let (s, v) = put_content(&app, &t, id, c).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let ws = worksheet_from(&app, &t, id, &format!("{} FAKE_TPL_TERMS", passage())).await;
    let state = generate(&app, &t, ws).await;
    for b in blocks(&state) {
        let instructions = b["instructions"].as_str().unwrap();
        assert!(
            instructions.contains("informative writing"),
            "{instructions}"
        );
        assert!(!instructions.contains("expository"), "{instructions}");
    }
}

/// A template read before supports, whose question patterns hold the sample's labelled boxes:
/// the boxes print under those labels, not as questions (the stored template is not changed).
#[tokio::test]
async fn a_template_read_before_supports_gets_its_labelled_boxes() {
    let app = test_app!();
    let t = app.teacher("T-TPL-SUP-5", "general").await;
    let tpl = ready_template(&app, &t).await;
    let id = tpl["id"].as_i64().unwrap();
    let mut c = analysis_template(false);
    c["sections"][0]["item_count"] = json!(2);
    c["sections"][0]["item_patterns"] = json!([
        "Directions: Underline the topic sentence.",
        "Do you think [句子] serves as the topic sentence? (A) True (B) False",
        "Sentence Frame for Sharing: Hi! In Paragraph [n], the author describes _____.",
    ]);
    let (s, v) = put_content(&app, &t, id, c).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert!(v["content"]["sections"][0].get("supports").is_none());
    let ws = worksheet_from(&app, &t, id, &passage()).await;
    generate(&app, &t, ws).await;
    let sheet = student_sheet(&app, &t, ws).await;
    for b in sheet["blocks"].as_array().unwrap() {
        assert_eq!(
            headings(b),
            ["Directions", "Sentence Frame for Sharing"],
            "{b}"
        );
        assert_eq!(b["items"].as_array().unwrap().len(), 2);
    }
    no_numbered_greeting(&sheet);
    let (_, v) = app
        .get(&format!("/api/worksheet-templates/{id}"), Some(&t))
        .await;
    assert!(v["content"]["sections"][0].get("supports").is_none());
}
