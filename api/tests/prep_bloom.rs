//! 備課對話 meets Bloom (docs/API.md「Phase 2.6」, the additions of Phase 2.7), with the fake
//! provider: the `comprehension`, `tiered` and `kit` turns polled to done, the session's options
//! (validation, echo, PATCH) and request passed into every worksheet it makes, the activity plan's
//! Bloom levels (`prep_activity` version 2), the kit's DOCX export and its failure modes, a revise
//! that keeps the tiers of a tiered worksheet, and the order of the suggested next steps.

#[macro_use]
mod common;

use std::time::Duration;

use axum::http::{Method, StatusCode};
use serde_json::{json, Value};

const P: [&str; 4] = [
    "Mrs. Lin put an umbrella stand outside her shop on a rainy Monday. A sign said, “Take one if you need it.”",
    "Her neighbors laughed at the idea. They were sure that every umbrella would be gone within a week.",
    "In the first month, twelve of the thirty umbrellas did disappear. However, something unexpected followed.",
    "Today the stand holds more umbrellas than it did at the start. Mrs. Lin says trust is like an umbrella.",
];

fn passage(marker: &str) -> String {
    format!("{}\n\n{}\n\n{}\n\n{} {marker}", P[0], P[1], P[2], P[3])
}

fn material(marker: &str) -> Value {
    json!({ "title": "The Umbrella Stand", "text": passage(marker) })
}

async fn create(app: &common::TestApp, t: &str, body: Value) -> Value {
    let (s, v) = app.post("/api/prep-sessions", Some(t), body).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    v
}

async fn wait(app: &common::TestApp, t: &str, id: i64) -> Value {
    for _ in 0..1200 {
        let (s, v) = app.get(&format!("/api/prep-sessions/{id}"), Some(t)).await;
        assert_eq!(s, StatusCode::OK, "{v}");
        if v["running"] == false {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("prep session {id} is still running");
}

async fn post_turn(app: &common::TestApp, t: &str, id: i64, body: Value) -> (StatusCode, Value) {
    app.post(&format!("/api/prep-sessions/{id}/turns"), Some(t), body)
        .await
}

async fn patch(app: &common::TestApp, t: &str, id: i64, body: Value) -> (StatusCode, Value) {
    app.send(
        Method::PATCH,
        &format!("/api/prep-sessions/{id}"),
        Some(t),
        Some("application/json"),
        body.to_string().into_bytes(),
    )
    .await
}

fn last(session: &Value) -> Value {
    session["turns"].as_array().unwrap().last().unwrap().clone()
}

fn sid(session: &Value) -> i64 {
    session["id"].as_i64().unwrap()
}

async fn worksheet(app: &common::TestApp, t: &str, turn: &Value) -> Value {
    let id = turn["worksheet"]["id"]
        .as_i64()
        .expect("the turn has a worksheet");
    let (s, v) = app.get(&format!("/api/worksheets/{id}"), Some(t)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v
}

fn blocks(ws: &Value) -> Vec<Value> {
    ws["content"]["blocks"].as_array().cloned().unwrap()
}

fn titles(ws: &Value) -> Vec<String> {
    blocks(ws)
        .iter()
        .map(|b| b["title"].as_str().unwrap().to_string())
        .collect()
}

fn message(v: &Value) -> String {
    v["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

async fn calls(app: &common::TestApp, module: &str) -> Vec<(i32, String, Value)> {
    sqlx::query_as(
        "SELECT attempt, status, input_snapshot FROM ai_interactions WHERE module = $1 ORDER BY id",
    )
    .bind(module)
    .fetch_all(&app.db)
    .await
    .unwrap()
}

/// The percent-encoding of `Content-Disposition`'s `filename*`.
fn encoded(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"._-~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

async fn kit_raw(
    app: &common::TestApp,
    t: &str,
    id: i64,
    turn_id: i64,
    query: &str,
) -> common::RawResponse {
    download_raw(
        app,
        t,
        &format!("/api/prep-sessions/{id}/turns/{turn_id}/kit{query}"),
    )
    .await
}

async fn activity_raw(
    app: &common::TestApp,
    t: &str,
    id: i64,
    turn_id: i64,
    query: &str,
) -> common::RawResponse {
    download_raw(
        app,
        t,
        &format!("/api/prep-sessions/{id}/turns/{turn_id}/activity{query}"),
    )
    .await
}

async fn download_raw(app: &common::TestApp, t: &str, path: &str) -> common::RawResponse {
    let auth = format!("Bearer {t}");
    app.send_raw(
        Method::GET,
        path,
        &[("authorization", auth.as_str())],
        Vec::new(),
    )
    .await
}

fn document_xml(body: Vec<u8>) -> String {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(body)).expect("a zip");
    let mut xml = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("word/document.xml").unwrap(), &mut xml)
        .unwrap();
    xml
}

async fn ready_template(app: &common::TestApp, t: &str) -> i64 {
    let (s, m) = app
        .post(
            "/api/materials",
            Some(t),
            json!({ "kind": "text", "title": "L2 Reading Worksheet",
                    "text": "Name: ____\n\n## Warm-up: [主題]\nWhat do you know?\n\n## Reading: [主題]\n1. Why?" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{m}");
    let (s, v) = app
        .post(
            "/api/worksheet-templates",
            Some(t),
            json!({ "material_id": m["id"] }),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    let id = v["id"].as_i64().unwrap();
    for _ in 0..400 {
        let (_, v) = app
            .get(&format!("/api/worksheet-templates/{id}"), Some(t))
            .await;
        if v["status"] == "ready" {
            return id;
        }
        assert_ne!(v["status"], "failed", "{v}");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("template did not get ready");
}

// ---------------------------------------------------------------- comprehension and tiered

#[tokio::test]
async fn comprehension_and_tiered_turns_carry_the_options_and_the_request() {
    let app = test_app!();
    let t = app.teacher("T-PB-1", "general").await;
    let template_id = ready_template(&app, &t).await;
    let request = "多出一點關於鄰居想法改變的題目";
    let objective = "能說出鄰居對雨傘架的想法怎麼改變";
    let started = create(
        &app,
        &t,
        json!({
            "kind": "comprehension", "request": request, "material": material(""),
            "template_id": template_id,
            "options": {
                "bloom": ["evaluate", "remember", "analyze", "remember"],
                "question_count": 5,
                "answer_formats": ["簡答題", "選擇題"],
                "content_objective": format!("  {objective}  "),
            },
        }),
    )
    .await;
    let id = sid(&started);
    // Echoed in their order, without the keys that are not set.
    assert_eq!(
        started["options"],
        json!({
            "bloom": ["remember", "analyze", "evaluate"],
            "question_count": 5,
            "answer_formats": ["選擇題", "簡答題"],
            "content_objective": objective,
        })
    );
    let first = last(&started);
    assert_eq!(first["kind"], "comprehension");
    assert_eq!(first["request"], request);
    assert!(
        first["worksheet"]["id"].is_i64(),
        "the worksheet exists at once"
    );

    let done = wait(&app, &t, id).await;
    let turn = last(&done);
    assert_eq!(turn["status"], "done", "{turn}");
    assert_eq!(turn["worksheet"]["running"], false);
    assert_eq!(done["next"], json!(["tiered"]));
    let ws = worksheet(&app, &t, &turn).await;
    let s = &ws["settings"];
    assert_eq!(s["output"], "comprehension");
    assert_eq!(s["bloom"], json!(["remember", "analyze", "evaluate"]));
    assert_eq!(s["question_count"], 5);
    assert_eq!(s["answer_formats"], json!(["選擇題", "簡答題"]));
    assert_eq!(s["content_objective"], objective);
    assert_eq!(ws["request"], request);
    assert_eq!(
        ws["template"],
        Value::Null,
        "templates only make the full worksheet"
    );
    assert_eq!(ws["prep_session_id"], id);
    assert_eq!(
        titles(&ws),
        ["Find the Facts", "Look Closer", "What Do You Think?"]
    );
    let counts: Vec<usize> = blocks(&ws)
        .iter()
        .map(|b| b["items"].as_array().unwrap().len())
        .collect();
    assert_eq!(counts, [2, 2, 1]);
    // The request and the objective reached the level calls.
    let level_calls = calls(&app, "worksheet_bloom_level").await;
    assert_eq!(level_calls.len(), 3);
    let logged = level_calls[0].2.to_string();
    assert!(
        logged.contains(request) && logged.contains(objective),
        "{logged}"
    );

    // A chip: the request is the chip's label, the worksheet gets the session's request.
    let (s, v) = post_turn(&app, &t, id, json!({ "kind": "tiered" })).await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    assert_eq!(last(&v)["request"], "差異化學習單");
    let done = wait(&app, &t, id).await;
    let turn = last(&done);
    assert_eq!(turn["status"], "done", "{turn}");
    assert_eq!(done["next"], json!([]));
    let ws = worksheet(&app, &t, &turn).await;
    assert_eq!(ws["settings"]["output"], "tiered");
    assert_eq!(ws["request"], request);
    assert_eq!(ws["settings"]["content_objective"], objective);
    assert_eq!(
        ws["content"]["levels"],
        json!(["basic", "standard", "advanced"])
    );
    let levels: Vec<(String, Value)> = blocks(&ws)
        .iter()
        .flat_map(|b| b["items"].as_array().unwrap().clone())
        .map(|i| {
            (
                i["bloom"].as_str().unwrap().to_string(),
                i["levels"].clone(),
            )
        })
        .collect();
    // 差異化學習單: every question in all three versions (T2).
    for (bloom, l) in &levels {
        assert_eq!(*l, json!(["basic", "standard", "advanced"]), "{bloom}");
    }

    // A chip with words of its own: those are this worksheet's request.
    let (s, _) = post_turn(
        &app,
        &t,
        id,
        json!({ "kind": "comprehension", "request": "只要兩個層次" }),
    )
    .await;
    assert_eq!(s, StatusCode::ACCEPTED);
    let done = wait(&app, &t, id).await;
    let ws = worksheet(&app, &t, &last(&done)).await;
    assert_eq!(ws["request"], "只要兩個層次");

    // The full worksheet still uses the session's template, with the options.
    let (s, _) = post_turn(&app, &t, id, json!({ "kind": "worksheet" })).await;
    assert_eq!(s, StatusCode::ACCEPTED);
    let done = wait(&app, &t, id).await;
    assert_eq!(last(&done)["status"], "done", "{done}");
    let ws = worksheet(&app, &t, &last(&done)).await;
    assert_eq!(ws["template"]["id"], template_id);
    assert_eq!(ws["settings"]["output"], "worksheet");
    assert_eq!(
        ws["settings"]["answer_formats"],
        json!(["選擇題", "簡答題"])
    );
    assert_eq!(ws["request"], request);
    assert_eq!(done["next"], json!([]));
}

#[tokio::test]
async fn a_bare_label_is_no_request() {
    let app = test_app!();
    let t = app.teacher("T-PB-2", "general").await;
    // The home page sends its chip label when nothing was typed.
    let session = create(
        &app,
        &t,
        json!({ "kind": "tiered", "request": "分層練習", "material": material("") }),
    )
    .await;
    assert_eq!(session["options"], json!({}));
    let done = wait(&app, &t, sid(&session)).await;
    let turn = last(&done);
    assert_eq!(turn["request"], "分層練習");
    let ws = worksheet(&app, &t, &turn).await;
    assert_eq!(ws["request"], "");
    assert_eq!(ws["settings"]["output"], "tiered");
    assert_eq!(ws["settings"]["bloom"], json!([]));
    assert_eq!(ws["settings"]["question_count"], Value::Null);
    assert_eq!(ws["settings"]["content_objective"], Value::Null);
    // 差異化學習單 default: 6 questions, one per level, each in all three versions.
    let n: usize = blocks(&ws)
        .iter()
        .map(|b| b["items"].as_array().unwrap().len())
        .sum();
    assert_eq!(n, 6);

    // The new name is a bare label too.
    let session = create(
        &app,
        &t,
        json!({ "kind": "tiered", "request": "差異化學習單", "material": material("") }),
    )
    .await;
    let done = wait(&app, &t, sid(&session)).await;
    let ws = worksheet(&app, &t, &last(&done)).await;
    assert_eq!(ws["request"], "");
}

// ---------------------------------------------------------------- options

#[tokio::test]
async fn options_are_validated_echoed_and_patched() {
    let app = test_app!();
    let t = app.teacher("T-PB-3", "general").await;
    let bad = |options: Value| {
        let app = &app;
        let t = &t;
        async move {
            let (s, v) = app
                .post(
                    "/api/prep-sessions",
                    Some(t),
                    json!({ "kind": "comprehension", "material": material(""), "options": options }),
                )
                .await;
            assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
            message(&v)
        }
    };
    assert!(bad(json!({ "bloom": ["thinking"] }))
        .await
        .contains("不是 Bloom 認知層次的選項"));
    assert!(bad(json!({ "question_count": 2 }))
        .await
        .contains("題數需為 3 到 30 題"));
    assert!(bad(json!({ "question_count": 31 }))
        .await
        .contains("題數需為 3 到 30 題"));
    assert!(
        bad(json!({ "bloom_counts": { "remember": 1, "create": 1 } }))
            .await
            .contains("合計 3 到 30 題")
    );
    assert!(bad(json!({ "bloom_counts": { "apply": 13 } }))
        .await
        .contains("0 到 12 題"));
    assert!(bad(json!({ "answer_formats": ["連連看"] }))
        .await
        .contains("連連看"));
    assert!(bad(json!({ "content_objective": "字".repeat(301) }))
        .await
        .contains("內容目標最多 300 字"));
    assert!(bad(json!({ "language_objective": "字".repeat(301) }))
        .await
        .contains("語言目標最多 300 字"));
    let count = |table: &'static str| {
        let db = app.db.clone();
        async move {
            sqlx::query_scalar::<_, i64>(&format!("SELECT COUNT(*) FROM {table}"))
                .fetch_one(&db)
                .await
                .unwrap()
        }
    };
    assert_eq!(count("prep_sessions").await, 0);
    assert_eq!(count("materials").await, 0, "nothing stored on a 400");

    // Explicit `null`s are the same as leaving the keys out.
    let session = create(
        &app,
        &t,
        json!({ "kind": "resources", "topic": "trust",
                "options": { "bloom": null, "question_count": null, "content_objective": "" } }),
    )
    .await;
    let id = sid(&session);
    assert_eq!(session["options"], json!({}));
    wait(&app, &t, id).await;

    let (s, v) = patch(&app, &t, id, json!({ "options": { "question_count": 6 } })).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["options"], json!({ "question_count": 6 }));
    let (_, v) = patch(&app, &t, id, json!({ "title": "信任" })).await;
    assert_eq!(v["options"], json!({ "question_count": 6 }), "absent: kept");
    let (s, v) = patch(
        &app,
        &t,
        id,
        json!({ "options": { "bloom_counts": { "apply": 3, "remember": 0 } } }),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(
        v["options"],
        json!({ "bloom_counts": { "remember": 0, "apply": 3 } }),
        "replaced as a whole"
    );
    let (s, v) = patch(&app, &t, id, json!({ "options": { "question_count": 99 } })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    let (_, v) = app.get(&format!("/api/prep-sessions/{id}"), Some(&t)).await;
    assert_eq!(
        v["options"],
        json!({ "bloom_counts": { "remember": 0, "apply": 3 } })
    );
    let (s, v) = patch(&app, &t, id, json!({ "options": null })).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["options"], json!({}));

    // Options patched in go into the next worksheet.
    let session = create(
        &app,
        &t,
        json!({ "kind": "activity", "material": material("") }),
    )
    .await;
    let id = sid(&session);
    wait(&app, &t, id).await;
    let (s, _) = patch(
        &app,
        &t,
        id,
        json!({ "options": { "bloom_counts": { "apply": 3 }, "answer_formats": ["填空題"] } }),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = post_turn(&app, &t, id, json!({ "kind": "comprehension" })).await;
    assert_eq!(s, StatusCode::ACCEPTED);
    let done = wait(&app, &t, id).await;
    let ws = worksheet(&app, &t, &last(&done)).await;
    assert_eq!(ws["settings"]["bloom_counts"], json!({ "apply": 3 }));
    assert_eq!(titles(&ws), ["Use It"]);
    let items = blocks(&ws)[0]["items"].as_array().unwrap().clone();
    assert_eq!(items.len(), 3);
    assert!(items.iter().all(|i| i["answer_format"] == "填空題"));
}

// ---------------------------------------------------------------- activity v2

#[tokio::test]
async fn the_activity_plan_tags_its_steps_with_bloom_levels() {
    let app = test_app!();
    let t = app.teacher("T-PB-4", "general").await;
    let objective = "能用 trust 說出自己的經驗";
    let request = "活動要有小組討論";
    let session = create(
        &app,
        &t,
        json!({ "kind": "activity", "request": request, "material": material(""),
                "options": { "content_objective": objective } }),
    )
    .await;
    let id = sid(&session);
    let done = wait(&app, &t, id).await;
    let turn = last(&done);
    assert_eq!(turn["status"], "done", "{turn}");
    let steps = turn["result"]["steps"].as_array().unwrap().clone();
    let bloom: Vec<Value> = steps.iter().map(|s| s["bloom"].clone()).collect();
    assert_eq!(
        bloom,
        [
            json!(["remember", "understand"]),
            // At most three, the first ones written, in Bloom order.
            json!(["understand", "apply", "analyze"]),
            json!(["evaluate", "create"]),
        ]
    );
    // The latest version of the prompt (5: the 語言目標 follow from the 內容目標; 4 was SIOP), with
    // the objective and the request. The teacher
    // gave a 內容目標 only: it is passed as is; the 語言目標 is left for the model to draft.
    let rows = calls(&app, "prep_activity").await;
    assert_eq!(rows.len(), 1);
    let snapshot = &rows[0].2;
    assert_eq!(snapshot["prompt"]["version_no"], 5);
    // Version 4 and later name the two fields; the combined line still carries the 內容目標.
    assert_eq!(
        snapshot["inputs"]["teacher_objective"],
        format!("內容目標：{objective}").as_str()
    );
    assert_eq!(snapshot["inputs"]["teacher_content_objective"], objective);
    assert_eq!(snapshot["inputs"]["teacher_request"], request);
    let user = snapshot["prompt"]["user"].as_str().unwrap();
    assert!(
        user.contains(&format!("學習目標（教師寫的）：內容目標：{objective}")),
        "{user}"
    );
    assert!(
        user.contains(&format!(
            "教師的內容目標：{objective}\n教師的語言目標：（沒有）"
        )),
        "{user}"
    );
    assert!(user.contains(&format!("教師的要求：{request}")), "{user}");
    assert!(!snapshot.to_string().contains("umbrella stand outside"));

    // A later chip: the session's request; nothing set reads 「（沒有）」.
    let plain = create(
        &app,
        &t,
        json!({ "kind": "activity", "request": "課堂活動", "material": material("") }),
    )
    .await;
    wait(&app, &t, sid(&plain)).await;
    let rows = calls(&app, "prep_activity").await;
    assert_eq!(rows[1].2["inputs"]["teacher_objective"], "（沒有）");
    assert_eq!(rows[1].2["inputs"]["teacher_request"], "（沒有）");
}

#[tokio::test]
async fn an_activity_downloads_as_its_teaching_flow() {
    let app = test_app!();
    let t = app.teacher("T-PB-11", "general").await;
    let other = app.teacher("T-PB-12", "general").await;
    let session = create(
        &app,
        &t,
        json!({ "kind": "activity", "request": "課堂活動", "material": material("") }),
    )
    .await;
    let id = sid(&session);
    let done = wait(&app, &t, id).await;
    let turn = last(&done);
    assert_eq!(turn["status"], "done", "{turn}");
    let turn_id = turn["id"].as_i64().unwrap();

    let r = activity_raw(&app, &t, id, turn_id, "?format=docx").await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.header("content-type").contains("wordprocessingml"));
    let cd = r.header("content-disposition").to_string();
    assert!(
        cd.contains(&encoded("The Umbrella Stand_課堂活動.docx")),
        "{cd}"
    );
    assert_eq!(&r.body[..2], b"PK");
    let xml = document_xml(r.body);
    let at = |s: &str| {
        xml.find(s)
            .unwrap_or_else(|| panic!("{s} is not in the activity"))
    };
    // The 教學流程 of a kit and nothing else: the plan's title, its goals, every step in order.
    assert!(at("The Umbrella Stand：課堂活動") < at("教學流程"));
    assert!(at("教學流程") < at(turn["result"]["title"].as_str().unwrap()));
    assert!(at("教學流程") < at("學習目標"));
    let mut previous = at("學習目標");
    for (i, step) in turn["result"]["steps"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let head = format!(
            "{}. {}｜{}",
            i + 1,
            step["phase"].as_str().unwrap(),
            step["title"].as_str().unwrap()
        );
        assert!(at(&head) > previous, "{head}");
        previous = at(&head);
    }
    assert!(!xml.contains("學生版") && !xml.contains("差異化學習單"));
    // Without a format it is a DOCX too.
    assert_eq!(
        activity_raw(&app, &t, id, turn_id, "").await.status,
        StatusCode::OK
    );

    // Errors: another format, another teacher, a turn of another session, no sign-in.
    assert_eq!(
        activity_raw(&app, &t, id, turn_id, "?format=pdf")
            .await
            .status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        activity_raw(&app, &other, id, turn_id, "?format=docx")
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        activity_raw(&app, &t, id, 999_999, "?format=docx")
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    let r = app
        .send_raw(
            Method::GET,
            &format!("/api/prep-sessions/{id}/turns/{turn_id}/activity?format=docx"),
            &[],
            Vec::new(),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);

    // A turn that is not a 課堂活動 has none to download.
    let (s, _) = post_turn(&app, &t, id, json!({ "kind": "comprehension" })).await;
    assert_eq!(s, StatusCode::ACCEPTED);
    let done = wait(&app, &t, id).await;
    let other_turn = last(&done)["id"].as_i64().unwrap();
    let r = activity_raw(&app, &t, id, other_turn, "?format=docx").await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);

    // A failed 課堂活動 has no plan: nothing to download.
    let failed = create(
        &app,
        &t,
        json!({ "kind": "activity", "material": material("FAKE_PREP_NO_PLAN") }),
    )
    .await;
    let failed_id = sid(&failed);
    let turn = last(&wait(&app, &t, failed_id).await);
    assert_eq!(turn["status"], "failed", "{turn}");
    let r = activity_raw(&app, &t, failed_id, turn["id"].as_i64().unwrap(), "").await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    let v: Value = serde_json::from_slice(&r.body).unwrap();
    assert!(message(&v).contains("沒有完成"), "{v}");

    // While it runs: 409.
    let slow = create(
        &app,
        &t,
        json!({ "kind": "activity", "request": "慢一點 FAKE_SLOW", "material": material("") }),
    )
    .await;
    let turn = last(&slow);
    assert_eq!(turn["status"], "running");
    let r = activity_raw(&app, &t, sid(&slow), turn["id"].as_i64().unwrap(), "").await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    let v: Value = serde_json::from_slice(&r.body).unwrap();
    assert!(message(&v).contains("還在產生"), "{v}");

    let exported: Vec<Value> = sqlx::query_scalar(
        "SELECT payload FROM usage_logs WHERE event = 'prep_activity_exported' ORDER BY id",
    )
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert_eq!(exported.len(), 2);
    assert_eq!(exported[0]["session_id"], id);
    assert_eq!(exported[0]["turn_id"], turn_id);
    assert!(exported[0]["bytes"].as_i64().unwrap() > 0);
}

// ---------------------------------------------------------------- 教學套件

#[tokio::test]
async fn a_kit_makes_a_plan_and_a_tiered_worksheet_and_exports_them() {
    let app = test_app!();
    let t = app.teacher("T-PB-5", "general").await;
    let other = app.teacher("T-PB-6", "general").await;
    let session = create(
        &app,
        &t,
        json!({ "kind": "kit", "request": "教學套件", "material": material(""),
                "options": { "bloom_counts": { "remember": 2, "understand": 1, "analyze": 1, "create": 1 } } }),
    )
    .await;
    let id = sid(&session);
    let first = last(&session);
    assert_eq!(first["kind"], "kit");
    assert_eq!(first["status"], "running");
    assert!(
        first["worksheet"]["id"].is_i64(),
        "the worksheet exists at once"
    );
    assert_eq!(first["error"], Value::Null);

    let done = wait(&app, &t, id).await;
    let turn = last(&done);
    assert_eq!(turn["status"], "done", "{turn}");
    assert_eq!(turn["error"], Value::Null);
    assert!(turn["interaction_id"].is_i64());
    let plan = &turn["result"];
    assert_eq!(plan["title"], "The Umbrella Stand 教學活動規劃（測試）");
    assert_eq!(plan["steps"][0]["bloom"], json!(["remember", "understand"]));
    let ws = worksheet(&app, &t, &turn).await;
    assert_eq!(ws["settings"]["output"], "tiered");
    assert_eq!(ws["request"], "");
    assert_eq!(
        ws["content"]["levels"],
        json!(["basic", "standard", "advanced"])
    );
    assert_eq!(
        titles(&ws),
        [
            "Find the Facts",
            "Understand the Text",
            "Look Closer",
            "Make Something New"
        ]
    );
    assert_eq!(done["next"], json!([]));

    fix_student_order(&app, ws["id"].as_i64().unwrap()).await;
    // The download.
    let turn_id = turn["id"].as_i64().unwrap();
    let r = kit_raw(&app, &t, id, turn_id, "?format=docx").await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.header("content-type").contains("wordprocessingml"));
    let cd = r.header("content-disposition").to_string();
    assert!(
        cd.contains(&encoded("The Umbrella Stand_教學套件.docx")),
        "{cd}"
    );
    assert_eq!(&r.body[..2], b"PK");
    let xml = document_xml(r.body);
    let at = |s: &str| {
        xml.find(s)
            .unwrap_or_else(|| panic!("{s} is not in the kit"))
    };
    // Student pages carry the neutral code (T9); then a teacher version per level.
    assert!(at("教學流程") < at("學生版（A 版）"));
    assert!(at("學生版（A 版）") < at("學生版（B 版）"));
    assert!(at("學生版（B 版）") < at("學生版（C 版）"));
    assert!(at("學生版（C 版）") < at("教師版（基礎版＝A 版）"));
    assert!(at("教師版（基礎版＝A 版）") < at("教師版（一般版＝B 版）"));
    assert!(at("教師版（一般版＝B 版）") < at("教師版（進階版＝C 版）"));
    let teacher = at("教師版（基礎版＝A 版）");
    for name in ["學生版（基礎版）", "學生版（一般版）", "學生版（進階版）"]
    {
        assert!(!xml.contains(name), "{name}");
    }
    // The plan with its Bloom levels; the A version's support; tags, answers and scoring notes
    // only in the teacher versions.
    assert!(at("The Umbrella Stand 教學活動規劃（測試）") < at("學生版（A 版）"));
    assert!(at("Bloom：記憶、理解") < at("學生版（A 版）"));
    assert!(at("Version A") > at("學生版（A 版）") && at("Version A") < at("學生版（B 版）"));
    assert!(at("Sentence frame: ") > at("學生版（A 版）"));
    assert!(at("〔Bloom：") > teacher);
    assert!(at("Answer: ") > teacher);
    assert!(at("Scoring: ") > teacher);
    assert!(xml.contains("〔Bloom：創造 Create〕"));
    // The goals: 內容目標 then 語言目標, each goal in Chinese with its English under it.
    assert!(at("學習目標") < at("內容目標"));
    assert!(at("內容目標") < at("學生能說出「The Umbrella Stand」各段的重點。"));
    assert!(
        at("學生能說出「The Umbrella Stand」各段的重點。")
            < at("Students will be able to tell the main point of each paragraph of the text.")
    );
    assert!(at("Students will be able to tell the main point") < at("語言目標"));
    assert!(at("語言目標") < at("Students will be able to use because and so"));
    assert!(at("語言目標") < at("1. 引起動機｜"));
    // Without a format it is a DOCX too.
    assert_eq!(
        kit_raw(&app, &t, id, turn_id, "").await.status,
        StatusCode::OK
    );

    // Errors: another format, another teacher, a turn of another session.
    assert_eq!(
        kit_raw(&app, &t, id, turn_id, "?format=pdf").await.status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        kit_raw(&app, &other, id, turn_id, "?format=docx")
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        kit_raw(&app, &t, id, 999_999, "?format=docx").await.status,
        StatusCode::NOT_FOUND
    );
    let r = app
        .send_raw(
            Method::GET,
            &format!("/api/prep-sessions/{id}/turns/{turn_id}/kit?format=docx"),
            &[],
            Vec::new(),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);

    // A plan stored before the goal groups (one Chinese line per goal) is still exported.
    sqlx::query(
        "UPDATE prep_turns SET result = jsonb_set(result, '{goals}', $2::jsonb) WHERE id = $1",
    )
    .bind(turn_id)
    .bind(json!([
        { "text": "學生能說出林太太設置傘架的原因。", "curriculum_codes": [] },
        { "text": "學生能用課文句型寫出兩句話。", "curriculum_codes": [] },
    ]))
    .execute(&app.db)
    .await
    .unwrap();
    let r = kit_raw(&app, &t, id, turn_id, "?format=docx").await;
    assert_eq!(r.status, StatusCode::OK);
    let xml = document_xml(r.body);
    let at = |s: &str| {
        xml.find(s)
            .unwrap_or_else(|| panic!("{s} is not in the kit"))
    };
    assert!(at("學習目標") < at("學生能說出林太太設置傘架的原因。"));
    assert!(at("學生能說出林太太設置傘架的原因。") < at("學生能用課文句型寫出兩句話。"));
    assert!(!xml.contains("內容目標") && !xml.contains("語言目標"));

    // The bottom bar revises the kit's worksheet.
    let ws_id = ws["id"].as_i64().unwrap();
    let (s, v) = post_turn(
        &app,
        &t,
        id,
        json!({ "kind": "revise", "request": "第一題改難一點" }),
    )
    .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    assert_eq!(last(&v)["kind"], "revise");
    let done = wait(&app, &t, id).await;
    let revised = last(&done);
    assert_eq!(revised["status"], "done", "{revised}");
    assert_eq!(
        revised["result"]["version_no"],
        ws["versions_count"].as_i64().unwrap() + 1
    );
    let (_, after) = app.get(&format!("/api/worksheets/{ws_id}"), Some(&t)).await;
    assert!(blocks(&after)[0]["items"][0]["prompt"]
        .as_str()
        .unwrap()
        .ends_with(" (revised)"));

    // A turn that is not a kit has no kit.
    let (s, _) = post_turn(&app, &t, id, json!({ "kind": "comprehension" })).await;
    assert_eq!(s, StatusCode::ACCEPTED);
    let done = wait(&app, &t, id).await;
    let other_turn = last(&done)["id"].as_i64().unwrap();
    let r = kit_raw(&app, &t, id, other_turn, "?format=docx").await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let exported: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM usage_logs WHERE event = 'prep_kit_exported'")
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(exported, 3);
}

#[tokio::test]
async fn a_running_kit_cannot_be_downloaded() {
    let app = test_app!();
    let t = app.teacher("T-PB-7", "general").await;
    // Every call waits: the kit stays running for a while.
    let session = create(
        &app,
        &t,
        json!({ "kind": "kit", "request": "慢一點 FAKE_SLOW", "material": material(""),
                "options": { "bloom_counts": { "remember": 3 } } }),
    )
    .await;
    let id = sid(&session);
    let turn = last(&session);
    assert_eq!(turn["status"], "running");
    let r = kit_raw(&app, &t, id, turn["id"].as_i64().unwrap(), "?format=docx").await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    let v: Value = serde_json::from_slice(&r.body).unwrap();
    assert!(message(&v).contains("還在產生"), "{v}");
    // Another turn waits for it.
    let (s, _) = post_turn(&app, &t, id, json!({ "kind": "activity" })).await;
    assert_eq!(s, StatusCode::CONFLICT);
}

#[tokio::test]
async fn a_kit_with_a_failed_part_keeps_the_other() {
    let app = test_app!();
    let t = app.teacher("T-PB-8", "general").await;

    // A long material (41 paragraphs) is not refused: both parts are made.
    let long: Vec<String> = (1..=41)
        .map(|i| format!("Paragraph number {i} says something about umbrellas."))
        .collect();
    let session = create(
        &app,
        &t,
        json!({ "kind": "kit", "material": { "title": "Long", "text": long.join("\n\n") } }),
    )
    .await;
    let id = sid(&session);
    let done = wait(&app, &t, id).await;
    let turn = last(&done);
    assert_eq!(turn["status"], "done", "{turn}");
    assert_eq!(turn["result"]["steps"].as_array().unwrap().len(), 3);

    // The plan fails: the worksheet is kept.
    let session = create(
        &app,
        &t,
        json!({ "kind": "kit", "material": material("FAKE_PREP_NO_PLAN") }),
    )
    .await;
    let id = sid(&session);
    let done = wait(&app, &t, id).await;
    let turn = last(&done);
    assert_eq!(turn["status"], "failed", "{turn}");
    let error = turn["error"].as_str().unwrap();
    assert!(
        error.starts_with("教學套件的課堂活動沒有完成：") && error.contains("再試一次"),
        "{error}"
    );
    assert_eq!(turn["result"], Value::Null);
    let ws = worksheet(&app, &t, &turn).await;
    assert_eq!(blocks(&ws).len(), 6);
    let r = kit_raw(&app, &t, id, turn["id"].as_i64().unwrap(), "?format=docx").await;
    assert_eq!(r.status, StatusCode::OK);
    let xml = document_xml(r.body);
    assert!(xml.contains("課堂活動沒有完成") && xml.contains("學生版（C 版）"));
    assert_eq!(done["next"], json!([]));

    // Both fail: one message for both, nothing to download.
    let session = create(
        &app,
        &t,
        json!({ "kind": "kit", "material": material("FAKE_BLOCK") }),
    )
    .await;
    let id = sid(&session);
    let done = wait(&app, &t, id).await;
    let turn = last(&done);
    assert_eq!(turn["status"], "failed", "{turn}");
    let error = turn["error"].as_str().unwrap();
    assert!(
        error.starts_with("教學套件沒有完成。課堂活動：") && error.contains("差異化學習單："),
        "{error}"
    );
    let r = kit_raw(&app, &t, id, turn["id"].as_i64().unwrap(), "?format=docx").await;
    assert_eq!(r.status, StatusCode::CONFLICT);
}

// ---------------------------------------------------------------- revise on a tiered worksheet

fn item_levels(ws: &Value) -> Vec<Vec<(String, Value, Value)>> {
    blocks(ws)
        .iter()
        .map(|b| {
            b["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|i| {
                    (
                        i["bloom"].as_str().unwrap_or_default().to_string(),
                        i["levels"].clone(),
                        i["hint"].clone(),
                    )
                })
                .collect()
        })
        .collect()
}

#[tokio::test]
async fn revise_keeps_the_tiers_of_a_tiered_worksheet() {
    let app = test_app!();
    let t = app.teacher("T-PB-9", "general").await;
    let session = create(
        &app,
        &t,
        json!({ "kind": "tiered", "material": material("") }),
    )
    .await;
    let id = sid(&session);
    let done = wait(&app, &t, id).await;
    let ws_turn = last(&done);
    let before = worksheet(&app, &t, &ws_turn).await;
    let ws_id = before["id"].as_i64().unwrap();
    let tiers = item_levels(&before);
    assert_eq!(
        tiers[0][0].1,
        json!(["basic", "standard", "advanced"]),
        "差異化學習單: every version"
    );

    // The first block with items (記憶): its items keep their versions, labels and hints.
    let (s, v) = post_turn(
        &app,
        &t,
        id,
        json!({ "kind": "revise", "request": "第一題改難一點" }),
    )
    .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    let done = wait(&app, &t, id).await;
    assert_eq!(last(&done)["status"], "done", "{}", last(&done));
    assert_eq!(
        last(&done)["result"]["changed_block_ids"],
        json!([blocks(&before)[0]["id"]])
    );
    let after = worksheet(&app, &t, &ws_turn).await;
    assert_eq!(item_levels(&after), tiers, "the tiers stay");
    assert!(blocks(&after)[0]["items"][0]["prompt"]
        .as_str()
        .unwrap()
        .ends_with(" (revised)"));

    // The last block (創造) gets a new item: in every version, with the block's level.
    let (s, v) = post_turn(
        &app,
        &t,
        id,
        json!({ "kind": "revise", "request": "最後一區加一題 FAKE_PREP_REVISE_ADD" }),
    )
    .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    let done = wait(&app, &t, id).await;
    assert_eq!(last(&done)["status"], "done", "{}", last(&done));
    let after = worksheet(&app, &t, &ws_turn).await;
    let now = item_levels(&after);
    let last_block = now.last().unwrap();
    assert_eq!(last_block.len(), tiers.last().unwrap().len() + 1);
    let added = last_block.last().unwrap();
    assert_eq!(added.0, "create");
    assert_eq!(added.1, json!(["basic", "standard", "advanced"]));
    assert_eq!(added.2, "");
    assert_eq!(
        &last_block[..last_block.len() - 1],
        &tiers.last().unwrap()[..],
        "the replaced items keep theirs"
    );
    // So the 基礎版 prints it too.
    let (_, sheet) = app
        .get(
            &format!("/api/worksheets/{ws_id}/sheet?level=basic&audience=student"),
            Some(&t),
        )
        .await;
    assert!(sheet.to_string().contains("(added)"));
    let (_, sheet) = app
        .get(
            &format!("/api/worksheets/{ws_id}/sheet?level=advanced&audience=student"),
            Some(&t),
        )
        .await;
    assert!(sheet.to_string().contains("(added)"));
}

/// T9: a generated 差異化學習單 stores its students' codes in a random order. Checks that the order
/// lists the three versions once, then fixes it to 基礎＝A, 一般＝B, 進階＝C so the rest of the test
/// can name the codes.
async fn fix_student_order(app: &common::TestApp, worksheet_id: i64) {
    let order: serde_json::Value = sqlx::query_scalar(
        "SELECT content->'student_order' FROM worksheet_versions
          WHERE worksheet_id = $1 ORDER BY version_no DESC LIMIT 1",
    )
    .bind(worksheet_id)
    .fetch_one(&app.db)
    .await
    .unwrap();
    let mut got: Vec<String> = order
        .as_array()
        .expect("a 差異化學習單 has a student_order")
        .iter()
        .map(|l| l.as_str().unwrap().to_string())
        .collect();
    got.sort();
    assert_eq!(got, ["advanced", "basic", "standard"]);
    sqlx::query(
        "UPDATE worksheet_versions
            SET content = jsonb_set(content, '{student_order}', '[\"basic\", \"standard\", \"advanced\"]')
          WHERE worksheet_id = $1",
    )
    .bind(worksheet_id)
    .execute(&app.db)
    .await
    .unwrap();
}
