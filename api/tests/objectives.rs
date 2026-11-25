//! SIOP objectives (docs/API.md「SIOP 目標」) with the fake provider:
//!
//! * G1: `content_objective` / `language_objective` in prep-session options and worksheet
//!   settings, the legacy `objective` read from stored rows and old clients, the prompt
//!   variables, and the worksheet's structured objectives (`content.objectives`);
//! * G2/G3: `POST /api/objective-drafts` (R1 by rule, R3 by the judge, one more round when a
//!   group is left empty) and the teacher's decisions on the drafts;
//! * the 完整學習單: E1 (the exit ticket checks every objective), Q4 (示範 → 獨立應用) and V2
//!   (task words with Chinese hints).

#[macro_use]
mod common;

use std::time::Duration;

use axum::http::StatusCode;
use manna_api::config::AiProviderKind;
use serde_json::{json, Value};

const P: [&str; 4] = [
    "Mrs. Lin put an umbrella stand outside her shop on a rainy Monday. A sign said, “Take one if you need it.”",
    "Her neighbors laughed at the idea. They were sure that every umbrella would be gone within a week.",
    "In the first month, twelve of the thirty umbrellas did disappear. However, something unexpected followed.",
    "Today the stand holds more umbrellas than it did at the start. Mrs. Lin says trust is like an umbrella.",
];

const CONTENT: &str = "I can explain why Mrs. Lin kept the umbrella stand.";
const LANGUAGE: &str = "I can use however to show a change in the story.";

fn passage(marker: &str) -> String {
    format!("{}\n\n{}\n\n{}\n\n{} {marker}", P[0], P[1], P[2], P[3])
}

fn material(marker: &str) -> Value {
    json!({ "title": "The Umbrella Stand", "text": passage(marker) })
}

async fn interactions(app: &common::TestApp, module: &str) -> Vec<(i64, i32, String, Value)> {
    sqlx::query_as(
        "SELECT id, attempt, status, input_snapshot FROM ai_interactions
          WHERE module = $1 ORDER BY id",
    )
    .bind(module)
    .fetch_all(&app.db)
    .await
    .unwrap()
}

async fn wait_session(app: &common::TestApp, t: &str, id: i64) -> Value {
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

async fn create_worksheet(app: &common::TestApp, t: &str, marker: &str, settings: Value) -> Value {
    let (s, v) = app
        .post(
            "/api/worksheets",
            Some(t),
            json!({ "material": material(marker), "settings": settings }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    v
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

fn objective_ids(state: &Value) -> Vec<String> {
    state["content"]["objectives"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["id"].as_str().unwrap().to_string())
        .collect()
}

// ---------------------------------------------------------------- G1: the two fields

#[tokio::test]
async fn the_two_objectives_reach_the_worksheet_and_every_prompt() {
    let app = test_app!();
    let t = app.teacher("R-OBJ-1", "general").await;
    let (s, session) = app
        .post(
            "/api/prep-sessions",
            Some(&t),
            json!({
                "kind": "comprehension", "request": "閱讀理解題", "material": material(""),
                "options": {
                    "content_objective": format!("  {CONTENT}  "),
                    "language_objective": LANGUAGE,
                },
            }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{session}");
    assert_eq!(
        session["options"],
        json!({ "content_objective": CONTENT, "language_objective": LANGUAGE })
    );
    let id = session["id"].as_i64().unwrap();
    let session = wait_session(&app, &t, id).await;
    let ws_id = session["turns"][0]["worksheet"]["id"].as_i64().unwrap();
    let (_, ws) = app.get(&format!("/api/worksheets/{ws_id}"), Some(&t)).await;
    assert_eq!(ws["settings"]["content_objective"], CONTENT);
    assert_eq!(ws["settings"]["language_objective"], LANGUAGE);
    assert!(ws["settings"].get("objective").is_none());
    // Structured objectives, printed at the top (T1).
    let objectives = ws["content"]["objectives"].as_array().unwrap();
    assert_eq!(objectives.len(), 2);
    assert_eq!(objectives[0]["id"], "teacher-c1");
    assert_eq!(objectives[0]["type"], "content");
    assert_eq!(objectives[0]["student_facing_en"], CONTENT);
    assert_eq!(objectives[1]["type"], "language");
    assert!(ws["settings"]["generation"]
        .as_array()
        .unwrap()
        .contains(&json!("顯示學習目標")));
    assert_eq!(ws["content"]["show_objectives"], true);
    let (_, sheet) = app
        .get(
            &format!("/api/worksheets/{ws_id}/sheet?level=standard&audience=student"),
            Some(&t),
        )
        .await;
    assert_eq!(sheet["objectives"], json!([CONTENT, LANGUAGE]));

    // The Bloom prompt (version 1 has only `{{teacher_objective}}`) gets both groups in one
    // line; the variables of the two fields are there for newer versions.
    let level = interactions(&app, "worksheet_bloom_level").await;
    let snap = &level[0].3;
    assert_eq!(snap["inputs"]["teacher_content_objective"], CONTENT);
    assert_eq!(snap["inputs"]["teacher_language_objective"], LANGUAGE);
    let combined = format!("內容目標：{CONTENT}｜語言目標：{LANGUAGE}");
    assert_eq!(snap["inputs"]["teacher_objective"], combined.as_str());
    let user = snap["prompt"]["user"].as_str().unwrap();
    assert!(
        user.contains(&format!("學習目標（教師寫的）：{combined}")),
        "{user}"
    );
    assert!(!user.contains("內容目標（教師寫的或採用的）"), "not twice");

    // An activity in the same session: prep_activity version 3 gets the combined line too.
    let (s, _) = app
        .post(
            &format!("/api/prep-sessions/{id}/turns"),
            Some(&t),
            json!({ "kind": "activity", "request": "規劃教學活動" }),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED);
    wait_session(&app, &t, id).await;
    let activity = interactions(&app, "prep_activity").await;
    let snap = &activity[0].3;
    assert_eq!(snap["inputs"]["teacher_content_objective"], CONTENT);
    let user = snap["prompt"]["user"].as_str().unwrap();
    assert!(
        user.contains(&format!("學習目標（教師寫的）：{combined}")),
        "{user}"
    );
}

#[tokio::test]
async fn a_new_prompt_version_gets_the_two_fields_under_their_headings() {
    let app = test_app!();
    let t = app.teacher("R-OBJ-2", "general").await;
    // A version that mentions only one of the two fields (version 3's template with its single
    // objective variable renamed): the other is added under its heading, the combined line is not.
    // (`prep_activity` 4 names both fields itself, so it can't show this.)
    sqlx::query(
        "INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
         SELECT prompt_key,
                (SELECT MAX(version_no) + 1 FROM prompt_versions WHERE prompt_key = 'prep_activity'),
                system_text,
                replace(user_template, '{{teacher_objective}}', '{{teacher_content_objective}}')
           FROM prompt_versions WHERE prompt_key = 'prep_activity' AND version_no = 3",
    )
    .execute(&app.db)
    .await
    .unwrap();
    let (s, session) = app
        .post(
            "/api/prep-sessions",
            Some(&t),
            json!({
                "kind": "activity", "material": material(""),
                "options": { "content_objective": CONTENT, "language_objective": LANGUAGE },
            }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{session}");
    wait_session(&app, &t, session["id"].as_i64().unwrap()).await;
    let snap = &interactions(&app, "prep_activity").await[0].3;
    let user = snap["prompt"]["user"].as_str().unwrap();
    assert!(
        user.contains(&format!("學習目標（教師寫的）：{CONTENT}")),
        "{user}"
    );
    assert!(
        user.contains(&format!("語言目標（教師寫的或採用的）：{LANGUAGE}")),
        "{user}"
    );
    assert!(
        !user.contains("內容目標：I can"),
        "no combined line: {user}"
    );
}

#[tokio::test]
async fn the_old_single_objective_is_read_as_the_content_objective() {
    let app = test_app!();
    let t = app.teacher("R-OBJ-3", "general").await;
    // An old client sends `objective`.
    let ws = create_worksheet(
        &app,
        &t,
        "",
        json!({ "objective": "學生能說明林太太為什麼不收起傘架" }),
    )
    .await;
    let id = ws["id"].as_i64().unwrap();
    assert_eq!(
        ws["settings"]["content_objective"],
        "學生能說明林太太為什麼不收起傘架"
    );
    assert_eq!(ws["settings"]["language_objective"], Value::Null);
    assert_eq!(objective_ids(&ws), ["teacher-c1"]);
    // A row stored before the two fields: settings with `objective`, no objectives yet.
    sqlx::query(
        "UPDATE worksheets
            SET settings = (settings - 'content_objective' - 'language_objective')
                           || '{\"objective\": \"學生能說出主旨\"}'::jsonb
          WHERE id = $1",
    )
    .bind(id)
    .execute(&app.db)
    .await
    .unwrap();
    let (_, latest) = app.get(&format!("/api/worksheets/{id}"), Some(&t)).await;
    let mut content = latest["content"].clone();
    content["objectives"] = json!([]);
    let (s, v) = app
        .put(
            &format!("/api/worksheets/{id}"),
            Some(&t),
            json!({ "content": content }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["settings"]["content_objective"], "學生能說出主旨");
    assert!(objective_ids(&v).is_empty());
    // The next generation adds it to the worksheet's objectives and to the prompts.
    let done = generate(&app, &t, id).await;
    assert_eq!(objective_ids(&done), ["teacher-c1"]);
    assert_eq!(done["content"]["objectives"][0]["text"], "學生能說出主旨");
    let tasks = interactions(&app, "worksheet_tasks").await;
    let user = tasks.last().unwrap().3["prompt"]["user"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(user.contains("O1（內容目標）：學生能說出主旨"), "{user}");

    // A session stored with `objective` is read the same way.
    let (s, session) = app
        .post(
            "/api/prep-sessions",
            Some(&t),
            json!({ "kind": "ask", "request": "怎麼開場？" }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{session}");
    let sid = session["id"].as_i64().unwrap();
    sqlx::query(
        "UPDATE prep_sessions SET options = '{\"objective\": \"能說出主旨\"}' WHERE id = $1",
    )
    .bind(sid)
    .execute(&app.db)
    .await
    .unwrap();
    let session = wait_session(&app, &t, sid).await;
    assert_eq!(
        session["options"],
        json!({ "content_objective": "能說出主旨" })
    );
}

#[tokio::test]
async fn changing_the_fields_changes_the_worksheets_objectives() {
    let app = test_app!();
    let t = app.teacher("R-OBJ-4", "general").await;
    // Without objectives the sheet does not print the heading.
    let ws = create_worksheet(&app, &t, "", json!({})).await;
    let id = ws["id"].as_i64().unwrap();
    assert!(objective_ids(&ws).is_empty());
    assert!(!ws["settings"]["generation"]
        .as_array()
        .unwrap()
        .contains(&json!("顯示學習目標")));
    // Given later (the options view saves the whole settings): objectives, shown.
    let mut settings = ws["settings"].clone();
    settings["content_objective"] = json!(format!("{CONTENT}\nI can retell the story."));
    settings["language_objective"] = json!(LANGUAGE);
    let (s, v) = app
        .put(
            &format!("/api/worksheets/{id}"),
            Some(&t),
            json!({ "settings": settings }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(
        objective_ids(&v),
        ["teacher-c1", "teacher-c2", "teacher-l1"]
    );
    assert!(v["settings"]["generation"]
        .as_array()
        .unwrap()
        .contains(&json!("顯示學習目標")));
    // Changed again: replaced, not added; an objective the teacher added on the sheet stays.
    let mut content = v["content"].clone();
    content["objectives"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "id": "o-own", "type": "content", "text": "", "student_facing_en": "I can draw the stand.", "origin": "teacher" }));
    let mut settings = v["settings"].clone();
    settings["content_objective"] = Value::Null;
    let (s, v) = app
        .put(
            &format!("/api/worksheets/{id}"),
            Some(&t),
            json!({ "settings": settings, "content": content }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(objective_ids(&v), ["o-own", "teacher-l1"]);
    // Too long is a 400 naming the field.
    let mut settings = v["settings"].clone();
    settings["language_objective"] = json!("字".repeat(301));
    let (s, v) = app
        .put(
            &format!("/api/worksheets/{id}"),
            Some(&t),
            json!({ "settings": settings }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(
        v["error"]["message"].as_str().unwrap().contains("語言目標"),
        "{v}"
    );
}

// ---------------------------------------------------------------- the 完整學習單: E1, Q4, V2

#[tokio::test]
async fn the_full_worksheet_checks_every_objective_and_moves_from_modelling_to_independent_work() {
    let app = test_app!();
    let t = app.teacher("R-OBJ-5", "general").await;
    let settings = json!({ "content_objective": CONTENT, "language_objective": LANGUAGE });
    let ws = create_worksheet(&app, &t, "", settings.clone()).await;
    let id = ws["id"].as_i64().unwrap();
    let done = generate(&app, &t, id).await;
    let blocks = done["content"]["blocks"].as_array().unwrap();

    // E1: the exit ticket checks both objectives; no notice, one call.
    let exit = blocks.iter().find(|b| b["kind"] == "exit_ticket").unwrap();
    assert_eq!(exit["objective_ids"], json!(["teacher-c1", "teacher-l1"]));
    assert!(done["content"]["notices"]
        .as_array()
        .unwrap()
        .iter()
        .all(|n| !n.as_str().unwrap().contains("Exit ticket 沒有檢核")));
    let calls = interactions(&app, "worksheet_exit_ticket").await;
    assert_eq!(calls.len(), 1);
    let user = calls[0].3["prompt"]["user"].as_str().unwrap();
    assert!(
        user.contains(&format!("O1（內容目標）：{CONTENT}")),
        "{user}"
    );
    assert!(
        user.contains(&format!("O2（語言目標）：{LANGUAGE}")),
        "{user}"
    );
    // Version 2 (0022_siop_objectives.sql); version 3 adds the 填空題 blank (0030_fill_in_blanks.sql),
    // version 4 that a sentence frame is not a question (0031_sentence_frames_apart.sql), version 5
    // the 是非題 statement and wrapped frames (0036_template_supports.sql).
    assert_eq!(calls[0].3["prompt"]["version_no"], 5);

    // Q4: one block per paragraph, 示範 → 共同練習 → 同儕練習 → 獨立應用, said in the notes.
    let activities: Vec<&Value> = blocks.iter().filter(|b| b["kind"] == "activity").collect();
    let stages: Vec<&str> = activities
        .iter()
        .map(|b| b["practice"].as_str().unwrap())
        .collect();
    assert_eq!(stages, ["model", "guided", "peer", "independent"]);
    assert!(activities[0]["teacher_notes"]
        .as_str()
        .unwrap()
        .starts_with("教學順序：示範"));
    let tasks = interactions(&app, "worksheet_tasks").await;
    assert_eq!(tasks.len(), 1, "in order: no repair call");
    // Version 4 (0029_graphic_organizers.sql) is version 3 with the organizer section; version 5
    // (0030_fill_in_blanks.sql) adds the 填空題 blank, version 6 (0031_sentence_frames_apart.sql)
    // that a sentence frame is not a question, version 7 (0036_template_supports.sql) the 是非題
    // statement and wrapped frames, version 8 (0050_per_question_frames.sql) the block's
    // instructions as a passage introduction, version 9 (0051_table_organizer_headers.sql) the
    // table's headings and row labels, version 10 (0053_block_intros.sql) an introduction only,
    // the how-to left to the steps.
    assert_eq!(tasks[0].3["prompt"]["version_no"], 10);
    assert!(tasks[0].3["prompt"]["system"]
        .as_str()
        .unwrap()
        .contains("示範 → 共同練習 → 同儕練習 → 獨立應用"));

    // V2: the words of the instructions, with Chinese hints, after the warm-up's key words.
    let warmup = blocks.iter().find(|b| b["kind"] == "warmup").unwrap();
    let task_words: Vec<(&str, &str)> = warmup["vocabulary"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|v| v["task_word"] == true)
        .map(|v| {
            (
                v["word"].as_str().unwrap(),
                v["definition"].as_str().unwrap(),
            )
        })
        .collect();
    assert!(
        task_words.contains(&("in your own words", "用你自己的話說")),
        "{task_words:?}"
    );
    assert!(
        task_words.contains(&("explain", "解釋、說明")),
        "{task_words:?}"
    );
    assert!(
        warmup["vocabulary"][0].get("task_word").is_none(),
        "key words first"
    );
    // They print in the vocabulary section of the sheet.
    let (_, sheet) = app
        .get(
            &format!("/api/worksheets/{id}/sheet?level=standard&audience=student"),
            Some(&t),
        )
        .await;
    let words: Vec<&str> = sheet["blocks"][0]["vocabulary"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["word"].as_str().unwrap())
        .collect();
    assert!(words.contains(&"in your own words"), "{words:?}");

    // Regenerating the exit ticket keeps them, once.
    let (s, _) = app
        .post(
            &format!("/api/worksheets/{id}/blocks/exit/regenerate"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED);
    let again = wait_worksheet(&app, &t, id).await;
    let warmup = again["content"]["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["kind"] == "warmup")
        .unwrap()
        .clone();
    let count = |w: &str| {
        warmup["vocabulary"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|v| v["word"] == w)
            .count()
    };
    assert_eq!(count("explain"), 1);

    // Regenerating one activity block keeps its stage.
    let block_id = activities[2]["id"].as_str().unwrap().to_string();
    let (s, _) = app
        .post(
            &format!("/api/worksheets/{id}/blocks/{block_id}/regenerate"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED);
    let again = wait_worksheet(&app, &t, id).await;
    let block = again["content"]["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["id"] == block_id.as_str())
        .unwrap()
        .clone();
    assert_eq!(block["practice"], "peer");
    let last = interactions(&app, "worksheet_tasks").await;
    let user = last.last().unwrap().3["prompt"]["user"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(user.contains("practice 是「同儕練習」"), "{user}");
}

#[tokio::test]
async fn an_unchecked_objective_or_a_backwards_order_is_repaired_or_reported() {
    let app = test_app!();
    let t = app.teacher("R-OBJ-6", "general").await;
    let settings = json!({ "content_objective": CONTENT, "language_objective": LANGUAGE });
    // E1: the exit ticket leaves the 語言目標 out in both calls: a notice says so.
    let ws = create_worksheet(
        &app,
        &t,
        "FAKE_WS_EXIT_PARTIAL FAKE_WS_PRACTICE_OFF",
        settings,
    )
    .await;
    let id = ws["id"].as_i64().unwrap();
    let done = generate(&app, &t, id).await;
    let calls = interactions(&app, "worksheet_exit_ticket").await;
    assert_eq!(calls.len(), 2, "one repair call");
    let checks: Value =
        sqlx::query_scalar("SELECT check_results FROM ai_interactions WHERE id = $1")
            .bind(calls[0].0)
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert!(
        checks.to_string().contains("covers_every_objective"),
        "{checks}"
    );
    let notices: Vec<&str> = done["content"]["notices"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n.as_str().unwrap())
        .collect();
    assert!(
        notices
            .iter()
            .any(|n| n.contains("Exit ticket 沒有檢核到語言目標「I can use however")),
        "{notices:?}"
    );
    // Q4: the first call wrote every block as 獨立應用; the repair call put them in order.
    let tasks = interactions(&app, "worksheet_tasks").await;
    assert_eq!(tasks.len(), 2);
    let stages: Vec<&str> = done["content"]["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|b| b["kind"] == "activity")
        .map(|b| b["practice"].as_str().unwrap())
        .collect();
    assert_eq!(stages, ["model", "guided", "peer", "independent"]);

    // When the repair call does no better, the server puts them in order itself.
    let ws = create_worksheet(&app, &t, "FAKE_WS_PRACTICE_ALWAYS", json!({})).await;
    let done = generate(&app, &t, ws["id"].as_i64().unwrap()).await;
    let stages: Vec<&str> = done["content"]["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|b| b["kind"] == "activity")
        .map(|b| b["practice"].as_str().unwrap())
        .collect();
    assert_eq!(stages, ["model", "guided", "peer", "independent"]);
}

// ---------------------------------------------------------------- G2/G3: drafting

async fn draft(app: &common::TestApp, t: &str, body: Value) -> Value {
    let (s, v) = app.post("/api/objective-drafts", Some(t), body).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v
}

fn codes(v: &Value) -> Vec<String> {
    v["objectives"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["code"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn manna_drafts_checked_objectives_and_records_what_the_teacher_decides() {
    let app = test_app!();
    let t = app.teacher("R-OBJ-7", "general").await;
    let v = draft(
        &app,
        &t,
        json!({ "kind": "comprehension", "material": material(""), "request": "閱讀理解題" }),
    )
    .await;
    assert_eq!(v["status"], "shown", "{v}");
    assert_eq!(v["message"], Value::Null);
    assert_eq!(v["hidden"], 0);
    // The 內容目標 come first, alone: the 語言目標 are derived from the ones the teacher decides on.
    assert_eq!(codes(&v), ["C1"]);
    let c = &v["objectives"][0];
    assert_eq!(c["kind"], "content");
    assert_eq!(c["index"], 0);
    assert!(c["text"].as_str().unwrap().starts_with("I can explain"));
    assert!(!c["zh"].as_str().unwrap().is_empty());
    assert!(!c["check_zh"].as_str().unwrap().is_empty());
    assert!(c.get("relation_zh").is_none());
    assert!(v["context"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["label"] == "教材"));

    // One draft, one judge; the material is logged as its length and a hash; the request is a
    // bare tool label, so it is no 「教師的要求」.
    let drafts = interactions(&app, "objective_draft").await;
    assert_eq!(drafts.len(), 1);
    assert_eq!(drafts[0].2, "shown");
    assert_eq!(v["interaction_id"], drafts[0].0);
    let snap = &drafts[0].3;
    let logged = snap.to_string();
    assert!(
        !logged.contains("umbrella stand outside"),
        "the text is not logged"
    );
    assert_eq!(snap["inputs"]["teacher_request"], "（沒有）");
    assert!(snap["inputs"]["purpose"]
        .as_str()
        .unwrap()
        .contains("閱讀理解題"));
    let judge = interactions(&app, "objective_check").await;
    assert_eq!(judge.len(), 1);
    assert!(judge[0].3["prompt"]["user"]
        .as_str()
        .unwrap()
        .contains("C1（內容目標）I can explain"));
    // The usage log.
    let logged: Value = sqlx::query_scalar(
        "SELECT payload FROM usage_logs WHERE event = 'objectives_drafted' ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(logged["shown"], 1);
    assert_eq!(logged["stage"], "content");
    let first_user = drafts[0].3["prompt"]["user"].as_str().unwrap();
    assert!(first_user.contains("【這次只寫內容目標】"), "{first_user}");

    // The 語言目標 are drafted after the 內容目標 are decided, from the one the teacher kept.
    let decided = "I can explain the main idea in my own words.";
    let lang = draft(
        &app,
        &t,
        json!({ "kind": "comprehension", "material": material(""), "stage": "language",
                "content": [decided] }),
    )
    .await;
    assert_eq!(lang["status"], "shown", "{lang}");
    assert_eq!(codes(&lang), ["L1"]);
    let l = &lang["objectives"][0];
    assert_eq!(l["kind"], "language");
    assert_eq!(l["index"], 0, "its place in its own call");
    assert_eq!(l["category"], "strategy");
    assert_eq!(l["supports"], "C1");
    assert!(l["relation_zh"].as_str().unwrap().starts_with("學生要用"));
    let drafts = interactions(&app, "objective_draft").await;
    assert_eq!(drafts.len(), 2);
    let user = drafts[1].3["prompt"]["user"].as_str().unwrap();
    assert!(user.contains("【這次只寫語言目標】"), "{user}");
    assert!(user.contains(&format!("- C1：{decided}")), "{user}");
    assert!(drafts[1].3["inputs"]["stage_note"]
        .as_str()
        .unwrap()
        .contains(decided));
    let judge = interactions(&app, "objective_check").await;
    assert!(judge[1].3["prompt"]["user"]
        .as_str()
        .unwrap()
        .contains(&format!("C1（內容目標，教師已決定，不用判斷）{decided}")));
    // A 語言目標 needs a decided 內容目標; the 內容目標 stage takes none.
    for body in [
        json!({ "kind": "comprehension", "material": material(""), "stage": "language" }),
        json!({ "kind": "comprehension", "material": material(""), "stage": "language", "content": ["  "] }),
        json!({ "kind": "comprehension", "material": material(""), "content": [decided] }),
        json!({ "kind": "comprehension", "material": material(""), "stage": "other" }),
    ] {
        let (st, _) = app
            .post("/api/objective-drafts", Some(&t), body.clone())
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{body}");
    }

    // 採用 the 內容目標, 修改 the 語言目標: the existing decision endpoint, on each call.
    let id = v["interaction_id"].as_i64().unwrap();
    let lang_id = lang["interaction_id"].as_i64().unwrap();
    let (s, _) = app
        .post(
            &format!("/api/ai/{id}/decision"),
            Some(&t),
            json!({ "item_index": 0, "decision": "adopt" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = app
        .post(
            &format!("/api/ai/{lang_id}/decision"),
            Some(&t),
            json!({ "item_index": 0, "decision": "revise", "revised_content": "I can reread a hard sentence." }),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let rows: Vec<(i32, String, Option<String>)> = sqlx::query_as(
        "SELECT item_index, decision, revised_content FROM teacher_decisions
          WHERE ai_interaction_id = ANY($1) ORDER BY id",
    )
    .bind(vec![id, lang_id])
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert_eq!(
        rows,
        [
            (0, "adopt".to_string(), None),
            (
                0,
                "revise".to_string(),
                Some("I can reread a hard sentence.".to_string())
            )
        ]
    );
    // Another teacher cannot decide on it.
    let other = app.teacher("R-OBJ-7B", "general").await;
    let (s, _) = app
        .post(
            &format!("/api/ai/{id}/decision"),
            Some(&other),
            json!({ "item_index": 0, "decision": "reject" }),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // 換一組: the earlier lines are sent, the model writes another set.
    let again = draft(
        &app,
        &t,
        json!({
            "kind": "comprehension", "material": material(""),
            "avoid": [v["objectives"][0]["text"]],
        }),
    )
    .await;
    assert!(again["objectives"][0]["text"]
        .as_str()
        .unwrap()
        .contains("summarize"));
    let user = interactions(&app, "objective_draft").await[2].3["prompt"]["user"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(user.contains("請不要重複這些"), "{user}");

    // 換掉沒勾的: the kept lines (as the teacher edited them) go into the request.
    let partial = draft(
        &app,
        &t,
        json!({
            "kind": "comprehension", "material": material(""),
            "avoid": [v["objectives"][0]["text"]],
            "keep": [{ "code": "C1", "text": "I can explain the main idea in my own words." }],
        }),
    )
    .await;
    assert_eq!(partial["status"], "shown");
    let user = interactions(&app, "objective_draft").await[3].3["prompt"]["user"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(user.contains("請不要重複這些"), "{user}");
    assert!(
        user.contains("- C1：I can explain the main idea in my own words."),
        "{user}"
    );
    // A kept line needs a code of the set and a line; at most three, one per code.
    for keep in [
        json!([{ "code": "C3", "text": "I can x." }]),
        json!([{ "code": "C1", "text": "  " }]),
        json!([{ "code": "C1", "text": "a" }, { "code": "C1", "text": "b" }]),
        json!([{ "code": "C1", "text": "a" }, { "code": "C2", "text": "b" },
               { "code": "L1", "text": "c" }, { "code": "L2", "text": "d" }]),
    ] {
        let (s, _) = app
            .post(
                "/api/objective-drafts",
                Some(&t),
                json!({ "kind": "comprehension", "material": material(""), "keep": keep }),
            )
            .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{keep}");
    }
}

#[tokio::test]
async fn drafts_that_fail_r1_or_r3_are_not_shown_and_the_server_tries_once_more() {
    let app = test_app!();
    let t = app.teacher("R-OBJ-8", "general").await;

    // R1 by rule: "understand" is dropped (and its 語言目標 with it); the same call is asked
    // again with the problems listed.
    let v = draft(
        &app,
        &t,
        json!({ "kind": "activity", "material": material("FAKE_OBJ_VAGUE") }),
    )
    .await;
    assert_eq!(v["status"], "shown");
    assert_eq!(codes(&v), ["C1"]);
    let drafts = interactions(&app, "objective_draft").await;
    assert_eq!(
        drafts
            .iter()
            .map(|d| (d.1, d.2.as_str()))
            .collect::<Vec<_>>(),
        [(1, "blocked"), (2, "shown")]
    );
    assert!(drafts[1].3["prompt"]["user"]
        .as_str()
        .unwrap()
        .contains("understand"));

    // R3 by the judge: round 1's 語言目標 is not needed; round 2 is drafted with the reasons and
    // shown (its own wording), round 1 is recorded as blocked with why.
    let decided = ["I can explain the main point with two examples from the text."];
    let v = draft(
        &app,
        &t,
        json!({ "kind": "worksheet", "material": material("FAKE_OBJ_R3"),
                "stage": "language", "content": decided }),
    )
    .await;
    assert_eq!(v["status"], "shown", "{v}");
    assert_eq!(codes(&v), ["L1"]);
    assert!(v["objectives"][0]["text"]
        .as_str()
        .unwrap()
        .contains("predict from the title"));
    let drafts = interactions(&app, "objective_draft").await;
    let (round1, round2) = (&drafts[2], &drafts[3]);
    assert_eq!(v["interaction_id"], round2.0);
    assert!(round2.3["prompt"]["user"]
        .as_str()
        .unwrap()
        .contains("【重新草擬】"));
    let (status, checks): (String, Value) =
        sqlx::query_as("SELECT status, check_results FROM ai_interactions WHERE id = $1")
            .bind(round1.0)
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(status, "blocked");
    assert!(checks.to_string().contains("not_needed"), "{checks}");

    // Every 語言目標 fails, in both rounds: nothing is shown, and the teacher is told. The
    // 內容目標 she decided on stay hers.
    let v = draft(
        &app,
        &t,
        json!({ "kind": "tiered", "material": material("FAKE_OBJ_R3_ALWAYS"),
                "stage": "language", "content": decided }),
    )
    .await;
    assert_eq!(v["status"], "blocked", "{v}");
    assert!(codes(&v).is_empty());
    assert!(v["message"].as_str().unwrap().contains("自己寫"));

    // Nothing usable at all.
    let v = draft(
        &app,
        &t,
        json!({ "kind": "kit", "material": material("FAKE_BLOCK") }),
    )
    .await;
    assert_eq!(v["status"], "blocked");
    assert!(v["objectives"].as_array().unwrap().is_empty());
    assert!(v["message"].as_str().unwrap().contains("自己寫"));
}

#[tokio::test]
async fn two_per_group_strategy_first_and_variety_within_a_unit() {
    let app = test_app!();
    let t = app.teacher("R-OBJ-9", "general").await;
    let c = draft(
        &app,
        &t,
        json!({ "kind": "comprehension", "material": material("FAKE_OBJ_TWO") }),
    )
    .await;
    assert_eq!(codes(&c), ["C1", "C2"]);
    let v = draft(
        &app,
        &t,
        json!({ "kind": "comprehension", "material": material("FAKE_OBJ_TWO"), "stage": "language",
                "content": [c["objectives"][0]["text"], c["objectives"][1]["text"]] }),
    )
    .await;
    assert_eq!(codes(&v), ["L2", "L1"], "the strategy first");
    let indexes: Vec<i64> = v["objectives"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["index"].as_i64().unwrap())
        .collect();
    assert_eq!(indexes, [1, 0], "positions in the model's output");
    assert_eq!(v["objectives"][1]["supports"], "C2");
    // The first two drafts put a strategy first: the third is asked for another kind.
    let ask = |kind: &'static str| {
        json!({ "kind": kind, "material": material(""), "stage": "language",
                "content": ["I can explain the main point."] })
    };
    draft(&app, &t, ask("activity")).await;
    draft(&app, &t, ask("worksheet")).await;
    let drafts = interactions(&app, "objective_draft").await;
    let third = &drafts[3].3;
    assert_eq!(third["objective_draft"]["strategy_first"], false);
    assert!(third["prompt"]["system"]
        .as_str()
        .unwrap()
        .contains("最近兩次的語言目標都以學習策略為主"));
    assert_eq!(third["inputs"]["recent_categories"], "學習策略、學習策略");
    assert_eq!(drafts[1].3["objective_draft"]["strategy_first"], true);
}

#[tokio::test]
async fn drafting_needs_material_and_respects_ownership() {
    let app = test_app!();
    let t = app.teacher("R-OBJ-10", "general").await;
    for (body, status) in [
        (json!({ "kind": "comprehension" }), StatusCode::BAD_REQUEST),
        (
            json!({ "kind": "comprehension", "material": { "text": "  " } }),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({ "kind": "ask", "material": material("") }),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({ "kind": "comprehension", "material_id": 999_999 }),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({ "kind": "comprehension", "material": material(""), "class_id": 999_999 }),
            StatusCode::NOT_FOUND,
        ),
    ] {
        let (s, v) = app
            .post("/api/objective-drafts", Some(&t), body.clone())
            .await;
        assert_eq!(s, status, "{body} → {v}");
    }
    let (s, _) = app
        .post(
            "/api/objective-drafts",
            None,
            json!({ "kind": "comprehension", "material": material("") }),
        )
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    // A library material with text.
    let (s, m) = app
        .post(
            "/api/materials",
            Some(&t),
            json!({ "kind": "text", "title": "The Umbrella Stand", "source_note": "", "text": passage("") }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{m}");
    let v = draft(
        &app,
        &t,
        json!({ "kind": "worksheet", "material_id": m["id"] }),
    )
    .await;
    assert_eq!(v["status"], "shown");
}

#[tokio::test]
async fn without_ai_there_is_nothing_to_draft() {
    let app = test_app!(|c: &mut manna_api::config::Config| c.ai_provider = AiProviderKind::None);
    let t = app.teacher("R-OBJ-11", "general").await;
    let v = draft(
        &app,
        &t,
        json!({ "kind": "comprehension", "material": material("") }),
    )
    .await;
    assert_eq!(v["status"], "disabled");
    assert_eq!(v["interaction_id"], Value::Null);
    assert!(v["objectives"].as_array().unwrap().is_empty());
}
