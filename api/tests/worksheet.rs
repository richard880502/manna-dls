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
/// internal names, not printed). The Chinese hints of the task words (SIOP V2: compare 比較)
/// are the one place Chinese is meant to be; they are left out here.
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
            if v["task_word"] != true {
                strings(&v["definition"], &mut out);
            }
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

/// The tasks calls of one worksheet's generation: (attempt, status, check_results).
async fn task_calls(app: &common::TestApp, id: i64) -> Vec<(i32, String, Value)> {
    sqlx::query_as(
        "SELECT attempt, status, check_results FROM ai_interactions
          WHERE module = 'worksheet_tasks' AND (input_snapshot -> 'worksheet' ->> 'id')::bigint = $1
          ORDER BY id",
    )
    .bind(id)
    .fetch_all(&app.db)
    .await
    .unwrap()
}

fn check<'a>(results: &'a Value, name: &str) -> &'a Value {
    results["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == name)
        .unwrap()
}

#[tokio::test]
async fn a_fill_in_item_always_has_a_blank_to_write_in() {
    let app = test_app!();
    let t = app.teacher("T-001", "general").await;
    // (marker, calls, the prompt's blank, the format it ends with, the repair recorded)
    let cases = [
        ("", 1, true, "填空題", None),
        ("FAKE_WS_NO_BLANK", 2, true, "填空題", None),
        (
            "FAKE_WS_NO_BLANK_ALWAYS",
            2,
            true,
            "填空題",
            Some("把題目裡參考答案的字詞換成 ______"),
        ),
        (
            "FAKE_WS_BLANK_LOST",
            2,
            false,
            "簡答題",
            Some("改成簡答題（印作答線）"),
        ),
    ];
    for (marker, calls, blank, format, repair) in cases {
        let id = direct(&app, &t, &passage(marker)).await["id"]
            .as_i64()
            .unwrap();
        let state = generate(&app, &t, id).await;
        let item = blocks(&state)
            .iter()
            .flat_map(|b| b["items"].as_array().unwrap().clone())
            .find(|i| {
                i["prompt"]
                    .as_str()
                    .unwrap()
                    .starts_with("Complete the sentence from")
            })
            .unwrap_or_else(|| panic!("{marker}: no fill-in item in {state}"));
        let prompt = item["prompt"].as_str().unwrap();
        let answer = item["answer"].as_str().unwrap();
        assert_eq!(item["answer_format"], format, "{marker}: {item}");
        assert_eq!(prompt.contains("______"), blank, "{marker}: {prompt}");
        assert!(
            !prompt.contains(&format!(" {answer} ")),
            "{marker}: the answer is not left in the prompt: {prompt}"
        );

        let rows = task_calls(&app, id).await;
        assert_eq!(rows.len(), calls, "{marker}: {rows:?}");
        let first = &rows[0].2;
        let wellformed = check(first, "items_wellformed");
        if calls == 1 {
            assert_eq!(wellformed["passed"], true, "{marker}: {first}");
            continue;
        }
        // The first call fails the check with the problem stated; the repair call is made.
        assert_eq!(wellformed["passed"], false, "{marker}: {first}");
        assert!(
            wellformed["detail"].as_str().unwrap().contains("沒有空格"),
            "{marker}: {wellformed}"
        );
        let (attempt, status, last) = rows.last().unwrap();
        assert_eq!((*attempt, status.as_str()), (2, "shown"), "{marker}");
        let repairs: Vec<&str> = last["repairs"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        match repair {
            None => assert!(
                !repairs
                    .iter()
                    .any(|r| r.contains("______") || r.contains("簡答題")),
                "{marker}: {repairs:?}"
            ),
            Some(note) => assert!(
                repairs.iter().any(|r| r.contains(note)),
                "{marker}: {repairs:?}"
            ),
        }
        // The student sheet gives the 簡答題 lines to write on.
        let student = sheet(&app, &t, id, "standard", "student").await;
        let printed = student["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|b| b["items"].as_array().unwrap().clone())
            .find(|i| i["id"] == item["id"])
            .unwrap();
        assert_eq!(printed["lines"].as_u64().unwrap() > 0, !blank, "{printed}");
    }
}

#[tokio::test]
async fn a_sentence_frame_is_printed_apart_not_numbered_as_a_question() {
    const FRAME: &str = "Hi! In the 3rd paragraph, the author describes ______. After reading it carefully, I identified ______ as one of the main characteristics of narrative writing, as seen in the text where ______.";
    let app = test_app!();
    let t = app.teacher("T-001", "general").await;
    for (marker, lifted) in [
        ("FAKE_WS_EXIT_FRAME", false),
        ("FAKE_WS_EXIT_FRAME_ALWAYS", true),
    ] {
        let id = direct(&app, &t, &passage(marker)).await["id"]
            .as_i64()
            .unwrap();
        let state = generate(&app, &t, id).await;
        let exit = blocks(&state)
            .into_iter()
            .find(|b| b["kind"] == "exit_ticket")
            .unwrap();

        // Call 1 wrote the frame as a question: sent back with the problem stated.
        let calls: Vec<(i32, Value)> = sqlx::query_as(
            "SELECT attempt, check_results FROM ai_interactions
              WHERE module = 'worksheet_exit_ticket'
                AND (input_snapshot -> 'worksheet' ->> 'id')::bigint = $1 ORDER BY id",
        )
        .bind(id)
        .fetch_all(&app.db)
        .await
        .unwrap();
        assert_eq!(calls.len(), 2, "{marker}");
        let wellformed = check(&calls[0].1, "items_wellformed");
        assert_eq!(wellformed["passed"], false, "{marker}");
        assert!(wellformed["detail"]
            .as_str()
            .unwrap()
            .contains("只有句型框架"));

        let items = exit["items"].as_array().unwrap();
        assert!(
            items
                .iter()
                .all(|i| !i["prompt"].as_str().unwrap().contains("I identified")),
            "{marker}: {exit}"
        );
        let frames: Vec<&Value> = exit["scaffolds"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["kind"] == "句型框架")
            .collect();
        if !lifted {
            // The repair call wrote a real question: nothing to move.
            assert!(frames.is_empty(), "{marker}: {exit}");
            continue;
        }
        // Still a frame after the repair: it is the block's Sentence frames box, and the
        // question asks to use it.
        assert_eq!(frames.len(), 1, "{exit}");
        assert_eq!(frames[0]["content"], FRAME);
        assert_eq!(frames[0]["type"], "verbal");
        assert_eq!(
            items[0]["prompt"],
            "Use the sentence frame above to write your answer."
        );
        assert_eq!(items[0]["answer_format"], "簡答題");
        let repairs = calls[1].1["repairs"].to_string();
        assert!(repairs.contains("句型框架移出題號"), "{repairs}");

        // Every printed form: the box un-numbered, the questions numbered without a gap.
        for audience in ["student", "teacher"] {
            let printed = sheet(&app, &t, id, "standard", audience).await;
            let nos: Vec<u64> = printed["blocks"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|b| b["items"].as_array().unwrap().clone())
                .map(|i| i["no"].as_u64().unwrap())
                .collect();
            assert_eq!(nos, (1..=nos.len() as u64).collect::<Vec<_>>());
            let last = printed["blocks"]
                .as_array()
                .unwrap()
                .last()
                .unwrap()
                .clone();
            assert_eq!(last["kind"], "exit_ticket");
            let support = last["supports"]
                .as_array()
                .unwrap()
                .iter()
                .find(|s| s["heading"] == "Sentence frames")
                .unwrap_or_else(|| panic!("{last}"))
                .clone();
            assert_eq!(support["content"], FRAME);
            assert!(last["items"]
                .as_array()
                .unwrap()
                .iter()
                .all(|i| !i["prompt"].as_str().unwrap().contains("I identified")));
        }
        let xml = export_docx(&app, &t, id).await;
        assert!(xml.contains("Sentence frames"));
        assert_eq!(
            xml.matches("Hi! In the 3rd paragraph").count(),
            1,
            "printed once, in the box"
        );
    }
}

/// A teacher found directions printed in a 「Sentence frames」 box. The scaffolds step sends them
/// back; written there again, they join the block's steps and the frame keeps only frames.
#[tokio::test]
async fn directions_written_as_sentence_frames_join_the_steps() {
    let app = test_app!();
    let t = app.teacher("T-001", "general").await;
    let id = direct(&app, &t, &passage("FAKE_WS_FRAME_STEPS_ALWAYS")).await["id"]
        .as_i64()
        .unwrap();
    let state = generate(&app, &t, id).await;
    let calls: Vec<(i32, Value)> = sqlx::query_as(
        "SELECT attempt, check_results FROM ai_interactions
          WHERE module = 'worksheet_scaffolds' ORDER BY id",
    )
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert_eq!(calls.len(), 2, "sent back once");
    let wellformed = check(&calls[0].1, "supports_wellformed");
    assert_eq!(wellformed["passed"], false);
    assert!(wellformed["detail"]
        .as_str()
        .unwrap()
        .contains("句型框架裡寫了作答步驟"));
    let activities: Vec<Value> = blocks(&state)
        .into_iter()
        .filter(|b| b["kind"] == "activity")
        .collect();
    assert!(!activities.is_empty());
    for b in &activities {
        let scaffolds = b["scaffolds"].as_array().unwrap();
        let frames = scaffolds
            .iter()
            .find(|s| s["kind"] == "句型框架")
            .unwrap_or_else(|| panic!("{b}"));
        assert_eq!(frames["content"], "The writer says ___ in order to ___.");
        let steps: Vec<&str> = scaffolds
            .iter()
            .filter(|s| s["type"] == "procedural")
            .flat_map(|s| s["steps"].as_array().unwrap())
            .map(|s| s.as_str().unwrap())
            .collect();
        for direction in [
            "Identify the main idea in the first sentence.",
            "Use one detail from the text as evidence.",
        ] {
            assert!(steps.contains(&direction), "{steps:?}");
        }
    }
    let printed = sheet(&app, &t, id, "standard", "student").await;
    assert!(!printed.to_string().contains("Use this frame"));
}

/// The student DOCX of the standard version, as its document XML.
async fn export_docx(app: &common::TestApp, t: &str, id: i64) -> String {
    let (status, _, _, bytes) = raw_get(
        app,
        &format!("/api/worksheets/{id}/export?format=docx&level=standard&audience=student"),
        t,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("word/document.xml").unwrap(), &mut xml)
        .unwrap();
    xml
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

    // The student PDF has the Chinese hints of the task words (SIOP V2) since the full
    // worksheet lists them: made with a CJK font, refused clearly otherwise.
    let (s, ct, _, bytes) = raw_get(
        &app,
        &format!("/api/worksheets/{id}/export?format=pdf&level=standard&audience=student"),
        &t,
    )
    .await;
    if manna_api::worksheet::export::pdf_supports_cjk() {
        assert_eq!(s, StatusCode::OK);
        assert_eq!(ct, "application/pdf");
        assert!(bytes.starts_with(b"%PDF-"));
        assert!(
            String::from_utf8_lossy(&bytes[bytes.len().saturating_sub(64)..]).contains("%%EOF")
        );
    } else {
        assert_eq!(s, StatusCode::CONFLICT);
    }

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

/// The editor's answer formats (a table with its own columns, choices kept by a format that does
/// not print them) are saved with the content, checked, and printed the same way in the
/// projection and the exports.
#[tokio::test]
async fn an_edited_answer_format_is_saved_projected_and_exported() {
    let app = test_app!();
    let t = app.teacher("T-001", "general").await;
    let id = direct(&app, &t, &passage("")).await["id"].as_i64().unwrap();
    let mut s = empty_settings();
    s["levels"] = json!(["standard"]);
    put_settings(&app, &t, id, s).await;
    let state = generate(&app, &t, id).await;
    let mut content = state["content"].clone();
    let b = blocks(&state)
        .iter()
        .position(|b| {
            b["kind"] == "activity" && b["items"].as_array().is_some_and(|i| i.len() >= 2)
        })
        .expect("an activity block with two questions");
    let items = content["blocks"][b]["items"].as_array_mut().unwrap();
    items[0]["answer_format"] = json!("表格整理");
    items[0]["prompt"] = json!("Complete the table about the stand.");
    items[0]["table"] = json!({ "columns": ["Problem", "Solution"], "rows": 4 });
    items[1]["answer_format"] = json!("簡答題");
    items[1]["options"] = json!(["Kept choice A", "Kept choice B"]);
    let base = state["version_no"].as_i64().unwrap();

    // A save from an older version is refused; from the latest it is stored.
    let (s, v) = app
        .put(
            &format!("/api/worksheets/{id}"),
            Some(&t),
            json!({ "content": content, "base_version_no": base - 1 }),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    let (s, v) = app
        .put(
            &format!("/api/worksheets/{id}"),
            Some(&t),
            json!({ "content": content, "base_version_no": base }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, reloaded) = app.get(&format!("/api/worksheets/{id}"), Some(&t)).await;
    let saved = &reloaded["content"]["blocks"][b]["items"];
    assert_eq!(
        saved[0]["table"],
        json!({ "columns": ["Problem", "Solution"], "rows": 4 })
    );
    assert_eq!(
        saved[1]["options"],
        json!(["Kept choice A", "Kept choice B"])
    );

    let sheet = sheet(&app, &t, id, "standard", "student").await;
    let printed_block = sheet["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|pb| pb["id"] == content["blocks"][b]["id"])
        .unwrap();
    let items = &printed_block["items"];
    assert_eq!(
        items[0]["table"],
        json!({ "columns": ["Problem", "Solution"], "rows": 4 })
    );
    assert_eq!(items[0]["lines"], 0);
    assert_eq!(items[1]["options"], json!([]));
    assert!(items[1].get("table").is_none());

    let (s, _, _, bytes) = raw_get(
        &app,
        &format!("/api/worksheets/{id}/export?format=docx&level=standard&audience=student"),
        &t,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("word/document.xml").unwrap(), &mut xml)
        .unwrap();
    assert!(xml.contains(">Problem<") && xml.contains(">Solution<"));
    assert!(!xml.contains("Kept choice A"));

    // A table outside the limits is refused with the teacher's words.
    let mut bad = reloaded["content"].clone();
    bad["blocks"][b]["items"][0]["table"] = json!({ "columns": [], "rows": 3 });
    let (s, v) = app
        .put(
            &format!("/api/worksheets/{id}"),
            Some(&t),
            json!({ "content": bad, "base_version_no": reloaded["version_no"] }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
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
        .contains("教師註記（提供給 AI）"));
    // Notes never reach the student sheet.
    let student = sheet(&app, &t, id, "standard", "student").await;
    assert!(!student.to_string().contains("學生常忽略"));
}

// ---------------------------------------------------------------- the real model's failure modes

#[tokio::test]
async fn a_runaway_output_is_kept_for_diagnosis_and_repaired() {
    let app = test_app!();
    let t = app.teacher("T-001", "general").await;
    let id = direct(&app, &t, &passage("FAKE_RUNAWAY")).await["id"]
        .as_i64()
        .unwrap();
    let state = generate(&app, &t, id).await;
    let outcomes: Vec<&str> = state["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|j| j["outcome"].as_str().unwrap())
        .collect();
    assert_eq!(outcomes, ["shown", "shown", "shown", "shown"], "{state}");
    // Every first call ran away; what it wrote is stored, shortened, with what kind it was.
    type Row = (String, i32, String, Option<Value>, Option<Value>);
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT module, attempt, status, output, check_results FROM ai_interactions
          WHERE module LIKE 'worksheet_%' ORDER BY id",
    )
    .fetch_all(&app.db)
    .await
    .unwrap();
    let first: Vec<&Row> = rows.iter().filter(|r| r.1 == 1).collect();
    assert_eq!(first.len(), 4);
    for (module, _, status, output, checks) in first {
        assert_eq!(status, "blocked", "{module}");
        let output = output.as_ref().unwrap();
        assert_eq!(output["truncated_at_max_tokens"], true);
        assert_eq!(output["runaway"], "whitespace", "{module}");
        assert_eq!(output["longest_whitespace"], 7505, "{module}: {output}");
        let excerpt = output["runaway_excerpt"].as_str().unwrap();
        assert!(excerpt.contains("⟨7505 whitespace⟩"), "{excerpt}");
        assert!(excerpt.contains("Before you read"));
        assert!(excerpt.len() < 600, "collapsed: {excerpt}");
        let detail = checks.as_ref().unwrap()["checks"][0]["detail"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(detail.contains("whitespace runaway"), "{detail}");
    }
    assert!(rows.iter().filter(|r| r.1 == 2).all(|r| r.2 == "shown"));
}

#[tokio::test]
async fn failed_steps_are_not_printed_and_scaffold_targets_are_forgiving() {
    let app = test_app!();
    let t = app.teacher("T-001", "general").await;
    let id = direct(
        &app,
        &t,
        &passage("FAKE_WS_WARMUP_BLOCK FAKE_WS_TARGET_ALIAS FAKE_WS_SPLICE_ALWAYS"),
    )
    .await["id"]
        .as_i64()
        .unwrap();
    let state = generate(&app, &t, id).await;
    let job = |step: &str| {
        state["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|j| j["step"] == step)
            .unwrap()
            .clone()
    };
    assert_eq!(job("warmup")["outcome"], "blocked");
    assert_eq!(job("tasks")["outcome"], "shown");
    assert_eq!(job("scaffolds")["outcome"], "shown");
    // "b1"-style targets and an unasked "warmup" set cost no repair call; spliced evidence
    // neither: both are put right in code and recorded as repaired.
    for module in ["worksheet_scaffolds", "worksheet_tasks"] {
        let rows: Vec<(i32, String, Value)> = sqlx::query_as(
            "SELECT attempt, status, check_results FROM ai_interactions
              WHERE module = $1 ORDER BY id",
        )
        .bind(module)
        .fetch_all(&app.db)
        .await
        .unwrap();
        assert_eq!(rows.len(), 1, "{module}: {rows:?}");
        assert_eq!(rows[0].1, "shown");
        assert_eq!(rows[0].2["passed"], true, "{module}: {}", rows[0].2);
        assert!(!rows[0].2["repairs"].as_array().unwrap().is_empty());
    }
    let activities: Vec<Value> = blocks(&state)
        .into_iter()
        .filter(|b| b["kind"] == "activity")
        .collect();
    // A verbal scaffold: the questions' own frames and hints stand in for the pooled boxes.
    assert!(activities.iter().all(|b| b["scaffolds"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["type"] == "verbal")
        || b["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["frame"].is_string() || i["hint"] != "")));

    // The editor keeps the empty warm-up with its message; no printed sheet shows it.
    let warm = &blocks(&state)[0];
    assert_eq!(warm["kind"], "warmup");
    assert!(warm["message"]
        .as_str()
        .unwrap()
        .contains("暖身沒有產生成功"));
    for audience in ["student", "teacher"] {
        let s = sheet(&app, &t, id, "standard", audience).await;
        let kinds: Vec<&str> = s["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b["kind"].as_str().unwrap())
            .collect();
        assert!(!kinds.contains(&"warmup"), "{audience}: {kinds:?}");
        assert_eq!(s["blocks"][0]["label"], "A");
        assert!(!s.to_string().contains("沒有產生成功"));
    }
    let (s, _, _, bytes) = raw_get(
        &app,
        &format!("/api/worksheets/{id}/export?format=docx&level=standard&audience=student"),
        &t,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(&bytes[..2], b"PK");
}

// ---------------------------------------------------------------- 字詞表: at the end, and 恢復

#[tokio::test]
async fn the_word_table_can_move_to_the_end_of_every_printed_form() {
    let app = test_app!();
    let t = app.teacher("T-001", "general").await;
    let id = direct(&app, &t, &passage("")).await["id"].as_i64().unwrap();
    let state = generate(&app, &t, id).await;
    let warm_words = state["content"]["blocks"][0]["vocabulary"]
        .as_array()
        .unwrap()
        .len();
    assert!(warm_words > 0, "the warm-up lists key words");
    let in_block = sheet(&app, &t, id, "standard", "student").await;
    assert_eq!(
        in_block["blocks"][0]["vocabulary"]
            .as_array()
            .unwrap()
            .len(),
        warm_words
    );
    assert!(in_block.get("end_words").is_none());

    let mut content = state["content"].clone();
    content["vocabulary_at_end"] = json!(true);
    let (s, saved) = app
        .put(
            &format!("/api/worksheets/{id}"),
            Some(&t),
            json!({ "content": content, "base_version_no": state["version_no"] }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{saved}");
    assert_eq!(saved["content"]["vocabulary_at_end"], true);
    assert!(saved["version_no"].as_i64() > state["version_no"].as_i64());
    // The words stay in the block's content (editing, regenerate); only the print moves.
    assert_eq!(
        saved["content"]["blocks"][0]["vocabulary"]
            .as_array()
            .unwrap()
            .len(),
        warm_words
    );

    let at_end = sheet(&app, &t, id, "standard", "student").await;
    for b in at_end["blocks"].as_array().unwrap() {
        assert!(b["vocabulary"].as_array().unwrap().is_empty(), "{b}");
    }
    assert_eq!(at_end["end_words"].as_array().unwrap().len(), warm_words);

    // DOCX: the Key words table comes after every block's text.
    let xml = export_docx(&app, &t, id).await;
    let last_block = xml.rfind("Heading2").expect("blocks are printed");
    let key_words = xml.find("Key words").expect("the table is printed");
    assert!(key_words > last_block, "the table is after the last block");
    assert_eq!(xml.matches("Key words").count(), 1);

    // PDF: still produced.
    let (status, _, _, bytes) = raw_get(
        &app,
        &format!("/api/worksheets/{id}/export?format=pdf&level=standard&audience=student"),
        &t,
    )
    .await;
    if manna_api::worksheet::export::pdf_supports_cjk() {
        assert_eq!(status, StatusCode::OK);
        assert_eq!(&bytes[..4], b"%PDF");
    }
}

#[tokio::test]
async fn a_deleted_word_table_comes_back_with_levels_from_the_word_list() {
    let app = test_app!();
    let t = app.teacher("T-001", "general").await;
    let id = direct(&app, &t, &passage("")).await["id"].as_i64().unwrap();
    let pi = app.pi_token().await;
    let (s, v) = app
        .post_csv("/api/admin/vocabulary/import", Some(&pi), &vocabulary_csv())
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let state = generate(&app, &t, id).await;
    let block_id = state["content"]["blocks"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let url = format!("/api/worksheets/{id}/blocks/{block_id}/restore-word-table");

    // Nothing was deleted yet.
    let (s, v) = app.post(&url, Some(&t), json!({})).await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");

    // What the editor sends for 「刪除」: the rows move to `vocabulary_removed`. A level the client
    // writes is never kept.
    let mut content = state["content"].clone();
    let rows = content["blocks"][0]["vocabulary"].clone();
    let count = rows.as_array().unwrap().len();
    assert!(count > 0);
    content["blocks"][0]["vocabulary"] = json!([]);
    content["blocks"][0]["vocabulary_removed"] = rows.clone();
    let (s, saved) = app
        .put(
            &format!("/api/worksheets/{id}"),
            Some(&t),
            json!({ "content": content, "base_version_no": state["version_no"] }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{saved}");
    assert!(saved["content"]["blocks"][0]["vocabulary"]
        .as_array()
        .unwrap()
        .is_empty());

    // Another teacher cannot restore it.
    let other = app.teacher("T-002", "general").await;
    let (s, _) = app.post(&url, Some(&other), json!({})).await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    let (s, restored) = app.post(&url, Some(&t), json!({})).await;
    assert_eq!(s, StatusCode::OK, "{restored}");
    let block = &restored["content"]["blocks"][0];
    let words = block["vocabulary"].as_array().unwrap();
    assert_eq!(words.len(), count);
    assert!(block.get("vocabulary_removed").is_none());
    for (before, after) in rows.as_array().unwrap().iter().zip(words) {
        assert_eq!(before["word"], after["word"]);
        assert_eq!(before["definition"], after["definition"]);
    }
    let unexpected = words.iter().find(|w| w["word"] == "unexpected");
    if let Some(w) = unexpected {
        assert_eq!(w["level"], 4, "the level is the word list's");
    }
    // A new version, kept like any other edit; the history can still show the deleted state.
    assert!(restored["version_no"].as_i64() > saved["version_no"].as_i64());
    let (s, _) = app.post(&url, Some(&t), json!({})).await;
    assert_eq!(s, StatusCode::CONFLICT, "nothing is left to restore");
}

#[tokio::test]
async fn sentence_frames_and_hints_go_under_their_own_question() {
    let app = test_app!();
    let t = app.teacher("T-001", "general").await;
    let id = direct(&app, &t, &passage("FAKE_WS_FRAME_BAD")).await["id"]
        .as_i64()
        .unwrap();
    // Hints come only when 引導問題 is ticked (or nothing is); frames with 句型框架.
    let mut s = empty_settings();
    s["levels"] = json!(["basic", "standard"]);
    s["generation"] = json!(["加入暖身活動", "提供引導問題", "提供字詞表", "提供句型框架"]);
    put_settings(&app, &t, id, s).await;
    let state = generate(&app, &t, id).await;

    // Call 1 wrote a frame without a blank and one for a question that is not there: the
    // problem was stated to the model, and the second call's frames are the ones kept.
    let calls: Vec<(i32, Value)> = sqlx::query_as(
        "SELECT attempt, check_results FROM ai_interactions
          WHERE module = 'worksheet_scaffolds'
            AND (input_snapshot -> 'worksheet' ->> 'id')::bigint = $1 ORDER BY id",
    )
    .bind(id)
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(check(&calls[0].1, "supports_wellformed")["passed"], false);
    assert_eq!(check(&calls[1].1, "supports_wellformed")["passed"], true);

    for b in blocks(&state)
        .into_iter()
        .filter(|b| b["kind"] == "activity")
    {
        // Not one box of frames or of guiding questions on top of the block.
        for s in b["scaffolds"].as_array().unwrap() {
            assert!(
                s["kind"] != "句型框架" && s["kind"] != "引導問題",
                "pooled: {s}"
            );
        }
        for item in b["items"].as_array().unwrap() {
            let open = item["answer_format"] == "簡答題" || item["answer_format"] == "段落寫作";
            let frame = item["frame"].as_str().unwrap_or_default();
            assert_eq!(open, !frame.is_empty(), "{item}");
            if open {
                assert!(frame.starts_with("Hello, everyone.") && frame.contains("___"));
            }
            assert!(
                !item["hint"].as_str().unwrap_or_default().is_empty(),
                "{} {item}",
                b["id"]
            );
        }
    }

    // Printed under the question, in both audiences; the hint in 基礎版.
    for audience in ["student", "teacher"] {
        let printed = sheet(&app, &t, id, "basic", audience).await;
        let items: Vec<Value> = printed["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|b| b["items"].as_array().unwrap().clone())
            .collect();
        let framed: Vec<&Value> = items.iter().filter(|i| i.get("frame").is_some()).collect();
        assert!(!framed.is_empty(), "{audience}");
        assert!(framed
            .iter()
            .all(|i| i["answer_format"] == "簡答題" || i["answer_format"] == "段落寫作"));
        assert!(items.iter().any(|i| i.get("hint").is_some()));
    }
}
