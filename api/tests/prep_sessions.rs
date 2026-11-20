//! Phase 2.6 (docs/API.md「備課對話」): sessions and their turns with the fake provider: each
//! kind polled to `done`, the needs-rules, the 409s, owner isolation, the bottom bar's revise
//! (one version, block history, undo), a revision that fails its checks
//! (`FAKE_PREP_BAD_REVISE`), PATCH, restart recovery and the hard delete that keeps worksheets.

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

const CURRICULUM_CSV: &str = "item_code,category,school_track,title,content,source_doc,\
source_version,source_page,official_code,item_kind,strand,strand_name,stage,subject,advanced,\
recurring,source_sha256
T-G-3-V-2,curriculum,general,3-Ⅴ-2 學習表現｜測試閱讀,能讀懂 umbrella 測試用的短文並找出主旨。,測試領綱.pdf,測試版,12,3-Ⅴ-2,learning_performance,3,測試閱讀,V,英語文,False,False,aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
T-G-3-V-10,curriculum,general,*3-Ⅴ-10 學習表現｜測試閱讀,能分析測試用短文的結構。,測試領綱.pdf,測試版,13,*3-Ⅴ-10,learning_performance,3,測試閱讀,V,英語文,True,False,aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
";

const MSG_NO_EXTERNAL: &str = "外部資源（影片、網站）搜尋還沒有開放。";

fn passage(marker: &str) -> String {
    format!("{}\n\n{}\n\n{}\n\n{} {marker}", P[0], P[1], P[2], P[3])
}

async fn import_curriculum(app: &common::TestApp) {
    let pi = app.pi_token().await;
    let (s, v) = app
        .post_csv("/api/admin/knowledge/import", Some(&pi), CURRICULUM_CSV)
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
}

async fn patch(app: &common::TestApp, path: &str, t: &str, body: Value) -> (StatusCode, Value) {
    app.send(
        Method::PATCH,
        path,
        Some(t),
        Some("application/json"),
        body.to_string().into_bytes(),
    )
    .await
}

async fn create(app: &common::TestApp, t: &str, body: Value) -> Value {
    let (s, v) = app.post("/api/prep-sessions", Some(t), body).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    v
}

/// A session from pasted material, first turn `kind`.
async fn pasted(app: &common::TestApp, t: &str, kind: &str, marker: &str) -> Value {
    create(
        app,
        t,
        json!({ "kind": kind, "material": { "title": "The Umbrella Stand", "text": passage(marker) } }),
    )
    .await
}

async fn wait(app: &common::TestApp, t: &str, id: i64) -> Value {
    for _ in 0..800 {
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

fn last(session: &Value) -> Value {
    session["turns"].as_array().unwrap().last().unwrap().clone()
}

fn sid(session: &Value) -> i64 {
    session["id"].as_i64().unwrap()
}

async fn interactions(app: &common::TestApp, module: &str) -> Vec<(i32, String, Value)> {
    sqlx::query_as(
        "SELECT attempt, status, input_snapshot FROM ai_interactions WHERE module = $1 ORDER BY id",
    )
    .bind(module)
    .fetch_all(&app.db)
    .await
    .unwrap()
}

async fn usage(app: &common::TestApp, event: &str) -> Vec<Value> {
    sqlx::query_scalar("SELECT payload FROM usage_logs WHERE event = $1 ORDER BY id")
        .bind(event)
        .fetch_all(&app.db)
        .await
        .unwrap()
}

// ---------------------------------------------------------------- worksheet / assessment

#[tokio::test]
async fn a_worksheet_turn_makes_an_ordinary_worksheet_and_waits_for_it() {
    let app = test_app!();
    let t = app.teacher("T-PREP-1", "general").await;
    let started = pasted(&app, &t, "worksheet", "").await;
    let id = sid(&started);
    assert_eq!(started["title"], "The Umbrella Stand");
    assert_eq!(started["topic"], Value::Null);
    let material_id = started["material"]["id"].as_i64().unwrap();
    assert_eq!(started["material"]["title"], "The Umbrella Stand");
    assert_eq!(started["turn_count"], 1);
    assert_eq!(started["last_kind"], "worksheet");
    let first = last(&started);
    assert_eq!(first["seq"], 1);
    assert_eq!(first["kind"], "worksheet");
    assert_eq!(first["request"], "做學習單");
    // The worksheet exists at once.
    let ws_id = first["worksheet"]["id"]
        .as_i64()
        .expect("worksheet set at once");

    let done = wait(&app, &t, id).await;
    let turn = last(&done);
    assert_eq!(turn["status"], "done", "{turn}");
    assert_eq!(turn["error"], Value::Null);
    assert!(turn["finished_at"].is_string());
    assert_eq!(turn["worksheet"]["running"], false);
    assert_eq!(turn["worksheet"]["title"], "The Umbrella Stand: Worksheet");
    assert!(turn["worksheet"]["version_no"].as_i64().unwrap() >= 2);
    assert_eq!(
        done["next"],
        json!(["comprehension", "tiered", "activity", "kit"])
    );

    // An ordinary worksheet: listed, generated from the session's material, linked back.
    let (s, list) = app.get("/api/worksheets", Some(&t)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(list[0]["id"], ws_id);
    assert_eq!(list[0]["prep_session_id"], id);
    assert_eq!(list[0]["generated"], true);
    let (_, ws) = app.get(&format!("/api/worksheets/{ws_id}"), Some(&t)).await;
    assert_eq!(ws["prep_session_id"], id);
    assert_eq!(ws["material"]["source"]["material_id"], material_id);
    assert!(!ws["content"]["blocks"].as_array().unwrap().is_empty());

    let created = usage(&app, "prep_session_created").await;
    assert_eq!(created.len(), 1);
    assert_eq!(created[0]["session_id"], id);
    assert_eq!(created[0]["kind"], "worksheet");
    let turns = usage(&app, "prep_turn_created").await;
    assert_eq!(
        turns[0],
        json!({ "session_id": id, "turn_id": turn["id"], "kind": "worksheet" })
    );
}

#[tokio::test]
async fn the_usage_log_records_which_page_started_a_session() {
    let app = test_app!();
    let t = app.teacher("T-PREP-ENTRY", "general").await;
    let material = json!({ "title": "The Umbrella Stand", "text": passage("") });
    // An unknown entry is ignored, never an error.
    for entry in [json!("home"), json!("create"), json!("elsewhere"), json!(7)] {
        create(
            &app,
            &t,
            json!({ "kind": "comprehension", "material": material, "entry": entry }),
        )
        .await;
    }
    // An old client sends no entry at all.
    create(
        &app,
        &t,
        json!({ "kind": "comprehension", "material": material }),
    )
    .await;
    let logged: Vec<(Value, Value)> = usage(&app, "prep_session_created")
        .await
        .iter()
        .map(|p| (p["entry"].clone(), p["ui_version"].clone()))
        .collect();
    assert_eq!(
        logged,
        vec![
            (json!("home"), json!(4)),
            (json!("create"), json!(4)),
            (Value::Null, json!(4)),
            (Value::Null, json!(4)),
            (Value::Null, json!(4)),
        ]
    );
}

#[tokio::test]
async fn an_assessment_has_its_own_title_and_settings() {
    let app = test_app!();
    let t = app.teacher("T-PREP-2", "general").await;
    let session = pasted(&app, &t, "assessment", "").await;
    let ws_id = last(&session)["worksheet"]["id"].as_i64().unwrap();
    let done = wait(&app, &t, sid(&session)).await;
    assert_eq!(last(&done)["status"], "done");
    assert_eq!(
        last(&done)["worksheet"]["title"],
        "評量：The Umbrella Stand"
    );
    let (_, ws) = app.get(&format!("/api/worksheets/{ws_id}"), Some(&t)).await;
    let settings = &ws["settings"];
    assert_eq!(
        settings["student_tasks"],
        json!(["理解內容", "字彙練習", "文法／句型練習"])
    );
    assert_eq!(
        settings["answer_formats"],
        json!(["填空題", "選擇題", "是非題", "配對題", "簡答題"])
    );
    assert_eq!(settings["generation"], json!(["顯示學習目標"]));
    assert_eq!(settings["levels"], json!(["standard"]));
    assert_eq!(ws["template"], Value::Null);
    let kinds: Vec<&str> = ws["content"]["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["kind"].as_str().unwrap())
        .collect();
    assert!(!kinds.contains(&"warmup"), "no warm-up: {kinds:?}");
    assert_eq!(
        done["next"],
        json!(["comprehension", "tiered", "activity", "kit", "worksheet"]),
        "assessment is no longer suggested"
    );
}

#[tokio::test]
async fn a_worksheet_that_cannot_start_fails_the_turn() {
    let app = test_app!();
    let t = app.teacher("T-PREP-3", "general").await;
    // More paragraphs than one generation takes: the worksheet is made, the start fails.
    let long: Vec<String> = (1..=41)
        .map(|i| format!("Paragraph number {i} says something about umbrellas."))
        .collect();
    let session = create(
        &app,
        &t,
        json!({ "kind": "worksheet", "material": { "title": "Long", "text": long.join("\n\n") } }),
    )
    .await;
    let turn = last(&session);
    assert_eq!(turn["status"], "failed", "{turn}");
    assert!(turn["error"].as_str().unwrap().contains("41 段"), "{turn}");
    assert!(turn["worksheet"]["id"].is_i64());
    assert_eq!(session["running"], false);
}

// ---------------------------------------------------------------- activity

#[tokio::test]
async fn an_activity_plan_is_checked_and_feeds_the_warm_up() {
    let app = test_app!();
    import_curriculum(&app).await;
    let t = app.teacher("T-PREP-4", "general").await;
    // FAKE_SLOW: the fake call waits 3 s, so the 201 reliably shows the turn still running
    // (without it the fake run could finish before the answer was read; main CI failed on it).
    let session = pasted(&app, &t, "activity", "FAKE_SLOW").await;
    let id = sid(&session);
    assert_eq!(last(&session)["status"], "running");
    assert_eq!(last(&session)["request"], "規劃教學活動");
    let done = wait(&app, &t, id).await;
    let turn = last(&done);
    assert_eq!(turn["status"], "done", "{turn}");
    assert!(turn["interaction_id"].is_i64());
    let plan = &turn["result"];
    assert_eq!(plan["title"], "The Umbrella Stand 教學活動規劃（測試）");
    // 內容目標 then 語言目標, at most two each, each goal in Chinese and English. Dropped: the
    // 內容目標 with a vague verb (了解) and the 語言目標 that serves it, the 語言目標 whose English
    // is Chinese, the one whose relation is not 「學生要用……，才能……」. The 學習策略 goal comes
    // first. Codes only from the candidates; the invented one is gone; text from the knowledge
    // base.
    assert_eq!(
        goal_kinds(&turn),
        ["content", "content", "language", "language"]
    );
    assert_eq!(
        plan["goals"][0],
        json!({
            "kind": "content",
            "zh": "學生能說出「The Umbrella Stand」各段的重點。",
            "en": "Students will be able to tell the main point of each paragraph of the text.",
            "curriculum_codes": ["T-G-3-V-2"],
        })
    );
    assert_eq!(
        plan["goals"][1]["zh"],
        "學生能根據課文說明主角做決定的原因。"
    );
    assert_eq!(plan["goals"][1]["curriculum_codes"], json!([]));
    assert_eq!(
        plan["goals"][2],
        json!({
            "kind": "language",
            "zh": "學生能在讀不懂時重讀並畫出每段的關鍵句。",
            "en": "Students will be able to reread and underline the key sentence of each paragraph when they are lost.",
            "curriculum_codes": [],
            "category": "strategy",
            "supports": 1,
            "relation": "學生要用重讀並畫出關鍵句的策略，才能說出各段的重點。",
        })
    );
    assert_eq!(
        plan["goals"][3]["en"],
        "Students will be able to use because and so to explain causes and results."
    );
    assert_eq!(plan["goals"][3]["category"], "function");
    assert_eq!(plan["goals"][3]["supports"], 2, "the second kept 內容目標");
    // The A6 fields: words (no repeat, English only), materials, higher-order questions
    // (analyze / evaluate / create only), the background the warm-up connects to, the review
    // and the assessment.
    assert_eq!(
        plan["key_vocabulary"],
        json!([
            { "word": "trust", "zh": "信任；課文的核心概念。" },
            { "word": "neighbor", "zh": "鄰居。" },
        ])
    );
    assert_eq!(
        plan["materials"],
        json!(["課文第 1–4 段", "老師準備的圖片"])
    );
    assert_eq!(
        plan["higher_order_questions"],
        json!([
            { "question": "Why do you think the plan worked in the end?", "bloom": "evaluate" },
            { "question": "What would you do if you were the main character?", "bloom": "create" },
        ])
    );
    assert!(plan["background"].as_str().unwrap().contains("借東西"));
    assert!(plan["review"].as_str().unwrap().contains("trust"));
    assert!(plan["assessment"].as_str().unwrap().contains("exit ticket"));
    assert_eq!(
        plan["curriculum"],
        json!([{ "code": "T-G-3-V-2", "text": "能讀懂 umbrella 測試用的短文並找出主旨。" }])
    );
    let steps = plan["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 3);
    assert_eq!(steps[0]["phase"], "引起動機");
    assert_eq!(steps[0]["options"].as_array().unwrap().len(), 3);
    assert_eq!(steps[1]["minutes"], 50, "clamped");
    assert_eq!(steps[1]["options"], json!([]), "options only for 引起動機");
    assert_eq!(steps[2]["phase"], "統整與評量");
    // A2 / A3: three groupings; the pair says what each partner does, the group lists its roles
    // and how the teacher monitors; individual work keeps no role; all four skills.
    let groupings: Vec<&str> = steps
        .iter()
        .map(|s| s["grouping"].as_str().unwrap())
        .collect();
    assert_eq!(groupings, ["兩人一組", "小組", "個人"]);
    assert_eq!(steps[0]["roles"].as_array().unwrap().len(), 2);
    assert_eq!(
        steps[1]["roles"][0],
        json!({ "role": "主持", "task": "帶大家逐段討論，確定每個人都發言。" })
    );
    assert_eq!(steps[1]["roles"].as_array().unwrap().len(), 4);
    assert!(steps[1]["monitoring"].as_str().unwrap().contains("巡視"));
    assert_eq!(steps[2]["roles"], json!([]));
    assert_eq!(steps[0]["skills"], json!(["聽", "說"]));
    assert_eq!(steps[1]["skills"], json!(["說", "讀"]));
    assert!(steps[0]["scaffolding"].as_str().unwrap().contains("示範"));
    assert_eq!(
        done["next"],
        json!(["comprehension", "tiered", "kit", "worksheet"])
    );

    // One call, logged without the material; its retrieval is logged too.
    let rows = interactions(&app, "prep_activity").await;
    assert_eq!(rows.len(), 1);
    let (attempt, status, snapshot) = &rows[0];
    assert_eq!((*attempt, status.as_str()), (1, "shown"));
    let logged = snapshot.to_string();
    assert!(!logged.contains("umbrella stand outside"), "{logged}");
    assert!(logged.contains("sha256"));
    assert_eq!(snapshot["request"]["max_tokens"], 8000);
    let retrieval: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM retrieval_logs WHERE ai_interaction_id = $1")
            .bind(turn["interaction_id"].as_i64().unwrap())
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(retrieval, 1);

    // The plan's 引起動機 ideas go into the next worksheet's warm-up brief.
    let (s, v) = turn_req(&app, &t, id, "worksheet").await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    let ws_id = last(&v)["worksheet"]["id"].as_i64().unwrap();
    let (_, ws) = app.get(&format!("/api/worksheets/{ws_id}"), Some(&t)).await;
    assert_eq!(
        ws["brief"]["warmup"],
        json!([
            "選項一：看圖片：展示老師準備的圖片，請學生兩兩猜主題。",
            "選項二：提問：問學生一個和生活經驗有關的問題。",
            "選項三：小遊戲：用三個關鍵字讓學生猜課文內容。",
        ])
    );
    let done = wait(&app, &t, id).await;
    assert_eq!(last(&done)["status"], "done");
    assert_eq!(done["next"], json!(["comprehension", "tiered", "kit"]));
}

async fn turn_req(app: &common::TestApp, t: &str, id: i64, kind: &str) -> (StatusCode, Value) {
    post_turn(app, t, id, json!({ "kind": kind })).await
}

#[tokio::test]
async fn an_activity_is_retried_once_and_fails_honestly() {
    let app = test_app!();
    let t = app.teacher("T-PREP-5", "general").await;
    let once = pasted(&app, &t, "activity", "FAKE_BLOCK_ONCE").await;
    let done = wait(&app, &t, sid(&once)).await;
    assert_eq!(last(&done)["status"], "done", "{done}");
    let rows = interactions(&app, "prep_activity").await;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].1, "blocked");
    assert_eq!((rows[1].0, rows[1].1.as_str()), (2, "shown"));
    assert_eq!(
        rows[1].2["request"]["temperature"].as_f64().unwrap(),
        0.6000000238418579
    );

    let blocked = pasted(&app, &t, "activity", "FAKE_BLOCK").await;
    let done = wait(&app, &t, sid(&blocked)).await;
    let turn = last(&done);
    assert_eq!(turn["status"], "failed");
    assert!(
        turn["error"].as_str().unwrap().contains("再試一次"),
        "{turn}"
    );
    assert_eq!(turn["result"], Value::Null);
    // A failed kind is still offered as a next step.
    assert!(done["next"]
        .as_array()
        .unwrap()
        .contains(&json!("activity")));

    let runaway = pasted(&app, &t, "activity", "FAKE_RUNAWAY").await;
    assert_eq!(last(&wait(&app, &t, sid(&runaway)).await)["status"], "done");
    let timeout = pasted(&app, &t, "activity", "FAKE_TIMEOUT").await;
    let turn = last(&wait(&app, &t, sid(&timeout)).await);
    assert_eq!(turn["status"], "failed");
    assert!(turn["error"].as_str().unwrap().contains("AI 目前沒有回應"));
}

/// The `kind` of each goal of a turn's plan, in order.
fn goal_kinds(turn: &Value) -> Vec<String> {
    turn["result"]["goals"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["kind"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn a_missing_goal_group_is_asked_for_again() {
    let app = test_app!();
    let t = app.teacher("T-PREP-5G", "general").await;
    // The first call has no 語言目標: the retry is asked for them and has both groups.
    let once = pasted(&app, &t, "activity", "FAKE_PREP_ONE_GROUP_ONCE").await;
    let turn = last(&wait(&app, &t, sid(&once)).await);
    assert_eq!(turn["status"], "done", "{turn}");
    assert_eq!(
        goal_kinds(&turn),
        ["content", "content", "language", "language"]
    );
    let rows = interactions(&app, "prep_activity").await;
    assert_eq!(rows.len(), 2);
    assert_eq!((rows[0].0, rows[0].1.as_str()), (1, "blocked"));
    assert_eq!((rows[1].0, rows[1].1.as_str()), (2, "shown"));
    let user = rows[1].2["prompt"]["user"].as_str().unwrap();
    assert!(user.contains("language_goals 沒有可用的語言目標"), "{user}");

    // Never any 語言目標: the retry's plan is still shown, with its 內容目標 only.
    let always = pasted(&app, &t, "activity", "FAKE_PREP_ONE_GROUP").await;
    let turn = last(&wait(&app, &t, sid(&always)).await);
    assert_eq!(turn["status"], "done", "{turn}");
    assert_eq!(goal_kinds(&turn), ["content", "content"]);
    let rows = interactions(&app, "prep_activity").await;
    assert_eq!(rows.len(), 4);
    assert_eq!((rows[2].0, rows[2].1.as_str()), (1, "blocked"));
    assert_eq!((rows[3].0, rows[3].1.as_str()), (2, "shown"));
}

#[tokio::test]
async fn a_lesson_without_groupings_or_skills_is_asked_for_again() {
    let app = test_app!();
    let t = app.teacher("T-PREP-5F", "general").await;
    // The first call has the whole class only, and reading only: the retry is asked for at
    // least two groupings and all four skills, and has them.
    let once = pasted(&app, &t, "activity", "FAKE_PREP_FLAT_ONCE").await;
    let turn = last(&wait(&app, &t, sid(&once)).await);
    assert_eq!(turn["status"], "done", "{turn}");
    let groupings: Vec<&str> = turn["result"]["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["grouping"].as_str().unwrap())
        .collect();
    assert_eq!(groupings, ["兩人一組", "小組", "個人"]);
    let rows = interactions(&app, "prep_activity").await;
    assert_eq!(rows.len(), 2);
    assert_eq!((rows[0].0, rows[0].1.as_str()), (1, "blocked"));
    assert_eq!((rows[1].0, rows[1].1.as_str()), (2, "shown"));
    let user = rows[1].2["prompt"]["user"].as_str().unwrap();
    assert!(user.contains("至少要用兩種分組方式"), "{user}");
    assert!(user.contains("這次沒有用到：聽、說、寫"), "{user}");

    // Never: the retry's plan is still shown as it is.
    let always = pasted(&app, &t, "activity", "FAKE_PREP_FLAT").await;
    let turn = last(&wait(&app, &t, sid(&always)).await);
    assert_eq!(turn["status"], "done", "{turn}");
    assert!(turn["result"]["steps"]
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s["grouping"] == "全班" && s["roles"] == json!([])));
    let rows = interactions(&app, "prep_activity").await;
    assert_eq!(rows.len(), 4);
    assert_eq!((rows[3].0, rows[3].1.as_str()), (2, "shown"));
}

// ---------------------------------------------------------------- resources

#[tokio::test]
async fn resources_list_curriculum_and_own_materials_without_a_model() {
    let app = test_app!();
    import_curriculum(&app).await;
    let t = app.teacher("T-PREP-6", "general").await;
    // Two of the teacher's own materials: one about umbrellas, one not.
    for (title, text) in [
        (
            "Umbrella poems",
            "Short poems about an umbrella in the rain.",
        ),
        ("Volcanoes", "Lava and ash."),
        // Shares one ordinary word with the lesson: not related.
        (
            "Rain report",
            "Weather stations measure rain, wind, temperature, humidity and pressure every hour across the island.",
        ),
    ] {
        let (s, v) = app
            .post(
                "/api/materials",
                Some(&t),
                json!({ "kind": "text", "title": title, "text": text }),
            )
            .await;
        assert_eq!(s, StatusCode::OK, "{v}");
    }
    let session = pasted(&app, &t, "resources", "").await;
    let done = wait(&app, &t, sid(&session)).await;
    let turn = last(&done);
    assert_eq!(turn["status"], "done", "{turn}");
    assert_eq!(turn["interaction_id"], Value::Null);
    let r = &turn["result"];
    let codes: Vec<&str> = r["curriculum"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["code"].as_str().unwrap())
        .collect();
    assert_eq!(codes, ["T-G-3-V-2"], "{r}");
    assert_eq!(r["curriculum"][0]["school_track"], "general");
    assert_eq!(r["curriculum"][0]["similarity"], Value::Null);
    let titles: Vec<&str> = r["materials"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["Umbrella poems"], "not the session's own material");
    assert_eq!(r["note"], MSG_NO_EXTERNAL);
    assert!(interactions(&app, "prep_ask").await.is_empty());

    // A topic only: nothing found.
    let topic = create(
        &app,
        &t,
        json!({ "kind": "resources", "topic": "量子力學" }),
    )
    .await;
    assert_eq!(topic["title"], "量子力學");
    assert_eq!(topic["material"], Value::Null);
    let done = wait(&app, &t, sid(&topic)).await;
    let r = &last(&done)["result"];
    assert_eq!(r["curriculum"], json!([]));
    assert_eq!(r["materials"], json!([]));
    assert_eq!(
        r["note"],
        format!("知識庫裡沒有找到相關條目。{MSG_NO_EXTERNAL}")
    );
    // Without material nothing is suggested (找教學資源 is no longer a suggestion).
    assert_eq!(done["next"], json!([]));
}

// ---------------------------------------------------------------- ask

#[tokio::test]
async fn a_question_gets_a_plain_answer_with_the_conversation_so_far() {
    let app = test_app!();
    let t = app.teacher("T-PREP-7", "general").await;
    let session = create(
        &app,
        &t,
        json!({ "kind": "ask", "request": "幫我想三個這一課的暖身活動，要很有趣，最好可以讓學生動起來的那種，而且不需要太多準備時間" }),
    )
    .await;
    let id = sid(&session);
    assert_eq!(
        session["title"],
        "幫我想三個這一課的暖身活動，要很有趣，最好可以讓學生動起來的那種，而且不需要太多準備時間"
            .chars()
            .take(40)
            .collect::<String>()
            + "…"
    );
    let done = wait(&app, &t, id).await;
    let turn = last(&done);
    assert_eq!(turn["status"], "done", "{turn}");
    let answer = turn["result"]["answer"].as_str().unwrap();
    assert!(!answer.contains("**") && !answer.contains('#'), "{answer}");
    assert!(answer.contains("參考了 0 則先前的對話"), "{answer}");
    assert!(!answer.contains("參考了你的教材"));
    assert_eq!(
        turn["result"]["follow_ups"],
        json!(["幫我做一張學習單", "再給我三個暖身活動"])
    );
    assert!(turn["interaction_id"].is_i64());
    assert_eq!(done["next"], json!([]), "no material, no topic");

    // The bottom bar without a worksheet asks again; the earlier turn is in the context.
    let (s, v) = post_turn(&app, &t, id, json!({ "request": "再多一個" })).await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    assert_eq!(last(&v)["kind"], "ask");
    let done = wait(&app, &t, id).await;
    let answer = last(&done)["result"]["answer"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(answer.contains("參考了 1 則先前的對話"), "{answer}");
    let rows = interactions(&app, "prep_ask").await;
    assert_eq!(rows.len(), 2);
    assert!(rows[1].2["inputs"]["question"] == "再多一個");

    // With material, and an unusable output.
    let with = create(
        &app,
        &t,
        json!({ "kind": "ask", "request": "這課的主旨是什麼？ FAKE_BLOCK",
                "material": { "title": "U", "text": passage("") } }),
    )
    .await;
    let turn = last(&wait(&app, &t, sid(&with)).await);
    assert_eq!(turn["status"], "failed", "{turn}");
    let rows = interactions(&app, "prep_ask").await;
    assert_eq!(rows.len(), 4, "tried twice");
    assert!(!rows[3].2.to_string().contains("umbrella stand outside"));
}

// ---------------------------------------------------------------- revise

async fn worksheet_session(app: &common::TestApp, t: &str) -> (i64, i64, Value) {
    let session = pasted(app, t, "worksheet", "").await;
    let id = sid(&session);
    let ws_id = last(&session)["worksheet"]["id"].as_i64().unwrap();
    let done = wait(app, t, id).await;
    assert_eq!(last(&done)["status"], "done");
    (id, ws_id, done)
}

#[tokio::test]
async fn the_bottom_bar_revises_the_worksheet_in_one_version() {
    let app = test_app!();
    let t = app.teacher("T-PREP-8", "general").await;
    let (id, ws_id, _) = worksheet_session(&app, &t).await;
    let (_, before) = app.get(&format!("/api/worksheets/{ws_id}"), Some(&t)).await;
    let versions_before = before["versions_count"].as_i64().unwrap();
    let target = before["content"]["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| !b["items"].as_array().unwrap().is_empty())
        .unwrap()
        .clone();
    let block_id = target["id"].as_str().unwrap();

    let (s, v) = post_turn(&app, &t, id, json!({ "request": "第一題改難一點" })).await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    assert_eq!(last(&v)["kind"], "revise");
    assert_eq!(last(&v)["request"], "第一題改難一點");
    let done = wait(&app, &t, id).await;
    let turn_v = last(&done);
    assert_eq!(turn_v["status"], "done", "{turn_v}");
    let result = &turn_v["result"];
    assert_eq!(result["changed_block_ids"], json!([block_id]));
    assert_eq!(result["version_no"], versions_before + 1);
    assert!(
        result["reply"].as_str().unwrap().contains("已修改"),
        "{result}"
    );

    let (_, after) = app.get(&format!("/api/worksheets/{ws_id}"), Some(&t)).await;
    assert_eq!(after["versions_count"], versions_before + 1);
    let (_, versions) = app
        .get(&format!("/api/worksheets/{ws_id}/versions"), Some(&t))
        .await;
    assert_eq!(versions[0]["cause"], "revise");
    let block = after["content"]["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["id"] == block_id)
        .unwrap()
        .clone();
    let first = block["items"][0].clone();
    assert!(first["prompt"].as_str().unwrap().ends_with(" (revised)"));
    assert_eq!(first["origin"], "ai");
    assert_eq!(first["levels"], after["content"]["levels"]);
    assert_eq!(
        block["scaffolds"], target["scaffolds"],
        "scaffolds are kept"
    );
    assert_eq!(block["vocabulary"], target["vocabulary"]);
    assert_eq!(after["undoable"], json!([block_id]));
    assert_eq!(
        turn_v["worksheet"],
        Value::Null,
        "a revise turn has no worksheet of its own"
    );

    // The per-block undo brings the block back.
    let (s, undone) = app
        .post(
            &format!("/api/worksheets/{ws_id}/blocks/{block_id}/undo"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{undone}");
    let restored = undone["content"]["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["id"] == block_id)
        .unwrap()
        .clone();
    assert_eq!(restored, target);

    let rows = interactions(&app, "prep_revise").await;
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].2.to_string().contains("umbrella stand outside"));
}

#[tokio::test]
async fn a_revision_that_fails_its_checks_changes_nothing() {
    let app = test_app!();
    let t = app.teacher("T-PREP-9", "general").await;
    let (id, ws_id, _) = worksheet_session(&app, &t).await;
    let (_, before) = app.get(&format!("/api/worksheets/{ws_id}"), Some(&t)).await;
    let (s, v) = post_turn(
        &app,
        &t,
        id,
        json!({ "kind": "revise", "request": "改成連連看 FAKE_PREP_BAD_REVISE" }),
    )
    .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    let done = wait(&app, &t, id).await;
    let turn_v = last(&done);
    assert_eq!(turn_v["status"], "done", "{turn_v}");
    let result = &turn_v["result"];
    assert_eq!(result["changed_block_ids"], json!([]));
    assert_eq!(result["version_no"], Value::Null);
    let reply = result["reply"].as_str().unwrap();
    assert!(reply.contains("沒有改成"), "{reply}");
    assert!(reply.contains("區"), "{reply}");
    let (_, after) = app.get(&format!("/api/worksheets/{ws_id}"), Some(&t)).await;
    assert_eq!(after["versions_count"], before["versions_count"]);
    assert_eq!(after["content"], before["content"]);
    assert_eq!(after["undoable"], json!([]));
    // Tried twice; the retry listed the problem.
    let rows = interactions(&app, "prep_revise").await;
    assert_eq!(rows.len(), 2);
    assert!(rows[1].2["prompt"]["user"]
        .as_str()
        .unwrap()
        .contains("第二次請求"));
}

// ---------------------------------------------------------------- rules and conflicts

#[tokio::test]
async fn needs_rules_are_enforced() {
    let app = test_app!();
    let t = app.teacher("T-PREP-10", "general").await;
    let bad = |body: Value| {
        let app = &app;
        let t = &t;
        async move {
            let (s, v) = app.post("/api/prep-sessions", Some(t), body).await;
            assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
            v["error"]["message"].as_str().unwrap().to_string()
        }
    };
    for kind in ["worksheet", "assessment", "activity"] {
        let m = bad(json!({ "kind": kind, "topic": "trust" })).await;
        assert!(m.contains("需要教材"), "{m}");
    }
    assert!(bad(json!({ "kind": "resources" }))
        .await
        .contains("教材或主題"));
    assert!(bad(json!({ "kind": "ask", "request": "  " }))
        .await
        .contains("問題"));
    assert!(bad(json!({ "kind": "revise", "request": "x" }))
        .await
        .contains("修改"));
    assert!(bad(json!({ "kind": "chat", "request": "x" }))
        .await
        .contains("kind"));
    for kind in ["comprehension", "tiered", "kit"] {
        let m = bad(json!({ "kind": kind, "topic": "trust" })).await;
        assert!(m.contains("需要教材"), "{kind}: {m}");
    }
    assert!(bad(json!({ "kind": "ask", "request": "x".repeat(2001) }))
        .await
        .contains("2000"));
    assert!(
        bad(json!({ "kind": "resources", "topic": "字".repeat(201) }))
            .await
            .contains("200")
    );
    let m =
        bad(json!({ "kind": "worksheet", "material": { "title": "Empty", "text": " " } })).await;
    assert!(m.contains("貼上"), "{m}");
    // A material without extracted text.
    let (s, link) = app
        .post(
            "/api/materials",
            Some(&t),
            json!({ "kind": "link", "title": "Talk", "url": "https://example.org/talk" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{link}");
    let m = bad(json!({ "kind": "worksheet", "material_id": link["id"] })).await;
    assert!(m.starts_with("這份檔案讀不到文字（可能是掃描檔）"), "{m}");
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM prep_sessions")
            .fetch_one(&app.db)
            .await
            .unwrap(),
        0
    );

    // Turns of a topic-only session.
    let topic = create(&app, &t, json!({ "kind": "resources", "topic": "trust" })).await;
    let id = sid(&topic);
    wait(&app, &t, id).await;
    for kind in [
        "worksheet",
        "activity",
        "assessment",
        "comprehension",
        "tiered",
        "kit",
    ] {
        let (s, v) = turn_req(&app, &t, id, kind).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{kind} {v}");
    }
    let (s, v) = post_turn(&app, &t, id, json!({ "kind": "ask" })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    let (s, v) = post_turn(
        &app,
        &t,
        id,
        json!({ "kind": "revise", "request": "改短一點" }),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT, "no worksheet: {v}");
    assert!(v["error"]["message"]
        .as_str()
        .unwrap()
        .contains("還沒有學習單"));
    // A chip again: the request is the chip's label.
    let (s, v) = turn_req(&app, &t, id, "resources").await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    assert_eq!(last(&v)["request"], "找教學資源");
}

#[tokio::test]
async fn one_turn_runs_at_a_time() {
    let app = test_app!();
    let t = app.teacher("T-PREP-11", "general").await;
    let session = create(
        &app,
        &t,
        json!({ "kind": "ask", "request": "慢一點 FAKE_SLOW" }),
    )
    .await;
    let id = sid(&session);
    assert_eq!(session["running"], true);
    let (s, v) = post_turn(&app, &t, id, json!({ "request": "還有呢？" })).await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    assert!(v["error"]["message"].as_str().unwrap().contains("還在處理"));
    let done = wait(&app, &t, id).await;
    assert_eq!(done["turns"].as_array().unwrap().len(), 1);

    // A worksheet still generating cannot be revised.
    let slow = pasted(&app, &t, "worksheet", "FAKE_SLOW").await;
    let (s, v) = post_turn(&app, &t, sid(&slow), json!({ "request": "改短一點" })).await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    assert!(v["error"]["message"].as_str().unwrap().contains("正在產生"));
}

#[tokio::test]
async fn a_turn_that_outlived_a_restart_fails_on_the_next_read() {
    let app = test_app!();
    let t = app.teacher("T-PREP-12", "general").await;
    let session = create(&app, &t, json!({ "kind": "resources", "topic": "trust" })).await;
    let id = sid(&session);
    wait(&app, &t, id).await;
    // A turn left running by a server that died.
    sqlx::query(
        "INSERT INTO prep_turns (session_id, seq, kind, request, status, created_at)
         VALUES ($1, 2, 'ask', '問題', 'running', now() - interval '1 hour')",
    )
    .bind(id)
    .execute(&app.db)
    .await
    .unwrap();
    let (_, v) = app.get(&format!("/api/prep-sessions/{id}"), Some(&t)).await;
    assert_eq!(v["running"], false);
    let turn = last(&v);
    assert_eq!(turn["status"], "failed");
    assert!(turn["error"].as_str().unwrap().contains("中斷"));
}

// ---------------------------------------------------------------- ownership, PATCH, list, delete

#[tokio::test]
async fn sessions_belong_to_their_teacher() {
    let app = test_app!();
    let t = app.teacher("T-PREP-13", "general").await;
    let other = app.teacher("T-PREP-14", "general").await;
    let session = create(&app, &t, json!({ "kind": "resources", "topic": "trust" })).await;
    let id = sid(&session);
    wait(&app, &t, id).await;
    let path = format!("/api/prep-sessions/{id}");
    let (s, _) = app.get(&path, Some(&other)).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = patch(&app, &path, &other, json!({ "title": "x" })).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = post_turn(&app, &other, id, json!({ "kind": "resources" })).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = app.delete(&path, Some(&other)).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (_, list) = app.get("/api/prep-sessions", Some(&other)).await;
    assert_eq!(list["sessions"], json!([]));
    let (s, _) = app.get("/api/prep-sessions", None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // Someone else's material, class or template cannot be used.
    let (_, m) = app
        .post(
            "/api/materials",
            Some(&other),
            json!({ "kind": "text", "title": "Theirs", "text": passage("") }),
        )
        .await;
    let (s, v) = app
        .post(
            "/api/prep-sessions",
            Some(&t),
            json!({ "kind": "activity", "material_id": m["id"] }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    let (_, class) = app
        .post("/api/classes", Some(&other), json!({ "name": "三年八班" }))
        .await;
    let (s, _) = app
        .post(
            "/api/prep-sessions",
            Some(&t),
            json!({ "kind": "resources", "topic": "x", "class_id": class["id"] }),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = patch(&app, &path, &t, json!({ "class_id": class["id"] })).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn patch_changes_the_context_and_the_list_is_newest_first() {
    let app = test_app!();
    let t = app.teacher("T-PREP-15", "general").await;
    let (_, class) = app
        .post(
            "/api/classes",
            Some(&t),
            json!({ "name": "二年三班", "class_profile": "程度落差大" }),
        )
        .await;
    let class_id = class["id"].as_i64().unwrap();
    let a = create(
        &app,
        &t,
        json!({ "kind": "resources", "topic": "trust", "class_id": class_id }),
    )
    .await;
    assert_eq!(a["class"], json!({ "id": class_id, "name": "二年三班" }));
    wait(&app, &t, sid(&a)).await;
    let b = create(&app, &t, json!({ "kind": "resources", "topic": "rain" })).await;
    wait(&app, &t, sid(&b)).await;
    let (_, list) = app.get("/api/prep-sessions", Some(&t)).await;
    let ids: Vec<i64> = list["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_i64().unwrap())
        .collect();
    assert_eq!(ids, [sid(&b), sid(&a)]);
    assert_eq!(list["sessions"][0]["turn_count"], 1);
    assert_eq!(list["sessions"][0]["last_kind"], "resources");
    assert!(list["sessions"][0].get("turns").is_none());

    let path = format!("/api/prep-sessions/{}", sid(&a));
    let (s, v) = patch(
        &app,
        &path,
        &t,
        json!({ "title": "  信任  ", "class_id": null, "template_id": null }),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["title"], "信任");
    assert_eq!(v["class"], Value::Null);
    assert_eq!(v["topic"], "trust");
    let (s, v) = patch(&app, &path, &t, json!({ "class_id": class_id })).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["class"]["id"], class_id);
    assert_eq!(v["title"], "信任", "absent fields stay");
    let (s, _) = patch(&app, &path, &t, json!({ "title": " " })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = patch(&app, &path, &t, json!({ "template_id": 999999 })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (_, list) = app.get("/api/prep-sessions", Some(&t)).await;
    assert_eq!(list["sessions"][0]["id"], sid(&a), "patched is newest");
}

#[tokio::test]
async fn deleting_a_session_hides_it_with_its_worksheets() {
    let app = test_app!();
    let t = app.teacher("T-PREP-16", "general").await;
    let (id, ws_id, _) = worksheet_session(&app, &t).await;
    // A worksheet made elsewhere stays.
    let (s, other_ws) = app
        .post(
            "/api/worksheets",
            Some(&t),
            json!({ "material": { "title": "Other", "text": passage("") } }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{other_ws}");
    let other = other_ws["id"].as_i64().unwrap();
    let path = format!("/api/prep-sessions/{id}");
    let (s, v) = app.delete(&path, Some(&t)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v, json!({ "ok": true, "worksheets_deleted": 1 }));

    // Gone for the teacher: the session, its worksheet, and both lists.
    let (s, _) = app.get(&path, Some(&t)).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = app.get(&format!("/api/worksheets/{ws_id}"), Some(&t)).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (_, sessions) = app.get("/api/prep-sessions", Some(&t)).await;
    assert_eq!(sessions["sessions"], json!([]));
    let (_, list) = app.get("/api/worksheets", Some(&t)).await;
    let ids: Vec<i64> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["id"].as_i64().unwrap())
        .collect();
    assert!(!ids.contains(&ws_id), "{list}");
    assert!(ids.contains(&other), "a worksheet of its own stays: {list}");
    // No more turns or changes on it.
    let (s, _) = post_turn(&app, &t, id, json!({ "request": "再改一次" })).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = patch(&app, &path, &t, json!({ "title": "x" })).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = app.delete(&path, Some(&t)).await;
    assert_eq!(s, StatusCode::NOT_FOUND, "a second delete finds nothing");

    // Kept for the research record: the session, its turns and the worksheet, marked deleted.
    let (session_deleted, turns): (bool, i64) = sqlx::query_as(
        "SELECT s.deleted_at IS NOT NULL,
                (SELECT COUNT(*) FROM prep_turns t WHERE t.session_id = s.id)
           FROM prep_sessions s WHERE s.id = $1",
    )
    .bind(id)
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert!(session_deleted);
    assert!(turns > 0, "turns stay");
    let (ws_deleted, ws_session): (bool, Option<i64>) = sqlx::query_as(
        "SELECT deleted_at IS NOT NULL, prep_session_id FROM worksheets WHERE id = $1",
    )
    .bind(ws_id)
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert!(ws_deleted);
    assert_eq!(ws_session, Some(id), "the worksheet keeps its session");
    let logged = usage(&app, "prep_session_deleted").await;
    assert_eq!(
        logged,
        vec![json!({ "session_id": id, "worksheet_ids": [ws_id] })]
    );
}
