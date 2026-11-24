//! 表格整理 tables (api/src/worksheet/table.rs) with the fake provider: every generated table
//! names what it compares (column headings) and what each row is about (row labels, printed as
//! a first column), a placeholder heading is repaired from the question, and an old worksheet
//! with 「Column 1」 headings still loads and prints.

#[macro_use]
mod common;

use std::time::Duration;

use axum::http::{Method, StatusCode};
use serde_json::{json, Value};

const P1: &str = "Mrs. Lin put an umbrella stand outside her shop on a rainy Monday. A sign said, “Take one if you need it.”";
const P2: &str = "Her neighbors laughed at the idea. They were sure that every umbrella would be gone within a week.";
const P3: &str = "In the first month, twelve of the thirty umbrellas did disappear. However, something unexpected followed.";

async fn made(app: &common::TestApp, t: &str, marker: &str, settings: Value) -> i64 {
    let (s, v) = app
        .post(
            "/api/worksheets",
            Some(t),
            json!({
                "material": { "title": "The Umbrella Stand", "text": format!("{P1}\n\n{P2}\n\n{P3} {marker}") },
                "settings": settings,
            }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    v["id"].as_i64().unwrap()
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
    for _ in 0..800 {
        let (_, v) = app
            .get(&format!("/api/worksheets/{id}/jobs"), Some(t))
            .await;
        if v["running"] == false {
            let (s, state) = app.get(&format!("/api/worksheets/{id}"), Some(t)).await;
            assert_eq!(s, StatusCode::OK, "{state}");
            return state;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("worksheet {id} did not finish generating");
}

async fn sheet(app: &common::TestApp, t: &str, id: i64, audience: &str) -> Value {
    let (s, v) = app
        .get(
            &format!("/api/worksheets/{id}/sheet?level=standard&audience={audience}"),
            Some(t),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v
}

async fn export(app: &common::TestApp, t: &str, id: i64, format: &str) -> Vec<u8> {
    let auth = format!("Bearer {t}");
    let r = app
        .send_raw(
            Method::GET,
            &format!("/api/worksheets/{id}/export?format={format}&level=standard&audience=student"),
            &[("authorization", auth.as_str())],
            Vec::new(),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    r.body
}

async fn docx(app: &common::TestApp, t: &str, id: i64) -> String {
    let body = export(app, t, id, "docx").await;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(body)).unwrap();
    let mut xml = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("word/document.xml").unwrap(), &mut xml)
        .unwrap();
    xml
}

fn items_of(v: &Value, key: &str) -> Vec<Value> {
    v[key]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|b| b["items"].as_array().cloned().unwrap_or_default())
        .collect()
}

fn table_items(state: &Value) -> Vec<Value> {
    items_of(&state["content"], "blocks")
        .into_iter()
        .filter(|i| i["answer_format"] == "表格整理")
        .collect()
}

const NAMED: [&str; 2] = ["Event in paragraph", "Reason Given in the Text"];

fn assert_real_headings(t: &Value) {
    let cols: Vec<&str> = t["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap())
        .collect();
    assert_eq!(cols.len(), 2, "{t}");
    assert!(cols[0].starts_with(NAMED[0]) && cols[1] == NAMED[1], "{t}");
    assert!(cols.iter().all(|c| !c.starts_with("Column")), "{t}");
    assert_eq!(t["row_labels"], json!(["Claim", "Supporting detail"]));
    assert_eq!(t["rows"], 2);
}

#[tokio::test]
async fn every_table_prompt_has_a_new_version_about_headings_and_row_labels() {
    let app = test_app!();
    for key in [
        "worksheet_tasks",
        "sop_paragraph_tasks",
        "worksheet_bloom_level",
        "worksheet_tiered_level",
        "worksheet_template_section",
    ] {
        let (version, text): (i32, String) = sqlx::query_as(
            "SELECT version_no, system_text FROM prompt_versions WHERE prompt_key = $1
              ORDER BY version_no DESC LIMIT 1",
        )
        .bind(key)
        .fetch_one(&app.db)
        .await
        .unwrap();
        for word in ["table", "columns", "row_labels", "Column 1", "Claim"] {
            assert!(text.contains(word), "{key} v{version} lacks {word}");
        }
        let (previous,): (String,) = sqlx::query_as(
            "SELECT system_text FROM prompt_versions WHERE prompt_key = $1 AND version_no = $2",
        )
        .bind(key)
        .bind(version - 1)
        .fetch_one(&app.db)
        .await
        .unwrap();
        assert!(
            text.starts_with(&previous),
            "{key}: the previous text, unchanged"
        );
    }
}

#[tokio::test]
async fn comprehension_tables_carry_headings_and_row_labels_to_the_sheet_and_exports() {
    let app = test_app!();
    let t = app.teacher("T-TBL-1", "general").await;
    let id = made(
        &app,
        &t,
        "",
        json!({ "output": "comprehension", "bloom": ["analyze"], "question_count": 3,
                "answer_formats": ["表格整理"] }),
    )
    .await;
    let state = generate(&app, &t, id).await;
    let tables = table_items(&state);
    assert_eq!(tables.len(), 3);
    for it in &tables {
        assert_real_headings(&it["table"]);
    }
    // The level call was asked for a table.
    let (snap,): (Value,) = sqlx::query_as(
        "SELECT input_snapshot FROM ai_interactions WHERE module = 'worksheet_bloom_level' LIMIT 1",
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert!(snap["prompt"]["system"]
        .as_str()
        .unwrap()
        .contains("row_labels"));

    // The student sheet: a corner cell, the headings, and a label on every row.
    let printed = items_of(&sheet(&app, &t, id, "student").await, "blocks");
    let tbl = &printed[0]["table"];
    assert_eq!(tbl["rows"], 2);
    assert_eq!(tbl["columns"][0], "");
    assert_eq!(tbl["columns"][2], "Reason Given in the Text");
    assert_eq!(tbl["row_labels"], json!(["Claim", "Supporting detail"]));

    let xml = docx(&app, &t, id).await;
    for text in ["Reason Given in the Text", "Claim", "Supporting detail"] {
        assert!(xml.contains(&format!(">{text}<")), "{text}");
    }
    assert!(!xml.contains("Column 1"));
    assert!(export(&app, &t, id, "pdf").await.starts_with(b"%PDF-"));
}

#[tokio::test]
async fn a_placeholder_heading_is_repaired_from_the_question_in_the_full_worksheet() {
    let app = test_app!();
    let t = app.teacher("T-TBL-2", "general").await;
    let id = made(
        &app,
        &t,
        "FAKE_TABLE_PLACEHOLDER",
        json!({ "answer_formats": ["表格整理"] }),
    )
    .await;
    let state = generate(&app, &t, id).await;
    let tables = table_items(&state);
    assert!(!tables.is_empty());
    for it in &tables {
        // The model wrote 「Column 1」 / 「Column 2」; the question names the real ones.
        let cols = it["table"]["columns"].as_array().unwrap();
        assert!(
            cols[0].as_str().unwrap().starts_with("Event in paragraph"),
            "{cols:?}"
        );
        assert_eq!(cols[1], "Reason Given in the Text");
        assert_eq!(it["table"]["row_labels"][0], "Claim");
    }
    assert!(!docx(&app, &t, id).await.contains("Column 1"));
}

#[tokio::test]
async fn old_tables_still_load_and_the_teacher_edits_headings_and_row_labels() {
    let app = test_app!();
    let t = app.teacher("T-TBL-3", "general").await;
    let id = made(&app, &t, "", json!({ "answer_formats": ["表格整理"] })).await;
    let state = generate(&app, &t, id).await;
    let mut content = state["content"].clone();
    let (bi, ii) = content["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .find_map(|(bi, b)| {
            b["items"]
                .as_array()
                .unwrap()
                .iter()
                .position(|i| i["answer_format"] == "表格整理")
                .map(|ii| (bi, ii))
        })
        .unwrap();
    // A worksheet saved before row labels: placeholder headings, no labels. It loads; the
    // headings the question names replace 「Column 1」 on the sheet (never printed).
    content["blocks"][bi]["items"][ii]["table"] =
        json!({ "columns": ["Column 1", "Column 2"], "rows": 3 });
    let (s, v) = app
        .put(
            &format!("/api/worksheets/{id}"),
            Some(&t),
            json!({ "content": content.clone() }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let printed = items_of(&sheet(&app, &t, id, "student").await, "blocks");
    let old = printed.iter().find(|i| i["table"].is_object()).unwrap();
    let cols = old["table"]["columns"].as_array().unwrap();
    assert!(cols[0].as_str().unwrap().starts_with("Event in paragraph"));
    assert_eq!(cols[1], "Reason Given in the Text");
    assert!(old["table"].get("row_labels").is_none());
    let (_, stored) = app.get(&format!("/api/worksheets/{id}"), Some(&t)).await;
    assert_eq!(
        stored["content"]["blocks"][bi]["items"][ii]["table"]["columns"],
        json!(["Column 1", "Column 2"]),
        "what is stored is left alone"
    );

    // The teacher fills in the headings and the labels.
    content["blocks"][bi]["items"][ii]["table"] = json!({
        "columns": ["Path A", "Path B"], "rows": 3,
        "row_labels": ["Claim", "Supporting detail", "Example"]
    });
    let (s, v) = app
        .put(
            &format!("/api/worksheets/{id}"),
            Some(&t),
            json!({ "content": content.clone() }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, saved) = app.get(&format!("/api/worksheets/{id}"), Some(&t)).await;
    assert_eq!(
        saved["content"]["blocks"][bi]["items"][ii]["table"]["row_labels"][2],
        "Example"
    );
    let printed = items_of(&sheet(&app, &t, id, "student").await, "blocks");
    let new = printed.iter().find(|i| i["table"].is_object()).unwrap();
    assert_eq!(new["table"]["columns"], json!(["", "Path A", "Path B"]));
    assert_eq!(new["table"]["row_labels"][2], "Example");

    // Too many labels are refused.
    content["blocks"][bi]["items"][ii]["table"]["row_labels"] = json!(vec!["a"; 13]);
    let (s, _) = app
        .put(
            &format!("/api/worksheets/{id}"),
            Some(&t),
            json!({ "content": content }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}
