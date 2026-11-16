//! Phase 2.7 (docs/API.md「Bloom 認知層次與閱讀理解題／分層練習」), with the fake provider:
//! reading-comprehension questions and tiered practice end to end (one call per Bloom level, the
//! judge, assembly, sheet and export), the judge's relabel notice, the evidence repair,
//! validation, regenerate / undo of one level's block, the teacher's request and objective in
//! the prompts, explicit per-level counts, the teacher's answer formats, and the full worksheet
//! (which now labels its items with a Bloom level).

#[macro_use]
mod common;

use std::time::Duration;

use axum::http::{Method, StatusCode};
use serde_json::{json, Value};

const P1: &str = "Mrs. Lin put an umbrella stand outside her shop on a rainy Monday. A sign said, “Take one if you need it.”";
const P2: &str = "Her neighbors laughed at the idea. They were sure that every umbrella would be gone within a week.";
const P3: &str = "In the first month, twelve of the thirty umbrellas did disappear. However, something unexpected followed.";
const P4: &str = "Today the stand holds more umbrellas than it did at the start. Mrs. Lin says trust is like an umbrella.";

const TITLES: [&str; 6] = [
    "Part A · Find the Facts",
    "Part B · Understand the Text",
    "Part C · Use It",
    "Part D · Look Closer",
    "Part E · What Do You Think?",
    "Part F · Make Something New",
];
const LEVELS: [&str; 6] = [
    "remember",
    "understand",
    "apply",
    "analyze",
    "evaluate",
    "create",
];

fn passage(marker: &str) -> String {
    format!("{P1}\n\n{P2}\n\n{P3}\n\n{P4} {marker}")
}

async fn create(app: &common::TestApp, t: &str, body: Value) -> (StatusCode, Value) {
    app.post("/api/worksheets", Some(t), body).await
}

/// A worksheet made directly from the passage, with the given partial settings and request.
async fn made(app: &common::TestApp, t: &str, marker: &str, settings: Value, request: &str) -> i64 {
    let (s, v) = create(
        app,
        t,
        json!({
            "material": { "title": "The Umbrella Stand", "text": passage(marker) },
            "settings": settings,
            "request": request,
        }),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    v["id"].as_i64().unwrap()
}

async fn wait(app: &common::TestApp, t: &str, id: i64) -> Value {
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
    wait(app, t, id).await
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

/// The document text of a DOCX export.
async fn docx(app: &common::TestApp, t: &str, id: i64, level: &str, audience: &str) -> String {
    let auth = format!("Bearer {t}");
    let r = app
        .send_raw(
            Method::GET,
            &format!("/api/worksheets/{id}/export?format=docx&level={level}&audience={audience}"),
            &[("authorization", auth.as_str())],
            Vec::new(),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(r.body)).unwrap();
    let mut xml = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("word/document.xml").unwrap(), &mut xml)
        .unwrap();
    xml
}

fn blocks(state: &Value) -> Vec<Value> {
    state["content"]["blocks"].as_array().cloned().unwrap()
}

fn items(block: &Value) -> Vec<Value> {
    block["items"].as_array().cloned().unwrap()
}

fn strs(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .map(|x| x.as_str().unwrap_or_default().to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// (attempt, status, input_snapshot) of every call of a module, in order.
async fn calls(app: &common::TestApp, module: &str) -> Vec<(i32, String, Value)> {
    sqlx::query_as(
        "SELECT attempt, status, input_snapshot FROM ai_interactions WHERE module = $1 ORDER BY id",
    )
    .bind(module)
    .fetch_all(&app.db)
    .await
    .unwrap()
}

fn error_message(v: &Value) -> String {
    v["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

#[tokio::test]
async fn comprehension_end_to_end() {
    let app = test_app!();
    let t = app.teacher("T-BLOOM-1", "general").await;
    let request = "題目多一點關於鄰居想法的改變";
    let (s, v) = create(
        &app,
        &t,
        json!({
            "material": { "title": "The Umbrella Stand", "text": passage("") },
            "settings": { "output": "comprehension" },
            "request": request,
        }),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    assert_eq!(v["settings"]["output"], "comprehension");
    assert_eq!(v["settings"]["bloom"], json!([]));
    assert_eq!(v["settings"]["question_count"], Value::Null);
    assert_eq!(v["settings"]["bloom_counts"], Value::Null);
    assert_eq!(v["settings"]["objective"], Value::Null);
    assert_eq!(v["request"], request);
    // Today's suggestions are kept under the partial settings.
    assert_eq!(v["settings"]["material_types"], json!(["閱讀文本"]));
    let id = v["id"].as_i64().unwrap();

    let state = generate(&app, &t, id).await;
    let jobs = state["jobs"].as_array().unwrap();
    let steps: Vec<&str> = jobs.iter().map(|j| j["step"].as_str().unwrap()).collect();
    assert_eq!(steps, ["bloom", "bloom_check"]);
    for j in jobs {
        assert_eq!(j["status"], "done");
        assert_eq!(j["outcome"], "shown", "{j}");
    }
    assert!(jobs[1]["interaction_id"].is_i64());

    // One block per level, in Bloom order, 8 questions as 2, 2, 1, 1, 1, 1.
    let bs = blocks(&state);
    assert_eq!(bs.len(), 6);
    let titles: Vec<&str> = bs.iter().map(|b| b["title"].as_str().unwrap()).collect();
    assert_eq!(titles, TITLES);
    let counts: Vec<usize> = bs.iter().map(|b| items(b).len()).collect();
    assert_eq!(counts, [2, 2, 1, 1, 1, 1]);
    let tasks: Vec<Vec<String>> = bs.iter().map(|b| strs(&b["student_tasks"])).collect();
    assert_eq!(
        tasks,
        [
            vec!["理解內容"],
            vec!["理解內容"],
            vec!["字彙練習"],
            vec!["比較／分類", "推論／預測"],
            vec!["評估／提出理由"],
            vec!["寫作"],
        ]
    );
    let text = passage("");
    for (b, level) in bs.iter().zip(LEVELS) {
        assert_eq!(b["kind"], "activity");
        assert_eq!(b["bloom"], level);
        assert_eq!(b["origin"], "ai");
        assert_eq!(b["scaffolds"], json!([]), "no scaffolds step");
        assert_eq!(b["message"], Value::Null);
        assert!(!b["instructions"].as_str().unwrap().is_empty());
        assert!(!b["paragraph_ids"].as_array().unwrap().is_empty());
        for it in items(b) {
            assert_eq!(it["bloom"], level);
            assert_eq!(it["levels"], json!(["standard"]));
            assert_eq!(it["origin"], "ai");
            assert!(!it["hint"].as_str().unwrap().is_empty());
            let evidence = it["evidence"].as_str().unwrap();
            if level == "remember" || level == "understand" {
                assert!(text.contains(evidence) && !evidence.is_empty(), "{it}");
            } else {
                assert_eq!(evidence, "");
            }
        }
    }
    let chosen = &state["content"]["chosen"];
    assert_eq!(
        chosen["reasons"][0],
        "Bloom：記憶 2、理解 2、應用 1、分析 1、評鑑 1、創造 1"
    );
    assert_eq!(chosen["item_budget"], 8);
    assert_eq!(chosen["settings"]["output"], "comprehension");
    assert_eq!(chosen["settings"]["question_count"], 8);
    assert_eq!(chosen["settings"]["bloom"], json!(LEVELS));
    assert_eq!(state["content"]["levels"], json!(["standard"]));
    assert_eq!(state["content"]["notices"], json!([]));
    let (_, versions) = app
        .get(&format!("/api/worksheets/{id}/versions"), Some(&t))
        .await;
    assert_eq!(versions[0]["cause"], "generate");

    // Every call is logged; the material only as length and hash; the request reaches it.
    let level_calls = calls(&app, "worksheet_bloom_level").await;
    assert_eq!(level_calls.len(), 6);
    for (attempt, status, snap) in &level_calls {
        assert_eq!((*attempt, status.as_str()), (1, "shown"));
        assert_eq!(snap["inputs"]["teacher_request"], request);
        assert!(snap["prompt"]["user"]
            .as_str()
            .unwrap()
            .contains(&format!("教師的要求：{request}")));
        assert!(snap["inputs"]["passage_numbered"]
            .as_str()
            .unwrap()
            .starts_with("［學習單教材"));
        assert!(!snap.to_string().contains("umbrella stand outside her shop"));
        assert_eq!(snap["schema"], "worksheet_bloom_level");
    }
    let judge = calls(&app, "worksheet_bloom_check").await;
    assert_eq!(judge.len(), 1);
    assert_eq!(judge[0].2["inputs"]["question_count"], "8");

    // Teacher sheet: a Bloom tag on every question and a summary per block; student: neither.
    let teacher = sheet(&app, &t, id, "standard", "teacher").await;
    let first = &teacher["blocks"][0];
    assert_eq!(first["title"], "Part A · Find the Facts");
    assert_eq!(first["bloom_summary"], "Bloom：記憶 2");
    assert_eq!(first["items"][0]["bloom"], "remember");
    assert_eq!(teacher["blocks"][4]["items"][0]["bloom"], "evaluate");
    let student = sheet(&app, &t, id, "standard", "student").await;
    let s = student.to_string();
    assert!(!s.contains("\"bloom\"") && !s.contains("bloom_summary") && !s.contains("\"hint\""));
    let xml = docx(&app, &t, id, "standard", "teacher").await;
    assert!(xml.contains("〔Bloom：記憶 Remember〕") && xml.contains("〔Bloom：創造 Create〕"));
    assert!(xml.contains("Bloom：理解 2"));
    let xml = docx(&app, &t, id, "standard", "student").await;
    assert!(!xml.contains("Bloom"));
}

#[tokio::test]
async fn tiered_practice_end_to_end() {
    let app = test_app!();
    let t = app.teacher("T-BLOOM-2", "general").await;
    // Levels are forced to all three whatever was selected.
    let id = made(
        &app,
        &t,
        "",
        json!({ "output": "tiered", "levels": ["standard"] }),
        "",
    )
    .await;
    let state = generate(&app, &t, id).await;
    assert_eq!(
        state["content"]["levels"],
        json!(["basic", "standard", "advanced"])
    );
    let bs = blocks(&state);
    let counts: Vec<usize> = bs.iter().map(|b| items(b).len()).collect();
    assert_eq!(counts, [2; 6], "12 questions by default");
    let expected = [
        json!(["basic"]),
        json!(["basic", "standard", "advanced"]),
        json!(["basic", "standard", "advanced"]),
        json!(["standard", "advanced"]),
        json!(["advanced"]),
        json!(["advanced"]),
    ];
    for (b, levels) in bs.iter().zip(&expected) {
        for it in items(b) {
            assert_eq!(&it["levels"], levels, "{}", b["bloom"]);
        }
    }
    let reasons = strs(&state["content"]["chosen"]["reasons"]);
    assert_eq!(
        reasons[0],
        "Bloom：記憶 2、理解 2、應用 2、分析 2、評鑑 2、創造 2"
    );
    assert!(reasons.iter().any(|r| {
        r
        == "基礎版：記憶＋理解＋應用（附提示）；一般版：理解到分析；進階版：理解、應用、分析到創造"
    }));

    // Each version shows its levels; hints only in 基礎版.
    let titles = |s: &Value| -> Vec<String> {
        s["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b["title"].as_str().unwrap().to_string())
            .collect()
    };
    let basic = sheet(&app, &t, id, "basic", "student").await;
    assert_eq!(
        titles(&basic),
        [
            "Part A · Find the Facts",
            "Part B · Understand the Text",
            "Part C · Use It"
        ]
    );
    for b in basic["blocks"].as_array().unwrap() {
        for it in b["items"].as_array().unwrap() {
            assert!(it["hint"].as_str().unwrap().starts_with("Look at"), "{it}");
            assert!(it.get("bloom").is_none());
        }
    }
    let standard = sheet(&app, &t, id, "standard", "student").await;
    assert_eq!(
        titles(&standard),
        [
            "Part A · Understand the Text",
            "Part B · Use It",
            "Part C · Look Closer"
        ],
        "the letters follow what is printed"
    );
    assert!(!standard.to_string().contains("\"hint\""));
    let advanced = sheet(&app, &t, id, "advanced", "teacher").await;
    assert_eq!(advanced["blocks"].as_array().unwrap().len(), 5);
    assert_eq!(
        advanced["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b["items"].as_array().unwrap().len())
            .sum::<usize>(),
        10
    );
    let xml = docx(&app, &t, id, "basic", "student").await;
    assert!(xml.contains("Hint: ") && !xml.contains("Bloom"));
    let xml = docx(&app, &t, id, "advanced", "student").await;
    assert!(!xml.contains("Hint: "));
}

#[tokio::test]
async fn the_judge_relabels_what_still_misses_with_a_notice() {
    let app = test_app!();
    let t = app.teacher("T-BLOOM-3", "general").await;
    let id = made(
        &app,
        &t,
        "FAKE_BLOOM_MISMATCH",
        json!({ "output": "comprehension" }),
        "",
    )
    .await;
    let state = generate(&app, &t, id).await;
    assert_eq!(
        state["content"]["notices"],
        json!(["第 7 題原本要出『評鑑』，Manna 判斷是『理解』，已改標為『理解』。"])
    );
    let evaluate = &blocks(&state)[4];
    assert_eq!(evaluate["bloom"], "evaluate", "the block keeps its level");
    let item = &items(evaluate)[0];
    assert_eq!(item["bloom"], "understand");
    assert!(
        item["prompt"].as_str().unwrap().contains("redo"),
        "the question was written again once: {item}"
    );
    // The others keep their target.
    let others: Vec<Value> = blocks(&state)
        .iter()
        .filter(|b| b["bloom"] != "evaluate")
        .flat_map(items)
        .collect();
    assert!(others
        .iter()
        .all(|it| !it["prompt"].as_str().unwrap().contains("redo")));
    // 6 level calls + 1 redo; two judgements (all 8, then the 1 rewritten).
    assert_eq!(calls(&app, "worksheet_bloom_level").await.len(), 7);
    let judge = calls(&app, "worksheet_bloom_check").await;
    assert_eq!(judge.len(), 2);
    assert_eq!(judge[1].2["inputs"]["question_count"], "1");
    let redo = calls(&app, "worksheet_bloom_level").await;
    let redo_user = redo[6].2["prompt"]["user"].as_str().unwrap();
    assert!(redo_user.contains("【重新出題】") && redo_user.contains("審查判斷：理解"));
    // The teacher sheet tags it as judged.
    let teacher = sheet(&app, &t, id, "standard", "teacher").await;
    assert_eq!(teacher["blocks"][4]["items"][0]["bloom"], "understand");
    assert_eq!(teacher["blocks"][4]["bloom_summary"], "Bloom：理解 1");
}

#[tokio::test]
async fn remember_evidence_that_is_not_verbatim_is_repaired() {
    let app = test_app!();
    let t = app.teacher("T-BLOOM-4", "general").await;
    let id = made(
        &app,
        &t,
        "FAKE_BLOOM_NO_EVIDENCE",
        json!({ "output": "comprehension", "bloom": ["remember", "apply"], "question_count": 4 }),
        "",
    )
    .await;
    let state = generate(&app, &t, id).await;
    let bs = blocks(&state);
    assert_eq!(
        bs.iter()
            .map(|b| b["title"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["Part A · Find the Facts", "Part B · Use It"]
    );
    let remember = items(&bs[0]);
    assert_eq!(remember.len(), 2);
    for it in &remember {
        assert!(
            P1.contains(it["evidence"].as_str().unwrap())
                || P2.contains(it["evidence"].as_str().unwrap()),
            "{it}"
        );
    }
    let rows: Vec<(i32, String, Value)> = calls(&app, "worksheet_bloom_level").await;
    let remember_calls: Vec<&(i32, String, Value)> = rows
        .iter()
        .filter(|(_, _, s)| s["inputs"]["bloom_id"] == "remember")
        .collect();
    assert_eq!(
        remember_calls
            .iter()
            .map(|(a, s, _)| (*a, s.as_str()))
            .collect::<Vec<_>>(),
        [(1, "blocked"), (2, "shown")]
    );
    let repair_note = remember_calls[1].2["repair_note"].as_str().unwrap();
    assert!(repair_note.contains("evidence"), "{repair_note}");
    let (status, report): (String, Value) = sqlx::query_as(
        "SELECT status, check_results FROM ai_interactions
          WHERE module = 'worksheet_bloom_level' AND attempt = 1
            AND input_snapshot -> 'inputs' ->> 'bloom_id' = 'remember'",
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(status, "blocked");
    assert!(report.to_string().contains("evidence_verbatim"));
    // Apply needs no evidence: one call, nothing quoted.
    let apply = items(&bs[1]);
    assert!(apply.iter().all(|it| it["evidence"] == ""));
}

#[tokio::test]
async fn invalid_bloom_settings_and_templates_with_other_outputs_are_400s() {
    let app = test_app!();
    let t = app.teacher("T-BLOOM-5", "general").await;
    let material = json!({ "title": "The Umbrella Stand", "text": passage("") });
    let bad_settings = [
        (json!({ "bloom": ["thinking"] }), "Bloom"),
        (json!({ "output": "quiz" }), "要做什麼"),
        (json!({ "question_count": 31 }), "題數"),
        (json!({ "question_count": 2 }), "題數"),
        (json!({ "bloom_counts": { "remember": 13 } }), "題數"),
        (
            json!({ "bloom_counts": { "remember": 1, "create": 1 } }),
            "題數",
        ),
        (json!({ "bloom_counts": { "synthesis": 3 } }), "Bloom"),
        (json!({ "objective": "o".repeat(301) }), "學習目標"),
    ];
    for (settings, needle) in &bad_settings {
        let (s, v) = create(
            &app,
            &t,
            json!({ "material": material, "settings": settings }),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{settings}: {v}");
        assert!(error_message(&v).contains(needle), "{settings}: {v}");
    }
    let (s, v) = create(
        &app,
        &t,
        json!({ "material": material, "request": "要".repeat(2001) }),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(error_message(&v).contains("2000"));

    // The same on PUT, and a teacher edit with a Bloom label outside the set.
    let id = made(&app, &t, "", json!({ "output": "comprehension" }), "").await;
    for (settings, needle) in &bad_settings {
        let (s, v) = app
            .put(
                &format!("/api/worksheets/{id}"),
                Some(&t),
                json!({ "settings": settings }),
            )
            .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{settings}: {v}");
        assert!(error_message(&v).contains(needle), "{settings}: {v}");
    }
    let state = generate(&app, &t, id).await;
    let mut content = state["content"].clone();
    content["blocks"][0]["items"][0]["bloom"] = json!("synthesis");
    let (s, v) = app
        .put(
            &format!("/api/worksheets/{id}"),
            Some(&t),
            json!({ "content": content }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    assert!(error_message(&v).contains("Bloom"), "{v}");
    // A valid edit: relabel, clear a label, write a hint.
    let mut content = state["content"].clone();
    content["blocks"][0]["items"][0]["bloom"] = json!("understand");
    content["blocks"][0]["items"][1]["bloom"] = Value::Null;
    content["blocks"][0]["items"][1]["hint"] = json!("Start with: She put …");
    let (s, v) = app
        .put(
            &format!("/api/worksheets/{id}"),
            Some(&t),
            json!({ "content": content }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["content"]["blocks"][0]["items"][0]["bloom"], "understand");
    assert_eq!(v["content"]["blocks"][0]["items"][1]["bloom"], Value::Null);
    assert_eq!(
        v["content"]["blocks"][0]["items"][1]["hint"],
        "Start with: She put …"
    );
    // A partial PUT keeps the fields it does not name; null clears an optional one.
    let (s, v) = app
        .put(
            &format!("/api/worksheets/{id}"),
            Some(&t),
            json!({ "settings": { "question_count": 6, "objective": "  Students compare the two views.  " },
                    "request": "多一點選擇題" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["settings"]["output"], "comprehension");
    assert_eq!(v["settings"]["question_count"], 6);
    assert_eq!(
        v["settings"]["objective"],
        "Students compare the two views."
    );
    assert_eq!(v["settings"]["material_types"], json!(["閱讀文本"]));
    assert_eq!(v["request"], "多一點選擇題");
    let (_, v) = app
        .put(
            &format!("/api/worksheets/{id}"),
            Some(&t),
            json!({ "settings": { "question_count": null }, "request": null }),
        )
        .await;
    assert_eq!(v["settings"]["question_count"], Value::Null);
    assert_eq!(
        v["settings"]["objective"],
        "Students compare the two views."
    );
    assert_eq!(v["request"], "");

    // A template only makes the full worksheet.
    let template_id = ready_template(&app, &t).await;
    let (s, v) = create(
        &app,
        &t,
        json!({ "material": material, "template_id": template_id,
                "settings": { "output": "tiered" } }),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(error_message(&v), "版型只適用於完整學習單。");
    let (s, v) = create(
        &app,
        &t,
        json!({ "material": material, "template_id": template_id }),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    let with_template = v["id"].as_i64().unwrap();
    let (s, v) = app
        .put(
            &format!("/api/worksheets/{with_template}"),
            Some(&t),
            json!({ "settings": { "output": "comprehension" } }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(error_message(&v), "版型只適用於完整學習單。");
    // … unless the template is dropped in the same request.
    let (s, v) = app
        .put(
            &format!("/api/worksheets/{with_template}"),
            Some(&t),
            json!({ "settings": { "output": "comprehension" }, "template_id": null }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["template"], Value::Null);
    // Choosing a template for a comprehension worksheet is refused too.
    let (s, _) = app
        .put(
            &format!("/api/worksheets/{id}"),
            Some(&t),
            json!({ "template_id": template_id }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
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

#[tokio::test]
async fn one_levels_block_is_regenerated_and_undone() {
    let app = test_app!();
    let t = app.teacher("T-BLOOM-6", "general").await;
    let id = made(&app, &t, "", json!({ "output": "comprehension" }), "").await;
    let before = generate(&app, &t, id).await;
    let target = blocks(&before)[3].clone();
    assert_eq!(target["bloom"], "analyze");
    let block_id = target["id"].as_str().unwrap().to_string();

    let (s, v) = app
        .post(
            &format!("/api/worksheets/{id}/blocks/{block_id}/regenerate"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    let after = wait(&app, &t, id).await;
    let jobs = after["jobs"].as_array().unwrap();
    assert_eq!(
        jobs.iter()
            .map(|j| (j["step"].as_str().unwrap(), j["block_id"].as_str().unwrap()))
            .collect::<Vec<_>>(),
        [
            ("bloom", block_id.as_str()),
            ("bloom_check", block_id.as_str())
        ]
    );
    assert!(jobs.iter().all(|j| j["outcome"] == "shown"), "{jobs:?}");
    let regenerated = blocks(&after)[3].clone();
    assert_eq!(regenerated["id"], target["id"]);
    assert_eq!(regenerated["title"], "Part D · Look Closer");
    assert_eq!(regenerated["bloom"], "analyze");
    let new_items = items(&regenerated);
    assert_eq!(new_items.len(), 1);
    assert_eq!(new_items[0]["bloom"], "analyze");
    assert!(new_items[0]["prompt"].as_str().unwrap().contains("v2"));
    assert_ne!(new_items[0]["prompt"], items(&target)[0]["prompt"]);
    // The other blocks are untouched.
    for (i, (a, b)) in blocks(&before).iter().zip(blocks(&after)).enumerate() {
        if i != 3 {
            assert_eq!(a, &b);
        }
    }
    assert!(strs(&after["undoable"]).contains(&block_id));
    // The level call saw the other blocks' questions; the judge only this block's one.
    let level_calls = calls(&app, "worksheet_bloom_level").await;
    let last = &level_calls.last().unwrap().2;
    assert_eq!(last["inputs"]["bloom_id"], "analyze");
    assert_eq!(last["inputs"]["question_count"], "1");
    let judge = calls(&app, "worksheet_bloom_check").await;
    assert_eq!(judge.last().unwrap().2["inputs"]["question_count"], "1");
    let (_, versions) = app
        .get(&format!("/api/worksheets/{id}/versions"), Some(&t))
        .await;
    assert_eq!(versions[0]["cause"], "regenerate");

    let (s, undone) = app
        .post(
            &format!("/api/worksheets/{id}/blocks/{block_id}/undo"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{undone}");
    assert_eq!(blocks(&undone)[3], target);
    assert_eq!(undone["content"]["blocks"], before["content"]["blocks"]);
}

#[tokio::test]
async fn the_request_and_the_objective_reach_every_prompt() {
    let app = test_app!();
    let t = app.teacher("T-BLOOM-7", "general").await;
    let request = "請多用和臺灣學生生活有關的例子";
    let objective = "Students can explain why the neighbors changed their minds.";
    // Per Bloom level: their own placeholders.
    let id = made(
        &app,
        &t,
        "",
        json!({ "output": "comprehension", "bloom": ["understand"], "question_count": 3,
                "objective": objective }),
        request,
    )
    .await;
    generate(&app, &t, id).await;
    let level = calls(&app, "worksheet_bloom_level").await;
    let user = level[0].2["prompt"]["user"].as_str().unwrap();
    assert!(
        user.contains(&format!("學習目標（教師寫的）：{objective}")),
        "{user}"
    );
    assert!(user.contains(&format!("教師的要求：{request}")), "{user}");
    assert_eq!(level[0].2["inputs"]["teacher_objective"], objective);

    // The full worksheet: in the shared block appended to every step's prompt.
    let id = made(&app, &t, "", json!({ "objective": objective }), request).await;
    generate(&app, &t, id).await;
    for module in [
        "worksheet_tasks",
        "worksheet_warmup",
        "worksheet_scaffolds",
        "worksheet_exit_ticket",
    ] {
        let rows = calls(&app, module).await;
        assert!(!rows.is_empty(), "{module}");
        let snap = &rows.last().unwrap().2;
        let user = snap["prompt"]["user"].as_str().unwrap();
        assert!(
            user.contains(&format!("學習目標（教師寫的）：{objective}"))
                && user.contains(&format!("教師的要求：{request}")),
            "{module}: {user}"
        );
    }
    // 「Manna 已帶入的資訊」 lists them.
    let (_, state) = app.get(&format!("/api/worksheets/{id}"), Some(&t)).await;
    let context = state["jobs"][0]["context"].to_string();
    assert!(context.contains("教師的要求") && context.contains("學習目標（教師寫的）"));
}

#[tokio::test]
async fn explicit_counts_override_levels_and_total() {
    let app = test_app!();
    let t = app.teacher("T-BLOOM-8", "general").await;
    let id = made(
        &app,
        &t,
        "",
        json!({ "output": "comprehension", "bloom": ["remember"], "question_count": 5,
                "bloom_counts": { "create": 2, "understand": 3, "analyze": 0 } }),
        "",
    )
    .await;
    let (_, state) = app.get(&format!("/api/worksheets/{id}"), Some(&t)).await;
    assert_eq!(
        state["settings"]["bloom_counts"],
        json!({ "understand": 3, "analyze": 0, "create": 2 }),
        "echoed, in Bloom order"
    );
    let state = generate(&app, &t, id).await;
    let bs = blocks(&state);
    assert_eq!(
        bs.iter()
            .map(|b| (b["title"].as_str().unwrap(), items(b).len()))
            .collect::<Vec<_>>(),
        [
            ("Part A · Understand the Text", 3),
            ("Part B · Make Something New", 2)
        ]
    );
    assert_eq!(
        state["content"]["chosen"]["reasons"][0],
        "Bloom：理解 3、創造 2"
    );
    assert_eq!(state["content"]["chosen"]["item_budget"], 5);
    // Clearing the counts goes back to bloom + question_count.
    let (s, v) = app
        .put(
            &format!("/api/worksheets/{id}"),
            Some(&t),
            json!({ "settings": { "bloom_counts": null } }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let state = generate(&app, &t, id).await;
    let bs = blocks(&state);
    assert_eq!(bs.len(), 1);
    assert_eq!(items(&bs[0]).len(), 5);
    assert_eq!(bs[0]["bloom"], "remember");
}

#[tokio::test]
async fn the_teachers_answer_formats_are_respected() {
    let app = test_app!();
    let t = app.teacher("T-BLOOM-9", "general").await;
    let id = made(
        &app,
        &t,
        "",
        json!({ "output": "comprehension", "bloom": ["remember", "apply", "create"],
                "question_count": 6, "answer_formats": ["段落寫作", "選擇題"] }),
        "",
    )
    .await;
    let state = generate(&app, &t, id).await;
    let formats: Vec<Vec<String>> = blocks(&state)
        .iter()
        .map(|b| {
            items(b)
                .iter()
                .map(|i| i["answer_format"].as_str().unwrap().to_string())
                .collect()
        })
        .collect();
    // remember ∩ selection = 選擇題; apply ∩ selection = ∅ → the selection; create = 段落寫作.
    assert_eq!(
        formats,
        [
            vec!["選擇題", "選擇題"],
            vec!["選擇題", "段落寫作"],
            vec!["段落寫作", "段落寫作"],
        ]
    );
    let level = calls(&app, "worksheet_bloom_level").await;
    let allowed: Vec<(String, String)> = level
        .iter()
        .map(|(_, _, s)| {
            (
                s["inputs"]["bloom_id"].as_str().unwrap().to_string(),
                s["inputs"]["answer_formats"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert!(allowed.contains(&("remember".into(), "選擇題".into())));
    assert!(allowed.contains(&("apply".into(), "選擇題、段落寫作".into())));
    assert!(allowed.contains(&("create".into(), "段落寫作".into())));
    // Nothing selected: every suitable format.
    let id = made(
        &app,
        &t,
        "",
        json!({ "output": "comprehension", "bloom": ["analyze"], "question_count": 4 }),
        "",
    )
    .await;
    let state = generate(&app, &t, id).await;
    let used: Vec<String> = items(&blocks(&state)[0])
        .iter()
        .map(|i| i["answer_format"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(used, ["表格整理", "圖像組織", "簡答題", "選擇題"]);
}

#[tokio::test]
async fn changing_the_material_keeps_what_to_make() {
    let app = test_app!();
    let t = app.teacher("T-BLOOM-13", "general").await;
    let id = made(
        &app,
        &t,
        "",
        json!({ "output": "tiered", "bloom": ["apply"], "objective": "Use the new words." }),
        "多出應用題",
    )
    .await;
    let (s, v) = app
        .put(
            &format!("/api/worksheets/{id}/material"),
            Some(&t),
            json!({ "title": "Another text", "text": format!("{P2}\n\n{P3}") }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["settings"]["output"], "tiered");
    assert_eq!(v["settings"]["bloom"], json!(["apply"]));
    assert_eq!(v["settings"]["objective"], "Use the new words.");
    assert_eq!(v["request"], "多出應用題");
}

#[tokio::test]
async fn the_full_worksheet_still_works_and_labels_its_items() {
    let app = test_app!();
    let t = app.teacher("T-BLOOM-10", "general").await;
    let (s, v) = create(
        &app,
        &t,
        json!({ "material": { "title": "The Umbrella Stand", "text": passage("") } }),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    assert_eq!(v["settings"]["output"], "worksheet");
    assert_eq!(v["request"], "");
    let id = v["id"].as_i64().unwrap();
    let state = generate(&app, &t, id).await;
    let steps: Vec<&str> = state["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|j| j["step"].as_str().unwrap())
        .collect();
    assert_eq!(steps, ["tasks", "warmup", "scaffolds", "exit_ticket"]);
    let bs = blocks(&state);
    assert_eq!(bs.first().unwrap()["kind"], "warmup");
    assert_eq!(bs.last().unwrap()["kind"], "exit_ticket");
    for b in &bs {
        assert_eq!(b["bloom"], Value::Null);
        for it in items(b) {
            if b["kind"] == "activity" {
                assert!(it["bloom"].is_string(), "{it}");
            } else {
                assert_eq!(it["bloom"], Value::Null);
            }
            assert_eq!(it["hint"], "");
        }
    }
    // The tasks step uses prompt version 2, whose schema asks for a Bloom label.
    let tasks = calls(&app, "worksheet_tasks").await;
    assert_eq!(tasks[0].2["prompt"]["version_no"], 2);
    assert!(tasks[0].2["prompt"]["system"]
        .as_str()
        .unwrap()
        .contains("13. bloom："));
    let v1: String = sqlx::query_scalar(
        "SELECT system_text FROM prompt_versions WHERE prompt_key = 'worksheet_tasks' AND version_no = 1",
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert!(!v1.contains("bloom"), "version 1 stays as it was");
    let teacher = sheet(&app, &t, id, "standard", "teacher").await;
    let activity = teacher["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["kind"] == "activity")
        .unwrap()
        .clone();
    assert!(activity["items"][0]["bloom"].is_string());
    assert!(activity["bloom_summary"]
        .as_str()
        .unwrap()
        .starts_with("Bloom："));
    // Regenerating an activity block of the full worksheet still runs tasks + scaffolds.
    let block_id = bs[1]["id"].as_str().unwrap().to_string();
    let (s, _) = app
        .post(
            &format!("/api/worksheets/{id}/blocks/{block_id}/regenerate"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED);
    let after = wait(&app, &t, id).await;
    let steps: Vec<&str> = after["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|j| j["step"].as_str().unwrap())
        .collect();
    assert_eq!(steps, ["tasks", "scaffolds"]);
}

#[tokio::test]
async fn another_teachers_worksheet_is_a_404() {
    let app = test_app!();
    let t = app.teacher("T-BLOOM-11", "general").await;
    let other = app.teacher("T-BLOOM-12", "general").await;
    let id = made(&app, &t, "", json!({ "output": "tiered" }), "x").await;
    let state = generate(&app, &t, id).await;
    let block_id = blocks(&state)[0]["id"].as_str().unwrap().to_string();
    for (method, path) in [
        ("get", format!("/api/worksheets/{id}")),
        ("post", format!("/api/worksheets/{id}/generate")),
        (
            "post",
            format!("/api/worksheets/{id}/blocks/{block_id}/regenerate"),
        ),
    ] {
        let (s, _) = if method == "get" {
            app.get(&path, Some(&other)).await
        } else {
            app.post(&path, Some(&other), json!({})).await
        };
        assert_eq!(s, StatusCode::NOT_FOUND, "{path}");
    }
    let (s, _) = app
        .put(
            &format!("/api/worksheets/{id}"),
            Some(&other),
            json!({ "request": "改掉" }),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}
