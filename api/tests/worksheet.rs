//! Phase 2.3 (docs/API.md「學習單產生器」): worksheets, generation as background jobs with the
//! fake provider, label validation, versions and amount, per-block regeneration and undo, the
//! lesson SOP prefill and sync-back, the vocabulary check, DOCX / PDF export and ownership.

#[macro_use]
mod common;

use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

const P1: &str = "Mrs. Lin put an umbrella stand outside her shop on a rainy Monday. A sign said, “Take one if you need it.”";
const P2: &str = "Her neighbors laughed at the idea. They were sure that every umbrella would be gone within a week.";
const P3: &str = "In the first month, twelve of the thirty umbrellas did disappear. However, something unexpected followed.";
const P4: &str = "Today the stand holds more umbrellas than it did at the start. Mrs. Lin says trust is like an umbrella.";

const FORBIDDEN: [&str; 7] = [
    "課綱議題",
    "SDGs",
    "SEL",
    "探究",
    "高層次思考",
    "專題任務",
    "合作活動",
];
const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn passage(marker: &str) -> String {
    format!("{P1}\n\n{P2}\n\n{P3}\n\n{P4} {marker}")
}

fn empty_settings() -> Value {
    json!({ "material_types": [], "student_tasks": [], "answer_formats": [], "generation": [],
            "levels": [], "minutes": null })
}

async fn direct(app: &common::TestApp, t: &str, text: &str) -> Value {
    let (s, v) = app
        .post(
            "/api/worksheets",
            Some(t),
            json!({ "material": { "title": "The Umbrella Stand", "text": text } }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    v
}

async fn put_settings(
    app: &common::TestApp,
    t: &str,
    id: i64,
    settings: Value,
) -> (StatusCode, Value) {
    app.put(
        &format!("/api/worksheets/{id}"),
        Some(t),
        json!({ "settings": settings }),
    )
    .await
}

async fn wait(app: &common::TestApp, t: &str, id: i64) -> Value {
    for _ in 0..600 {
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

fn strings(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => out.push(s.clone()),
        Value::Array(a) => a.iter().for_each(|x| strings(x, out)),
        Value::Object(o) => o.values().for_each(|x| strings(x, out)),
        _ => {}
    }
}

fn keys(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Array(a) => a.iter().for_each(|x| keys(x, out)),
        Value::Object(o) => {
            for (k, x) in o {
                out.push(k.clone());
                keys(x, out);
            }
        }
        _ => {}
    }
}

fn has_cjk(s: &str) -> bool {
    s.chars().any(|c| {
        ('\u{4e00}'..='\u{9fff}').contains(&c)
            || ('\u{3000}'..='\u{303f}').contains(&c)
            || ('\u{ff00}'..='\u{ffef}').contains(&c)
    })
}

/// Printed text of a student sheet: what students see (block kinds and scaffold kind labels are
/// internal names, not printed).
fn printed(sheet: &Value) -> Vec<String> {
    let mut out = vec![sheet["title"].as_str().unwrap_or_default().to_string()];
    strings(&sheet["objectives"], &mut out);
    for b in sheet["blocks"].as_array().unwrap() {
        for k in ["title", "instructions"] {
            out.push(b[k].as_str().unwrap_or_default().to_string());
        }
        strings(&b["steps"], &mut out);
        for v in b["vocabulary"].as_array().unwrap() {
            strings(&v["word"], &mut out);
            strings(&v["definition"], &mut out);
        }
        for s in b["supports"].as_array().unwrap() {
            strings(&s["content"], &mut out);
        }
        for i in b["items"].as_array().unwrap() {
            strings(&i["prompt"], &mut out);
            strings(&i["options"], &mut out);
        }
    }
    out
}

fn blocks(state: &Value) -> Vec<Value> {
    state["content"]["blocks"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

fn kinds(state: &Value) -> Vec<String> {
    blocks(state)
        .iter()
        .map(|b| b["kind"].as_str().unwrap().to_string())
        .collect()
}

async fn raw_get(
    app: &common::TestApp,
    path: &str,
    token: &str,
) -> (StatusCode, String, String, Vec<u8>) {
    let req = Request::builder()
        .uri(path)
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let header = |name: &str| {
        resp.headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string()
    };
    let (ct, cd) = (header("content-type"), header("content-disposition"));
    let bytes = resp
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec();
    (status, ct, cd, bytes)
}

// ---------------------------------------------------------------- generation

#[tokio::test]
async fn nothing_selected_still_generates_and_says_what_was_chosen() {
    let app = test_app!();
    let t = app.teacher("T-001", "general").await;
    let ws = direct(&app, &t, &passage("")).await;
    let id = ws["id"].as_i64().unwrap();
    assert_eq!(
        ws["lesson"],
        Value::Null,
        "a direct worksheet is standalone"
    );
    assert_eq!(ws["material"]["source"]["kind"], "pasted");
    assert_eq!(ws["material"]["paragraphs"].as_array().unwrap().len(), 4);
    assert_eq!(ws["suggested"]["material_types"], json!(["閱讀文本"]));
    assert_eq!(ws["generated"], false);
    assert_eq!(ws["title"], "The Umbrella Stand: Worksheet");
    assert!(ws["filename"]
        .as_str()
        .unwrap()
        .contains("The Umbrella Stand"));

    // The teacher unticks everything: zero selections in all four blocks.
    let (s, v) = put_settings(&app, &t, id, empty_settings()).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let state = generate(&app, &t, id).await;
    assert_eq!(state["generated"], true);
    let jobs = state["jobs"].as_array().unwrap();
    assert_eq!(
        jobs.iter()
            .map(|j| j["step"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["tasks", "warmup", "scaffolds", "exit_ticket"]
    );
    assert!(jobs.iter().all(|j| j["outcome"] == "shown"), "{jobs:?}");
    let chosen = &state["content"]["chosen"];
    for part in [
        "material_types",
        "student_tasks",
        "answer_formats",
        "generation",
        "levels",
        "minutes",
    ] {
        assert!(
            chosen["auto"].as_array().unwrap().contains(&json!(part)),
            "{part}"
        );
    }
    assert_eq!(chosen["settings"]["material_types"], json!(["閱讀文本"]));
    assert!(
        !chosen["settings"]["student_tasks"]
            .as_array()
            .unwrap()
            .is_empty(),
        "what Manna used is reported"
    );
    assert!(!chosen["settings"]["answer_formats"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(state["content"]["levels"], json!(["standard"]));

    // Order: warm-up first, one activity block per paragraph in order, exit ticket last.
    assert_eq!(
        kinds(&state),
        [
            "warmup",
            "activity",
            "activity",
            "activity",
            "activity",
            "exit_ticket"
        ]
    );
    let paragraph_ids: Vec<Value> = blocks(&state)[1..5]
        .iter()
        .map(|b| b["paragraph_ids"][0].clone())
        .collect();
    assert_eq!(
        paragraph_ids,
        [json!("p1"), json!("p2"), json!("p3"), json!("p4")]
    );
    // Every block has a procedural scaffold the teacher never asked for.
    for b in blocks(&state) {
        let scaffolds = b["scaffolds"].as_array().unwrap();
        assert!(
            scaffolds.iter().any(|s| s["type"] == "procedural"
                && s["auto"] == true
                && !s["steps"].as_array().unwrap().is_empty()),
            "{b}"
        );
    }
    // Labels on the blocks come from the spec's sets only.
    let labels: Vec<String> = blocks(&state)
        .iter()
        .flat_map(|b| {
            b["student_tasks"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_str().unwrap().to_string())
                .collect::<Vec<_>>()
        })
        .collect();
    for f in FORBIDDEN {
        assert!(!labels.iter().any(|l| l == f));
    }

    // The student version is English only and has no answers; the teacher version has them.
    let student = sheet(&app, &t, id, "standard", "student").await;
    let text = printed(&student);
    assert!(text.iter().all(|s| !has_cjk(s)), "{text:?}");
    let mut k = Vec::new();
    keys(&student, &mut k);
    assert!(
        !k.iter()
            .any(|k| k == "answer" || k == "evidence" || k == "teacher_notes"),
        "{k:?}"
    );
    let teacher = sheet(&app, &t, id, "standard", "teacher").await;
    let answers: usize = teacher["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|b| b["items"].as_array().unwrap().iter())
        .filter(|i| i.get("answer").is_some())
        .count();
    assert!(answers > 0);
    // No reference answer leaks into the student sheet (an answer that is also one of the
    // printed choices of a multiple-choice item is not a leak).
    let printed_text = printed(&student);
    let options: Vec<String> = {
        let mut o = Vec::new();
        for b in student["blocks"].as_array().unwrap() {
            for i in b["items"].as_array().unwrap() {
                strings(&i["options"], &mut o);
            }
        }
        o
    };
    for b in teacher["blocks"].as_array().unwrap() {
        for i in b["items"].as_array().unwrap() {
            let Some(a) = i["answer"].as_str() else {
                continue;
            };
            if a.split_whitespace().count() < 4 || options.iter().any(|o| o == a) {
                continue;
            }
            assert!(
                !printed_text.iter().any(|p| p.contains(a)),
                "answer leaked: {a}"
            );
        }
    }

    // Every model call is logged through the AI pipeline with its prompt key and scaffold.
    let modules: Vec<(String, i32)> =
        sqlx::query_as("SELECT i.module, i.attempt FROM ai_interactions i ORDER BY i.id")
            .fetch_all(&app.db)
            .await
            .unwrap();
    assert_eq!(
        modules.iter().map(|(m, _)| m.as_str()).collect::<Vec<_>>(),
        [
            "worksheet_tasks",
            "worksheet_warmup",
            "worksheet_scaffolds",
            "worksheet_exit_ticket"
        ]
    );
    let with_scaffold: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM ai_interactions i JOIN scaffolds s ON s.id = i.scaffold_id
          WHERE s.scaffold_code LIKE 'SC-WS-%' AND i.prompt_version_id IS NOT NULL",
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(with_scaffold, 4);
}

#[tokio::test]
async fn labels_outside_the_spec_sets_are_rejected() {
    let app = test_app!();
    let t = app.teacher("T-001", "general").await;
    let id = direct(&app, &t, &passage("")).await["id"].as_i64().unwrap();
    for field in [
        "material_types",
        "student_tasks",
        "answer_formats",
        "generation",
    ] {
        for label in FORBIDDEN {
            let mut s = empty_settings();
            s[field] = json!([label]);
            let (status, v) = put_settings(&app, &t, id, s).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{field} {label}: {v}");
        }
    }
    let mut s = empty_settings();
    s["levels"] = json!(["expert"]);
    let (status, _) = put_settings(&app, &t, id, s).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let mut s = empty_settings();
    s["minutes"] = json!(2);
    let (status, _) = put_settings(&app, &t, id, s).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    // Every exact label is accepted, multi-select across all groups.
    let all = json!({
        "material_types": ["閱讀文本", "聽力素材", "影片素材", "圖像素材", "資料／數據"],
        "student_tasks": ["理解內容", "字彙練習", "文法／句型練習", "比較／分類", "推論／預測", "評估／提出理由",
                          "蒐集資料", "整理／分析資料", "討論／分享", "小組共同完成", "寫作", "製作成果", "成果發表",
                          "學習反思", "自我評量", "同儕互評"],
        "answer_formats": ["填空題", "選擇題", "是非題", "配對題", "簡答題", "表格整理", "圖像組織", "段落寫作"],
        "generation": ["加入暖身活動", "顯示學習目標", "提供引導問題", "顯示作答示例", "提供字詞表", "提供句型框架"],
        "levels": ["advanced", "basic", "standard"],
        "minutes": 40,
    });
    let (status, v) = put_settings(&app, &t, id, all).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(
        v["settings"]["levels"],
        json!(["basic", "standard", "advanced"])
    );

    // Edits of the document are checked against the same sets.
    let state = generate(&app, &t, id).await;
    let mut content = state["content"].clone();
    content["blocks"][1]["student_tasks"] = json!(["探究"]);
    let (status, _) = app
        .put(
            &format!("/api/worksheets/{id}"),
            Some(&t),
            json!({ "content": content }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let mut content = state["content"].clone();
    content["blocks"][1]["items"][0]["answer_format"] = json!("問答題");
    let (status, _) = app
        .put(
            &format!("/api/worksheets/{id}"),
            Some(&t),
            json!({ "content": content }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn only_the_selected_versions_are_made_and_they_share_the_core() {
    let app = test_app!();
    let t = app.teacher("T-001", "general").await;
    let id = direct(&app, &t, &passage("")).await["id"].as_i64().unwrap();
    let mut s = empty_settings();
    s["levels"] = json!(["basic", "advanced"]);
    s["generation"] = json!(["加入暖身活動", "提供引導問題"]);
    put_settings(&app, &t, id, s).await;
    let state = generate(&app, &t, id).await;
    assert_eq!(state["content"]["levels"], json!(["basic", "advanced"]));
    let (status, _) = app
        .get(
            &format!("/api/worksheets/{id}/sheet?level=standard"),
            Some(&t),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "一般版 was not selected");
    let basic = sheet(&app, &t, id, "basic", "student").await;
    let advanced = sheet(&app, &t, id, "advanced", "student").await;
    let prompts = |s: &Value| -> Vec<String> {
        s["blocks"][1]["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["prompt"].as_str().unwrap().to_string())
            .collect()
    };
    let (b, a) = (prompts(&basic), prompts(&advanced));
    assert!(
        b.iter().any(|p| p.starts_with("Underline")),
        "basic has the support item"
    );
    assert!(
        a.iter().any(|p| p.starts_with("Do you agree")),
        "advanced has the challenge item"
    );
    let core: Vec<&String> = b.iter().filter(|p| a.contains(p)).collect();
    assert!(!core.is_empty(), "the core tasks are the same");
    let supports = |s: &Value| s["blocks"][1]["supports"].as_array().unwrap().len();
    assert!(supports(&basic) > supports(&advanced));
    // Same objectives in every version.
    assert_eq!(basic["objectives"], advanced["objectives"]);
}

#[tokio::test]
async fn the_amount_follows_the_time() {
    let app = test_app!();
    let t = app.teacher("T-001", "general").await;
    let mut cores = Vec::new();
    for minutes in [10, 50] {
        let id = direct(&app, &t, &passage("")).await["id"].as_i64().unwrap();
        let mut s = empty_settings();
        s["minutes"] = json!(minutes);
        put_settings(&app, &t, id, s).await;
        let state = generate(&app, &t, id).await;
        let budget = state["content"]["chosen"]["item_budget"].as_u64().unwrap();
        let levels = state["content"]["levels"].as_array().unwrap().len();
        let core = blocks(&state)
            .iter()
            .filter(|b| b["kind"] == "activity")
            .flat_map(|b| b["items"].as_array().unwrap().clone())
            .filter(|i| i["levels"].as_array().unwrap().len() == levels)
            .count();
        cores.push((budget, core));
    }
    assert!(cores[0].0 < cores[1].0, "{cores:?}");
    assert!(cores[0].1 < cores[1].1, "{cores:?}");
}

#[tokio::test]
async fn failures_are_honest_and_never_block_editing() {
    let app = test_app!();
    let t = app.teacher("T-001", "general").await;

    let id = direct(
        &app,
        &t,
        &passage("FAKE_WS_WARMUP_BLOCK FAKE_WS_NO_PROCEDURAL FAKE_WS_CJK_ALWAYS FAKE_WS_BAD_LABEL"),
    )
    .await["id"]
        .as_i64()
        .unwrap();
    let state = generate(&app, &t, id).await;
    let warm = &blocks(&state)[0];
    assert_eq!(warm["kind"], "warmup");
    assert!(
        warm["message"]
            .as_str()
            .unwrap()
            .contains("暖身沒有產生成功"),
        "{warm}"
    );
    let warm_job = state["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|j| j["step"] == "warmup")
        .unwrap()
        .clone();
    assert_eq!(warm_job["outcome"], "blocked");
    for b in blocks(&state).iter().filter(|b| b["kind"] == "activity") {
        assert!(b["scaffolds"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["type"] == "procedural" && s["auto"] == true));
        assert!(!b["student_tasks"]
            .as_array()
            .unwrap()
            .contains(&json!("探究")));
    }
    let student = sheet(&app, &t, id, "standard", "student").await;
    assert!(printed(&student).iter().all(|s| !has_cjk(s)));
    // The repair call changed the request.
    let temps: Vec<Value> = sqlx::query_scalar(
        "SELECT input_snapshot -> 'request' -> 'temperature' FROM ai_interactions
          WHERE module = 'worksheet_tasks' ORDER BY id",
    )
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert_eq!(temps.len(), 2);
    assert_ne!(temps[0], temps[1]);
    // The empty warm-up can be written by hand.
    let mut content = state["content"].clone();
    content["blocks"][0]["items"] = json!([{ "id": "mine-1", "prompt": "What do you know about umbrellas?",
        "levels": ["standard"], "origin": "teacher", "answer_format": "簡答題" }]);
    content["blocks"][0]["message"] = Value::Null;
    let (s, v) = app
        .put(
            &format!("/api/worksheets/{id}"),
            Some(&t),
            json!({ "content": content }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(
        v["content"]["blocks"][0]["items"][0]["prompt"],
        "What do you know about umbrellas?"
    );

    // A provider that does not answer: every step says so, the sheet is still editable.
    let id = direct(&app, &t, &passage("FAKE_TIMEOUT")).await["id"]
        .as_i64()
        .unwrap();
    let state = generate(&app, &t, id).await;
    let outcomes: Vec<&str> = state["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|j| j["outcome"].as_str().unwrap())
        .collect();
    assert_eq!(
        outcomes,
        ["timeout", "timeout", "skipped", "timeout"],
        "nothing to put scaffolds on"
    );
    assert!(blocks(&state).iter().all(|b| b["message"]
        .as_str()
        .is_some_and(|m| m.contains("沒有回應"))));
    assert_eq!(state["generated"], true);

    // No material: a clear 400, nothing started.
    let (s, v) = app.post("/api/worksheets", Some(&t), json!({})).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    let empty = v["id"].as_i64().unwrap();
    let (s, v) = app
        .post(
            &format!("/api/worksheets/{empty}/generate"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(v["error"]["message"].as_str().unwrap().contains("貼上教材"));
}

#[tokio::test]
async fn one_block_regenerates_and_undo_brings_back_the_previous_version() {
    let app = test_app!();
    let t = app.teacher("T-001", "general").await;
    let id = direct(&app, &t, &passage("")).await["id"].as_i64().unwrap();
    let state = generate(&app, &t, id).await;
    let before = blocks(&state)[2].clone();
    let others: Vec<Value> = blocks(&state)
        .iter()
        .filter(|b| b["id"] != before["id"])
        .cloned()
        .collect();
    let block_id = before["id"].as_str().unwrap();
    let versions_before = state["versions_count"].as_i64().unwrap();

    let (s, v) = app
        .post(
            &format!("/api/worksheets/{id}/blocks/{block_id}/regenerate"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    let state = wait(&app, &t, id).await;
    let after = blocks(&state)[2].clone();
    assert_eq!(after["id"], before["id"]);
    assert_eq!(after["paragraph_ids"], before["paragraph_ids"]);
    assert_ne!(after["items"], before["items"], "the block was made again");
    assert!(after["items"][0]["prompt"]
        .as_str()
        .unwrap()
        .contains("[draft"));
    let others_after: Vec<Value> = blocks(&state)
        .iter()
        .filter(|b| b["id"] != before["id"])
        .cloned()
        .collect();
    assert_eq!(others, others_after, "only that block changed");
    assert_eq!(state["undoable"], json!([block_id]));
    assert_eq!(
        state["versions_count"].as_i64().unwrap(),
        versions_before + 1
    );

    let (s, state) = app
        .post(
            &format!("/api/worksheets/{id}/blocks/{block_id}/undo"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{state}");
    assert_eq!(blocks(&state)[2], before);
    assert_eq!(state["undoable"], json!([]));
    let (versions_status, versions) = app
        .get(&format!("/api/worksheets/{id}/versions"), Some(&t))
        .await;
    assert_eq!(versions_status, StatusCode::OK);
    assert_eq!(versions[0]["cause"], "undo");
    assert_eq!(versions[1]["cause"], "regenerate");
    let rejected: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM teacher_decisions WHERE decision = 'reject'")
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(rejected, 1);
    let (s, _) = app
        .post(
            &format!("/api/worksheets/{id}/blocks/{block_id}/undo"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT, "nothing left to undo");
    let (s, _) = app
        .post(
            &format!("/api/worksheets/{id}/blocks/nope/regenerate"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------- lesson SOP

async fn lesson_with_sop(app: &common::TestApp, t: &str) -> i64 {
    let unit = app.create_unit(t, "L3 The Umbrella Stand").await;
    let lesson = app.create_lesson(t, unit, "第 2 節 精讀與提問").await;
    let (s, v) = app
        .put(
            &format!("/api/lessons/{lesson}/sop/material"),
            Some(t),
            json!({ "title": "The Umbrella Stand", "text": passage("") }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let content = json!({
        "objectives": [
            { "id": "o1", "type": "content", "text": "學生能說明林太太設置傘架的原因", "student_facing_en": "I can explain why Mrs. Lin started the stand.", "origin": "teacher" },
            { "id": "o2", "type": "language", "text": "學生能使用 however 表達轉折", "student_facing_en": "I can use however to show a change.", "origin": "teacher" }
        ],
        "task_blocks": [
            { "id": "tb1", "paragraph_ids": ["p2"], "tasks": [
                { "id": "t1", "function": "analyze", "text": "Why did the neighbors laugh?", "scaffolds": { "verbal": "They laughed because ___.", "instructional": "", "procedural": "" }, "origin": "teacher" }
            ] }
        ],
        "warmup": { "background": [{ "id": "w1", "text": "Have you ever borrowed an umbrella?" }], "vocabulary": [{ "id": "v1", "word": "unexpected" }] },
        "exit_ticket": { "items": [{ "id": "e1", "kind": "reflection", "text": "What surprised you?" }] }
    });
    let (s, v) = app
        .put(
            &format!("/api/lessons/{lesson}/sop"),
            Some(t),
            json!({ "content": content }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    lesson
}

async fn sop_versions(app: &common::TestApp, t: &str, lesson: i64) -> usize {
    let (_, v) = app
        .get(&format!("/api/lessons/{lesson}/sop/versions"), Some(t))
        .await;
    v.as_array().unwrap().len()
}

#[tokio::test]
async fn from_a_lesson_everything_is_prefilled_and_the_sop_is_only_changed_on_confirm() {
    let app = test_app!();
    let t = app.teacher("T-001", "general").await;
    let lesson = lesson_with_sop(&app, &t).await;
    let (s, ws) = app
        .post("/api/worksheets", Some(&t), json!({ "lesson_id": lesson }))
        .await;
    assert_eq!(s, StatusCode::CREATED, "{ws}");
    let id = ws["id"].as_i64().unwrap();
    assert_eq!(ws["lesson"]["id"], lesson);
    assert_eq!(ws["material"]["source"]["kind"], "lesson_sop");
    assert_eq!(ws["material"]["paragraphs"].as_array().unwrap().len(), 4);
    assert_eq!(ws["content"]["objectives"].as_array().unwrap().len(), 2);
    assert_eq!(ws["content"]["objectives"][0]["sop_id"], "o1");
    assert_eq!(
        ws["brief"]["tasks"][0]["text"],
        "Why did the neighbors laugh?"
    );
    assert_eq!(ws["brief"]["tasks"][0]["sop_block_id"], "tb1");
    assert_eq!(
        ws["brief"]["warmup"],
        json!(["Have you ever borrowed an umbrella?"])
    );
    assert_eq!(ws["brief"]["vocabulary"], json!(["unexpected"]));
    assert_eq!(ws["brief"]["exit_ticket"][0]["text"], "What surprised you?");
    // Suggested options come from the SOP: 分析資訊 → 推論／預測 (spec §8), warm-up, objectives, frames.
    let suggested = &ws["suggested"];
    assert_eq!(suggested["material_types"], json!(["閱讀文本"]));
    assert_eq!(suggested["student_tasks"], json!(["推論／預測"]));
    for g in [
        "加入暖身活動",
        "顯示學習目標",
        "提供引導問題",
        "提供句型框架",
        "提供字詞表",
    ] {
        assert!(
            suggested["generation"]
                .as_array()
                .unwrap()
                .contains(&json!(g)),
            "{g}"
        );
    }
    assert_eq!(
        ws["settings"], ws["suggested"],
        "settings start as the suggestion"
    );
    assert_eq!(ws["sop_sync"], json!({ "linked": true, "changes": 0 }));

    // Everything prefilled can be removed; the worksheet still generates.
    let sop_count = sop_versions(&app, &t, lesson).await;
    let mut content = ws["content"].clone();
    content["objectives"][0]["student_facing_en"] =
        json!("I can explain, with evidence, why Mrs. Lin started the stand.");
    content["objectives"].as_array_mut().unwrap().remove(1);
    let (s, v) = app
        .put(
            &format!("/api/worksheets/{id}"),
            Some(&t),
            json!({ "content": content, "brief": { "tasks": [], "warmup": [], "vocabulary": [], "exit_ticket": [] },
                    "settings": empty_settings() }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["brief"]["tasks"], json!([]));
    assert_eq!(
        sop_versions(&app, &t, lesson).await,
        sop_count,
        "no automatic overwrite"
    );
    assert!(v["sop_sync"]["changes"].as_u64().unwrap() >= 1);
    let state = generate(&app, &t, id).await;
    assert_eq!(state["generated"], true);
    assert_eq!(
        sop_versions(&app, &t, lesson).await,
        sop_count,
        "generating does not touch the SOP either"
    );
    let student = sheet(&app, &t, id, "standard", "student").await;
    assert_eq!(
        student["objectives"],
        json!(["I can explain, with evidence, why Mrs. Lin started the stand."])
    );

    // Sync back: preview, refuse without confirmation, apply only what was ticked.
    let (s, preview) = app
        .get(&format!("/api/worksheets/{id}/sop-sync"), Some(&t))
        .await;
    assert_eq!(s, StatusCode::OK, "{preview}");
    assert_eq!(preview["core_suggestions"], json!([]));
    let changes = preview["changes"].as_array().unwrap();
    let objective = changes.iter().find(|c| c["key"] == "objective:o1").unwrap();
    assert_eq!(objective["action"], "update");
    assert!(changes
        .iter()
        .any(|c| c["key"] == "remove-objective:o2" && c["action"] == "remove"));
    let sop_version_no = preview["sop_version_no"].clone();
    let (s, _) = app
        .post(
            &format!("/api/worksheets/{id}/sop-sync"),
            Some(&t),
            json!({ "keys": ["objective:o1"], "sop_version_no": sop_version_no }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "no confirmation, no write");
    assert_eq!(sop_versions(&app, &t, lesson).await, sop_count);
    let (s, v) = app
        .post(
            &format!("/api/worksheets/{id}/sop-sync"),
            Some(&t),
            json!({ "confirm": true, "keys": ["objective:o1"], "sop_version_no": sop_version_no }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["applied"], 1);
    assert_eq!(sop_versions(&app, &t, lesson).await, sop_count + 1);
    let (_, sop) = app
        .get(&format!("/api/lessons/{lesson}/sop"), Some(&t))
        .await;
    let objectives = sop["content"]["objectives"].as_array().unwrap();
    assert_eq!(objectives.len(), 2, "the unticked removal was not applied");
    assert_eq!(
        objectives[0]["student_facing_en"],
        "I can explain, with evidence, why Mrs. Lin started the stand."
    );
    // A stale preview is refused.
    let (s, _) = app
        .post(
            &format!("/api/worksheets/{id}/sop-sync"),
            Some(&t),
            json!({ "confirm": true, "keys": ["objective:o1"], "sop_version_no": sop_version_no }),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
}

#[tokio::test]
async fn a_standalone_worksheet_can_be_linked_or_saved_as_a_lesson() {
    let app = test_app!();
    let t = app.teacher("T-001", "general").await;
    let id = direct(&app, &t, &passage("")).await["id"].as_i64().unwrap();
    let (s, _) = app
        .get(&format!("/api/worksheets/{id}/sop-sync"), Some(&t))
        .await;
    assert_eq!(s, StatusCode::CONFLICT, "not linked");
    generate(&app, &t, id).await;
    let unit = app.create_unit(&t, "L3").await;
    let (s, v) = app
        .post(
            &format!("/api/worksheets/{id}/save-as-lesson"),
            Some(&t),
            json!({ "unit_id": unit, "title": "雨傘課" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let lesson = v["lesson"]["id"].as_i64().unwrap();
    assert_eq!(v["lesson"]["title"], "雨傘課");
    let (_, sop) = app
        .get(&format!("/api/lessons/{lesson}/sop"), Some(&t))
        .await;
    assert_eq!(sop["material"]["paragraphs"].as_array().unwrap().len(), 4);
    assert!(sop["content"]["task_blocks"].as_array().unwrap().len() >= 4);
    assert_eq!(v["sop_sync"]["changes"], 0, "everything was written");
    // Unlink and link again.
    let (s, v) = app
        .put(
            &format!("/api/worksheets/{id}/link"),
            Some(&t),
            json!({ "lesson_id": null }),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["lesson"], Value::Null);
    let (s, v) = app
        .put(
            &format!("/api/worksheets/{id}/link"),
            Some(&t),
            json!({ "lesson_id": lesson }),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["lesson"]["id"], lesson);
    let (_, list) = app
        .get(&format!("/api/worksheets?lesson_id={lesson}"), Some(&t))
        .await;
    assert_eq!(list.as_array().unwrap().len(), 1);
}

// ---------------------------------------------------------------- vocabulary check

fn vocabulary_csv() -> String {
    let rows = [
        "umbrella,umbrella,2,n.,base,",
        "neighbor,neighbor,2,n.,base,",
        "disappear,disappear,3,v.,base,",
        "unexpected,unexpected,4,adj.,base,",
        "trust,trust,3,n./v.,base,",
        "stand,stand,1,v./n.,base,",
        "laugh,laugh,1,v.,base,",
        "the,the,1,art.,base,",
        "sign,sign,2,n.,base,",
        "month,month,1,n.,base,",
    ];
    let body: Vec<String> = rows
        .iter()
        .map(|r| format!("{r},TEST LIST v1,測試詞彙表.pdf,5,{SHA}"))
        .collect();
    format!(
        "word,headword,level,pos,kind,appendix_category,list_version,source_doc,source_page,source_sha256\n{}\n",
        body.join("\n")
    )
}

#[tokio::test]
async fn vocabulary_check_is_stored_with_its_version_and_goes_stale() {
    let app = test_app!();
    let t = app.teacher("T-001", "general").await;
    let id = direct(&app, &t, &passage("")).await["id"].as_i64().unwrap();
    let state = generate(&app, &t, id).await;
    let (s, v) = app
        .post(
            &format!("/api/worksheets/{id}/checks/vocabulary"),
            Some(&t),
            json!({ "level": "standard" }),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT, "no word list imported yet: {v}");

    let pi = app.pi_token().await;
    let (s, v) = app
        .post_csv("/api/admin/vocabulary/import", Some(&pi), &vocabulary_csv())
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, v) = app
        .post(
            &format!("/api/worksheets/{id}/checks/vocabulary"),
            Some(&t),
            json!({ "level": "basic" }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "基礎版 was not made: {v}");
    let (s, check) = app
        .post(
            &format!("/api/worksheets/{id}/checks/vocabulary"),
            Some(&t),
            json!({ "level": "standard", "target_level": 1 }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{check}");
    assert_eq!(check["version_no"], state["version_no"]);
    assert_eq!(check["stale"], false);
    assert_eq!(check["analysis"]["wordlist_version"], "TEST LIST v1");
    assert!(check["analysis"]["disclaimer"]
        .as_str()
        .unwrap()
        .contains("備課輔助"));
    assert!(check["analysis"]["basis"]["denominator_note"].is_string());
    let above = check["analysis"]["above_target"].as_array().unwrap();
    let umbrella = above
        .iter()
        .find(|a| a["base"] == "umbrella")
        .expect("level 2 above target 1");
    let line = umbrella["positions"][0]["line"].as_u64().unwrap() as usize;
    assert_eq!(check["lines"][line - 1], "標題", "the position is named");
    assert!(
        !check["analysis"]["unknown"].as_array().unwrap().is_empty(),
        "unlisted words are listed apart"
    );
    // Answers are not part of what students read, so not analysed.
    let mut content = state["content"].clone();
    content["blocks"][1]["title"] = json!("Paragraph 1: Find the facts");
    let (s, _) = app
        .put(
            &format!("/api/worksheets/{id}"),
            Some(&t),
            json!({ "content": content }),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let (_, checks) = app
        .get(&format!("/api/worksheets/{id}/checks"), Some(&t))
        .await;
    assert_eq!(
        checks[0]["stale"], true,
        "an edit makes the old result stale"
    );
    assert_eq!(checks[0]["version_no"], state["version_no"]);
    // Key-word levels in the warm-up come from the list, never from the client.
    let mut content = state["content"].clone();
    let warm = content["blocks"][0]["vocabulary"].as_array_mut().unwrap();
    warm.clear();
    warm.push(json!({ "word": "unexpected", "level": 1, "definition": "surprising" }));
    let (_, v) = app
        .put(
            &format!("/api/worksheets/{id}"),
            Some(&t),
            json!({ "content": content }),
        )
        .await;
    assert_eq!(v["content"]["blocks"][0]["vocabulary"][0]["level"], 4);
}

// ---------------------------------------------------------------- export and ownership

#[tokio::test]
async fn export_returns_docx_and_pdf_for_both_audiences() {
    let app = test_app!();
    let t = app.teacher("T-001", "general").await;
    let id = direct(&app, &t, &passage("")).await["id"].as_i64().unwrap();
    let mut s = empty_settings();
    s["levels"] = json!(["basic", "standard"]);
    put_settings(&app, &t, id, s).await;
    generate(&app, &t, id).await;

    let (s, ct, cd, bytes) = raw_get(
        &app,
        &format!("/api/worksheets/{id}/export?format=docx&level=basic&audience=student"),
        &t,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert!(ct.contains("wordprocessingml"));
    assert!(
        cd.starts_with("attachment;") && cd.contains("filename*=UTF-8''"),
        "{cd}"
    );
    assert_eq!(&bytes[..2], b"PK");
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("word/document.xml"));

    let (s, ct, _, bytes) = raw_get(
        &app,
        &format!("/api/worksheets/{id}/export?format=pdf&level=standard&audience=student"),
        &t,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(ct, "application/pdf");
    assert!(bytes.starts_with(b"%PDF-"));
    assert!(String::from_utf8_lossy(&bytes[bytes.len().saturating_sub(64)..]).contains("%%EOF"));

    let (s, _, _, bytes) = raw_get(
        &app,
        &format!("/api/worksheets/{id}/export?format=docx&level=standard&audience=teacher"),
        &t,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(&bytes[..2], b"PK");
    // The teacher PDF has Chinese notes: made when a CJK font is available, refused clearly otherwise.
    let (s, _, _, bytes) = raw_get(
        &app,
        &format!("/api/worksheets/{id}/export?format=pdf&level=standard&audience=teacher"),
        &t,
    )
    .await;
    if manna_api::worksheet::export::pdf_supports_cjk() {
        assert_eq!(s, StatusCode::OK);
        assert!(bytes.starts_with(b"%PDF-"));
    } else {
        assert_eq!(s, StatusCode::CONFLICT);
    }

    let (s, _, _, _) = raw_get(
        &app,
        &format!("/api/worksheets/{id}/export?format=pdf&level=advanced"),
        &t,
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "進階版 was not selected");
    let (s, _, _, _) = raw_get(
        &app,
        &format!("/api/worksheets/{id}/export?format=odt&level=basic"),
        &t,
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn worksheets_belong_to_their_teacher() {
    let app = test_app!();
    let t = app.teacher("T-001", "general").await;
    let other = app.teacher("T-002", "general").await;
    let id = direct(&app, &t, &passage("")).await["id"].as_i64().unwrap();
    generate(&app, &t, id).await;
    for path in [
        format!("/api/worksheets/{id}"),
        format!("/api/worksheets/{id}/jobs"),
        format!("/api/worksheets/{id}/sheet?level=standard"),
        format!("/api/worksheets/{id}/versions"),
        format!("/api/worksheets/{id}/checks"),
        format!("/api/worksheets/{id}/export?format=docx&level=standard"),
    ] {
        let (s, _, _, _) = raw_get(&app, &path, &other).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{path}");
    }
    let (s, _) = app
        .post(
            &format!("/api/worksheets/{id}/generate"),
            Some(&other),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = app
        .put(
            &format!("/api/worksheets/{id}"),
            Some(&other),
            json!({ "title": "x" }),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (_, list) = app.get("/api/worksheets", Some(&other)).await;
    assert_eq!(list, json!([]));
    // Someone else's lesson cannot be the source either.
    let lesson = lesson_with_sop(&app, &t).await;
    let (s, _) = app
        .post(
            "/api/worksheets",
            Some(&other),
            json!({ "lesson_id": lesson }),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = app
        .put(
            &format!("/api/worksheets/{id}/link"),
            Some(&t),
            json!({ "lesson_id": 999_999 }),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    // Deleted worksheets are gone for their teacher too.
    let (s, _) = app
        .send(
            axum::http::Method::DELETE,
            &format!("/api/worksheets/{id}"),
            Some(&t),
            None,
            Vec::new(),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = app.get(&format!("/api/worksheets/{id}"), Some(&t)).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn pasted_material_goes_to_the_library_and_only_shared_notes_reach_the_model() {
    let app = test_app!();
    let t = app.teacher("T-001", "general").await;
    let ws = direct(&app, &t, &passage("")).await;
    let id = ws["id"].as_i64().unwrap();
    let material = ws["material"]["source"]["material_id"]
        .as_i64()
        .expect("stored in the library");
    let (s, lib) = app.get("/api/materials", Some(&t)).await;
    assert_eq!(s, StatusCode::OK, "{lib}");
    assert!(lib.to_string().contains("The Umbrella Stand"));
    // One note shared with the AI, one not.
    for (note, share) in [("學生常忽略這個轉折", true), ("只給我自己看", false)] {
        let (s, v) = app
            .post(
                &format!("/api/materials/{material}/annotations"),
                Some(&t),
                json!({ "paragraph_id": "p3", "start": 0, "end": 12, "note": note, "share_with_ai": share }),
            )
            .await;
        assert_eq!(s, StatusCode::OK, "{v}");
    }
    let state = generate(&app, &t, id).await;
    let users: Vec<String> = sqlx::query_scalar(
        "SELECT input_snapshot -> 'prompt' ->> 'user' FROM ai_interactions WHERE module = 'worksheet_tasks'",
    )
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert!(users[0].contains("學生常忽略這個轉折"), "{}", users[0]);
    assert!(!users[0].contains("只給我自己看"));
    let tasks_job = state["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|j| j["step"] == "tasks")
        .unwrap()
        .clone();
    assert!(tasks_job["context"]
        .to_string()
        .contains("教師注記（提供給 AI）"));
    // Notes never reach the student sheet.
    let student = sheet(&app, &t, id, "standard", "student").await;
    assert!(!student.to_string().contains("學生常忽略"));
}
