//! The AI job queue (docs/AI_REQUEST_CONTROL.md): dispatch order and fairness, per-teacher
//! limits, fencing of a lost lease, recovery after a restart, cancelling, and the queue with the
//! fake provider (slowed with `fake_latency_ms`): worksheets and prep turns run through the
//! dispatcher, never more than `ai_max_running_jobs` at once.

#[macro_use]
mod common;

use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use axum::http::StatusCode;
use manna_api::config::Config;
use manna_api::error::AppError;
use manna_api::queue::{self, NewJob};
use serde_json::{json, Value};
use sqlx::PgPool;

const LEASE: Duration = Duration::from_secs(90);

async fn teacher_id(db: &PgPool, research_id: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM teachers WHERE research_id = $1")
        .bind(research_id)
        .fetch_one(db)
        .await
        .unwrap()
}

async fn pi_id(db: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT id FROM teachers WHERE role = 'pi' LIMIT 1")
        .fetch_one(db)
        .await
        .unwrap()
}

static NEXT_REF: AtomicI64 = AtomicI64::new(1);

async fn add(db: &PgPool, teacher: i64, priority: u8) -> i64 {
    add_kind(db, "test_job", teacher, priority).await
}

async fn add_kind(db: &PgPool, kind: &str, teacher: i64, priority: u8) -> i64 {
    queue::enqueue(
        db,
        NewJob {
            kind,
            teacher_id: teacher,
            priority,
            params: json!({}),
            ref_kind: "test",
            ref_id: NEXT_REF.fetch_add(1, Ordering::SeqCst),
        },
        1000,
    )
    .await
    .unwrap()
}

async fn status(db: &PgPool, id: i64) -> String {
    sqlx::query_scalar("SELECT status FROM ai_jobs WHERE id = $1")
        .bind(id)
        .fetch_one(db)
        .await
        .unwrap()
}

// ---------------------------------------------------------------- order and fairness

#[tokio::test]
async fn jobs_are_claimed_by_priority_then_arrival() {
    let app = test_app!();
    app.teacher("T-Q-ORDER", "general").await;
    let t = teacher_id(&app.db, "T-Q-ORDER").await;
    let background = add(&app.db, t, 2).await;
    let general = add(&app.db, t, 1).await;
    let interactive = add(&app.db, t, 0).await;
    let interactive_later = add(&app.db, t, 0).await;
    let mut order = Vec::new();
    while let Some(job) = queue::claim_next(&app.db, 99, LEASE).await.unwrap() {
        order.push(job.id);
    }
    assert_eq!(order, [interactive, interactive_later, general, background]);
}

#[tokio::test]
async fn a_teacher_with_one_job_does_not_wait_for_anothers_five() {
    let app = test_app!();
    app.teacher("T-Q-A", "general").await;
    app.teacher("T-Q-B", "general").await;
    let (a, b) = (
        teacher_id(&app.db, "T-Q-A").await,
        teacher_id(&app.db, "T-Q-B").await,
    );
    let mut a_jobs = Vec::new();
    for _ in 0..5 {
        a_jobs.push(add(&app.db, a, 1).await);
    }
    let b_job = add(&app.db, b, 1).await;
    // Per-teacher limit high: only fairness decides.
    let mut order = Vec::new();
    while let Some(job) = queue::claim_next(&app.db, 99, LEASE).await.unwrap() {
        order.push(job.id);
    }
    let b_at = order.iter().position(|id| *id == b_job).unwrap();
    let fifth = order.iter().position(|id| *id == a_jobs[4]).unwrap();
    assert!(b_at < fifth, "{order:?}");
    // Runs-so-far decide, so B goes second.
    assert_eq!(b_at, 1, "{order:?}");
}

#[tokio::test]
async fn the_per_teacher_running_cap_holds_and_the_pi_is_exempt() {
    let app = test_app!();
    app.teacher("T-Q-CAP", "general").await;
    let t = teacher_id(&app.db, "T-Q-CAP").await;
    let pi = pi_id(&app.db).await;
    for _ in 0..4 {
        add(&app.db, t, 1).await;
    }
    assert!(queue::claim_next(&app.db, 2, LEASE)
        .await
        .unwrap()
        .is_some());
    assert!(queue::claim_next(&app.db, 2, LEASE)
        .await
        .unwrap()
        .is_some());
    assert!(
        queue::claim_next(&app.db, 2, LEASE)
            .await
            .unwrap()
            .is_none(),
        "a third job of the same teacher waits"
    );
    for _ in 0..3 {
        add(&app.db, pi, 1).await;
    }
    for _ in 0..3 {
        let job = queue::claim_next(&app.db, 2, LEASE).await.unwrap().unwrap();
        assert_eq!(job.teacher_id, pi);
    }
}

#[tokio::test]
async fn the_queued_cap_rejects_even_concurrent_requests() {
    let app = test_app!();
    app.teacher("T-Q-QCAP", "general").await;
    let t = teacher_id(&app.db, "T-Q-QCAP").await;
    let mut tasks = Vec::new();
    for _ in 0..8 {
        let db = app.db.clone();
        tasks.push(tokio::spawn(async move {
            queue::enqueue(
                &db,
                NewJob {
                    kind: "test_job",
                    teacher_id: t,
                    priority: 1,
                    params: json!({}),
                    ref_kind: "test",
                    ref_id: NEXT_REF.fetch_add(1, Ordering::SeqCst),
                },
                3,
            )
            .await
        }));
    }
    let mut ok = 0;
    for task in tasks {
        match task.await.unwrap() {
            Ok(_) => ok += 1,
            Err(AppError::Conflict(m)) => assert!(m.contains("排隊中的工作太多"), "{m}"),
            Err(e) => panic!("{e}"),
        }
    }
    assert_eq!(ok, 3, "exactly the cap gets in, whatever the interleaving");
}

#[tokio::test]
async fn two_claimers_never_get_the_same_job() {
    let app = test_app!();
    app.teacher("T-Q-RACE", "general").await;
    let t = teacher_id(&app.db, "T-Q-RACE").await;
    for _ in 0..20 {
        add(&app.db, t, 1).await;
    }
    let mut tasks = Vec::new();
    for _ in 0..4 {
        let db = app.db.clone();
        tasks.push(tokio::spawn(async move {
            let mut mine = Vec::new();
            while let Some(job) = queue::claim_next(&db, 99, LEASE).await.unwrap() {
                mine.push(job.id);
            }
            mine
        }));
    }
    let mut all = Vec::new();
    for task in tasks {
        all.extend(task.await.unwrap());
    }
    let total = all.len();
    all.sort();
    all.dedup();
    assert_eq!(total, 20);
    assert_eq!(all.len(), 20, "a job was claimed twice");
}

#[tokio::test]
async fn one_active_job_per_ref() {
    let app = test_app!();
    app.teacher("T-Q-REF", "general").await;
    let t = teacher_id(&app.db, "T-Q-REF").await;
    let new = || NewJob {
        kind: "test_job",
        teacher_id: t,
        priority: 0,
        params: json!({}),
        ref_kind: "prep_turn",
        ref_id: 424242,
    };
    let first = queue::enqueue(&app.db, new(), 10).await.unwrap();
    assert!(matches!(
        queue::enqueue(&app.db, new(), 10).await,
        Err(AppError::Conflict(_))
    ));
    let job = queue::claim_next(&app.db, 2, LEASE).await.unwrap().unwrap();
    assert!(matches!(
        queue::enqueue(&app.db, new(), 10).await,
        Err(AppError::Conflict(_))
    ));
    assert!(queue::finish(&app.db, first, job.attempts, Ok(()))
        .await
        .unwrap());
    queue::enqueue(&app.db, new(), 10).await.unwrap();
}

// ---------------------------------------------------------------- lease fencing and recovery

#[tokio::test]
async fn an_old_worker_cannot_touch_a_reclaimed_job() {
    let app = test_app!();
    app.teacher("T-Q-FENCE", "general").await;
    let t = teacher_id(&app.db, "T-Q-FENCE").await;
    let id = add(&app.db, t, 1).await;
    let old = queue::claim_next(&app.db, 2, LEASE).await.unwrap().unwrap();
    assert_eq!(old.attempts, 1);
    // The lease runs out and the job is recovered (replayable) and claimed again.
    sqlx::query("UPDATE ai_jobs SET lease_until = now() - interval '1 second' WHERE id = $1")
        .bind(id)
        .execute(&app.db)
        .await
        .unwrap();
    let recovery = queue::recover_expired(&app.db, 3, &["test_job"])
        .await
        .unwrap();
    assert_eq!((recovery.requeued, recovery.failed.len()), (1, 0));
    let new = queue::claim_next(&app.db, 2, LEASE).await.unwrap().unwrap();
    assert_eq!((new.id, new.attempts), (id, 2));
    // The first worker neither renews nor finishes the new run.
    assert!(!queue::renew_lease(&app.db, id, old.attempts, LEASE)
        .await
        .unwrap());
    assert!(!queue::finish(&app.db, id, old.attempts, Err("boom"))
        .await
        .unwrap());
    assert!(!queue::finish(&app.db, id, old.attempts, Ok(()))
        .await
        .unwrap());
    assert_eq!(status(&app.db, id).await, "running");
    assert!(queue::renew_lease(&app.db, id, new.attempts, LEASE)
        .await
        .unwrap());
    assert!(queue::finish(&app.db, id, new.attempts, Ok(()))
        .await
        .unwrap());
    assert_eq!(status(&app.db, id).await, "done");
    // Finished: nothing renews it any more.
    assert!(!queue::renew_lease(&app.db, id, new.attempts, LEASE)
        .await
        .unwrap());
}

#[tokio::test]
async fn ahead_counts_only_queued_jobs_and_is_none_otherwise() {
    let app = test_app!();
    app.teacher("T-Q-AHEAD", "general").await;
    let t = teacher_id(&app.db, "T-Q-AHEAD").await;
    let first = add(&app.db, t, 1).await;
    let second = add(&app.db, t, 1).await;
    let urgent = add(&app.db, t, 0).await;
    assert_eq!(queue::ahead(&app.db, first).await.unwrap(), Some(1));
    assert_eq!(queue::ahead(&app.db, second).await.unwrap(), Some(2));
    assert_eq!(queue::ahead(&app.db, urgent).await.unwrap(), Some(0));
    let claimed = queue::claim_next(&app.db, 5, LEASE).await.unwrap().unwrap();
    assert_eq!(claimed.id, urgent);
    assert_eq!(
        queue::ahead(&app.db, urgent).await.unwrap(),
        None,
        "running"
    );
    queue::finish(&app.db, urgent, claimed.attempts, Ok(()))
        .await
        .unwrap();
    assert_eq!(queue::ahead(&app.db, urgent).await.unwrap(), None, "done");
    assert_eq!(
        queue::ahead(&app.db, 987654321).await.unwrap(),
        None,
        "unknown"
    );
    assert_eq!(queue::eta_secs(4, 2, Some(30.0)), Some(60));
    assert_eq!(queue::eta_secs(4, 2, None), None);
}

#[tokio::test]
async fn cancelling_is_for_the_owner_while_the_job_is_queued() {
    let app = test_app!();
    let owner = app.teacher("T-Q-CANCEL", "general").await;
    let other = app.teacher("T-Q-CANCEL2", "general").await;
    let t = teacher_id(&app.db, "T-Q-CANCEL").await;
    let id = add(&app.db, t, 1).await;
    let (s, _) = app
        .delete(&format!("/api/ai-jobs/{id}"), Some(&other))
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert_eq!(status(&app.db, id).await, "queued");
    let (s, v) = app
        .delete(&format!("/api/ai-jobs/{id}"), Some(&owner))
        .await;
    assert_eq!(s, StatusCode::NO_CONTENT, "{v}");
    assert_eq!(status(&app.db, id).await, "cancelled");
    assert_eq!(queue::ahead(&app.db, id).await.unwrap(), None);
    // A job that started cannot be cancelled.
    let running = add(&app.db, t, 1).await;
    queue::claim_next(&app.db, 2, LEASE).await.unwrap().unwrap();
    let (s, v) = app
        .delete(&format!("/api/ai-jobs/{running}"), Some(&owner))
        .await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("取消"),
        "{v}"
    );
}

fn queued(c: &mut Config) {
    c.ai_queue_enabled = true;
}

const TEXT: &str = "Mrs. Lin put an umbrella stand outside her shop on a rainy Monday. A sign said, “Take one if you need it.”\n\nHer neighbors laughed at the idea. They were sure that every umbrella would be gone within a week.\n\nIn the first month, twelve of the thirty umbrellas did disappear. However, something unexpected followed.\n\nToday the stand holds more umbrellas than it did at the start. Mrs. Lin says trust is like an umbrella.";

async fn new_worksheet(app: &common::TestApp, t: &str) -> i64 {
    let (s, v) = app
        .post(
            "/api/worksheets",
            Some(t),
            json!({ "material": { "title": "The Umbrella Stand", "text": TEXT } }),
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
    v
}

async fn job_of(db: &PgPool, ref_kind: &str, ref_id: i64) -> (i64, String, i32) {
    sqlx::query_as(
        "SELECT id, status, attempts FROM ai_jobs WHERE ref_kind = $1 AND ref_id = $2
          ORDER BY id DESC LIMIT 1",
    )
    .bind(ref_kind)
    .bind(ref_id)
    .fetch_one(db)
    .await
    .unwrap()
}

#[tokio::test]
async fn recovery_requeues_replayable_work_and_fails_the_rest_with_their_rows() {
    let app = test_app!(queued);
    let t = app.teacher("T-Q-RECOVER", "general").await;
    let tid = teacher_id(&app.db, "T-Q-RECOVER").await;

    // A worksheet run lost on its second attempt: failed, its steps closed for 再試一次.
    let lost = new_worksheet(&app, &t).await;
    let started = generate(&app, &t, lost).await;
    assert_eq!(started["progress"]["status"], "running");
    let (job, state, _) = job_of(&app.db, "worksheet", lost).await;
    assert_eq!(state, "queued");
    sqlx::query(
        "UPDATE ai_jobs SET status = 'running', attempts = 2, started_at = now(),
                lease_until = now() - interval '1 second' WHERE id = $1",
    )
    .bind(job)
    .execute(&app.db)
    .await
    .unwrap();

    // A worksheet run lost on its first attempt: queued again.
    let again = new_worksheet(&app, &t).await;
    generate(&app, &t, again).await;
    let (again_job, ..) = job_of(&app.db, "worksheet", again).await;
    sqlx::query(
        "UPDATE ai_jobs SET status = 'running', attempts = 1, started_at = now(),
                lease_until = now() - interval '1 second' WHERE id = $1",
    )
    .bind(again_job)
    .execute(&app.db)
    .await
    .unwrap();

    // A prep turn lost on its second attempt fails with the message.
    let (s, v) = app
        .post(
            "/api/prep-sessions",
            Some(&t),
            json!({ "kind": "chat", "request": "哈囉" }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    let session = v["id"].as_i64().unwrap();
    let turn: i64 = sqlx::query_scalar("SELECT id FROM prep_turns WHERE session_id = $1")
        .bind(session)
        .fetch_one(&app.db)
        .await
        .unwrap();
    let (turn_job, state, _) = job_of(&app.db, "prep_turn", turn).await;
    assert_eq!(state, "queued");
    sqlx::query(
        "UPDATE ai_jobs SET status = 'running', attempts = 2, started_at = now(),
                lease_until = now() - interval '1 second' WHERE id = $1",
    )
    .bind(turn_job)
    .execute(&app.db)
    .await
    .unwrap();

    // A job of a kind with no handler fails too.
    let stray = add_kind(&app.db, "stray_kind", tid, 1).await;
    queue::claim_next(&app.db, 99, LEASE).await.unwrap();
    sqlx::query("UPDATE ai_jobs SET lease_until = now() - interval '1 second' WHERE id = $1")
        .bind(stray)
        .execute(&app.db)
        .await
        .unwrap();

    let (requeued, failed) = queue::dispatcher::recover(&app.state).await.unwrap();
    assert_eq!((requeued, failed), (1, 3));
    assert_eq!(job_of(&app.db, "worksheet", again).await.1, "queued");
    assert_eq!(job_of(&app.db, "worksheet", lost).await.1, "failed");
    assert_eq!(job_of(&app.db, "prep_turn", turn).await.1, "failed");

    let (_, jobs) = app
        .get(&format!("/api/worksheets/{lost}/jobs"), Some(&t))
        .await;
    assert_eq!(jobs["running"], false, "{jobs}");
    assert!(jobs["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .all(|j| j["status"] == "done" && j["outcome"] == "error"));
    assert_eq!(jobs["progress"]["status"], "failed", "{jobs}");
    let (_, session_state) = app
        .get(&format!("/api/prep-sessions/{session}"), Some(&t))
        .await;
    assert_eq!(
        session_state["turns"][0]["status"], "failed",
        "{session_state}"
    );
    assert_eq!(
        session_state["turns"][0]["error"],
        "伺服器重新啟動，這次沒有完成。請按再試一次。"
    );
}

// ---------------------------------------------------------------- the queue at work

fn load_test(c: &mut Config) {
    c.ai_queue_enabled = true;
    c.ai_max_running_jobs = 2;
    c.ai_max_running_per_teacher = 2;
    c.fake_latency_ms = 120;
}

async fn running_count(db: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM ai_jobs WHERE status = 'running'")
        .fetch_one(db)
        .await
        .unwrap()
}

#[tokio::test]
async fn ten_worksheets_all_finish_and_never_more_than_two_run() {
    let app = test_app!(load_test);
    let mut tokens = Vec::new();
    for i in 0..5 {
        tokens.push(app.teacher(&format!("T-Q-LOAD-{i}"), "general").await);
    }
    let mut worksheets = Vec::new();
    for t in &tokens {
        for _ in 0..2 {
            let id = new_worksheet(&app, t).await;
            generate(&app, t, id).await;
            worksheets.push((t.clone(), id));
        }
    }
    // Everything is queued before the dispatcher starts; the 202s already carry the position.
    let queued: i64 = sqlx::query_scalar("SELECT count(*) FROM ai_jobs WHERE status = 'queued'")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(queued, 10);
    let dispatcher = tokio::spawn(queue::dispatcher::run(app.state.clone()));
    let mut peak = 0;
    for _ in 0..1500 {
        peak = peak.max(running_count(&app.db).await);
        assert!(app.state.queue.running() <= 2);
        let done: i64 = sqlx::query_scalar("SELECT count(*) FROM ai_jobs WHERE status = 'done'")
            .fetch_one(&app.db)
            .await
            .unwrap();
        if done == 10 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(peak <= 2, "peak {peak}");
    assert!(peak >= 1);
    for (t, id) in &worksheets {
        let (_, jobs) = app
            .get(&format!("/api/worksheets/{id}/jobs"), Some(t))
            .await;
        assert_eq!(jobs["running"], false, "{jobs}");
        assert!(
            jobs["jobs"]
                .as_array()
                .unwrap()
                .iter()
                .all(|j| j["outcome"] == "shown" || j["outcome"] == "skipped"),
            "{jobs}"
        );
        assert!(jobs["progress"]["queue"].is_null());
        let (_, state) = app.get(&format!("/api/worksheets/{id}"), Some(t)).await;
        assert!(state["content"]["blocks"]
            .as_array()
            .is_some_and(|b| !b.is_empty()));
    }
    let logged: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM usage_logs WHERE event = 'ai_job_finished'
            AND payload ->> 'kind' = 'worksheet_run' AND payload ->> 'status' = 'done'",
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(logged, 10);
    app.state.queue.stop();
    dispatcher.await.unwrap();
}

#[tokio::test]
async fn a_chat_turn_behind_worksheets_runs_first_and_the_page_sees_the_queue() {
    let app = test_app!(|c: &mut Config| {
        c.ai_queue_enabled = true;
        c.ai_max_running_jobs = 1;
        c.ai_max_running_per_teacher = 5;
        c.fake_latency_ms = 60;
    });
    let t = app.teacher("T-Q-PRIO", "general").await;
    let mut worksheets = Vec::new();
    for _ in 0..3 {
        let id = new_worksheet(&app, &t).await;
        generate(&app, &t, id).await;
        worksheets.push(id);
    }
    // The second worksheet has the first ahead of it, in the answer of the page.
    let (_, jobs) = app
        .get(&format!("/api/worksheets/{}/jobs", worksheets[1]), Some(&t))
        .await;
    assert_eq!(jobs["progress"]["queue"]["ahead"], 1, "{jobs}");
    assert!(jobs["progress"]["queue"]["job_id"].as_i64().is_some());
    assert!(
        jobs["progress"]["queue"]["eta_secs"].is_null(),
        "no data yet"
    );

    // A chat turn made last still goes first (priority 0), and its progress says so.
    let (s, session) = app
        .post(
            "/api/prep-sessions",
            Some(&t),
            json!({ "kind": "chat", "request": "哈囉" }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{session}");
    let turn = &session["turns"][0];
    assert_eq!(turn["status"], "running");
    assert_eq!(turn["progress"]["queue"]["ahead"], 0, "{turn}");
    let id = session["id"].as_i64().unwrap();
    // Backdated far past the stale limit: a waiting turn is not failed for waiting.
    sqlx::query(
        "UPDATE prep_turns SET created_at = now() - interval '3 days' WHERE session_id = $1",
    )
    .bind(id)
    .execute(&app.db)
    .await
    .unwrap();
    let (_, still) = app.get(&format!("/api/prep-sessions/{id}"), Some(&t)).await;
    assert_eq!(still["turns"][0]["status"], "running", "{still}");

    let dispatcher = tokio::spawn(queue::dispatcher::run(app.state.clone()));
    for _ in 0..1500 {
        let open: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM ai_jobs WHERE status IN ('queued', 'running')",
        )
        .fetch_one(&app.db)
        .await
        .unwrap();
        if open == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let order: Vec<String> = sqlx::query_scalar(
        "SELECT kind FROM ai_jobs WHERE status = 'done' ORDER BY started_at, id",
    )
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert_eq!(order.len(), 4, "{order:?}");
    assert_eq!(order[0], "prep_turn", "{order:?}");
    let (_, done) = app.get(&format!("/api/prep-sessions/{id}"), Some(&t)).await;
    assert_eq!(done["turns"][0]["status"], "done", "{done}");
    app.state.queue.stop();
    dispatcher.await.unwrap();
}

#[tokio::test]
async fn cancelling_a_queued_worksheet_run_and_a_queued_turn_closes_their_rows() {
    let app = test_app!(queued);
    let t = app.teacher("T-Q-CANCEL-ROWS", "general").await;
    let ws = new_worksheet(&app, &t).await;
    generate(&app, &t, ws).await;
    let (job, ..) = job_of(&app.db, "worksheet", ws).await;
    let (s, v) = app.delete(&format!("/api/ai-jobs/{job}"), Some(&t)).await;
    assert_eq!(s, StatusCode::NO_CONTENT, "{v}");
    let (_, jobs) = app
        .get(&format!("/api/worksheets/{ws}/jobs"), Some(&t))
        .await;
    assert_eq!(jobs["running"], false, "{jobs}");
    assert_eq!(jobs["progress"]["status"], "failed", "{jobs}");
    // The worksheet can be generated again.
    generate(&app, &t, ws).await;

    let (s, session) = app
        .post(
            "/api/prep-sessions",
            Some(&t),
            json!({ "kind": "chat", "request": "哈囉" }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{session}");
    let id = session["id"].as_i64().unwrap();
    let turn = session["turns"][0]["id"].as_i64().unwrap();
    let (turn_job, ..) = job_of(&app.db, "prep_turn", turn).await;
    let (s, _) = app
        .delete(&format!("/api/ai-jobs/{turn_job}"), Some(&t))
        .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (_, state) = app.get(&format!("/api/prep-sessions/{id}"), Some(&t)).await;
    assert_eq!(state["turns"][0]["status"], "failed", "{state}");
    assert_eq!(state["turns"][0]["error"], "已取消排隊。");
}

#[tokio::test]
async fn the_queue_refuses_when_too_many_jobs_wait() {
    let app = test_app!(|c: &mut Config| {
        c.ai_queue_enabled = true;
        c.ai_max_queued_per_teacher = 2;
    });
    let t = app.teacher("T-Q-TOOMANY", "general").await;
    for _ in 0..2 {
        let id = new_worksheet(&app, &t).await;
        generate(&app, &t, id).await;
    }
    let id = new_worksheet(&app, &t).await;
    let (s, v) = app
        .post(
            &format!("/api/worksheets/{id}/generate"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("排隊中的工作太多"),
        "{v}"
    );
    // Nothing was left half-made: no job rows for the refused run.
    let rows: i64 =
        sqlx::query_scalar("SELECT count(*) FROM worksheet_jobs WHERE worksheet_id = $1")
            .bind(id)
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(rows, 0);
}

#[tokio::test]
async fn a_replayed_worksheet_run_starts_over_and_finishes() {
    let app = test_app!(queued);
    let t = app.teacher("T-Q-REPLAY", "general").await;
    let ws = new_worksheet(&app, &t).await;
    generate(&app, &t, ws).await;
    let (job, ..) = job_of(&app.db, "worksheet", ws).await;
    // The first attempt died half way: its jobs were running, its lease ran out.
    sqlx::query("UPDATE worksheet_jobs SET status = 'running', phase = 'x' WHERE worksheet_id = $1 AND step = 'tasks'")
        .bind(ws)
        .execute(&app.db)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE ai_jobs SET status = 'running', attempts = 1, started_at = now(),
                lease_until = now() - interval '1 second' WHERE id = $1",
    )
    .bind(job)
    .execute(&app.db)
    .await
    .unwrap();
    assert_eq!(
        queue::dispatcher::recover(&app.state).await.unwrap(),
        (1, 0)
    );
    let dispatcher = tokio::spawn(queue::dispatcher::run(app.state.clone()));
    for _ in 0..1500 {
        if job_of(&app.db, "worksheet", ws).await.1 == "done" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let (_, state, attempts) = job_of(&app.db, "worksheet", ws).await;
    assert_eq!((state.as_str(), attempts), ("done", 2));
    let (_, jobs) = app
        .get(&format!("/api/worksheets/{ws}/jobs"), Some(&t))
        .await;
    assert_eq!(jobs["running"], false, "{jobs}");
    assert_eq!(jobs["progress"]["status"], "done", "{jobs}");
    app.state.queue.stop();
    dispatcher.await.unwrap();
}

async fn wait_session(app: &common::TestApp, t: &str, id: i64) -> Value {
    for _ in 0..1500 {
        let (s, v) = app.get(&format!("/api/prep-sessions/{id}"), Some(t)).await;
        assert_eq!(s, StatusCode::OK, "{v}");
        if v["running"] == false {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("prep session {id} is still running");
}

/// A worksheet turn (one queue entry: its worksheet's run) and a kit (two entries that never
/// wait for each other) both finish with a single running slot.
#[tokio::test]
async fn worksheet_turns_and_kits_finish_through_the_queue_without_deadlock() {
    let app = test_app!(|c: &mut Config| {
        c.ai_queue_enabled = true;
        c.ai_max_running_jobs = 1;
        c.ai_max_running_per_teacher = 1;
        c.fake_latency_ms = 20;
    });
    let dispatcher = tokio::spawn(queue::dispatcher::run(app.state.clone()));
    let t = app.teacher("T-Q-KIT", "general").await;
    let (s, v) = app
        .post(
            "/api/prep-sessions",
            Some(&t),
            json!({ "kind": "worksheet", "material": { "title": "The Umbrella Stand", "text": TEXT } }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    let id = v["id"].as_i64().unwrap();
    let done = wait_session(&app, &t, id).await;
    assert_eq!(done["turns"][0]["status"], "done", "{done}");
    let turn: i64 = sqlx::query_scalar("SELECT id FROM prep_turns WHERE session_id = $1")
        .bind(id)
        .fetch_one(&app.db)
        .await
        .unwrap();
    let own: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ai_jobs WHERE ref_kind = 'prep_turn' AND ref_id = $1",
    )
    .bind(turn)
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(
        own, 0,
        "a worksheet turn is its worksheet's run, nothing else"
    );

    let (s, v) = app
        .post(
            &format!("/api/prep-sessions/{id}/turns"),
            Some(&t),
            json!({ "kind": "kit", "request": "" }),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    let done = wait_session(&app, &t, id).await;
    let kit = done["turns"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(kit["kind"], "kit");
    assert_eq!(kit["status"], "done", "{kit}");
    let kinds: Vec<String> =
        sqlx::query_scalar("SELECT kind FROM ai_jobs WHERE status = 'done' ORDER BY id")
            .fetch_all(&app.db)
            .await
            .unwrap();
    assert_eq!(
        kinds.iter().filter(|k| *k == "worksheet_run").count(),
        2,
        "{kinds:?}"
    );
    assert_eq!(
        kinds.iter().filter(|k| *k == "prep_turn").count(),
        1,
        "{kinds:?}"
    );
    app.state.queue.stop();
    dispatcher.await.unwrap();
}

// ---------------------------------------------------------------- the other job kinds
//
// block_regenerate, block_revise, sop_step, template_extract and siop_check (memory_detect stays
// direct, see docs/AI_REQUEST_CONTROL.md §10). Each is queued with the feature row it reports
// into; the dispatcher is not started here, the tests step it with `dispatch_ready`.

const MSG_LOST: &str = "伺服器重新啟動，這次沒有完成。請按再試一次。";
const MSG_CANCELLED: &str = "已取消排隊。";

#[derive(Clone, Copy, Debug)]
enum Kind {
    BlockRegenerate,
    BlockRevise,
    SopStep,
    TemplateExtract,
    SiopCheck,
}

const KINDS: [Kind; 5] = [
    Kind::BlockRegenerate,
    Kind::BlockRevise,
    Kind::SopStep,
    Kind::TemplateExtract,
    Kind::SiopCheck,
];

impl Kind {
    fn job_kind(self) -> &'static str {
        match self {
            Kind::BlockRegenerate => "block_regenerate",
            Kind::BlockRevise => "block_revise",
            Kind::SopStep => "sop_step",
            Kind::TemplateExtract => "template_extract",
            Kind::SiopCheck => "siop_check",
        }
    }

    fn priority(self) -> i16 {
        match self {
            Kind::BlockRegenerate | Kind::BlockRevise => 0,
            Kind::SopStep | Kind::TemplateExtract => 1,
            Kind::SiopCheck => 2,
        }
    }

    fn ref_kind(self) -> &'static str {
        match self {
            Kind::BlockRegenerate => "worksheet",
            Kind::BlockRevise => "worksheet_proposal",
            Kind::SopStep => "sop_job",
            Kind::TemplateExtract => "worksheet_template",
            Kind::SiopCheck => "siop_check",
        }
    }
}

/// A job of one kind that was just asked for and waits in the queue.
struct Started {
    kind: Kind,
    token: String,
    /// The `ai_jobs` row.
    job: i64,
    /// What the feature row is called by (worksheet, proposal, SOP job, template, check).
    ref_id: i64,
    /// A second id the feature's page needs (the worksheet of a proposal, the turn and session of
    /// a check).
    context: (i64, i64),
}

#[derive(Debug, PartialEq)]
enum Feature {
    Open,
    Done,
    Failed(String),
}

/// Steps the dispatcher until the job of `(ref_kind, ref_id)` has ended.
async fn run_until_ended(app: &common::TestApp, ref_kind: &str, ref_id: i64) {
    for _ in 0..1500 {
        queue::dispatcher::dispatch_ready(&app.state).await;
        let (_, state, _) = job_of(&app.db, ref_kind, ref_id).await;
        if !matches!(state.as_str(), "queued" | "running") {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the job of {ref_kind} {ref_id} did not end");
}

/// A worksheet with a generated version (through the queue) and its first activity block.
async fn generated_worksheet(app: &common::TestApp, t: &str) -> (i64, String) {
    let ws = new_worksheet(app, t).await;
    generate(app, t, ws).await;
    run_until_ended(app, "worksheet", ws).await;
    let (_, state) = app.get(&format!("/api/worksheets/{ws}"), Some(t)).await;
    let block = state["content"]["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["kind"] == "activity")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    (ws, block)
}

async fn started(app: &common::TestApp, kind: Kind, tag: &str) -> Started {
    let token = app
        .teacher(&format!("T-Q-{tag}-{}", kind.job_kind()), "general")
        .await;
    let t = token.as_str();
    let (ref_id, context) = match kind {
        Kind::BlockRegenerate => {
            let (ws, block) = generated_worksheet(app, t).await;
            let (s, v) = app
                .post(
                    &format!("/api/worksheets/{ws}/blocks/{block}/regenerate"),
                    Some(t),
                    json!({}),
                )
                .await;
            assert_eq!(s, StatusCode::ACCEPTED, "{v}");
            (ws, (ws, 0))
        }
        Kind::BlockRevise => {
            let (ws, block) = generated_worksheet(app, t).await;
            let (s, v) = app
                .post(
                    &format!("/api/worksheets/{ws}/blocks/{block}/revise"),
                    Some(t),
                    json!({ "instruction": "Make the questions a little shorter." }),
                )
                .await;
            assert_eq!(s, StatusCode::ACCEPTED, "{v}");
            assert_eq!(v["status"], "running");
            (v["id"].as_i64().unwrap(), (ws, 0))
        }
        Kind::SopStep => {
            let unit = app.create_unit(t, "L3 The Umbrella Stand").await;
            let lesson = app.create_lesson(t, unit, "第 2 節 精讀與提問").await;
            let (s, v) = app
                .put(
                    &format!("/api/lessons/{lesson}/sop/material"),
                    Some(t),
                    json!({ "title": "The Umbrella Stand", "source_note": "", "text": TEXT }),
                )
                .await;
            assert_eq!(s, StatusCode::OK, "{v}");
            let (s, v) = app
                .post(
                    &format!("/api/lessons/{lesson}/sop/analysis"),
                    Some(t),
                    json!({}),
                )
                .await;
            assert_eq!(s, StatusCode::ACCEPTED, "{v}");
            assert_eq!(v["status"], "running");
            (v["id"].as_i64().unwrap(), (lesson, 0))
        }
        Kind::TemplateExtract => {
            let (s, m) = app
                .post(
                    "/api/materials",
                    Some(t),
                    json!({ "kind": "text", "title": "我的學習單", "text": format!(
                        "Name: ______\n\n## Warm-up: [主題]\nWhat do you know about umbrellas?\n\n\
                         ## Paragraph [n]: Details\n1. What does Ms. Chen put outside?\n\n{TEXT}"
                    ) }),
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
            assert_eq!(v["status"], "extracting");
            (v["id"].as_i64().unwrap(), (0, 0))
        }
        Kind::SiopCheck => {
            let (s, v) = app
                .post(
                    "/api/prep-sessions",
                    Some(t),
                    json!({ "kind": "activity", "material": { "title": "The Umbrella Stand", "text": TEXT } }),
                )
                .await;
            assert_eq!(s, StatusCode::CREATED, "{v}");
            let session = v["id"].as_i64().unwrap();
            let turn: i64 = sqlx::query_scalar("SELECT id FROM prep_turns WHERE session_id = $1")
                .bind(session)
                .fetch_one(&app.db)
                .await
                .unwrap();
            // The turn runs through the queue (started once, so that the check it opens is
            // not started too); its closing opens the check, which is queued from a task of
            // its own: wait for the entry, not for a run.
            queue::dispatcher::dispatch_ready(&app.state).await;
            for _ in 0..1500 {
                if job_of(&app.db, "prep_turn", turn).await.1 == "done" {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            let mut check = None;
            for _ in 0..500 {
                check = sqlx::query_scalar::<_, i64>(
                    "SELECT c.id FROM siop_checks c WHERE c.turn_id = $1",
                )
                .bind(turn)
                .fetch_optional(&app.db)
                .await
                .unwrap();
                let queued: Option<i64> = match check {
                    Some(id) => sqlx::query_scalar(
                        "SELECT id FROM ai_jobs WHERE ref_kind = 'siop_check' AND ref_id = $1",
                    )
                    .bind(id)
                    .fetch_optional(&app.db)
                    .await
                    .unwrap(),
                    None => None,
                };
                if queued.is_some() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            (check.expect("the turn opened a check"), (session, turn))
        }
    };
    let (job, state, _) = job_of(&app.db, kind.ref_kind(), ref_id).await;
    assert_eq!(state, "queued", "{kind:?}");
    Started {
        kind,
        token,
        job,
        ref_id,
        context,
    }
}

/// What the feature's own page says about its row.
async fn feature(app: &common::TestApp, s: &Started) -> Feature {
    let t = Some(s.token.as_str());
    match s.kind {
        Kind::BlockRegenerate => {
            let (_, v) = app
                .get(&format!("/api/worksheets/{}/jobs", s.ref_id), t)
                .await;
            if v["running"] == true {
                return Feature::Open;
            }
            let failed = v["jobs"]
                .as_array()
                .unwrap()
                .iter()
                .find(|j| j["outcome"] == "error");
            match failed {
                Some(j) => Feature::Failed(j["message"].as_str().unwrap_or_default().to_string()),
                None => Feature::Done,
            }
        }
        Kind::BlockRevise => {
            let (_, v) = app
                .get(
                    &format!("/api/worksheets/{}/proposals/{}", s.context.0, s.ref_id),
                    t,
                )
                .await;
            match v["status"].as_str().unwrap() {
                "running" => Feature::Open,
                "failed" => Feature::Failed(v["error"].as_str().unwrap_or_default().to_string()),
                _ => Feature::Done,
            }
        }
        Kind::SopStep => {
            let (_, v) = app.get(&format!("/api/sop-jobs/{}", s.ref_id), t).await;
            if v["status"] == "running" {
                Feature::Open
            } else if v["result"]["status"] == "error" {
                Feature::Failed(v["result"]["message"].as_str().unwrap().to_string())
            } else {
                Feature::Done
            }
        }
        Kind::TemplateExtract => {
            let (_, v) = app
                .get(&format!("/api/worksheet-templates/{}", s.ref_id), t)
                .await;
            match v["status"].as_str().unwrap() {
                "extracting" => Feature::Open,
                "failed" => Feature::Failed(v["error"].as_str().unwrap_or_default().to_string()),
                _ => Feature::Done,
            }
        }
        Kind::SiopCheck => {
            let (session, turn) = s.context;
            let (_, v) = app
                .get(
                    &format!("/api/prep-sessions/{session}/turns/{turn}/siop-check"),
                    t,
                )
                .await;
            match v["status"].as_str().unwrap() {
                "running" => Feature::Open,
                "failed" => Feature::Failed(v["error"].as_str().unwrap_or_default().to_string()),
                _ => Feature::Done,
            }
        }
    }
}

/// The `queue` object the feature's page shows while the job waits.
async fn page_queue(app: &common::TestApp, s: &Started) -> Value {
    let t = Some(s.token.as_str());
    match s.kind {
        Kind::BlockRegenerate => {
            let (_, v) = app
                .get(&format!("/api/worksheets/{}/jobs", s.ref_id), t)
                .await;
            v["progress"]["queue"].clone()
        }
        Kind::BlockRevise => {
            let (_, v) = app
                .get(
                    &format!("/api/worksheets/{}/proposals/{}", s.context.0, s.ref_id),
                    t,
                )
                .await;
            v["queue"].clone()
        }
        Kind::SopStep => {
            let (_, v) = app.get(&format!("/api/sop-jobs/{}", s.ref_id), t).await;
            v["queue"].clone()
        }
        Kind::TemplateExtract => {
            let (_, v) = app
                .get(&format!("/api/worksheet-templates/{}", s.ref_id), t)
                .await;
            v["queue"].clone()
        }
        Kind::SiopCheck => {
            let (session, turn) = s.context;
            let (_, v) = app
                .get(
                    &format!("/api/prep-sessions/{session}/turns/{turn}/siop-check"),
                    t,
                )
                .await;
            v["queue"].clone()
        }
    }
}

fn expire_lease(job: i64, attempts: i32) -> String {
    format!(
        "UPDATE ai_jobs SET status = 'running', attempts = {attempts}, started_at = now(),
                lease_until = now() - interval '1 second' WHERE id = {job}"
    )
}

#[tokio::test]
async fn every_other_kind_is_queued_with_its_priority_and_runs_to_the_end() {
    let app = test_app!(queued);
    for kind in KINDS {
        let s = started(&app, kind, "RUN").await;
        let (job_kind, priority): (String, i16) =
            sqlx::query_as("SELECT kind, priority FROM ai_jobs WHERE id = $1")
                .bind(s.job)
                .fetch_one(&app.db)
                .await
                .unwrap();
        assert_eq!(
            (job_kind.as_str(), priority),
            (kind.job_kind(), kind.priority())
        );
        // Nothing carries the teacher's words: the params are ids (and the track).
        let params: Value = sqlx::query_scalar("SELECT params FROM ai_jobs WHERE id = $1")
            .bind(s.job)
            .fetch_one(&app.db)
            .await
            .unwrap();
        assert!(
            !params.to_string().contains("umbrella"),
            "{kind:?} {params}"
        );
        assert!(!params.to_string().contains("shorter"), "{kind:?} {params}");
        assert_eq!(feature(&app, &s).await, Feature::Open, "{kind:?}");
        // The page says where it stands.
        let q = page_queue(&app, &s).await;
        assert_eq!(q["job_id"], s.job, "{kind:?} {q}");
        assert_eq!(q["ahead"], 0, "{kind:?} {q}");
        // Queued for a long time: the page's stale sweep does not take it for dead.
        sqlx::query("UPDATE ai_jobs SET queued_at = now() - interval '3 days' WHERE id = $1")
            .bind(s.job)
            .execute(&app.db)
            .await
            .unwrap();
        let backdate = match kind {
            Kind::BlockRegenerate => "UPDATE worksheet_jobs SET created_at = now() - interval '3 days' WHERE worksheet_id = $1",
            Kind::BlockRevise => "UPDATE worksheet_block_proposals SET created_at = now() - interval '3 days' WHERE id = $1",
            Kind::SopStep => "UPDATE lesson_sop_jobs SET created_at = now() - interval '3 days' WHERE id = $1",
            Kind::TemplateExtract => "UPDATE worksheet_templates SET extract_started_at = now() - interval '3 days' WHERE id = $1",
            Kind::SiopCheck => "UPDATE siop_checks SET created_at = now() - interval '3 days' WHERE id = $1",
        };
        sqlx::query(backdate)
            .bind(s.ref_id)
            .execute(&app.db)
            .await
            .unwrap();
        assert_eq!(feature(&app, &s).await, Feature::Open, "{kind:?} stale");

        run_until_ended(&app, kind.ref_kind(), s.ref_id).await;
        assert_eq!(job_of(&app.db, kind.ref_kind(), s.ref_id).await.1, "done");
        assert_eq!(feature(&app, &s).await, Feature::Done, "{kind:?}");
        assert!(page_queue(&app, &s).await.is_null(), "{kind:?}");
        let logged: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM usage_logs WHERE event = 'ai_job_finished'
                AND payload ->> 'kind' = $1 AND payload ->> 'status' = 'done'",
        )
        .bind(kind.job_kind())
        .fetch_one(&app.db)
        .await
        .unwrap();
        assert_eq!(logged, 1, "{kind:?}");
    }
}

#[tokio::test]
async fn cancelling_a_queued_job_of_every_other_kind_closes_its_row() {
    let app = test_app!(queued);
    for kind in KINDS {
        let s = started(&app, kind, "CANCEL").await;
        let (status, v) = app
            .delete(&format!("/api/ai-jobs/{}", s.job), Some(&s.token))
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{kind:?} {v}");
        assert_eq!(
            feature(&app, &s).await,
            Feature::Failed(MSG_CANCELLED.to_string()),
            "{kind:?}"
        );
        assert_eq!(
            job_of(&app.db, kind.ref_kind(), s.ref_id).await.1,
            "cancelled"
        );
    }
}

#[tokio::test]
async fn a_job_of_every_other_kind_lost_on_its_last_attempt_fails_its_row() {
    let app = test_app!(queued);
    for kind in KINDS {
        let s = started(&app, kind, "LOST").await;
        // The test config allows two attempts: the second one was lost with its server.
        sqlx::query(&expire_lease(s.job, 2))
            .execute(&app.db)
            .await
            .unwrap();
        let (requeued, failed) = queue::dispatcher::recover(&app.state).await.unwrap();
        assert_eq!((requeued, failed), (0, 1), "{kind:?}");
        assert_eq!(job_of(&app.db, kind.ref_kind(), s.ref_id).await.1, "failed");
        assert_eq!(
            feature(&app, &s).await,
            Feature::Failed(MSG_LOST.to_string()),
            "{kind:?}"
        );
    }
}

#[tokio::test]
async fn a_job_of_every_other_kind_lost_on_its_first_attempt_runs_again() {
    let app = test_app!(queued);
    for kind in KINDS {
        let s = started(&app, kind, "REPLAY").await;
        sqlx::query(&expire_lease(s.job, 1))
            .execute(&app.db)
            .await
            .unwrap();
        let (requeued, failed) = queue::dispatcher::recover(&app.state).await.unwrap();
        assert_eq!((requeued, failed), (1, 0), "{kind:?}");
        assert_eq!(feature(&app, &s).await, Feature::Open, "{kind:?}");
        run_until_ended(&app, kind.ref_kind(), s.ref_id).await;
        let (_, state, attempts) = job_of(&app.db, kind.ref_kind(), s.ref_id).await;
        assert_eq!((state.as_str(), attempts), ("done", 2), "{kind:?}");
        assert_eq!(feature(&app, &s).await, Feature::Done, "{kind:?}");
    }
}

#[tokio::test]
async fn the_queue_off_path_keeps_starting_these_directly() {
    // The default (and the test) configuration: no job rows, the work runs where it was asked.
    let app = test_app!();
    let t = app.teacher("T-Q-DIRECT", "general").await;
    let unit = app.create_unit(&t, "U").await;
    let lesson = app.create_lesson(&t, unit, "L").await;
    let (s, v) = app
        .put(
            &format!("/api/lessons/{lesson}/sop/material"),
            Some(&t),
            json!({ "title": "The Umbrella Stand", "source_note": "", "text": TEXT }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, job) = app
        .post(
            &format!("/api/lessons/{lesson}/sop/analysis"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{job}");
    assert!(job["queue"].is_null());
    let id = job["id"].as_i64().unwrap();
    for _ in 0..400 {
        let (_, v) = app.get(&format!("/api/sop-jobs/{id}"), Some(&t)).await;
        if v["status"] == "done" {
            assert_eq!(v["result"]["status"], "shown", "{v}");
            let jobs: i64 = sqlx::query_scalar("SELECT count(*) FROM ai_jobs")
                .fetch_one(&app.db)
                .await
                .unwrap();
            assert_eq!(jobs, 0);
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("the SOP job did not finish");
}

/// A worksheet turn made from a sample waits for its template (`spawn_after_template`, a plain
/// background task); with the queue on the extraction is a queued job, the turn is not failed
/// for waiting behind it, and it makes its worksheet once the template is ready.
#[tokio::test]
async fn a_turn_waiting_for_a_queued_template_extraction_follows_it() {
    let app = test_app!(|c: &mut Config| {
        c.ai_queue_enabled = true;
        c.ai_max_running_jobs = 1;
        c.ai_max_running_per_teacher = 1;
        c.fake_latency_ms = 20;
    });
    let t = app.teacher("T-Q-TPLWAIT", "general").await;
    let (s, sample) = app
        .post(
            "/api/materials",
            Some(&t),
            json!({ "kind": "text", "title": "L2 Reading Worksheet", "text":
                "Name: ______\n\n## Warm-up: [主題]\nWhat do you know about the shop?\n\n\
                 ## Paragraph [n]: Details\n1. What does the owner put outside? 2. Why?\n\n\
                 ## Writing: [主題]\nWrite 80 words about trust." }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{sample}");
    let (s, v) = app
        .post(
            "/api/prep-sessions",
            Some(&t),
            json!({
                "kind": "worksheet",
                "material": { "title": "The Umbrella Stand", "text": TEXT },
                "template_material_id": sample["id"],
            }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    let id = v["id"].as_i64().unwrap();
    let template = v["template"]["id"].as_i64().unwrap();
    assert_eq!(v["turns"][0]["status"], "running");
    let (job, state, _) = job_of(&app.db, "worksheet_template", template).await;
    assert_eq!(state, "queued");
    let kind: String = sqlx::query_scalar("SELECT kind FROM ai_jobs WHERE id = $1")
        .bind(job)
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(kind, "template_extract");
    // Waiting far longer than the stale limit does not fail the template or its turn.
    sqlx::query("UPDATE worksheet_templates SET extract_started_at = now() - interval '3 days' WHERE id = $1")
        .bind(template)
        .execute(&app.db)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE prep_turns SET created_at = now() - interval '3 days' WHERE session_id = $1",
    )
    .bind(id)
    .execute(&app.db)
    .await
    .unwrap();
    let (_, still) = app.get(&format!("/api/prep-sessions/{id}"), Some(&t)).await;
    assert_eq!(still["turns"][0]["status"], "running", "{still}");
    assert_eq!(still["turns"][0]["template"]["status"], "extracting");

    let dispatcher = tokio::spawn(queue::dispatcher::run(app.state.clone()));
    let done = wait_session(&app, &t, id).await;
    assert_eq!(done["turns"][0]["status"], "done", "{done}");
    assert_eq!(done["turns"][0]["template"]["status"], "ready");
    assert_eq!(
        job_of(&app.db, "worksheet_template", template).await.1,
        "done"
    );
    let runs: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ai_jobs WHERE kind = 'worksheet_run' AND status = 'done'",
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(runs, 1, "the worksheet was made after the template");
    app.state.queue.stop();
    dispatcher.await.unwrap();
}
