//! 後台管理 → 總覽 → AI 請求 / `GET /api/admin/ai-traffic`: aggregates of the queue and the
//! model calls. PI only; an empty database gives zeros. Contract: docs/API.md「AI 流量」.

#[macro_use]
mod common;

use axum::http::StatusCode;
use serde_json::json;
use sqlx::PgPool;

async fn teacher_id(db: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT id FROM teachers ORDER BY id LIMIT 1")
        .fetch_one(db)
        .await
        .unwrap()
}

/// A finished job: `finished` minutes ago, after `wait` seconds queued and `run` seconds running.
async fn finished_job(
    db: &PgPool,
    t: i64,
    kind: &str,
    status: &str,
    mins: i32,
    wait: i32,
    run: i32,
) {
    sqlx::query(
        "INSERT INTO ai_jobs (kind, teacher_id, priority, status, queued_at, started_at, finished_at)
         VALUES ($1, $2, 1, $3,
                 now() - make_interval(mins => $4) - make_interval(secs => $5 + $6),
                 now() - make_interval(mins => $4) - make_interval(secs => $6),
                 now() - make_interval(mins => $4))",
    )
    .bind(kind)
    .bind(t)
    .bind(status)
    .bind(mins)
    .bind(f64::from(wait))
    .bind(f64::from(run))
    .execute(db)
    .await
    .unwrap();
}

async fn call(
    db: &PgPool,
    t: i64,
    created: &str,
    status: &str,
    detail: Option<&str>,
    latency: i32,
    usage: serde_json::Value,
) {
    sqlx::query(&format!(
        "INSERT INTO ai_interactions (teacher_id, module, input_snapshot, provider, model, status,
                                      error_detail, latency_ms, usage, created_at)
         VALUES ($1, 'worksheet', '{{}}', 'fake', 'fake-model', $2, $3, $4, $5, {created})"
    ))
    .bind(t)
    .bind(status)
    .bind(detail)
    .bind(latency)
    .bind(usage)
    .execute(db)
    .await
    .unwrap();
}

#[tokio::test]
async fn is_pi_only() {
    let app = test_app!();
    let teacher = app.teacher("T-X", "general").await;
    let (s, _) = app.get("/api/admin/ai-traffic", Some(&teacher)).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = app.get("/api/admin/ai-traffic", None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn an_empty_database_gives_zeros() {
    let app = test_app!();
    let pi = app.pi_token().await;
    let (s, v) = app.get("/api/admin/ai-traffic", Some(&pi)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["now"]["running"], 0);
    assert_eq!(v["now"]["queued"], 0);
    assert_eq!(v["now"]["oldest_wait_secs"], 0);
    assert_eq!(
        v["now"]["max_running"],
        app.state.config.ai_max_running_jobs
    );
    assert_eq!(v["now"]["queue_enabled"], app.state.config.ai_queue_enabled);
    assert!(v["now"]["quota_blocked_secs"].is_null());
    let h = &v["last_hour"];
    assert_eq!(h["jobs"], json!({ "done": 0, "failed": 0, "cancelled": 0 }));
    assert_eq!(h["wait_secs"], json!({ "p50": 0.0, "p95": 0.0 }));
    assert_eq!(h["run_secs"], json!({}));
    assert_eq!(h["calls"], 0);
    assert_eq!(
        h["tokens"],
        json!({ "prompt": 0, "cached": 0, "completion": 0, "reasoning": 0 })
    );
    assert_eq!(h["latency_ms"], json!({ "p50": 0.0, "p95": 0.0 }));
    let days = v["last_7_days"].as_array().unwrap();
    assert_eq!(days.len(), 7);
    assert!(days
        .iter()
        .all(|d| d["calls"] == 0 && d["prompt_tokens"] == 0));
}

#[tokio::test]
async fn counts_jobs_calls_tokens_and_days_from_the_tables() {
    let app = test_app!();
    let pi = app.pi_token().await;
    let t = teacher_id(&app.db).await;

    // Queue now: one running, two queued (the older one waiting about 90 s).
    for (status, age) in [("running", 5), ("queued", 90), ("queued", 10)] {
        sqlx::query(
            "INSERT INTO ai_jobs (kind, teacher_id, priority, status, queued_at)
             VALUES ('worksheet_run', $1, 1, $2, now() - make_interval(secs => $3))",
        )
        .bind(t)
        .bind(status)
        .bind(f64::from(age))
        .execute(&app.db)
        .await
        .unwrap();
    }
    // Finished in the last hour: waits 2/4/6/8/10 s, worksheet_run runs 10..50 s.
    for (i, w) in [2, 4, 6, 8, 10].into_iter().enumerate() {
        finished_job(
            &app.db,
            t,
            "worksheet_run",
            "done",
            10,
            w,
            10 * (i as i32 + 1),
        )
        .await;
    }
    finished_job(&app.db, t, "prep_turn", "failed", 20, 1, 7).await;
    finished_job(&app.db, t, "prep_turn", "done", 30, 1, 9).await;
    // Older than an hour: not counted.
    finished_job(&app.db, t, "prep_turn", "done", 120, 100, 100).await;

    let usage = |p: i64, c: i64, o: i64, r: i64| json!({ "prompt_tokens": p, "cached_tokens": c, "completion_tokens": o, "reasoning_tokens": r });
    let recent = "now() - interval '5 minutes'";
    call(
        &app.db,
        t,
        recent,
        "shown",
        None,
        1000,
        usage(100, 60, 10, 4),
    )
    .await;
    call(
        &app.db,
        t,
        recent,
        "shown",
        None,
        2000,
        usage(200, 100, 20, 6),
    )
    .await;
    call(
        &app.db,
        t,
        recent,
        "shown",
        None,
        3000,
        usage(300, 0, 30, 0),
    )
    .await;
    call(
        &app.db,
        t,
        recent,
        "error",
        Some("rate_limited: 429"),
        4000,
        json!(null),
    )
    .await;
    call(
        &app.db,
        t,
        recent,
        "error",
        Some("quota: insufficient_quota"),
        5000,
        json!(null),
    )
    .await;
    call(
        &app.db,
        t,
        recent,
        "error",
        Some("overloaded: 529"),
        6000,
        json!(null),
    )
    .await;
    call(
        &app.db,
        t,
        recent,
        "timeout",
        Some("timeout"),
        7000,
        json!(null),
    )
    .await;
    // Two hours ago (this Taipei day or the previous one) and 2 days ago at 00:30 Taipei time.
    call(
        &app.db,
        t,
        "now() - interval '2 hours'",
        "shown",
        None,
        500,
        usage(1000, 0, 100, 0),
    )
    .await;
    let two_days = "(((now() AT TIME ZONE 'Asia/Taipei')::date - 2) + time '00:30') AT TIME ZONE 'Asia/Taipei'";
    call(
        &app.db,
        t,
        two_days,
        "shown",
        None,
        500,
        usage(5000, 1000, 500, 0),
    )
    .await;
    // Eight days ago: outside the window.
    call(
        &app.db,
        t,
        "now() - interval '8 days'",
        "shown",
        None,
        500,
        usage(9, 9, 9, 9),
    )
    .await;

    let (s, v) = app.get("/api/admin/ai-traffic", Some(&pi)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["now"]["running"], 1);
    assert_eq!(v["now"]["queued"], 2);
    let oldest = v["now"]["oldest_wait_secs"].as_i64().unwrap();
    assert!((90..=95).contains(&oldest), "{oldest}");

    let h = &v["last_hour"];
    assert_eq!(h["jobs"], json!({ "done": 6, "failed": 1, "cancelled": 0 }));
    // Waits 2,4,6,8,10,1,1 -> sorted 1,1,2,4,6,8,10: p50 = 4, p95 = 9.4.
    assert_eq!(h["wait_secs"]["p50"], 4.0);
    assert_eq!(h["wait_secs"]["p95"], 9.4);
    assert_eq!(
        h["run_secs"]["worksheet_run"],
        json!({ "p50": 30.0, "p95": 48.0 })
    );
    assert_eq!(
        h["run_secs"]["prep_turn"],
        json!({ "p50": 8.0, "p95": 8.9 })
    );
    assert_eq!(h["calls"], 7);
    assert_eq!(h["errors"], 4);
    assert_eq!(h["rate_limited"], 1);
    assert_eq!(h["quota"], 1);
    assert_eq!(h["overloaded"], 1);
    assert_eq!(
        h["tokens"],
        json!({ "prompt": 600, "cached": 160, "completion": 60, "reasoning": 10 })
    );
    assert_eq!(h["latency_ms"], json!({ "p50": 4000.0, "p95": 6700.0 }));

    let days = v["last_7_days"].as_array().unwrap();
    assert_eq!(days.len(), 7);
    let today: String = sqlx::query_scalar(
        "SELECT to_char((now() AT TIME ZONE 'Asia/Taipei')::date, 'YYYY-MM-DD')",
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(days[6]["date"], today.as_str());
    let two: String = sqlx::query_scalar(
        "SELECT to_char((now() AT TIME ZONE 'Asia/Taipei')::date - 2, 'YYYY-MM-DD')",
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(days[4]["date"], two.as_str());
    assert_eq!(days[4]["calls"], 1);
    assert_eq!(days[4]["prompt_tokens"], 5000);
    assert_eq!(days[4]["cached_tokens"], 1000);
    assert_eq!(days[4]["completion_tokens"], 500);
    // Everything else lands in today or yesterday (the 2-hour-old call may cross midnight).
    let calls: i64 = days.iter().map(|d| d["calls"].as_i64().unwrap()).sum();
    assert_eq!(calls, 9);
    let jobs: i64 = days.iter().map(|d| d["jobs"].as_i64().unwrap()).sum();
    assert_eq!(jobs, 11, "3 queued or running + 8 finished");
}

/// The payload carries no teacher identity or text.
#[tokio::test]
async fn carries_no_per_teacher_data() {
    let app = test_app!();
    let pi = app.pi_token().await;
    let (_, v) = app.get("/api/admin/ai-traffic", Some(&pi)).await;
    let text = v.to_string();
    assert!(!text.contains("teacher") && !text.contains("research_id"));
}
