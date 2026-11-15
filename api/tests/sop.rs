//! Phase 2.1 (docs/API.md「課例 SOP」): the versioned SOP document, material paragraphs, the two
//! AI stages as background jobs (fake provider), decisions, ownership and legacy lessons.

#[macro_use]
mod common;

use std::time::Duration;

use axum::http::StatusCode;
use manna_api::config::AiProviderKind;
use serde_json::{json, Value};

const P1: &str = "Mrs. Lin put an umbrella stand outside her shop on a rainy Monday. A sign said, “Take one if you need it.”";
const P2: &str = "Her neighbors laughed at the idea. They were sure that every umbrella would be gone within a week.";
const P3: &str = "In the first month, twelve of the thirty umbrellas did disappear. However, something unexpected followed.";
const P4: &str = "Today the stand holds more umbrellas than it did at the start. Mrs. Lin says trust is like an umbrella.";

const CURRICULUM_CSV: &str = "item_code,category,school_track,title,content,source_doc,\
source_version,source_page,official_code,item_kind,strand,strand_name,stage,subject,advanced,\
recurring,source_sha256
T-G-3-V-2,curriculum,general,3-Ⅴ-2 學習表現｜測試閱讀,能讀懂測試用的短文並找出主旨。,測試領綱.pdf,測試版,12,3-Ⅴ-2,learning_performance,3,測試閱讀,V,英語文,False,False,aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
T-G-3-V-10,curriculum,general,*3-Ⅴ-10 學習表現｜測試閱讀,能分析測試用短文的結構。,測試領綱.pdf,測試版,13,*3-Ⅴ-10,learning_performance,3,測試閱讀,V,英語文,True,False,aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
T-G-3-IV-1,curriculum,general,3-Ⅳ-1 學習表現｜測試閱讀,能讀懂國中測試用的短文。,測試領綱.pdf,測試版,11,3-Ⅳ-1,learning_performance,3,測試閱讀,IV,英語文,False,False,aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
T-V-3-V-1,curriculum,vocational,3-Ⅴ-1 學習表現｜測試閱讀,能讀懂技高測試用的說明文件。,測試技高領綱.pdf,測試版,9,3-Ⅴ-1,learning_performance,3,測試閱讀,V,英語文,False,False,bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
T-V-3-V-2,curriculum,vocational,3-Ⅴ-2 學習表現｜測試閱讀,能擷取技高測試用文件的關鍵資訊。,測試技高領綱.pdf,測試版,9,3-Ⅴ-2,learning_performance,3,測試閱讀,V,英語文,False,False,bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
";

/// Official curriculum items (synthetic texts): two senior-high items for 普高, one junior-high
/// item (out of the default scope) and two for 技高.
async fn import_curriculum(app: &common::TestApp) {
    let pi = app.pi_token().await;
    let (s, v) = app
        .post_csv("/api/admin/knowledge/import", Some(&pi), CURRICULUM_CSV)
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
}

fn passage(marker: &str) -> String {
    format!("{P1}\n\n{P2}\n\n{P3}\n\n{P4} {marker}")
}

fn err_code(v: &Value) -> &str {
    v["error"]["code"].as_str().unwrap_or("<none>")
}

async fn lesson(app: &common::TestApp, t: &str) -> i64 {
    let unit = app.create_unit(t, "L3 The Umbrella Stand").await;
    app.create_lesson(t, unit, "第 2 節 精讀與提問").await
}

async fn put_material(app: &common::TestApp, t: &str, lesson: i64, text: &str) -> Value {
    let (s, v) = app
        .put(
            &format!("/api/lessons/{lesson}/sop/material"),
            Some(t),
            json!({ "title": "The Umbrella Stand", "source_note": "課本第三課", "text": text }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v
}

async fn put_sop(
    app: &common::TestApp,
    t: &str,
    lesson: i64,
    content: Value,
) -> (StatusCode, Value) {
    app.put(
        &format!("/api/lessons/{lesson}/sop"),
        Some(t),
        json!({ "content": content }),
    )
    .await
}

async fn get_sop(app: &common::TestApp, t: &str, lesson: i64) -> Value {
    let (s, v) = app
        .get(&format!("/api/lessons/{lesson}/sop"), Some(t))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v
}

/// Starts a job and polls it to the end, the way the page does.
async fn run_job(app: &common::TestApp, t: &str, lesson: i64, kind: &str) -> Value {
    let (s, job) = app
        .post(
            &format!("/api/lessons/{lesson}/sop/{kind}"),
            Some(t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{job}");
    wait_job(app, t, job["id"].as_i64().unwrap()).await
}

async fn wait_job(app: &common::TestApp, t: &str, id: i64) -> Value {
    for _ in 0..400 {
        let (s, job) = app.get(&format!("/api/sop-jobs/{id}"), Some(t)).await;
        assert_eq!(s, StatusCode::OK, "{job}");
        if job["status"] == "done" {
            return job;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("job {id} did not finish");
}

#[derive(sqlx::FromRow, Debug)]
struct Interaction {
    id: i64,
    module: String,
    attempt: i32,
    retry_of: Option<i64>,
    status: String,
    input_snapshot: Value,
    check_results: Option<Value>,
    prompt_key: Option<String>,
    scaffold_code: Option<String>,
}

async fn interactions(app: &common::TestApp, lesson: i64) -> Vec<Interaction> {
    sqlx::query_as(
        "SELECT i.id, i.module, i.attempt, i.retry_of, i.status, i.input_snapshot,
                i.check_results, p.prompt_key, s.scaffold_code
           FROM ai_interactions i
           LEFT JOIN prompt_versions p ON p.id = i.prompt_version_id
           LEFT JOIN scaffolds s ON s.id = i.scaffold_id
          WHERE i.lesson_id = $1 ORDER BY i.id",
    )
    .bind(lesson)
    .fetch_all(&app.db)
    .await
    .unwrap()
}

fn failed_checks(report: &Value) -> Vec<&str> {
    report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["passed"] == false)
        .map(|c| c["name"].as_str().unwrap())
        .collect()
}

fn paragraph_ids(state: &Value) -> Vec<&str> {
    state["material"]["paragraphs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap())
        .collect()
}

// ---------------------------------------------------------------- document, material, versions

#[tokio::test]
async fn legacy_lessons_read_as_blank_sop_and_start_with_or_without_one() {
    let app = test_app!();
    let t = app.teacher("T-SOP-01", "general").await;
    let id = lesson(&app, &t).await;
    // The plan written before the SOP existed: one objective and steps.
    let (s, v) = app
        .post(
            &format!("/api/lessons/{id}/versions"),
            Some(&t),
            json!({ "objective": "學生能說明倒裝的語意功能",
                    "steps": [{ "position": 1, "content": "暖身", "icap_level": "active" }] }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");

    let sop = get_sop(&app, &t, id).await;
    assert_eq!(sop["version_no"], Value::Null);
    assert_eq!(sop["versions_count"], 0);
    assert_eq!(sop["material"], Value::Null);
    assert_eq!(sop["content"]["objectives"], json!([]));
    assert_eq!(sop["content"]["exit_ticket"]["items"], json!([]));
    assert_eq!(sop["jobs"], json!({ "analysis": null, "objectives": null }));
    // The old single objective is handed to stage 3, not rewritten into the SOP.
    assert_eq!(sop["today_objective"], "學生能說明倒裝的語意功能");

    // The old plan is untouched and the lesson starts without any SOP.
    let (s, detail) = app.get(&format!("/api/lessons/{id}"), Some(&t)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(detail["latest_version"]["steps"][0]["content"], "暖身");
    let (s, run) = app
        .post(&format!("/api/lessons/{id}/runs"), Some(&t), json!({}))
        .await;
    assert_eq!(s, StatusCode::OK, "{run}");
    let run_id = run["id"].as_i64().unwrap();
    let traced: (Option<i64>, Option<i64>) = sqlx::query_as(
        "SELECT lesson_version_id, lesson_sop_version_id FROM lesson_runs WHERE id = $1",
    )
    .bind(run_id)
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert!(traced.0.is_some());
    assert_eq!(traced.1, None);
    let (s, _) = app
        .post(&format!("/api/runs/{run_id}/end"), Some(&t), json!({}))
        .await;
    assert_eq!(s, StatusCode::OK);

    // With an SOP the next run records which SOP version was in use.
    put_material(&app, &t, id, &passage("")).await;
    let (s, run) = app
        .post(&format!("/api/lessons/{id}/runs"), Some(&t), json!({}))
        .await;
    assert_eq!(s, StatusCode::OK, "{run}");
    let sop_version: Option<i32> = sqlx::query_scalar(
        "SELECT v.version_no FROM lesson_runs r
           JOIN lesson_sop_versions v ON v.id = r.lesson_sop_version_id WHERE r.id = $1",
    )
    .bind(run["id"].as_i64().unwrap())
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(sop_version, Some(1));

    // A lesson with an SOP and no plan at all starts too.
    let only_sop = app
        .create_lesson(&t, detail["unit_id"].as_i64().unwrap(), "只有 SOP")
        .await;
    let (s, v) = put_sop(
        &app,
        &t,
        only_sop,
        json!({ "objectives": [{ "id": "o1", "type": "language", "text": "學生能用 however" }] }),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, run) = app
        .post(
            &format!("/api/lessons/{only_sop}/runs"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{run}");
}

#[tokio::test]
async fn material_is_split_in_code_and_paragraph_ids_are_stable() {
    let app = test_app!();
    let t = app.teacher("T-SOP-02", "general").await;
    let id = lesson(&app, &t).await;

    // Nothing stored and nothing pasted: no version is made.
    let (s, v) = app
        .put(
            &format!("/api/lessons/{id}/sop/material"),
            Some(&t),
            json!({ "text": "  \n" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["version_no"], Value::Null);

    // Hard-wrapped lines inside a paragraph are joined; blank lines part paragraphs.
    let (head, tail) = P1.split_at(P1.find(" outside").unwrap());
    let wrapped = format!("{head}\n{}\n\n\n{P2}\n\n{P3}", tail.trim_start());
    let v1 = put_material(&app, &t, id, &wrapped).await;
    assert_eq!(v1["version_no"], 1);
    assert_eq!(paragraph_ids(&v1), ["p1", "p2", "p3"]);
    assert_eq!(v1["material"]["paragraphs"][0]["text"], P1);
    assert_eq!(v1["material"]["paragraphs"][2]["no"], 3);
    assert_eq!(v1["material"]["title"], "The Umbrella Stand");
    assert_eq!(v1["material"]["source_note"], "課本第三課");
    assert!(v1["material"]["word_count"].as_i64().unwrap() > 40);

    // Saving the same thing again adds nothing.
    let again = put_material(&app, &t, id, &wrapped).await;
    assert_eq!(again["version_no"], 1);
    assert_eq!(again["versions_count"], 1);

    // A paragraph analysis written against p2 survives edits around it.
    let (s, saved) = put_sop(
        &app,
        &t,
        id,
        json!({ "analysis": { "paragraphs": [
            { "paragraph_id": "p2", "function": "鄰居的反應", "summary": "", "features": [] }
        ] } }),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{saved}");
    assert_eq!(saved["version_no"], 2);

    // P1 rewritten, a paragraph added at the end: P2 and P3 keep their ids, ids are not reused.
    let edited = format!("A new opening line.\n\n{P2}\n\n{P3}\n\n{P4}");
    let v3 = put_material(&app, &t, id, &edited).await;
    assert_eq!(v3["version_no"], 3);
    assert_eq!(paragraph_ids(&v3), ["p4", "p2", "p3", "p5"]);
    assert_eq!(v3["material"]["version_no"], 2);
    assert_eq!(
        v3["content"]["analysis"]["paragraphs"][0]["paragraph_id"],
        "p2"
    );

    // Without blank lines every line is a paragraph.
    let lines = put_material(&app, &t, id, &format!("{P2}\n{P3}")).await;
    assert_eq!(paragraph_ids(&lines), ["p2", "p3"]);

    // Removing the material keeps the SOP content; pasting it back continues the ids.
    let (s, removed) = app
        .put(
            &format!("/api/lessons/{id}/sop/material"),
            Some(&t),
            json!({ "text": "" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{removed}");
    assert_eq!(removed["material"], Value::Null);
    assert_eq!(
        removed["content"]["analysis"]["paragraphs"][0]["function"],
        "鄰居的反應"
    );
    let back = put_material(&app, &t, id, &format!("{P3}\n\nBrand new.")).await;
    assert_eq!(paragraph_ids(&back), ["p3", "p6"]);

    // Append-only: every state is still there, newest first.
    let (s, list) = app
        .get(&format!("/api/lessons/{id}/sop/versions"), Some(&t))
        .await;
    assert_eq!(s, StatusCode::OK);
    let numbers: Vec<i64> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["version_no"].as_i64().unwrap())
        .collect();
    assert_eq!(numbers, [6, 5, 4, 3, 2, 1]);
    assert_eq!(list[1]["material_version_no"], Value::Null);
    let (s, first) = app
        .get(&format!("/api/lessons/{id}/sop/versions/1"), Some(&t))
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(paragraph_ids(&first), ["p1", "p2", "p3"]);
    assert_eq!(first["content"]["analysis"]["paragraphs"], json!([]));
    let (s, _) = app
        .get(&format!("/api/lessons/{id}/sop/versions/99"), Some(&t))
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    let too_long = "字".repeat(50_001);
    let (s, v) = app
        .put(
            &format!("/api/lessons/{id}/sop/material"),
            Some(&t),
            json!({ "text": too_long }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
}

#[tokio::test]
async fn sop_versions_are_append_only_and_nothing_is_required() {
    let app = test_app!();
    let t = app.teacher("T-SOP-03", "general").await;
    let id = lesson(&app, &t).await;

    // Every stage can be saved on its own, in any order, with fields left out.
    let (s, v1) = put_sop(
        &app,
        &t,
        id,
        json!({ "exit_ticket": { "items": [{ "id": "e1", "kind": "reflection", "text": "今天哪裡最難？" }] } }),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v1}");
    assert_eq!(v1["version_no"], 1);
    assert_eq!(
        v1["content"]["exit_ticket"]["items"][0]["origin"],
        "teacher"
    );
    assert_eq!(v1["content"]["objectives"], json!([]));

    let full = json!({
        "objectives": [
            { "id": "o1", "type": "content", "text": "學生能說出設立傘架的原因", "origin": "teacher" },
            { "id": "o2", "type": "language", "text": "", "origin": "teacher" }
        ],
        "paragraph_goals": [{ "paragraph_id": "p1", "objective_ids": ["o1"], "note": "" }],
        "task_blocks": [{
            "id": "b1", "paragraph_ids": ["p1", "p2"],
            "tasks": [{ "id": "t1", "function": "compare", "text": "Complete the table.",
                        "scaffolds": { "verbal": "Which word…?", "instructional": "", "procedural": "兩人一組" } },
                      { "id": "t2", "function": null, "text": "" }]
        }],
        "warmup": {
            "background": [{ "id": "w1", "text": "Have you ever borrowed an umbrella?" }],
            "vocabulary": [{ "id": "v1", "word": "trust", "note": "" }]
        },
        "exit_ticket": { "items": [{ "id": "e1", "kind": "reflection", "text": "今天哪裡最難？" }] }
    });
    let (s, v2) = put_sop(&app, &t, id, full.clone()).await;
    assert_eq!(s, StatusCode::OK, "{v2}");
    assert_eq!(v2["version_no"], 2);
    assert_eq!(
        v2["content"]["task_blocks"][0]["tasks"][0]["function"],
        "compare"
    );
    assert_eq!(
        v2["content"]["task_blocks"][0]["tasks"][1]["function"],
        Value::Null
    );
    assert_eq!(v2["content"]["objectives"][0]["link_status"], "unlinked");

    // An unchanged save (what auto-save sends when nothing moved) makes no version.
    let (s, same) = put_sop(&app, &t, id, v2["content"].clone()).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(same["version_no"], 2);
    assert_eq!(same["versions_count"], 2);

    // Version 1 is still exactly what it was.
    let (_, first) = app
        .get(&format!("/api/lessons/{id}/sop/versions/1"), Some(&t))
        .await;
    assert_eq!(first["content"]["objectives"], json!([]));
    assert_eq!(first["version_no"], 1);
    let rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM lesson_sop_versions WHERE lesson_id = $1")
            .bind(id)
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(rows, 2);

    // Shape errors are 400s and store nothing.
    for bad in [
        json!({ "objectives": [{ "id": "o1", "type": "content" }, { "id": "o1", "type": "language" }] }),
        json!({ "objectives": [{ "id": "", "type": "content" }] }),
        json!({ "objectives": [{ "id": "o1", "type": "unsorted" }] }),
        json!({ "task_blocks": [{ "id": "b1", "tasks": [{ "id": "t1", "function": "remember" }] }] }),
        json!({ "warmup": { "vocabulary": [{ "id": "v1", "word": "字".repeat(4001) }] } }),
    ] {
        let (s, v) = put_sop(&app, &t, id, bad.clone()).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{bad} -> {v}");
        assert_eq!(err_code(&v), "bad_request");
    }
    assert_eq!(get_sop(&app, &t, id).await["version_no"], 2);
}

#[tokio::test]
async fn curriculum_links_take_text_and_source_from_the_knowledge_base() {
    let app = test_app!();
    let t = app.teacher("T-SOP-04", "general").await;
    let id = lesson(&app, &t).await;
    import_curriculum(&app).await;

    // Whatever text a client sends for a link, the stored one is the database's.
    let (s, saved) = put_sop(
        &app,
        &t,
        id,
        json!({ "objectives": [{
            "id": "o1", "type": "content", "text": "學生能找出作者的論點", "link_status": "unlinked",
            "links": [{ "item_code": "T-G-3-V-10", "why": "找論點", "text": "我自己編的條文",
                        "official_code": "9-9", "source_doc": "假的來源", "source_version": "v9" }]
        }] }),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{saved}");
    let link = &saved["content"]["objectives"][0]["links"][0];
    assert_eq!(link["text"], "能分析測試用短文的結構。");
    assert_eq!(link["official_code"], "*3-Ⅴ-10");
    assert_eq!(link["title"], "*3-Ⅴ-10 學習表現｜測試閱讀");
    assert_eq!(link["source_doc"], "測試領綱.pdf");
    assert_eq!(link["source_version"], "測試版");
    assert_eq!(link["source_page"], "13");
    assert_eq!(link["why"], "找論點");
    assert_eq!(saved["content"]["objectives"][0]["link_status"], "linked");

    // A code that is not in the knowledge base cannot be linked; neither can a hand-written
    // note filed under curriculum (the seeded demo items say they are not curriculum text).
    for code in ["9-V-2", "DEMO-CUR-002"] {
        let (s, v) = put_sop(
            &app,
            &t,
            id,
            json!({ "objectives": [{ "id": "o1", "type": "content", "links": [{ "item_code": code }] }] }),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
        assert!(v["error"]["message"].as_str().unwrap().contains(code));
    }

    // A link saved earlier stays readable after the item is retired.
    sqlx::query("UPDATE knowledge_items SET retired_at = now() WHERE item_code = 'T-G-3-V-10'")
        .execute(&app.db)
        .await
        .unwrap();
    let mut content = saved["content"].clone();
    content["objectives"][0]["text"] = json!("改寫過的目標");
    let (s, v) = put_sop(&app, &t, id, content).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let kept = &v["content"]["objectives"][0]["links"][0];
    assert_eq!(kept["item_code"], "T-G-3-V-10");
    assert_eq!(kept["text"], "能分析測試用短文的結構。");
}

#[tokio::test]
async fn only_the_owner_reaches_a_lesson_sop() {
    let app = test_app!();
    let owner = app.teacher("T-SOP-05", "general").await;
    let other = app.teacher("T-SOP-06", "general").await;
    let id = lesson(&app, &owner).await;
    put_material(&app, &owner, id, &passage("")).await;
    let job = run_job(&app, &owner, id, "analysis").await;

    for (s, v) in [
        app.get(&format!("/api/lessons/{id}/sop"), Some(&other))
            .await,
        app.put(
            &format!("/api/lessons/{id}/sop"),
            Some(&other),
            json!({ "content": {} }),
        )
        .await,
        app.put(
            &format!("/api/lessons/{id}/sop/material"),
            Some(&other),
            json!({ "text": "x" }),
        )
        .await,
        app.get(&format!("/api/lessons/{id}/sop/versions"), Some(&other))
            .await,
        app.get(&format!("/api/lessons/{id}/sop/versions/1"), Some(&other))
            .await,
        app.post(
            &format!("/api/lessons/{id}/sop/analysis"),
            Some(&other),
            json!({}),
        )
        .await,
        app.post(
            &format!("/api/lessons/{id}/sop/objectives"),
            Some(&other),
            json!({}),
        )
        .await,
        app.get(&format!("/api/sop-jobs/{}", job["id"]), Some(&other))
            .await,
    ] {
        assert_eq!(s, StatusCode::NOT_FOUND, "{v}");
    }
    let (s, _) = app.get(&format!("/api/lessons/{id}/sop"), None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    // Nothing of the other teacher's attempts was stored.
    assert_eq!(get_sop(&app, &owner, id).await["version_no"], 1);
}

// ---------------------------------------------------------------- (a) passage analysis

#[tokio::test]
async fn analysis_happy_path_is_logged_and_never_written_into_the_sop() {
    let app = test_app!();
    let t = app.teacher("T-SOP-10", "general").await;
    let id = lesson(&app, &t).await;
    let state = put_material(&app, &t, id, &passage("")).await;
    let ids = paragraph_ids(&state)
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();

    let job = run_job(&app, &t, id, "analysis").await;
    assert_eq!(job["kind"], "analysis");
    assert_eq!(job["sop_version_no"], 1);
    assert_eq!(job["decided"], json!([]));
    let result = &job["result"];
    assert_eq!(result["status"], "shown", "{result}");
    let out = &result["output"];
    assert_eq!(out["missing_paragraph_ids"], json!([]));
    assert_eq!(out["dropped_evidence"], 0);
    assert!(out["main_idea"].as_str().unwrap().contains("主旨"));
    let paragraphs = out["paragraphs"].as_array().unwrap();
    assert_eq!(paragraphs.len(), 4);
    for (i, p) in paragraphs.iter().enumerate() {
        assert_eq!(p["paragraph_id"], ids[i].as_str());
        assert_eq!(p["paragraph_no"], i as i64 + 1);
        let source = state["material"]["paragraphs"][i]["text"].as_str().unwrap();
        for f in p["features"].as_array().unwrap() {
            assert!(source.contains(f["evidence"].as_str().unwrap()), "{f}");
        }
    }
    assert_eq!(out["language_focus"][0]["paragraph_id"], "p1");
    let labels: Vec<&str> = result["context"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["label"].as_str().unwrap())
        .collect();
    assert_eq!(labels, ["教材"]);

    // One model call, logged through the usual pipeline with the new prompt key and scaffold.
    let rows = interactions(&app, id).await;
    assert_eq!(rows.len(), 1, "{rows:?}");
    let row = &rows[0];
    assert_eq!(Some(row.id), result["interaction_id"].as_i64());
    assert_eq!(
        (row.module.as_str(), row.attempt, row.status.as_str()),
        ("sop_analysis", 1, "shown")
    );
    assert_eq!(row.prompt_key.as_deref(), Some("sop_passage_analysis"));
    assert_eq!(row.scaffold_code.as_deref(), Some("SC-SOP-ANALYSIS"));
    assert_eq!(row.check_results.as_ref().unwrap()["passed"], true);
    assert_eq!(row.input_snapshot["request"]["max_tokens"], 6000);
    // The material itself is not copied into the log.
    let logged = row.input_snapshot.to_string();
    assert!(!logged.contains("umbrella stand outside"), "{logged}");
    assert!(logged.contains("sha256"));

    // The AI wrote nothing into the SOP: adopting is the teacher's save.
    let sop = get_sop(&app, &t, id).await;
    assert_eq!(sop["version_no"], 1);
    assert_eq!(sop["content"]["analysis"]["paragraphs"], json!([]));
    assert_eq!(sop["jobs"]["analysis"]["id"], job["id"]);
    assert_eq!(sop["jobs"]["objectives"], Value::Null);

    // Decisions go through the existing endpoint and come back as `decided`.
    let interaction = result["interaction_id"].as_i64().unwrap();
    for body in [
        json!({ "item_index": 1, "decision": "adopt" }),
        json!({ "item_index": 2, "decision": "revise", "revised_content": "改寫後的段落功能" }),
        json!({ "item_index": 3, "decision": "reject", "reason": "better_idea" }),
    ] {
        let (s, v) = app
            .post(&format!("/api/ai/{interaction}/decision"), Some(&t), body)
            .await;
        assert_eq!(s, StatusCode::OK, "{v}");
    }
    let decisions: Vec<(i32, String, Option<String>)> = sqlx::query_as(
        "SELECT item_index, decision, revised_content FROM teacher_decisions
          WHERE ai_interaction_id = $1 ORDER BY item_index",
    )
    .bind(interaction)
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert_eq!(
        decisions,
        vec![
            (1, "adopt".to_string(), None),
            (
                2,
                "revise".to_string(),
                Some("改寫後的段落功能".to_string())
            ),
            (3, "reject".to_string(), None),
        ]
    );
    let sop = get_sop(&app, &t, id).await;
    assert_eq!(sop["jobs"]["analysis"]["decided"], json!([1, 2, 3]));
}

#[tokio::test]
async fn analysis_asks_again_for_paragraphs_the_model_left_out() {
    let app = test_app!();
    let t = app.teacher("T-SOP-11", "general").await;

    // The model stops after two paragraphs; the repair call delivers the other two.
    let id = lesson(&app, &t).await;
    put_material(&app, &t, id, &passage("FAKE_SOP_STOP_EARLY")).await;
    let job = run_job(&app, &t, id, "analysis").await;
    let out = &job["result"]["output"];
    assert_eq!(job["result"]["status"], "shown", "{job}");
    assert_eq!(out["paragraphs"].as_array().unwrap().len(), 4);
    assert_eq!(out["missing_paragraph_ids"], json!([]));
    let rows = interactions(&app, id).await;
    assert_eq!(rows.len(), 2);
    assert_eq!((rows[0].attempt, rows[0].status.as_str()), (1, "blocked"));
    assert_eq!(
        failed_checks(rows[0].check_results.as_ref().unwrap()),
        ["paragraphs_one_to_one"]
    );
    assert_eq!((rows[1].attempt, rows[1].status.as_str()), (2, "shown"));
    assert_eq!(rows[1].retry_of, Some(rows[0].id));
    assert_eq!(Some(rows[1].id), job["result"]["interaction_id"].as_i64());
    let repairs = &rows[1].check_results.as_ref().unwrap()["repairs"];
    assert_eq!(repairs["repaired_paragraphs"], json!([3, 4]));
    // The repair is a different request: the gateway caches identical ones.
    let t1 = rows[0].input_snapshot["request"]["temperature"]
        .as_f64()
        .unwrap();
    let t2 = rows[1].input_snapshot["request"]["temperature"]
        .as_f64()
        .unwrap();
    assert!(t2 > t1, "{t1} {t2}");
    assert!(rows[1].input_snapshot["repair"]["note"]
        .as_str()
        .unwrap()
        .contains("[P3]、[P4]"));

    // The repair call does not deliver them either: the teacher still gets the first two.
    let id = lesson(&app, &t).await;
    put_material(&app, &t, id, &passage("FAKE_SOP_STOP_ALWAYS")).await;
    let job = run_job(&app, &t, id, "analysis").await;
    let out = &job["result"]["output"];
    assert_eq!(job["result"]["status"], "shown", "{job}");
    assert_eq!(out["paragraphs"].as_array().unwrap().len(), 2);
    assert_eq!(out["missing_paragraph_ids"], json!(["p3", "p4"]));
    let rows = interactions(&app, id).await;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].status, "shown");
    assert_eq!(
        rows[1].check_results.as_ref().unwrap()["repairs"]["missing_paragraph_ids"],
        json!(["p3", "p4"])
    );

    // A teacher's second try is not the same request as the first either.
    let again = run_job(&app, &t, id, "analysis").await;
    assert_ne!(again["id"], job["id"]);
    let rows = interactions(&app, id).await;
    let first_try = rows[0].input_snapshot["request"]["temperature"]
        .as_f64()
        .unwrap();
    let second_try = rows[2].input_snapshot["request"]["temperature"]
        .as_f64()
        .unwrap();
    assert!(second_try > first_try);
}

#[tokio::test]
async fn spliced_evidence_is_repaired_once_and_otherwise_dropped() {
    let app = test_app!();
    let t = app.teacher("T-SOP-12", "general").await;

    let id = lesson(&app, &t).await;
    put_material(&app, &t, id, &passage("FAKE_SOP_SPLICE")).await;
    let job = run_job(&app, &t, id, "analysis").await;
    let out = &job["result"]["output"];
    assert_eq!(job["result"]["status"], "shown", "{job}");
    assert_eq!(out["dropped_evidence"], 0);
    let evidence = out["paragraphs"][0]["features"][0]["evidence"]
        .as_str()
        .unwrap();
    assert!(P1.contains(evidence), "{evidence}");
    assert!(!evidence.contains("..."));
    let rows = interactions(&app, id).await;
    assert_eq!(rows.len(), 2);
    assert_eq!(
        failed_checks(rows[0].check_results.as_ref().unwrap()),
        ["evidence_verbatim"]
    );
    assert_eq!(
        rows[1].check_results.as_ref().unwrap()["repairs"]["repaired_paragraphs"],
        json!([1])
    );
    assert!(rows[1].input_snapshot["repair"]["note"]
        .as_str()
        .unwrap()
        .contains("不是該段原文的連續片段"));

    // Still spliced after the repair: that evidence item is dropped, the rest is shown.
    let id = lesson(&app, &t).await;
    put_material(&app, &t, id, &passage("FAKE_SOP_SPLICE_ALWAYS")).await;
    let job = run_job(&app, &t, id, "analysis").await;
    let out = &job["result"]["output"];
    assert_eq!(job["result"]["status"], "shown", "{job}");
    assert_eq!(out["dropped_evidence"], 1);
    assert_eq!(out["paragraphs"].as_array().unwrap().len(), 4);
    assert_eq!(out["paragraphs"][0]["features"], json!([]));
    assert!(!out.to_string().contains("..."), "{out}");
    assert_eq!(
        out["paragraphs"][1]["features"].as_array().unwrap().len(),
        1
    );
}

#[tokio::test]
async fn analysis_failures_end_the_job_without_blocking_the_teacher() {
    let app = test_app!();
    let t = app.teacher("T-SOP-13", "general").await;
    let id = lesson(&app, &t).await;

    // No material yet: nothing to read, said plainly.
    for kind in ["analysis", "objectives"] {
        let (s, v) = app
            .post(
                &format!("/api/lessons/{id}/sop/{kind}"),
                Some(&t),
                json!({}),
            )
            .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
        assert!(v["error"]["message"].as_str().unwrap().contains("貼上教材"));
    }

    put_material(&app, &t, id, &passage("FAKE_TIMEOUT")).await;
    let job = run_job(&app, &t, id, "analysis").await;
    assert_eq!(job["result"]["status"], "timeout");
    assert_eq!(job["result"]["output"], Value::Null);
    assert!(job["result"]["message"].as_str().unwrap().contains("手動"));
    assert_eq!(interactions(&app, id).await[0].status, "timeout");

    put_material(&app, &t, id, &passage("FAKE_BLOCK")).await;
    let job = run_job(&app, &t, id, "analysis").await;
    assert_eq!(job["result"]["status"], "blocked", "{job}");
    let rows = interactions(&app, id).await;
    assert_eq!(rows.len(), 3);
    assert_eq!((rows[2].attempt, rows[2].status.as_str()), (2, "blocked"));

    put_material(&app, &t, id, &passage("FAKE_ERROR")).await;
    let job = run_job(&app, &t, id, "analysis").await;
    assert_eq!(job["result"]["status"], "error");

    // More paragraphs than one call can be trusted with.
    let long = (0..41)
        .map(|i| format!("Paragraph number {i} is here."))
        .collect::<Vec<_>>()
        .join("\n\n");
    put_material(&app, &t, id, &long).await;
    let (s, v) = app
        .post(
            &format!("/api/lessons/{id}/sop/analysis"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");

    // The SOP itself still saves after all of that.
    let (s, _) = put_sop(
        &app,
        &t,
        id,
        json!({ "analysis": { "main_idea": "自己寫的主旨" } }),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn a_second_request_joins_the_running_job() {
    let app = test_app!();
    let t = app.teacher("T-SOP-14", "general").await;
    let id = lesson(&app, &t).await;
    put_material(&app, &t, id, &passage("FAKE_SLOW")).await;
    let (s, first) = app
        .post(
            &format!("/api/lessons/{id}/sop/analysis"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{first}");
    assert_eq!(first["status"], "running");
    assert_eq!(first["result"], Value::Null);
    assert!(!first["phase"].as_str().unwrap().is_empty());
    let (s, second) = app
        .post(
            &format!("/api/lessons/{id}/sop/analysis"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED);
    assert_eq!(second["id"], first["id"]);
    // A job of the other kind runs beside it.
    let (s, other) = app
        .post(
            &format!("/api/lessons/{id}/sop/objectives"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED);
    assert_ne!(other["id"], first["id"]);
    // The page shows the running job after a reload.
    assert_eq!(
        get_sop(&app, &t, id).await["jobs"]["analysis"]["status"],
        "running"
    );

    let done = wait_job(&app, &t, first["id"].as_i64().unwrap()).await;
    assert_eq!(done["result"]["status"], "shown");
    assert!(done["finished_at"].is_string());
    wait_job(&app, &t, other["id"].as_i64().unwrap()).await;
    assert_eq!(
        interactions(&app, id)
            .await
            .iter()
            .filter(|r| r.module == "sop_analysis")
            .count(),
        1
    );

    // A job left running by a server that went away is closed the next time it is read.
    sqlx::query(
        "INSERT INTO lesson_sop_jobs (lesson_id, teacher_id, kind, status, created_at)
         SELECT $1, teacher_id, 'analysis', 'running', now() - interval '1 hour'
           FROM lesson_sop_jobs WHERE lesson_id = $1 LIMIT 1",
    )
    .bind(id)
    .execute(&app.db)
    .await
    .unwrap();
    let stale = get_sop(&app, &t, id).await;
    assert_eq!(stale["jobs"]["analysis"]["status"], "done");
    assert_eq!(stale["jobs"]["analysis"]["result"]["status"], "error");
}

#[tokio::test]
async fn ai_off_leaves_every_stage_usable_by_hand() {
    let app = test_app!(|c: &mut manna_api::config::Config| c.ai_provider = AiProviderKind::None);
    let t = app.teacher("T-SOP-15", "general").await;
    let id = lesson(&app, &t).await;
    put_material(&app, &t, id, &passage("")).await;
    let (s, job) = app
        .post(
            &format!("/api/lessons/{id}/sop/objectives"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{job}");
    assert_eq!(job["status"], "done");
    assert_eq!(job["result"]["status"], "disabled");
    assert!(interactions(&app, id).await.is_empty());
    let (s, _) = put_sop(
        &app,
        &t,
        id,
        json!({ "objectives": [{ "id": "o1", "type": "content", "text": "自己寫的目標" }] }),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
}

// ---------------------------------------------------------------- (b) objectives

#[tokio::test]
async fn objectives_are_linked_from_the_database_and_never_overwrite_the_teacher() {
    let app = test_app!();
    let t = app.teacher("T-SOP-20", "general").await;
    let id = lesson(&app, &t).await;
    import_curriculum(&app).await;
    put_material(&app, &t, id, &passage("")).await;
    let own = json!({ "objectives": [
        { "id": "mine", "type": "language", "text": "學生能用 Not only … but also 倒裝句", "origin": "teacher" }
    ] });
    let (s, saved) = put_sop(&app, &t, id, own).await;
    assert_eq!(s, StatusCode::OK, "{saved}");

    let job = run_job(&app, &t, id, "objectives").await;
    let result = &job["result"];
    assert_eq!(result["status"], "shown", "{result}");
    let out = &result["output"];
    assert_eq!(out["curriculum_available"], 2);
    let objectives = out["objectives"].as_array().unwrap();
    assert_eq!(objectives.len(), 3);
    assert_eq!(objectives[0]["type"], "content");
    assert_eq!(objectives[1]["type"], "language");
    // Type, reason, linked item code, official text and source: all there, text from the DB.
    let link = &objectives[0]["links"][0];
    assert_eq!(objectives[0]["link_status"], "linked");
    assert_eq!(link["item_code"], "T-G-3-V-2");
    assert_eq!(link["official_code"], "3-Ⅴ-2");
    assert_eq!(link["title"], "3-Ⅴ-2 學習表現｜測試閱讀");
    assert_eq!(link["text"], "能讀懂測試用的短文並找出主旨。");
    assert_eq!(link["source_doc"], "測試領綱.pdf");
    assert_eq!(link["source_version"], "測試版");
    assert_eq!(link["source_page"], "12");
    assert!(!link["why"].as_str().unwrap().is_empty());
    assert!(!objectives[0]["reason"].as_str().unwrap().is_empty());
    assert!(objectives[0]["student_facing_en"]
        .as_str()
        .unwrap()
        .starts_with("I can"));
    assert_eq!(objectives[0]["paragraph_ids"], json!(["p1"]));
    // Nothing fits the third one: it is kept and marked, not given an invented link.
    assert_eq!(objectives[2]["link_status"], "unlinked");
    assert_eq!(objectives[2]["links"], json!([]));
    assert_eq!(result["sources"].as_array().unwrap().len(), 2);
    let labels: Vec<&str> = result["context"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["label"].as_str().unwrap())
        .collect();
    assert_eq!(labels, ["教材", "你已經寫的目標", "可連結的課綱條目"]);

    let rows = interactions(&app, id).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].module.as_str(), rows[0].status.as_str()),
        ("sop_objectives", "shown")
    );
    assert_eq!(rows[0].prompt_key.as_deref(), Some("sop_objectives"));
    assert_eq!(rows[0].scaffold_code.as_deref(), Some("SC-SOP-OBJECTIVES"));
    assert_eq!(rows[0].input_snapshot["request"]["max_tokens"], 4000);
    // The teacher's own objective went along as context, and the offered codes were logged.
    assert!(rows[0].input_snapshot["inputs"]["teacher_objectives"]
        .as_str()
        .unwrap()
        .contains("Not only"));
    let offered: Vec<String> = sqlx::query_scalar(
        "SELECT unnest(item_codes) FROM retrieval_logs WHERE ai_interaction_id = $1",
    )
    .bind(rows[0].id)
    .fetch_all(&app.db)
    .await
    .unwrap();
    // Senior-high items of the teacher's track only, in reading order (2 before 10).
    assert_eq!(offered, ["T-G-3-V-2", "T-G-3-V-10"]);

    // The recommendation changed nothing in the SOP.
    let sop = get_sop(&app, &t, id).await;
    assert_eq!(sop["version_no"], 2);
    assert_eq!(sop["content"]["objectives"].as_array().unwrap().len(), 1);
    assert_eq!(sop["content"]["objectives"][0]["id"], "mine");

    // Adopting one (edited) is the teacher saving it, with its origin; the decision is logged.
    let mut content = sop["content"].clone();
    content["objectives"].as_array_mut().unwrap().push(json!({
        "id": "o-ai-1", "type": "content", "text": "學生能說出傘為什麼回來（我改過）",
        "reason": objectives[0]["reason"], "student_facing_en": objectives[0]["student_facing_en"],
        "paragraph_ids": ["p1"], "origin": "ai_edited",
        "links": [{ "item_code": "T-G-3-V-2", "why": link["why"] }]
    }));
    let (s, saved) = put_sop(&app, &t, id, content).await;
    assert_eq!(s, StatusCode::OK, "{saved}");
    let adopted = &saved["content"]["objectives"][1];
    assert_eq!(adopted["origin"], "ai_edited");
    assert_eq!(adopted["link_status"], "linked");
    assert_eq!(adopted["links"][0]["text"], link["text"]);
    assert_eq!(
        saved["content"]["objectives"][0]["text"],
        "學生能用 Not only … but also 倒裝句"
    );
    let interaction = result["interaction_id"].as_i64().unwrap();
    let (s, _) = app
        .post(
            &format!("/api/ai/{interaction}/decision"),
            Some(&t),
            json!({ "item_index": 0, "decision": "revise", "revised_content": "學生能說出傘為什麼回來（我改過）" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = app
        .post(
            &format!("/api/ai/{interaction}/decision"),
            Some(&t),
            json!({ "item_index": 2, "decision": "reject" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        get_sop(&app, &t, id).await["jobs"]["objectives"]["decided"],
        json!([0, 2])
    );
}

#[tokio::test]
async fn invented_curriculum_codes_never_reach_the_teacher() {
    let app = test_app!();
    let t = app.teacher("T-SOP-21", "general").await;
    import_curriculum(&app).await;

    // Call 1 cites a code that was not offered; the repair call is clean.
    let id = lesson(&app, &t).await;
    put_material(&app, &t, id, &passage("FAKE_SOP_INVENT")).await;
    let job = run_job(&app, &t, id, "objectives").await;
    assert_eq!(job["result"]["status"], "shown", "{job}");
    assert!(!job["result"].to_string().contains("9-V-99"));
    let rows = interactions(&app, id).await;
    assert_eq!(rows.len(), 2);
    assert_eq!((rows[0].attempt, rows[0].status.as_str()), (1, "blocked"));
    assert_eq!(
        failed_checks(rows[0].check_results.as_ref().unwrap()),
        ["cited_codes_exist"]
    );
    assert_eq!((rows[1].attempt, rows[1].status.as_str()), (2, "shown"));
    assert_eq!(rows[1].retry_of, Some(rows[0].id));
    assert!(rows[1].input_snapshot["repair"]["note"]
        .as_str()
        .unwrap()
        .contains("9-V-99"));
    let t1 = rows[0].input_snapshot["request"]["temperature"]
        .as_f64()
        .unwrap();
    let t2 = rows[1].input_snapshot["request"]["temperature"]
        .as_f64()
        .unwrap();
    assert!(t2 > t1);

    // It keeps citing it: the invented link is dropped, the real one stays.
    let id = lesson(&app, &t).await;
    put_material(&app, &t, id, &passage("FAKE_SOP_INVENT_ALWAYS")).await;
    let job = run_job(&app, &t, id, "objectives").await;
    let out = &job["result"]["output"];
    assert_eq!(job["result"]["status"], "shown", "{job}");
    assert!(!job["result"].to_string().contains("9-V-99"));
    assert_eq!(out["objectives"][0]["links"].as_array().unwrap().len(), 1);
    assert_eq!(out["objectives"][0]["links"][0]["item_code"], "T-G-3-V-2");
    let rows = interactions(&app, id).await;
    assert_eq!(
        rows[1].check_results.as_ref().unwrap()["repairs"]["dropped_codes"],
        json!(["9-V-99"])
    );

    // No curriculum items at all for this track: invented links leave the objective unlinked.
    sqlx::query("UPDATE knowledge_items SET retired_at = now() WHERE category = 'curriculum'")
        .execute(&app.db)
        .await
        .unwrap();
    let id = lesson(&app, &t).await;
    put_material(&app, &t, id, &passage("FAKE_SOP_INVENT_ALWAYS")).await;
    let job = run_job(&app, &t, id, "objectives").await;
    let out = &job["result"]["output"];
    assert_eq!(job["result"]["status"], "shown", "{job}");
    assert_eq!(out["curriculum_available"], 0);
    for o in out["objectives"].as_array().unwrap() {
        assert_eq!(o["link_status"], "unlinked");
        assert_eq!(o["links"], json!([]));
    }
    assert_eq!(job["result"]["sources"], json!([]));
}

#[tokio::test]
async fn objectives_unlinked_and_single_type_outputs() {
    let app = test_app!();
    let t = app.teacher("T-SOP-22", "vocational").await;
    import_curriculum(&app).await;

    let id = lesson(&app, &t).await;
    put_material(&app, &t, id, &passage("FAKE_SOP_UNLINKED")).await;
    let job = run_job(&app, &t, id, "objectives").await;
    let out = &job["result"]["output"];
    assert_eq!(job["result"]["status"], "shown", "{job}");
    // Items were on offer (the vocational ones), the model linked none: all are marked.
    assert_eq!(out["curriculum_available"], 2);
    assert!(out["objectives"]
        .as_array()
        .unwrap()
        .iter()
        .all(|o| o["link_status"] == "unlinked"));
    assert_eq!(interactions(&app, id).await.len(), 1);

    // Content objectives only, twice: SIOP needs both kinds, so nothing is shown.
    let id = lesson(&app, &t).await;
    put_material(&app, &t, id, &passage("FAKE_SOP_ONE_TYPE")).await;
    let job = run_job(&app, &t, id, "objectives").await;
    assert_eq!(job["result"]["status"], "blocked", "{job}");
    assert_eq!(job["result"]["output"], Value::Null);
    let rows = interactions(&app, id).await;
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|r| r.status == "blocked"));
    assert_eq!(
        failed_checks(rows[1].check_results.as_ref().unwrap()),
        ["both_siop_types_present"]
    );

    // The class the teacher works in supplies the class profile, as for other AI calls.
    let (s, class) = app
        .post(
            "/api/classes",
            Some(&t),
            json!({ "name": "高一 3 班", "class_profile": "約三分之一讀長句會卡住" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{class}");
    let id = lesson(&app, &t).await;
    put_material(&app, &t, id, &passage("")).await;
    let (s, job) = app
        .post(
            &format!("/api/lessons/{id}/sop/objectives"),
            Some(&t),
            json!({ "class_id": class["id"] }),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{job}");
    let job = wait_job(&app, &t, job["id"].as_i64().unwrap()).await;
    let context = job["result"]["context"].as_array().unwrap();
    assert!(
        context
            .iter()
            .any(|c| c["label"] == "班級特性（高一 3 班）"
                && c["preview"] == "約三分之一讀長句會卡住")
    );
    let rows = interactions(&app, id).await;
    assert_eq!(
        rows[0].input_snapshot["inputs"]["class_profile"],
        "約三分之一讀長句會卡住"
    );
}
