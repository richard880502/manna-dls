//! Long and hard-wrapped materials: pasted text copied out of a PDF becomes a few paragraphs,
//! the 40-paragraph refusal is gone, a worksheet can use only some of a material's paragraphs,
//! and a pasted material can be edited in place (docs/API.md「長教材」).

#[macro_use]
mod common;

use std::time::Duration;

use axum::http::StatusCode;
use serde_json::{json, Value};

/// Made-up article, hard-wrapped at about 60 characters as a PDF copy would be.
fn wrapped_article(paragraphs: usize) -> String {
    let mut lines: Vec<String> = Vec::new();
    for n in 1..=paragraphs {
        lines.push(format!(
            "In chapter {n} the harbor town woke up slowly under a grey sky"
        ));
        lines.push("and the fishing boats rocked on the quiet water while the".to_string());
        lines.push("gulls circled above the old stone pier looking for food".to_string());
        lines.push("or a place to rest.".to_string());
    }
    lines.join("\n")
}

async fn wait_idle(app: &common::TestApp, t: &str, ws: i64) {
    for _ in 0..800 {
        let (_, v) = app.get(&format!("/api/worksheets/{ws}"), Some(t)).await;
        if v["running"] == false {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("worksheet {ws} is still running");
}

async fn usage(app: &common::TestApp, event: &str) -> Vec<Value> {
    sqlx::query_scalar("SELECT payload FROM usage_logs WHERE event = $1 ORDER BY id")
        .bind(event)
        .fetch_all(&app.db)
        .await
        .unwrap()
}

#[tokio::test]
async fn a_hard_wrapped_paste_becomes_a_few_paragraphs() {
    let app = test_app!();
    let t = app.teacher("T-LONG-1", "general").await;
    let text = wrapped_article(12);
    assert_eq!(text.lines().count(), 48);
    // The preview and the stored material agree.
    let (s, v) = app
        .post(
            "/api/materials/split-preview",
            Some(&t),
            json!({ "text": text }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["paragraph_count"], 12);
    let (s, m) = app
        .post(
            "/api/materials",
            Some(&t),
            json!({ "kind": "text", "title": "Harbor", "text": text }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{m}");
    assert_eq!(m["paragraphs"].as_array().unwrap().len(), 12);
    assert!(m["paragraphs"][0]["text"]
        .as_str()
        .unwrap()
        .ends_with("or a place to rest."));
    // Text that really is one line per paragraph keeps its lines.
    let (_, v) = app
        .post(
            "/api/materials/split-preview",
            Some(&t),
            json!({ "text": "Apples\nPears\nPlums\nCherries\nGrapes\nMelons" }),
        )
        .await;
    assert_eq!(v["paragraph_count"], 6);
}

#[tokio::test]
async fn a_material_text_edit_keeps_the_ids_of_unchanged_paragraphs_and_is_logged() {
    let app = test_app!();
    let t = app.teacher("T-LONG-2", "general").await;
    let (_, m) = app
        .post(
            "/api/materials",
            Some(&t),
            json!({ "kind": "text", "title": "Harbor", "text": "One.\n\nTwo.\n\nThree." }),
        )
        .await;
    let id = m["id"].as_i64().unwrap();
    let ids: Vec<String> = m["paragraphs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap().to_string())
        .collect();
    let (s, v) = app
        .put(
            &format!("/api/materials/{id}"),
            Some(&t),
            json!({ "text": "One.\n\nTwo changed.\n\nThree.", "edit_from": "prep_session" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let after: Vec<&str> = v["paragraphs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap())
        .collect();
    assert_eq!((after[0], after[2]), (ids[0].as_str(), ids[2].as_str()));
    assert_ne!(after[1], ids[1]);
    let logged = usage(&app, "material_text_edited").await;
    assert_eq!(logged.len(), 1);
    assert_eq!(logged[0]["from"], "prep_session");
    assert_eq!(logged[0]["paragraphs_after"], 3);
    assert!(logged[0].get("text").is_none());
    // An edit that changes nothing is not logged.
    let (s, _) = app
        .put(
            &format!("/api/materials/{id}"),
            Some(&t),
            json!({ "text": "One.\n\nTwo changed.\n\nThree." }),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(usage(&app, "material_text_edited").await.len(), 1);
}

#[tokio::test]
async fn a_worksheet_can_use_only_some_paragraphs_of_a_long_material() {
    let app = test_app!();
    let t = app.teacher("T-LONG-3", "general").await;
    let long: Vec<String> = (1..=50)
        .map(|i| format!("Paragraph number {i} says something about harbors."))
        .collect();
    let (_, m) = app
        .post(
            "/api/materials",
            Some(&t),
            json!({ "kind": "text", "title": "Long", "text": long.join("\n\n") }),
        )
        .await;
    let ids: Vec<String> = m["paragraphs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap().to_string())
        .collect();
    let (s, ws) = app
        .post(
            "/api/worksheets",
            Some(&t),
            json!({ "material_id": m["id"] }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{ws}");
    let wid = ws["id"].as_i64().unwrap();
    assert_eq!(ws["material"]["paragraphs"].as_array().unwrap().len(), 50);

    // 50 paragraphs generate: no refusal.
    let (s, v) = app
        .post(
            &format!("/api/worksheets/{wid}/generate"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    wait_idle(&app, &t, wid).await;

    // Pick paragraphs 10..=15 for this worksheet.
    let chosen: Vec<&String> = ids[9..15].iter().collect();
    let path = format!("/api/worksheets/{wid}/material-selection");
    let (s, v) = app
        .put(&path, Some(&t), json!({ "paragraph_ids": chosen }))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["material"]["paragraphs"].as_array().unwrap().len(), 6);
    assert_eq!(v["material"]["source"]["total_paragraphs"], 50);
    assert_eq!(
        v["material"]["source"]["selected_ids"]
            .as_array()
            .unwrap()
            .len(),
        6
    );
    let (s, v) = app
        .post(
            &format!("/api/worksheets/{wid}/generate"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    wait_idle(&app, &t, wid).await;
    // The library material is untouched.
    let (_, again) = app
        .get(&format!("/api/materials/{}", m["id"]), Some(&t))
        .await;
    assert_eq!(again["paragraphs"].as_array().unwrap().len(), 50);

    // Not a paragraph of it, or someone else's worksheet.
    let (s, _) = app
        .put(&path, Some(&t), json!({ "paragraph_ids": ["zzz"] }))
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let other = app.teacher("T-LONG-3B", "general").await;
    let (s, _) = app
        .put(&path, Some(&other), json!({ "paragraph_ids": [ids[0]] }))
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // Back to all of it.
    let (s, v) = app
        .put(&path, Some(&t), json!({ "paragraph_ids": null }))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["material"]["paragraphs"].as_array().unwrap().len(), 50);
    assert!(v["material"]["source"].get("selected_ids").is_none());
    let logged = usage(&app, "worksheet_material_selection").await;
    assert_eq!(logged.len(), 2);
    assert_eq!(
        (logged[0]["selected"].clone(), logged[0]["total"].clone()),
        (json!(6), json!(50))
    );
}

#[tokio::test]
async fn a_worksheet_from_pasted_text_has_no_library_selection() {
    let app = test_app!();
    let t = app.teacher("T-LONG-4", "general").await;
    // A direct worksheet with a blank material has nothing to select from.
    let (s, ws) = app.post("/api/worksheets", Some(&t), json!({})).await;
    assert_eq!(s, StatusCode::CREATED, "{ws}");
    let (s, v) = app
        .put(
            &format!("/api/worksheets/{}/material-selection", ws["id"]),
            Some(&t),
            json!({ "paragraph_ids": ["p1"] }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
}
