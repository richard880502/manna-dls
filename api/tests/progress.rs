//! 產生進度 (docs/API.md「產生進度」) with the fake provider: the steps of a run advance one by
//! one in the polling responses (the 完整學習單 on the worksheet page, a 差異化學習單, a 課堂活動
//! and a 教學套件 in the 備課對話, drafting objectives on /create), failed steps carry the error
//! the page already shows, a block regeneration names its own steps, and rows made before
//! progress was kept have none. `FAKE_PACED` makes every call take 400 ms, so the polls see
//! the steps go by (a poll can be slow while the whole suite runs, so the tests ask for most
//! of the steps in order, not every one).

#[macro_use]
mod common;

use std::time::Duration;

use axum::http::StatusCode;
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

const POLL: Duration = Duration::from_millis(15);

fn keys(progress: &Value) -> Vec<String> {
    progress["steps"]
        .as_array()
        .unwrap_or_else(|| panic!("no steps: {progress}"))
        .iter()
        .map(|s| s["key"].as_str().unwrap().to_string())
        .collect()
}

fn labels(progress: &Value) -> Vec<String> {
    progress["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["label"].as_str().unwrap().to_string())
        .collect()
}

fn statuses(progress: &Value) -> Vec<String> {
    progress["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["status"].as_str().unwrap().to_string())
        .collect()
}

/// What the polls saw: the current step's key each time it changed.
#[derive(Default)]
struct Watch {
    seen: Vec<String>,
    /// Two parts run side by side (a kit): no order to check within a poll.
    side_by_side: bool,
}

impl Watch {
    /// Records a poll. For a run whose steps run one after another, everything before the
    /// current step is over (done, or failed while the run went on) and everything after it
    /// still to come.
    fn see(&mut self, progress: &Value) {
        let Some(i) = progress["current"].as_u64().map(|i| i as usize) else {
            return;
        };
        assert_eq!(progress["status"], "running", "{progress}");
        if self.side_by_side {
            return;
        }
        let st = statuses(progress);
        assert!(
            st[..i].iter().all(|s| s == "done" || s == "failed"),
            "before the current step: {progress}"
        );
        assert!(
            st[i + 1..].iter().all(|s| s == "pending"),
            "after the current step: {progress}"
        );
        let key = progress["steps"][i]["key"].as_str().unwrap().to_string();
        if self.seen.last() != Some(&key) {
            self.seen.push(key);
        }
    }

    /// The polls saw the steps in their order, never going back, and at least `min` of them.
    fn assert_advanced(&self, steps: &[String], min: usize) {
        let at: Vec<usize> = self
            .seen
            .iter()
            .map(|k| {
                steps
                    .iter()
                    .position(|s| s == k)
                    .unwrap_or_else(|| panic!("{k} is not a step of {steps:?}"))
            })
            .collect();
        assert!(
            at.windows(2).all(|w| w[0] < w[1]),
            "went back: {:?} of {steps:?}",
            self.seen
        );
        assert!(
            self.seen.len() >= min,
            "saw only {:?} of {steps:?}",
            self.seen
        );
    }
}

// ---------------------------------------------------------------- the worksheet page

async fn direct(app: &common::TestApp, t: &str, text: &str) -> i64 {
    let (s, v) = app
        .post(
            "/api/worksheets",
            Some(t),
            json!({ "material": { "title": "The Umbrella Stand", "text": text } }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    v["id"].as_i64().unwrap()
}

/// Polls `GET …/jobs` until the run is over, watching its progress; returns the state then.
async fn watch_worksheet(app: &common::TestApp, t: &str, id: i64, watch: &mut Watch) -> Value {
    for _ in 0..2000 {
        let (s, v) = app
            .get(&format!("/api/worksheets/{id}/jobs"), Some(t))
            .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        if !v["progress"].is_null() {
            watch.see(&v["progress"]);
        }
        if v["running"] == false {
            let (s, state) = app.get(&format!("/api/worksheets/{id}"), Some(t)).await;
            assert_eq!(s, StatusCode::OK, "{state}");
            return state;
        }
        tokio::time::sleep(POLL).await;
    }
    panic!("worksheet {id} did not finish generating");
}

#[tokio::test]
async fn the_full_worksheet_goes_through_its_four_steps() {
    let app = test_app!();
    let t = app.teacher("T-PROG-1", "general").await;
    let id = direct(&app, &t, &passage("FAKE_PACED")).await;

    // A worksheet never generated has no progress.
    let (_, fresh) = app.get(&format!("/api/worksheets/{id}"), Some(&t)).await;
    assert!(fresh["progress"].is_null(), "{}", fresh["progress"]);

    let (s, started) = app
        .post(
            &format!("/api/worksheets/{id}/generate"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{started}");
    // The 202 already lists the steps (pictures are off here, so there is no 配圖).
    let p = &started["progress"];
    assert_eq!(
        labels(p),
        ["讀教材", "寫逐段任務", "寫暖身", "寫鷹架", "寫 Exit ticket"]
    );
    assert_eq!(p["status"], "running");
    let steps = keys(p);
    assert_eq!(
        steps,
        ["read", "tasks", "warmup", "scaffolds", "exit_ticket"]
    );

    let mut watch = Watch::default();
    watch.see(p);
    let state = watch_worksheet(&app, &t, id, &mut watch).await;
    // Four model calls of 400 ms each: most of them seen, in order.
    watch.assert_advanced(&steps, 3);

    let p = &state["progress"];
    assert_eq!(p["status"], "done", "{p}");
    assert!(p["current"].is_null());
    assert!(statuses(p).iter().all(|s| s == "done"), "{p}");
    assert_eq!(keys(p), steps);
    // The jobs say the same.
    let (_, jobs) = app
        .get(&format!("/api/worksheets/{id}/jobs"), Some(&t))
        .await;
    assert_eq!(jobs["progress"], state["progress"]);

    // One block again: its own steps.
    let block = state["content"]["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["kind"] == "activity")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (s, v) = app
        .post(
            &format!("/api/worksheets/{id}/blocks/{block}/regenerate"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    assert_eq!(
        labels(&v["progress"]),
        ["讀教材", "重寫這一區的任務", "寫鷹架"]
    );
    let mut again = Watch::default();
    let state = watch_worksheet(&app, &t, id, &mut again).await;
    again.assert_advanced(&keys(&state["progress"]), 2);
    assert_eq!(state["progress"]["status"], "done");
}

#[tokio::test]
async fn failed_steps_say_why_and_skipped_ones_are_left_out() {
    let app = test_app!();
    let t = app.teacher("T-PROG-2", "general").await;
    // The provider does not answer: every model step fails, the scaffolds have nothing to work
    // on and are skipped.
    let id = direct(&app, &t, &passage("FAKE_TIMEOUT")).await;
    let (s, _) = app
        .post(
            &format!("/api/worksheets/{id}/generate"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED);
    let state = watch_worksheet(&app, &t, id, &mut Watch::default()).await;
    let p = &state["progress"];
    assert_eq!(p["status"], "failed", "{p}");
    assert!(p["current"].is_null());
    assert_eq!(keys(p), ["read", "tasks", "warmup", "exit_ticket"]);
    assert_eq!(statuses(p), ["done", "failed", "failed", "failed"]);
    // The step says what the job says.
    let tasks_job = state["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|j| j["step"] == "tasks")
        .unwrap();
    assert_eq!(p["steps"][1]["message"], tasks_job["message"]);
    assert!(p["steps"][1]["message"]
        .as_str()
        .unwrap()
        .contains("沒有回應"));

    // A run that died with the server (its jobs closed by the stale rule, its steps never
    // settled) reads as failed at the step it was on.
    let id = direct(&app, &t, &passage("")).await;
    let (s, _) = app
        .post(
            &format!("/api/worksheets/{id}/generate"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED);
    watch_worksheet(&app, &t, id, &mut Watch::default()).await;
    sqlx::query(
        "UPDATE worksheets SET progress = jsonb_set(jsonb_set(progress, '{finished}', 'false'),
                '{steps,2,status}', '\"running\"')
          WHERE id = $1",
    )
    .bind(id)
    .execute(&app.db)
    .await
    .unwrap();
    let (_, state) = app.get(&format!("/api/worksheets/{id}"), Some(&t)).await;
    let p = &state["progress"];
    assert_eq!(p["status"], "failed", "{p}");
    assert_eq!(p["steps"][2]["status"], "failed");
    assert!(p["steps"][2]["message"].as_str().unwrap().contains("中斷"));
}

#[tokio::test]
async fn rows_from_before_progress_have_none() {
    let app = test_app!();
    let t = app.teacher("T-PROG-3", "general").await;
    let id = direct(&app, &t, &passage("")).await;
    let (s, _) = app
        .post(
            &format!("/api/worksheets/{id}/generate"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED);
    let state = watch_worksheet(&app, &t, id, &mut Watch::default()).await;
    assert_eq!(state["progress"]["status"], "done");
    // What every row made before the migration has.
    sqlx::query("UPDATE worksheets SET progress = NULL WHERE id = $1")
        .bind(id)
        .execute(&app.db)
        .await
        .unwrap();
    let (_, state) = app.get(&format!("/api/worksheets/{id}"), Some(&t)).await;
    assert!(state["progress"].is_null());
    assert_eq!(state["running"], false);
    let (_, jobs) = app
        .get(&format!("/api/worksheets/{id}/jobs"), Some(&t))
        .await;
    assert!(jobs["progress"].is_null());

    // Steps kept for an earlier run are not this run's.
    sqlx::query(
        "UPDATE worksheets SET progress = '{\"run_no\": 99, \"steps\": [{\"key\": \"read\",
                \"label\": \"讀教材\", \"status\": \"done\"}], \"finished\": true}'
          WHERE id = $1",
    )
    .bind(id)
    .execute(&app.db)
    .await
    .unwrap();
    let (_, state) = app.get(&format!("/api/worksheets/{id}"), Some(&t)).await;
    assert!(state["progress"].is_null());

    // A prep turn from before: no progress, the rest as it was.
    let (s, session) = app
        .post(
            "/api/prep-sessions",
            Some(&t),
            json!({ "kind": "activity",
                    "material": { "title": "The Umbrella Stand", "text": passage("") } }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{session}");
    let sid = session["id"].as_i64().unwrap();
    let done = watch_session(&app, &t, sid, &mut Watch::default()).await;
    assert_eq!(last(&done)["status"], "done");
    sqlx::query("UPDATE prep_turns SET progress = NULL")
        .execute(&app.db)
        .await
        .unwrap();
    let (_, v) = app
        .get(&format!("/api/prep-sessions/{sid}"), Some(&t))
        .await;
    let turn = last(&v);
    assert!(turn["progress"].is_null(), "{turn}");
    assert!(turn["result"].is_object());
}

// ---------------------------------------------------------------- the prep conversation

fn last(session: &Value) -> Value {
    session["turns"].as_array().unwrap().last().unwrap().clone()
}

/// Polls the session until its last turn's progress is over (its SIOP 自我檢核 included),
/// watching that progress.
async fn watch_session(app: &common::TestApp, t: &str, id: i64, watch: &mut Watch) -> Value {
    for _ in 0..2000 {
        let (s, v) = app.get(&format!("/api/prep-sessions/{id}"), Some(t)).await;
        assert_eq!(s, StatusCode::OK, "{v}");
        let p = last(&v)["progress"].clone();
        if !p.is_null() {
            watch.see(&p);
        }
        let open = p["status"] == "running" || (p.is_null() && v["running"] == true);
        if !open {
            return v;
        }
        tokio::time::sleep(POLL).await;
    }
    panic!("prep session {id} did not finish");
}

#[tokio::test]
async fn a_differentiated_worksheet_turn_goes_level_by_level_then_the_judge_and_siop() {
    let app = test_app!();
    let t = app.teacher("T-PROG-4", "general").await;
    let (s, session) = app
        .post(
            "/api/prep-sessions",
            Some(&t),
            json!({ "kind": "tiered", "request": "差異化學習單",
                    "material": { "title": "The Umbrella Stand", "text": passage("FAKE_PACED") },
                    "options": { "bloom": ["remember", "understand", "analyze"] } }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{session}");
    let first = last(&session);
    let p = &first["progress"];
    assert_eq!(
        labels(p),
        [
            "讀教材",
            "出『記憶』題與三版支持",
            "出『理解』題與三版支持",
            "出『分析』題與三版支持",
            "判斷 Bloom 層次",
            "SIOP 自我檢核"
        ],
        "{p}"
    );
    let steps = keys(p);
    assert_eq!(
        steps,
        [
            "read",
            "bloom:remember",
            "bloom:understand",
            "bloom:analyze",
            "bloom_check",
            "siop"
        ]
    );
    let mut watch = Watch::default();
    watch.see(p);
    let id = session["id"].as_i64().unwrap();
    let done = watch_session(&app, &t, id, &mut watch).await;
    // Five calls of 400 ms (three levels, the judge, the check): most of them seen, in order.
    watch.assert_advanced(&steps, 3);
    let turn = last(&done);
    assert_eq!(turn["status"], "done");
    assert_eq!(turn["siop_check"]["status"], "done");
    let p = &turn["progress"];
    assert_eq!(p["status"], "done", "{p}");
    assert!(statuses(p).iter().all(|s| s == "done"));
    // The worksheet page shows the same run, without the turn's SIOP 自我檢核.
    let ws = turn["worksheet"]["id"].as_i64().unwrap();
    let (_, state) = app.get(&format!("/api/worksheets/{ws}"), Some(&t)).await;
    assert_eq!(keys(&state["progress"]), steps[..5].to_vec());
}

#[tokio::test]
async fn an_activity_plan_reads_finds_curriculum_plans_and_is_checked() {
    let app = test_app!();
    let t = app.teacher("T-PROG-5", "general").await;
    let (s, session) = app
        .post(
            "/api/prep-sessions",
            Some(&t),
            json!({ "kind": "activity",
                    "material": { "title": "The Umbrella Stand", "text": passage("FAKE_PACED") } }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{session}");
    let p = &last(&session)["progress"];
    assert_eq!(
        labels(p),
        ["讀教材", "找課綱條目", "規劃教學活動", "SIOP 自我檢核"]
    );
    let steps = keys(p);
    assert_eq!(steps, ["read", "curriculum", "plan", "siop"]);
    let mut watch = Watch::default();
    watch.see(p);
    let id = session["id"].as_i64().unwrap();
    let done = watch_session(&app, &t, id, &mut watch).await;
    // The plan call and the check take 400 ms each.
    watch.assert_advanced(&steps, 2);
    assert!(watch.seen.contains(&"plan".to_string()), "{:?}", watch.seen);
    let turn = last(&done);
    assert_eq!(turn["progress"]["status"], "done", "{}", turn["progress"]);

    // A plan that fails twice: the plan step failed, with the turn's error; nothing after it.
    let (s, session) = app
        .post(
            "/api/prep-sessions",
            Some(&t),
            json!({ "kind": "activity",
                    "material": { "title": "The Umbrella Stand", "text": passage("FAKE_TIMEOUT") } }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{session}");
    let id = session["id"].as_i64().unwrap();
    let done = watch_session(&app, &t, id, &mut Watch::default()).await;
    let turn = last(&done);
    assert_eq!(turn["status"], "failed");
    let p = &turn["progress"];
    assert_eq!(p["status"], "failed", "{p}");
    assert_eq!(keys(p), ["read", "curriculum", "plan"]);
    assert_eq!(statuses(p), ["done", "done", "failed"]);
    assert_eq!(p["steps"][2]["message"], turn["error"]);
}

#[tokio::test]
async fn a_kit_shows_its_plan_and_its_worksheet_in_one_list() {
    let app = test_app!();
    let t = app.teacher("T-PROG-6", "general").await;
    let (s, session) = app
        .post(
            "/api/prep-sessions",
            Some(&t),
            json!({ "kind": "kit", "request": "教學套件",
                    "material": { "title": "The Umbrella Stand", "text": passage("") },
                    "options": { "bloom": ["remember", "create"] } }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{session}");
    let id = session["id"].as_i64().unwrap();
    let mut watch = Watch {
        side_by_side: true,
        ..Watch::default()
    };
    let done = watch_session(&app, &t, id, &mut watch).await;
    let turn = last(&done);
    assert_eq!(turn["status"], "done", "{turn}");
    let p = &turn["progress"];
    // 讀教材 once for both parts.
    assert_eq!(
        keys(p),
        [
            "read",
            "curriculum",
            "plan",
            "bloom:remember",
            "bloom:create",
            "bloom_check",
            "siop"
        ],
        "{p}"
    );
    assert_eq!(p["status"], "done");
}

// ---------------------------------------------------------------- drafting objectives (/create)

#[tokio::test]
async fn drafting_objectives_reports_its_steps_while_the_page_waits() {
    let app = test_app!();
    let t = app.teacher("T-PROG-7", "general").await;
    let key = "draft-0123456789";
    let body = json!({ "kind": "comprehension", "progress_id": key,
                       "material": { "title": "The Umbrella Stand", "text": passage("FAKE_PACED") } });
    let draft = app.post("/api/objective-drafts", Some(&t), body);
    let watch = async {
        let mut watch = Watch::default();
        for _ in 0..2000 {
            let (s, v) = app
                .get(&format!("/api/objective-drafts/progress/{key}"), Some(&t))
                .await;
            assert_eq!(s, StatusCode::OK, "{v}");
            let p = &v["progress"];
            if !p.is_null() {
                watch.see(p);
                if p["status"] != "running" {
                    return (watch, p.clone());
                }
            }
            tokio::time::sleep(POLL).await;
        }
        panic!("the drafting did not finish");
    };
    let ((s, result), (watch, p)) = tokio::join!(draft, watch);
    assert_eq!(s, StatusCode::OK, "{result}");
    assert_eq!(result["status"], "shown", "{result}");
    assert_eq!(
        labels(&p),
        ["讀教材", "找課綱條目", "草擬學習目標", "檢查目標"]
    );
    assert_eq!(p["status"], "done");
    let steps = keys(&p);
    watch.assert_advanced(&steps, 2);
    assert!(
        watch.seen.contains(&"draft".to_string()),
        "{:?}",
        watch.seen
    );
    assert!(
        watch.seen.contains(&"check".to_string()),
        "{:?}",
        watch.seen
    );

    // Another teacher's key finds nothing; a bad key is a 400.
    let other = app.teacher("T-PROG-8", "general").await;
    let (s, v) = app
        .get(
            &format!("/api/objective-drafts/progress/{key}"),
            Some(&other),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert!(v["progress"].is_null());
    let (s, _) = app
        .get("/api/objective-drafts/progress/bad", Some(&t))
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = app
        .post(
            "/api/objective-drafts",
            Some(&t),
            json!({ "kind": "comprehension", "progress_id": "no spaces allowed",
                    "material": { "text": passage("") } }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_poll_reads_progress_only_for_turns_the_page_can_still_show() {
    let app = test_app!();
    let t = app.teacher("T-PROG-9", "general").await;
    let (s, session) = app
        .post(
            "/api/prep-sessions",
            Some(&t),
            json!({ "kind": "worksheet",
                    "material": { "title": "The Umbrella Stand", "text": passage("") } }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{session}");
    let id = session["id"].as_i64().unwrap();
    watch_session(&app, &t, id, &mut Watch::default()).await;
    for kind in ["comprehension", "activity"] {
        let (s, v) = app
            .post(
                &format!("/api/prep-sessions/{id}/turns"),
                Some(&t),
                json!({ "kind": kind }),
            )
            .await;
        assert_eq!(s, StatusCode::ACCEPTED, "{v}");
        watch_session(&app, &t, id, &mut Watch::default()).await;
    }
    // Just finished: a page that watched them can still show how they ended.
    let (_, v) = app.get(&format!("/api/prep-sessions/{id}"), Some(&t)).await;
    let turns = v["turns"].as_array().unwrap();
    assert_eq!(turns.len(), 3);
    assert!(
        turns.iter().all(|t| t["progress"]["status"] == "done"),
        "{v}"
    );

    // Long over: no progress, and nothing is read for it.
    sqlx::query(
        "UPDATE prep_turns SET finished_at = now() - interval '2 hours',
                created_at = now() - interval '2 hours'",
    )
    .execute(&app.db)
    .await
    .unwrap();
    sqlx::query("UPDATE siop_checks SET created_at = now() - interval '2 hours'")
        .execute(&app.db)
        .await
        .unwrap();
    let (_, v) = app.get(&format!("/api/prep-sessions/{id}"), Some(&t)).await;
    let turns = v["turns"].as_array().unwrap();
    assert!(turns.iter().all(|t| t["progress"].is_null()), "{v}");
    // The rest of each turn is as before.
    assert!(turns.iter().all(|t| t["status"] == "done"));
    assert!(turns[0]["worksheet"]["id"].is_i64());

    // A running turn (FAKE_SLOW: its plan call waits 3 s) has progress; the old ones still not.
    let (s, v) = app
        .post(
            &format!("/api/prep-sessions/{id}/turns"),
            Some(&t),
            json!({ "kind": "activity", "request": "再規劃一次 FAKE_SLOW" }),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    let (_, v) = app.get(&format!("/api/prep-sessions/{id}"), Some(&t)).await;
    let turns = v["turns"].as_array().unwrap();
    assert_eq!(turns.len(), 4);
    assert!(turns[..3].iter().all(|t| t["progress"].is_null()), "{v}");
    assert_eq!(turns[3]["progress"]["status"], "running", "{}", turns[3]);
    watch_session(&app, &t, id, &mut Watch::default()).await;
}
