//! 圖像組織 (docs/API.md「圖像組織」) with the fake provider: organizers written by the model in
//! the full worksheet, 閱讀理解題 and 差異化學習單, cleaned by the server (unknown type, too many
//! boxes), printed per version and audience on the sheet and in the exports, edited and saved by
//! the teacher, and old organizer questions that still print a box.

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

async fn sheet(app: &common::TestApp, t: &str, id: i64, level: &str, audience: &str) -> Value {
    let (s, v) = app
        .get(
            &format!("/api/worksheets/{id}/sheet?level={level}&audience={audience}"),
            Some(t),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v
}

async fn export(
    app: &common::TestApp,
    t: &str,
    id: i64,
    format: &str,
    level: &str,
    audience: &str,
) -> Vec<u8> {
    let auth = format!("Bearer {t}");
    let r = app
        .send_raw(
            Method::GET,
            &format!(
                "/api/worksheets/{id}/export?format={format}&level={level}&audience={audience}"
            ),
            &[("authorization", auth.as_str())],
            Vec::new(),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    r.body
}

async fn docx(app: &common::TestApp, t: &str, id: i64, level: &str, audience: &str) -> String {
    let body = export(app, t, id, "docx", level, audience).await;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(body)).unwrap();
    let mut xml = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("word/document.xml").unwrap(), &mut xml)
        .unwrap();
    xml
}

/// Every item of the worksheet.
fn all_items(state: &Value) -> Vec<Value> {
    state["content"]["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|b| b["items"].as_array().cloned().unwrap_or_default())
        .collect()
}

/// Every printed item of a sheet.
fn sheet_items(s: &Value) -> Vec<Value> {
    s["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|b| b["items"].as_array().cloned().unwrap_or_default())
        .collect()
}

fn cells(org: &Value) -> Vec<Value> {
    org["sections"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|s| s["cells"].as_array().cloned().unwrap())
        .collect()
}

async fn latest_prompt(app: &common::TestApp, key: &str) -> (i32, String) {
    sqlx::query_as(
        "SELECT version_no, system_text FROM prompt_versions WHERE prompt_key = $1
          ORDER BY version_no DESC LIMIT 1",
    )
    .bind(key)
    .fetch_one(&app.db)
    .await
    .unwrap()
}

#[tokio::test]
async fn every_organizer_prompt_has_a_new_version() {
    let app = test_app!();
    for key in [
        "worksheet_tasks",
        "worksheet_bloom_level",
        "worksheet_tiered_level",
        "worksheet_template_section",
    ] {
        let (version, text) = latest_prompt(&app, key).await;
        assert!(version >= 2, "{key}");
        for word in ["organizer", "t_chart", "venn", "mind_map", "prefill"] {
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
async fn comprehension_organizers_print_empty_for_students_and_with_answers_for_teachers() {
    let app = test_app!();
    let t = app.teacher("T-ORG-1", "general").await;
    let id = made(
        &app,
        &t,
        "",
        json!({ "output": "comprehension", "bloom": ["analyze"], "question_count": 3,
                "answer_formats": ["圖像組織"] }),
    )
    .await;
    let state = generate(&app, &t, id).await;
    let items = all_items(&state);
    assert_eq!(items.len(), 3);
    let kinds: Vec<&str> = items
        .iter()
        .map(|i| i["organizer"]["type"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["t_chart", "cause_effect", "sequence"]);
    // The level call ran the organizer prompt.
    let (snap,): (Value,) = sqlx::query_as(
        "SELECT input_snapshot FROM ai_interactions WHERE module = 'worksheet_bloom_level' LIMIT 1",
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert!(snap["prompt"]["system"]
        .as_str()
        .unwrap()
        .contains("organizer"));

    let student = sheet(&app, &t, id, "standard", "student").await;
    let printed = sheet_items(&student);
    let chart = &printed[0]["organizer"];
    assert_eq!(chart["type"], "t_chart");
    assert_eq!(chart["name"], "T-chart");
    assert_eq!(printed[0]["lines"], 0, "the organizer replaces the lines");
    let student_cells = cells(chart);
    assert!(
        student_cells[0]["given"].as_str().unwrap().contains(" in "),
        "pre-filled"
    );
    assert!(student_cells.iter().skip(1).all(|c| c["given"] == ""));
    assert!(student_cells.iter().all(|c| c.get("answer").is_none()));
    let teacher = sheet(&app, &t, id, "standard", "teacher").await;
    let teacher_cells = cells(&sheet_items(&teacher)[0]["organizer"]);
    assert!(teacher_cells[1]["answer"].is_string(), "{teacher_cells:?}");

    let xml = docx(&app, &t, id, "standard", "student").await;
    for label in [
        "Before",
        "After",
        "1. Cause",
        "2. Effect",
        "1. First",
        "3. Finally",
    ] {
        assert!(xml.contains(label), "{label}");
    }
    let answer = teacher_cells[1]["answer"].as_str().unwrap();
    assert!(!xml.contains(&format!(">{answer}<")));
    assert!(docx(&app, &t, id, "standard", "teacher")
        .await
        .contains(&format!(">{answer}<")));
    assert!(export(&app, &t, id, "pdf", "standard", "student")
        .await
        .starts_with(b"%PDF-"));
}

#[tokio::test]
async fn an_unknown_type_and_too_many_boxes_are_cleaned() {
    let app = test_app!();
    let t = app.teacher("T-ORG-2", "general").await;
    let id = made(
        &app,
        &t,
        "FAKE_ORG_BAD",
        json!({ "output": "comprehension", "bloom": ["analyze"], "question_count": 3,
                "answer_formats": ["圖像組織"] }),
    )
    .await;
    let state = generate(&app, &t, id).await;
    let org = &all_items(&state)[0]["organizer"];
    assert_eq!(org["type"], "sequence", "fishbone with eight sections");
    let sections = org["sections"].as_array().unwrap();
    assert_eq!(sections.len(), 6);
    assert!(sections
        .iter()
        .all(|s| s["cells"].as_array().unwrap().len() == 2));
    let given = cells(org)
        .iter()
        .filter(|c| !c["prefilled"].as_array().unwrap().is_empty())
        .count();
    assert_eq!(given, 11, "one cell left to write");
    // The cleaning is logged as code repairs, without a repair call.
    let (attempts,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM ai_interactions WHERE module = 'worksheet_bloom_level'",
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(attempts, 1);
}

#[tokio::test]
async fn the_basic_version_gets_more_pre_filled_cells() {
    let app = test_app!();
    let t = app.teacher("T-ORG-3", "general").await;
    let id = made(
        &app,
        &t,
        "",
        json!({ "output": "tiered", "bloom": ["analyze"], "question_count": 3,
                "answer_formats": ["圖像組織"] }),
    )
    .await;
    let state = generate(&app, &t, id).await;
    let items = all_items(&state);
    assert!(
        items.iter().all(|i| i["organizer"].is_object()),
        "{items:?}"
    );
    let given = |s: &Value| {
        sheet_items(s)
            .iter()
            .map(|i| {
                cells(&i["organizer"])
                    .iter()
                    .filter(|c| c["given"] != "")
                    .count()
            })
            .sum::<usize>()
    };
    let basic = given(&sheet(&app, &t, id, "basic", "student").await);
    let standard = given(&sheet(&app, &t, id, "standard", "student").await);
    let advanced = given(&sheet(&app, &t, id, "advanced", "student").await);
    assert!(
        basic > standard && standard >= advanced && advanced >= 1,
        "{basic} {standard} {advanced}"
    );
}

#[tokio::test]
async fn the_full_worksheet_writes_organizers_and_the_teacher_edits_them() {
    let app = test_app!();
    let t = app.teacher("T-ORG-4", "general").await;
    let id = made(&app, &t, "", json!({ "answer_formats": ["圖像組織"] })).await;
    let state = generate(&app, &t, id).await;
    let organizers: Vec<Value> = all_items(&state)
        .into_iter()
        .filter(|i| i["answer_format"] == "圖像組織")
        .collect();
    assert!(!organizers.is_empty());
    assert!(organizers
        .iter()
        .all(|i| i["organizer"]["type"].is_string()));

    // The teacher switches the first one to a Venn diagram with her own labels.
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
                .position(|i| i["answer_format"] == "圖像組織")
                .map(|ii| (bi, ii))
        })
        .unwrap();
    content["blocks"][bi]["items"][ii]["organizer"] = json!({
        "type": "venn", "title": "Mrs. Lin and her neighbors", "center": "",
        "sections": [
            { "label": "Mrs. Lin", "cells": [{ "answer": "trusts people", "given": "trusts people", "prefilled": ["basic", "standard", "advanced"] }] },
            { "label": "Both", "cells": [{ "answer": "live on the street" }] },
            { "label": "The neighbors", "cells": [{ "answer": "laughed at first" }] }
        ]
    });
    // An old organizer question without an organizer, and one with levels out of order.
    let mut old = content["blocks"][bi]["items"][ii].clone();
    old["id"] = json!("old-1");
    old.as_object_mut().unwrap().remove("organizer");
    content["blocks"][bi]["items"]
        .as_array_mut()
        .unwrap()
        .push(old);
    let (s, v) = app
        .put(
            &format!("/api/worksheets/{id}"),
            Some(&t),
            json!({ "content": content.clone() }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let student = sheet(&app, &t, id, "standard", "student").await;
    let venn = sheet_items(&student)
        .into_iter()
        .find(|i| i["organizer"]["type"] == "venn")
        .unwrap();
    assert_eq!(venn["organizer"]["sections"][2]["label"], "The neighbors");
    let old = sheet_items(&student)
        .into_iter()
        .find(|i| i["id"] == "old-1")
        .unwrap();
    assert!(old.get("organizer").is_none(), "an old question has none");
    let xml = docx(&app, &t, id, "standard", "student").await;
    assert!(xml.contains("Mrs. Lin and her neighbors") && xml.contains(">trusts people<"));
    assert!(!xml.contains("laughed at first"));
    assert!(
        xml.contains("<w:trHeight w:val=\"2000\"/>"),
        "the old question's box"
    );

    // Out of bounds or an unknown type: refused.
    let mut bad = content.clone();
    bad["blocks"][bi]["items"][ii]["organizer"]["sections"] = json!((0..7)
        .map(|_| json!({ "label": "x", "cells": [] }))
        .collect::<Vec<_>>());
    let (s, _) = app
        .put(
            &format!("/api/worksheets/{id}"),
            Some(&t),
            json!({ "content": bad }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let mut bad = content.clone();
    bad["blocks"][bi]["items"][ii]["organizer"]["type"] = json!("fishbone");
    let (s, _) = app
        .put(
            &format!("/api/worksheets/{id}"),
            Some(&t),
            json!({ "content": bad }),
        )
        .await;
    assert!(s.is_client_error(), "{s}");
}
